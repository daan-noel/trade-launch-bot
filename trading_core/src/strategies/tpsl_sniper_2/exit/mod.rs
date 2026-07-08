//! Exit ladder — the single place that decides **when a held position exits**.
//!
//! Two triggers share one set of feature predicates so they can never drift:
//!   • trade-driven ([`find_trade_driven_exit`]) — re-evaluated when a trade prints.
//!   • clock-driven ([`find_clock_driven_exit`]) — re-evaluated on a wall-clock
//!     timer, so deadline exits (Stall / TimeStop) fire even while a token is
//!     silent and no trade is arriving.
//!
//! Ladder priority (first match on a trade wins):
//!   LiquidityExit → StopLoss → TakeProfit → TrailingStop → Stall → TimeStop.
//!
//! Every feature is inert by default: with all `p_*` exit params unset/zero the
//! trade walk reproduces the legacy fixed TP/SL behavior exactly.
//!
//! **To add an exit feature:** read its rule param near the top of
//! [`find_trade_driven_exit`], add one `.or_else(...)` arm at the right priority
//! in the ladder, and — if it is time-based — mirror it in
//! [`find_clock_driven_exit`] using a shared `*_triggered` predicate.

use chrono::{DateTime, Duration, Utc};

use crate::config::constants::MAX_FILL_WAIT_SLOTS;
use crate::models::trade::{Trade, TradeRow};
use crate::models::{Position, PositionStatus, Tpsl2Rule};

use super::util::{none_if_zero_f64, none_if_zero_u64};

/// The typed reason a position exited. Replaces the old stringly-typed reason
/// that was round-tripped through `&str`; the engine returns this directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitReason {
    TakeProfit,
    StopLoss,
    /// E1 · price fell `p_exit_trailing_stop_pct`% below the peak-since-entry.
    TrailingStop,
    /// E3 · no new higher-high for `p_exit_stall_secs`.
    Stall,
    /// E2 · held past the `p_exit_time_stop_secs` deadline.
    TimeStop,
    /// E4 · **real** SOL reserves crashed `p_exit_liquidity_drop_pct`% below their peak.
    LiquidityExit,
    /// Analysis-only death-close: the ladder never fired but the token is provably
    /// dead (liquidity gone + gone silent). Closed at the last meaningful trade.
    /// Never produced by the live paper poll. See [`crate::strategies::death`].
    Dead,
}

impl ExitReason {
    /// Stable wire/string form, persisted on positions and surfaced by the
    /// backtest. The single source of truth for the reason strings.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::TakeProfit => "TakeProfit",
            Self::StopLoss => "StopLoss",
            Self::TrailingStop => "TrailingStop",
            Self::Stall => "Stall",
            Self::TimeStop => "TimeStop",
            Self::LiquidityExit => "LiquidityExit",
            Self::Dead => "Dead",
        }
    }
}

impl std::fmt::Display for ExitReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// The resolved exit: the fill price/tx/time of the trade the position exits on,
/// plus the reason that fired. Replaces the old positional
/// `(f64, String, DateTime<Utc>, String)` tuple so call sites read fields by
/// name (`fill.reason`, not `exit.3`).
#[derive(Debug, Clone, PartialEq)]
pub struct ExitFill {
    pub price: f64,
    pub tx_signature: String,
    /// Slot of the fill trade — the unambiguous key the sweep drill-in uses to
    /// resolve this fill's real `tx_signature` from the `trades` table (the slim
    /// `CorpusTrade` carries no signature, so the in-row `tx_signature` is empty
    /// on the sweep path). Live reads the signature directly and ignores this.
    pub slot: u64,
    pub block_time: DateTime<Utc>,
    pub reason: ExitReason,
}

/// Running state accumulated while walking a position's post-entry trades:
/// the peak price (drives the trailing stop), the time of the most recent new
/// higher-high (drives the stall clock), and the peak **real** SOL reserves
/// (drives the liquidity-death exit — never virtual, which wash trading can
/// fake). The clock-driven sweep rebuilds this from history so it can measure
/// Stall against `now` instead of the last trade.
#[derive(Debug, Clone, Copy)]
pub struct ExitWalkState {
    pub peak_price: f64,
    pub last_higher_high_time: DateTime<Utc>,
    pub peak_reserves: f64,
}

impl ExitWalkState {
    /// Initial state at entry: peak is the entry price, the stall clock runs
    /// from entry (so a position that never prints a new high can still stall),
    /// and no reserves have been seen yet.
    pub fn starting_at(entry_price: f64, entry_time: DateTime<Utc>) -> Self {
        Self {
            peak_price: entry_price,
            last_higher_high_time: entry_time,
            peak_reserves: 0.0,
        }
    }

    /// Fold one post-entry trade into the running peaks.
    pub fn update_with_trade<T: TradeRow>(&mut self, trade: &T) {
        let price = trade.price_per_token();
        if price > self.peak_price {
            self.peak_price = price;
            self.last_higher_high_time = trade.block_time();
        }
        if let Some(reserves) = trade.real_reserve_sol() {
            if reserves > self.peak_reserves {
                self.peak_reserves = reserves;
            }
        }
    }

    /// Replay the full post-entry history into a fresh state — the peaks as of
    /// the last trade. Used to seed [`CachedExitState`] the first time a position
    /// is swept; afterwards the state is advanced incrementally, never rebuilt.
    pub fn rebuild_from_trades<T: TradeRow>(
        trades: &[T],
        entry_price: f64,
        entry_time: DateTime<Utc>,
    ) -> Self {
        let mut state = Self::starting_at(entry_price, entry_time);
        for t in trades.iter().filter(|t| t.block_time() > entry_time) {
            state.update_with_trade(t);
        }
        state
    }
}

/// Per-position memoized [`ExitWalkState`] for the clock-driven sweep.
///
/// The sweep used to clone a token's full trade history and re-walk it from
/// entry on every 1s tick, for every holding position — O(holdings × trades)
/// of cloning + folding per second. Instead we fold each token's history into
/// this state once (when the position is first seen) and then advance it
/// incrementally as new trades print, so the sweep only reads a `Copy` snapshot.
///
/// `consumed_abs` is the **absolute** count of cache trades already folded —
/// `trades_base + window_index`, not a raw index into the (capped, front-trimmed)
/// `trades` vec. The token cache trims the oldest trades once history overruns
/// `MAX_TRADES_RETAINED` and advances `trades_base` by the number dropped, so the
/// not-yet-folded trades are always the window slice `[consumed_abs - base ..]`.
/// Tracking the cursor absolutely is what lets a front-trim slide the window
/// without ever skipping or double-folding a trade. If the cursor lands past the
/// window end (a token evicted and re-tracked from empty, or — only under a
/// pathological cap overrun — unfolded trades trimmed away) we rebuild from
/// whatever the window holds rather than trust a stale cursor.
/// The live path always seeds it from the cache's [`CachedTrade`]; the exit unit
/// tests drive it from `Trade`.
///
/// [`CachedTrade`]: crate::state::token_cache::CachedTrade
#[derive(Debug, Clone)]
pub struct CachedExitState {
    pub state: ExitWalkState,
    entry_time: DateTime<Utc>,
    entry_price: f64,
    consumed_abs: u64,
}

impl CachedExitState {
    /// Seed from the retained post-entry history (one-time, at first sight of the
    /// position) and record the absolute fold cursor. `base` is the token's
    /// `trades_base` (count already trimmed from the front).
    pub fn build<T: TradeRow>(
        trades: &[T],
        base: u64,
        entry_price: f64,
        entry_time: DateTime<Utc>,
    ) -> Self {
        Self {
            state: ExitWalkState::rebuild_from_trades(trades, entry_price, entry_time),
            entry_time,
            entry_price,
            consumed_abs: base + trades.len() as u64,
        }
    }

    /// An empty (unfolded) state whose absolute cursor sits at the window front
    /// (`base`), so a following [`advance_and_find_exit`](Self::advance_and_find_exit)
    /// folds the entire retained window. Used to seed the trade gate so its first
    /// pass reproduces a full re-walk while memoizing for subsequent incremental
    /// pings.
    pub fn build_unfolded(base: u64, entry_price: f64, entry_time: DateTime<Utc>) -> Self {
        Self {
            state: ExitWalkState::starting_at(entry_price, entry_time),
            entry_time,
            entry_price,
            consumed_abs: base,
        }
    }

    /// Fold any trades appended since the last advance into the running peaks.
    /// `base` is the token's current `trades_base`; `consumed_abs - base` is the
    /// window index of the first unfolded trade. Folds exactly the absolute range
    /// `[consumed_abs .. base + trades.len())` — the genuinely-new trades —
    /// regardless of how many were trimmed in between. A cursor past the window
    /// end means the history was reset/over-trimmed, so rebuild from the window.
    ///
    /// Peak-only fold retained as the memo correctness oracle (the `cached_state_*`
    /// tests pin [`advance_and_find_exit`]'s folding against it); the live trade
    /// gate folds + evaluates in one pass via [`advance_and_find_exit`].
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn advance<T: TradeRow>(&mut self, trades: &[T], base: u64) {
        let start = self.consumed_abs.saturating_sub(base);
        if start > trades.len() as u64 {
            self.state =
                ExitWalkState::rebuild_from_trades(trades, self.entry_price, self.entry_time);
            self.consumed_abs = base + trades.len() as u64;
            return;
        }
        for t in &trades[start as usize..] {
            if t.block_time() > self.entry_time {
                self.state.update_with_trade(t);
            }
        }
        self.consumed_abs = base + trades.len() as u64;
    }

    /// Incremental trade-gate: fold the genuinely-new trades into the running
    /// peaks **and**, in the same pass, evaluate the exit ladder against each
    /// newly-folded post-entry trade, returning the first [`ExitReason`] that
    /// fires.
    ///
    /// Decision-equivalent to a full [`find_trade_driven_exit`] re-walk: peaks are
    /// accumulated by the identical fold, the ladder runs against the state as of
    /// each trade via the shared [`ladder_reason`] predicate, and only new trades
    /// can newly fire — an old trade that fired would already have exited the
    /// position.
    pub fn advance_and_find_exit<T: TradeRow>(
        &mut self,
        trades: &[T],
        base: u64,
        params: &LadderParams,
    ) -> Option<ExitReason> {
        if self.entry_price <= 0.0 {
            self.consumed_abs = base + trades.len() as u64;
            return None;
        }
        let start = self.consumed_abs.saturating_sub(base);
        let rebuild = start > trades.len() as u64;
        if rebuild {
            // Lost the cursor: re-fold from the window start. Reset the peaks so
            // the walk below is a clean replay of whatever the window now holds.
            self.state = ExitWalkState::starting_at(self.entry_price, self.entry_time);
        }
        let from = if rebuild { 0 } else { start as usize };
        self.consumed_abs = base + trades.len() as u64;

        let mut fired = None;
        for t in &trades[from..] {
            if t.block_time() <= self.entry_time {
                continue;
            }
            self.state.update_with_trade(t);
            if fired.is_none() {
                fired = ladder_reason(&self.state, t, self.entry_time, self.entry_price, params);
            }
            // Keep folding the rest so the memoized peaks stay current for the
            // clock sweep even after the gate has already decided to exit.
        }
        fired
    }
}

/// The exit-ladder rule params, resolved once from a [`Tpsl2Rule`] so the per-trade
/// predicate ([`ladder_reason`]) is a pure function of state + trade + params. Lets
/// the full re-walk and the incremental [`CachedExitState::advance_and_find_exit`]
/// share one ladder definition that can never drift.
#[derive(Clone)]
pub struct LadderParams {
    take_profit_pct: f64,
    stop_loss_pct: f64,
    trailing_stop_pct: Option<f64>,
    time_stop_secs: Option<u64>,
    stall_secs: Option<u64>,
    liquidity_drop_pct: Option<f64>,
}

impl LadderParams {
    pub fn from_rule(rule: &Tpsl2Rule) -> Self {
        Self {
            take_profit_pct: rule.p_exit_take_profit,
            stop_loss_pct: rule.p_exit_stop_loss,
            trailing_stop_pct: none_if_zero_f64(rule.p_exit_trailing_stop_pct), // E1
            time_stop_secs: none_if_zero_u64(rule.p_exit_time_stop_secs),       // E2
            stall_secs: none_if_zero_u64(rule.p_exit_stall_secs),               // E3
            liquidity_drop_pct: none_if_zero_f64(rule.p_exit_liquidity_drop_pct), // E4
        }
    }

    /// E2 TimeStop deadline (seconds since entry), if configured.
    pub fn time_stop_secs(&self) -> Option<u64> {
        self.time_stop_secs
    }

    /// E3 Stall deadline (seconds since last higher-high), if configured.
    pub fn stall_secs(&self) -> Option<u64> {
        self.stall_secs
    }
}

/// The exit ladder for a single trade `t`, given the running walk `state` (peaks
/// as of `t`, inclusive). First feature that fires wins (ladder order). Shared by
/// [`find_trade_driven_exit`] and [`CachedExitState::advance_and_find_exit`].
fn ladder_reason<T: TradeRow>(
    state: &ExitWalkState,
    t: &T,
    entry_time: DateTime<Utc>,
    entry_price: f64,
    params: &LadderParams,
) -> Option<ExitReason> {
    let price = t.price_per_token();
    let block_time = t.block_time();
    let pct = ((price - entry_price) / entry_price) * 100.0;
    None
        .or_else(|| {
            // E4: reserves crash below the peak-since-entry. REAL reserves only.
            params.liquidity_drop_pct.and_then(|drop| {
                t.real_reserve_sol().and_then(|reserves| {
                    (state.peak_reserves > 0.0
                        && reserves < state.peak_reserves * (1.0 - drop / 100.0))
                    .then_some(ExitReason::LiquidityExit)
                })
            })
        })
        .or_else(|| (pct <= -params.stop_loss_pct).then_some(ExitReason::StopLoss))
        .or_else(|| (pct >= params.take_profit_pct).then_some(ExitReason::TakeProfit))
        .or_else(|| {
            // E1: bank the reversal once price falls `trail`% below the peak.
            params.trailing_stop_pct.and_then(|trail| {
                (state.peak_price > 0.0 && price <= state.peak_price * (1.0 - trail / 100.0))
                    .then_some(ExitReason::TrailingStop)
            })
        })
        .or_else(|| {
            // E3: sell the flatline. Ranks above the time stop, below trailing.
            params.stall_secs.and_then(|secs| {
                stall_triggered(state.last_higher_high_time, block_time, secs)
                    .then_some(ExitReason::Stall)
            })
        })
        .or_else(|| {
            // E2: cut once held past the deadline. Lowest priority.
            params.time_stop_secs.and_then(|secs| {
                time_stop_triggered(entry_time, block_time, secs).then_some(ExitReason::TimeStop)
            })
        })
}

// ── Shared time-feature predicates ───────────────────────────────────────────
// Each takes the evaluation instant `at`, so the trade walk passes the trade's
// block_time and the clock sweep passes `now` — one definition, two callers.

/// E3 · Stall: at least `stall_secs` have elapsed since the last new higher-high.
fn stall_triggered(last_higher_high_time: DateTime<Utc>, at: DateTime<Utc>, stall_secs: u64) -> bool {
    (at - last_higher_high_time).num_seconds() >= stall_secs as i64
}

/// E2 · TimeStop: the position has been held past `time_stop_secs` from entry.
fn time_stop_triggered(entry_time: DateTime<Utc>, at: DateTime<Utc>, time_stop_secs: u64) -> bool {
    at >= entry_time + Duration::seconds(time_stop_secs as i64)
}

/// Walk a position's post-entry trades chronologically and return the first exit
/// the ladder fires, or `None` if the position is still open. Trades are assumed
/// slot/time-sorted upstream.
pub fn find_trade_driven_exit<T: TradeRow>(
    trades: &[T],
    entry_time: DateTime<Utc>,
    entry_price: f64,
    rule: &Tpsl2Rule,
) -> Option<ExitFill> {
    if entry_price <= 0.0 {
        return None;
    }
    let params = LadderParams::from_rule(rule);
    // Analysis path (backtest / sweep / detect): when the ladder fires but its
    // worst-case fill window holds no trade, fill at the **firing trade itself** —
    // a market exit modelling the bot's own sell — instead of dropping the exit.
    // Without this, a token whose price provably crossed TP/SL right at a sparse,
    // gappy stretch (classically the curve blow-off just before migration, where
    // the next curve trade is > MAX_FILL_WAIT_SLOTS away and the post-migration AMM
    // fills are absent from the corpus) was mislabeled `Open`. The live paper poll
    // (`find_trade_driven_exit_with_slot`) deliberately keeps the strict window: an
    // empty window there means the fill has not indexed yet, so the poll waits it
    // out (booking a failed exit on timeout) rather than inventing a fill.
    run_exit_walk(trades, entry_time, entry_price, &params, true)
        .map(|(fill, _)| fill)
        // Death-close fallback: the ladder never fired but the token is provably dead
        // (liquidity gone + silent) → close the bag at the last meaningful trade rather
        // than leave it `Open` marked to a stale price. Analysis-only; live closes
        // silent tokens via its clock sweep. See `strategies::death`.
        .or_else(|| {
            crate::strategies::death::find_death_point(trades, entry_time, Utc::now()).map(|d| {
                ExitFill {
                    price: d.price,
                    tx_signature: d.tx_signature,
                    slot: d.slot,
                    block_time: d.block_time,
                    reason: ExitReason::Dead,
                }
            })
        })
}

/// [`find_trade_driven_exit`] that also returns the **firing slot** `S` — the slot
/// of the trade the ladder fired on (distinct from the recorded fill's slot, which
/// may be in a later slot). The live paper poll uses this to know when the fill
/// window has fully indexed before recording the fill; the plain wrapper above
/// drops it, so backtest/sweep behavior is unchanged.
pub fn find_trade_driven_exit_with_slot<T: TradeRow>(
    trades: &[T],
    entry_time: DateTime<Utc>,
    entry_price: f64,
    rule: &Tpsl2Rule,
) -> Option<(ExitFill, u64)> {
    if entry_price <= 0.0 {
        return None;
    }
    let params = LadderParams::from_rule(rule);
    // Live paper poll: NO market fill — the empty-window case must stay `None` so the
    // poll keeps waiting for the real fill to index (see `find_trade_driven_exit`).
    run_exit_walk(trades, entry_time, entry_price, &params, false)
}

/// The shared post-entry walk behind [`find_trade_driven_exit`].
///
/// `market_fill_on_empty_window` chooses what happens when the ladder fires but the
/// worst-case fill window `{S, next_slot}` contains no trade: `true` (analysis) fills
/// at the firing trade itself (a market exit), `false` (live paper poll) returns
/// `None` so the caller waits the fill out. In every non-empty-window case the two
/// are byte-identical.
fn run_exit_walk<T: TradeRow>(
    trades: &[T],
    entry_time: DateTime<Utc>,
    entry_price: f64,
    params: &LadderParams,
    market_fill_on_empty_window: bool,
) -> Option<(ExitFill, u64)> {
    let mut state = ExitWalkState::starting_at(entry_price, entry_time);

    // Single pass over the post-entry trades. The first trade where the ladder fires
    // decides the exit `reason` and the firing slot `S`. The fill window is:
    // slot S (always) + the next observed slot after S if within MAX_FILL_WAIT_SLOTS.
    // Only trades that appear after the firing tx are candidates (the loop naturally
    // handles this — after firing, subsequent iterations are past the fire point).
    // `fill_min` accumulates the lowest price in the window. If the window is empty
    // the exit is not taken (returns None).
    let mut cur_slot: Option<u64> = None;
    let mut fill_min: Option<&T> = None;
    let mut pending: Option<ExitReason> = None;
    let mut fire_slot: u64 = 0;
    let mut fired_at: Option<&T> = None; // the firing trade — market-fill fallback
    let mut next_slot: Option<u64> = None; // first slot > fire_slot seen after firing

    let is_lower = |t: &T, cur: Option<&T>| {
        cur.is_none_or(|m| {
            t.price_per_token()
                .partial_cmp(&m.price_per_token())
                .unwrap_or(std::cmp::Ordering::Equal)
                .is_lt()
        })
    };

    for t in trades.iter().filter(|t| t.block_time() > entry_time) {
        let slot = t.slot();

        // Already fired: accumulate lowest price in window {fire_slot, next_slot}.
        // next_slot = first slot > fire_slot seen; included only if within MAX_FILL_WAIT_SLOTS.
        // A trade past the window finalizes; None fill_min means no fill → not taken.
        if let Some(reason) = pending {
            // Discover next_slot lazily on the first trade past fire_slot.
            if slot > fire_slot && next_slot.is_none() {
                next_slot = Some(slot);
            }

            let window_closed = match next_slot {
                // next_slot too far → window is fire_slot only; anything after closes it
                Some(ns) if ns > fire_slot + MAX_FILL_WAIT_SLOTS => slot > fire_slot,
                // next_slot close → window = {fire_slot, next_slot}; slot beyond next closes it
                Some(ns) => slot > ns,
                // still on fire_slot, window not yet closed
                None => false,
            };

            if window_closed {
                // Empty window on the analysis path falls back to the firing trade
                // (market exit); the live poll passes `false` and keeps it `None`.
                let et = fill_min.or(if market_fill_on_empty_window { fired_at } else { None });
                return et.map(|et| (
                    ExitFill {
                        price: et.price_per_token(),
                        tx_signature: et.tx_signature().to_string(),
                        slot: et.slot(),
                        block_time: et.block_time(),
                        reason,
                    },
                    fire_slot,
                ));
            }
            if is_lower(t, fill_min) {
                fill_min = Some(t);
            }
            continue;
        }

        // Not yet fired: reset the running min at each new slot.
        if cur_slot != Some(slot) {
            cur_slot = Some(slot);
            fill_min = None;
        }
        if is_lower(t, fill_min) {
            fill_min = Some(t);
        }

        state.update_with_trade(t);

        // First feature that fires on this trade wins (ladder order). Shared with
        // the incremental gate via `ladder_reason` so they can never drift.
        if let Some(reason) = ladder_reason(&state, t, entry_time, entry_price, params) {
            pending = Some(reason);
            fire_slot = slot;
            fired_at = Some(t); // capture for the empty-window market-fill fallback
            fill_min = None; // reset; window starts fresh after firing tx
            next_slot = None;
        }
    }

    // History ended while a pending exit's window was still open. The analysis path
    // then market-fills at the firing trade; the live poll (fallback `None`) leaves
    // the exit un-taken so it can keep waiting for the fill to index.
    pending.and_then(|reason| {
        let et = fill_min.or(if market_fill_on_empty_window { fired_at } else { None });
        et.map(|et| (
            ExitFill {
                price: et.price_per_token(),
                tx_signature: et.tx_signature().to_string(),
                slot: et.slot(),
                block_time: et.block_time(),
                reason,
            },
            fire_slot,
        ))
    })
}

/// The time-based exits (E2 TimeStop / E3 Stall) measured against wall-clock
/// `now` rather than a trade timestamp, so they fire while a token is silent.
/// Covers ONLY the two clock features — price-based exits can't change between
/// trades and stay on [`find_trade_driven_exit`]. Stall outranks TimeStop,
/// matching the trade-walk ladder order.
pub fn find_clock_driven_exit(
    state: &ExitWalkState,
    entry_time: DateTime<Utc>,
    params: &LadderParams,
    now: DateTime<Utc>,
) -> Option<ExitReason> {
    if let Some(secs) = params.stall_secs {
        if stall_triggered(state.last_higher_high_time, now, secs) {
            return Some(ExitReason::Stall);
        }
    }
    if let Some(secs) = params.time_stop_secs {
        if time_stop_triggered(entry_time, now, secs) {
            return Some(ExitReason::TimeStop);
        }
    }
    None
}

/// Live **trade-driven** gate: should this Holding position exit given the
/// latest in-memory trade history? Applies the holding/entry guards, then runs
/// the full ladder and returns just the reason (the live path resolves the
/// actual fill separately, against freshly indexed trades).
///
/// Retained as a full-walk reference for the gate tests; the live trade gate now
/// runs incrementally through `runtime_cache::exit_state_advance_and_find_exit`
/// (decision-equivalent, no per-ping full re-walk).
#[cfg_attr(not(test), allow(dead_code))]
pub fn should_position_exit_on_trade(
    position: &Position,
    trades: &[Trade],
    rule: &Tpsl2Rule,
) -> Option<ExitReason> {
    let entry_time = clock_entry_time(position)?;
    find_trade_driven_exit(trades, entry_time, position.entry_price.unwrap_or(0.0), rule).map(|fill| fill.reason)
}

/// Live **clock-driven** gate: should this Holding position exit on the
/// wall-clock sweep? Cheap-exits when the rule has no time features, else checks
/// the deadlines against `now`. `state` is the position's memoized walk state
/// ([`CachedExitState`]), kept current by the trade path — the sweep no longer
/// rebuilds it from history per tick.
#[cfg_attr(not(test), allow(dead_code))]
pub fn should_position_exit_on_clock(
    position: &Position,
    state: &ExitWalkState,
    params: &LadderParams,
    now: DateTime<Utc>,
) -> Option<ExitReason> {
    let entry_time = clock_entry_time(position)?;
    if params.time_stop_secs.is_none() && params.stall_secs.is_none() {
        return None;
    }
    find_clock_driven_exit(state, entry_time, params, now)
}

/// Shared guard for both live gates: a position is evaluable only while Holding
/// with a recorded entry. A 0 entry price means the fill isn't indexed yet —
/// evaluating it would divide by ~0 and fire a phantom TakeProfit, flapping the
/// position ExitPending→Holding. Returns the entry time once those hold.
pub fn clock_entry_time(position: &Position) -> Option<DateTime<Utc>> {
    if (position.status != PositionStatus::Holding
        && position.status != PositionStatus::Arming
        && position.status != PositionStatus::BuySubmitted)
        || position.entry_price.is_none()
    {
        return None;
    }
    position.entry_time
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::trade::{Trade, TradeType};
    use uuid::Uuid;

    fn base_time() -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap()
    }

    /// A buy whose `price_per_token` equals `price` (token_amount = 1.0).
    fn buy(price: f64, slot: u64, secs: i64) -> Trade {
        Trade::new(
            "mint".into(),
            "wallet".into(),
            TradeType::Buy,
            price, // amount_sol
            1,   // token_amount → price_per_token = price
            format!("sig-{slot}-{secs}"),
            slot,
            base_time() + Duration::seconds(secs),
        )
    }

    /// A buy carrying explicit **real** SOL reserves (price still equals `price`).
    /// E4 reads real reserves now (never virtual — wash trading can fake virtual).
    fn buy_resv(price: f64, slot: u64, secs: i64, reserves: f64) -> Trade {
        let mut t = buy(price, slot, secs);
        t.real_reserve_sol = Some(reserves);
        t
    }

    /// Minimal rule with explicit TP/SL + optional E1/E2/E3/E4; else inert.
    fn rule_with(
        take_profit: f64,
        stop_loss: f64,
        trailing: Option<f64>,
        time_stop_secs: Option<u64>,
        stall_secs: Option<u64>,
        liquidity_drop_pct: Option<f64>,
    ) -> Tpsl2Rule {
        Tpsl2Rule::new(
            "test".into(),
            None,
            None,
            None,
            serde_json::Value::Array(vec![]),
            "paper".into(),
            1.0, // buy_amount_sol
            take_profit,
            stop_loss,
            None,
            None,
            None,
            None,
            Some(0.0),
            trailing,
            time_stop_secs,
            stall_secs,
            liquidity_drop_pct,
        )
    }

    /// A Holding position entered at `entry_price` at `base_time()`.
    fn holding(entry_price: f64, rule_id: Uuid) -> Position {
        let mut p = Position::new("mint".into(), "wallet".into(), "TPSL2".into(), rule_id);
        p.entry_price = Some(entry_price);
        p.entry_tx_signatures = vec!["entry-sig".into()];
        p.entry_token_amount = Some(1);
        p.entry_time = Some(base_time());
        p
    }

    // ── Trade-driven ladder (`find_trade_driven_exit`) ───────────────────────

    // Each series includes a fill-slot trade (firing_slot + 1) so the new
    // [F+1, F+3] window has at least one trade to fill against.

    fn moonshot_then_reversal() -> Vec<Trade> {
        vec![
            buy(2.0, 2, 1), // +100%
            buy(3.0, 3, 2), // +200% (peak)
            buy(2.5, 4, 3), // -16.7% off peak
            buy(2.0, 5, 4), // <= 2.1 → trailing fires here (slot F=5)
            buy(2.0, 6, 5), // slot F+1 — fill window
        ]
    }

    #[test]
    fn trailing_stop_fires_on_reversal() {
        let trades = moonshot_then_reversal();
        let rule = rule_with(1000.0, 90.0, Some(30.0), None, None, None);

        let exit = find_trade_driven_exit(&trades, base_time(), 1.0, &rule)
            .expect("trailing stop should trigger an exit, not stay Open");

        assert_eq!(exit.reason, ExitReason::TrailingStop);
        // Fill is in slot F+1 (slot 6), price 2.0.
        assert!((exit.price - 2.0).abs() < 1e-9, "expected fill at 2.0, got {}", exit.price);
        assert_eq!(exit.block_time, base_time() + Duration::seconds(5));
    }

    #[test]
    fn disabled_trailing_leaves_position_open() {
        let trades = moonshot_then_reversal();
        let rule = rule_with(1000.0, 90.0, None, None, None, None);
        assert!(find_trade_driven_exit(&trades, base_time(), 1.0, &rule).is_none());

        // Zero behaves identically to None (ignore_zero convention).
        let rule_zero = rule_with(1000.0, 90.0, Some(0.0), Some(0), Some(0), Some(0.0));
        assert!(find_trade_driven_exit(&trades, base_time(), 1.0, &rule_zero).is_none());
    }

    #[test]
    fn stop_loss_takes_priority_over_trailing() {
        // SL fires at slot 3; fill at slot 4 (F+1).
        let trades = vec![buy(3.0, 2, 1), buy(0.05, 3, 2), buy(0.05, 4, 3)];
        let rule = rule_with(1000.0, 90.0, Some(30.0), None, None, None);
        let exit = find_trade_driven_exit(&trades, base_time(), 1.0, &rule).expect("should exit");
        assert_eq!(exit.reason, ExitReason::StopLoss);
    }

    #[test]
    fn exit_fill_window_is_fire_slot_and_next_slot() {
        // SL fires at slot 3 (price 0.7, −30%).
        // Window = {slot 3 (no post-fire trades), slot 4 (next_slot, ≤ 3 + MAX_FILL_WAIT_SLOTS)}.
        // Slots 5 and 6 are beyond next_slot → excluded. Fill = min in window = 0.6.
        let trades = vec![buy(0.7, 3, 10), buy(0.6, 4, 11), buy(0.9, 5, 12), buy(0.5, 6, 13)];
        let rule = rule_with(1000.0, 20.0, None, None, None, None);
        let exit = find_trade_driven_exit(&trades, base_time(), 1.0, &rule).expect("should exit");
        assert_eq!(exit.reason, ExitReason::StopLoss);
        // Window = {slot 3 (empty after fire), slot 4}; min = 0.6.
        assert!((exit.price - 0.6).abs() < 1e-9, "fill in {{F, F+1}}, got {}", exit.price);
        assert_eq!(exit.block_time, base_time() + Duration::seconds(11));
        // Fire slot is still S = 3 (the live poll waits MAX_FILL_WAIT_SLOTS from there).
        let (_, fire_slot) =
            find_trade_driven_exit_with_slot(&trades, base_time(), 1.0, &rule).unwrap();
        assert_eq!(fire_slot, 3);
    }

    #[test]
    fn exit_market_fills_at_firing_trade_when_window_empty() {
        // SL fires at slot 3 but no subsequent trades → the strict fill window is
        // empty. The ANALYSIS path (`find_trade_driven_exit`) market-fills at the
        // firing trade itself so a provably-crossed exit isn't mislabeled `Open`
        // (the migrated-token / sparse-blow-off case).
        let trades = vec![buy(0.7, 3, 10)];
        let rule = rule_with(1000.0, 20.0, None, None, None, None);
        let exit = find_trade_driven_exit(&trades, base_time(), 1.0, &rule)
            .expect("analysis path market-fills the fired exit at the firing trade");
        assert_eq!(exit.reason, ExitReason::StopLoss);
        assert!((exit.price - 0.7).abs() < 1e-9, "fills at the firing trade, got {}", exit.price);

        // The LIVE paper resolver keeps the strict empty-window semantics: `None`, so
        // the fill-poll waits the real fill out (and books a failed exit on timeout)
        // rather than inventing a fill.
        assert!(find_trade_driven_exit_with_slot(&trades, base_time(), 1.0, &rule).is_none());
    }

    fn flat_series() -> Vec<Trade> {
        // time-stop fires at slot 4 (+30s); slot 5 (+40s) is the fill slot.
        vec![buy(1.0, 2, 10), buy(1.0, 3, 20), buy(1.0, 4, 30), buy(1.0, 5, 40)]
    }

    #[test]
    fn time_stop_fires_at_deadline_trade() {
        // time_stop=25s fires on the trade at +30s (slot 4). Fill at slot 5 (+40s).
        let trades = flat_series();
        let rule = rule_with(1000.0, 90.0, None, Some(25), None, None);
        let exit = find_trade_driven_exit(&trades, base_time(), 1.0, &rule).expect("should exit");
        assert_eq!(exit.reason, ExitReason::TimeStop);
        assert_eq!(exit.block_time, base_time() + Duration::seconds(40));
    }

    #[test]
    fn price_exit_preempts_later_time_stop() {
        // SL fires at slot 2 (+10s). Fill at slot 3 (+30s, in [F+1, F+3]).
        let trades = vec![buy(0.5, 2, 10), buy(0.5, 3, 30)];
        let rule = rule_with(1000.0, 20.0, None, Some(25), None, None);
        let exit = find_trade_driven_exit(&trades, base_time(), 1.0, &rule).expect("should exit");
        assert_eq!(exit.reason, ExitReason::StopLoss);
        assert_eq!(exit.block_time, base_time() + Duration::seconds(30));
    }

    fn peak_then_stall() -> Vec<Trade> {
        // Stall fires at slot 5 (+40s); slot 6 (+50s) is the fill slot.
        vec![buy(2.0, 2, 10), buy(2.0, 3, 20), buy(2.0, 4, 30), buy(2.0, 5, 40), buy(2.0, 6, 50)]
    }

    #[test]
    fn stall_fires_after_flatline() {
        // stall_secs=25; last HH at +10s. Fires at +40s (slot 5). Fill at slot 6.
        let trades = peak_then_stall();
        let rule = rule_with(1000.0, 90.0, None, None, Some(25), None);
        let exit = find_trade_driven_exit(&trades, base_time(), 1.0, &rule).expect("should exit");
        assert_eq!(exit.reason, ExitReason::Stall);
        assert_eq!(exit.block_time, base_time() + Duration::seconds(50));
    }

    #[test]
    fn steady_new_highs_do_not_stall() {
        let trades = vec![buy(2.0, 2, 10), buy(3.0, 3, 20), buy(4.0, 4, 30), buy(5.0, 5, 40)];
        let rule = rule_with(1000.0, 90.0, None, None, Some(15), None);
        assert!(find_trade_driven_exit(&trades, base_time(), 1.0, &rule).is_none());
    }

    #[test]
    fn trailing_outranks_stall_on_same_trade() {
        // Trailing fires at slot 5 (1.3 ≤ 2.0*0.7=1.4). Fill at slot 6.
        let trades = vec![
            buy(2.0, 2, 10), buy(1.5, 3, 20), buy(1.45, 4, 30), buy(1.3, 5, 40),
            buy(1.3, 6, 50), // fill slot
        ];
        let rule = rule_with(1000.0, 90.0, Some(30.0), None, Some(25), None);
        let exit = find_trade_driven_exit(&trades, base_time(), 1.0, &rule).expect("should exit");
        assert_eq!(exit.reason, ExitReason::TrailingStop);
        assert_eq!(exit.block_time, base_time() + Duration::seconds(50));
    }

    fn rising_then_reserve_crash() -> Vec<Trade> {
        // Liquidity fires at slot 5; slot 6 is the fill slot.
        vec![
            buy_resv(1.0, 2, 10, 100.0),
            buy_resv(1.0, 3, 20, 120.0),
            buy_resv(1.0, 4, 30, 130.0),
            buy_resv(1.0, 5, 40, 50.0),
            buy_resv(1.0, 6, 50, 45.0), // fill slot
        ]
    }

    #[test]
    fn liquidity_exit_fires_on_reserve_crash() {
        let trades = rising_then_reserve_crash();
        let rule = rule_with(1000.0, 90.0, None, None, None, Some(50.0));
        let exit = find_trade_driven_exit(&trades, base_time(), 1.0, &rule).expect("should exit");
        assert_eq!(exit.reason, ExitReason::LiquidityExit);
        assert_eq!(exit.block_time, base_time() + Duration::seconds(50));
    }

    #[test]
    fn liquidity_exit_outranks_stop_loss() {
        // Liquidity fires at slot 3; fill at slot 4.
        let trades = vec![
            buy_resv(1.0, 2, 10, 100.0),
            buy_resv(0.05, 3, 20, 40.0),
            buy_resv(0.05, 4, 30, 35.0), // fill slot
        ];
        let rule = rule_with(1000.0, 90.0, None, None, None, Some(50.0));
        let exit = find_trade_driven_exit(&trades, base_time(), 1.0, &rule).expect("should exit");
        assert_eq!(exit.reason, ExitReason::LiquidityExit);
    }

    // ── Live trade gate (`should_position_exit_on_trade`) ────────────────────

    #[test]
    fn live_gate_fires_trailing_stop() {
        let rule = rule_with(1000.0, 90.0, Some(30.0), None, None, None);
        let pos = holding(1.0, rule.id);
        // Trailing fires at slot 4 (price 2.0 ≤ 3.0*0.7=2.1). Fill at slot 5.
        let trades = vec![buy(2.0, 2, 1), buy(3.0, 3, 2), buy(2.0, 4, 3), buy(2.0, 5, 4)];
        assert_eq!(
            should_position_exit_on_trade(&pos, &trades, &rule),
            Some(ExitReason::TrailingStop)
        );
    }

    #[test]
    fn live_gate_waits_for_recorded_entry() {
        let rule = rule_with(50.0, 20.0, None, None, None, None);
        let mut pos = holding(1.0, rule.id);
        pos.entry_time = None;
        let trades = vec![buy(0.1, 2, 1)];
        assert_eq!(should_position_exit_on_trade(&pos, &trades, &rule), None);
    }

    #[test]
    fn live_gate_skips_non_holding() {
        let rule = rule_with(50.0, 20.0, None, None, None, None);
        let mut pos = holding(1.0, rule.id);
        pos.status = PositionStatus::ExitPending;
        let trades = vec![buy(0.1, 2, 1)];
        assert_eq!(should_position_exit_on_trade(&pos, &trades, &rule), None);
    }

    // ── Live clock gate (`should_position_exit_on_clock`) ────────────────────

    /// Build the memoized walk state the live sweep would feed the clock gate.
    fn walk(pos: &Position, trades: &[Trade]) -> ExitWalkState {
        ExitWalkState::rebuild_from_trades(
            trades,
            pos.entry_price.unwrap_or(0.0),
            pos.entry_time.unwrap_or_else(base_time),
        )
    }

    #[test]
    fn clock_gate_fires_time_stop_after_silence() {
        let rule = rule_with(1000.0, 90.0, None, Some(300), None, None);
        let pos = holding(1.0, rule.id);
        let trades = vec![buy(1.0, 2, 30)]; // one trade then silence
        let now = base_time() + Duration::seconds(600);
        assert_eq!(
            should_position_exit_on_clock(&pos, &walk(&pos, &trades), &LadderParams::from_rule(&rule), now),
            Some(ExitReason::TimeStop)
        );
    }

    #[test]
    fn clock_gate_fires_stall_during_silence() {
        let rule = rule_with(1000.0, 90.0, None, None, Some(60), None);
        let pos = holding(1.0, rule.id);
        let trades = vec![buy(2.0, 2, 10)]; // new high at +10s, then quiet
        let now = base_time() + Duration::seconds(300);
        assert_eq!(
            should_position_exit_on_clock(&pos, &walk(&pos, &trades), &LadderParams::from_rule(&rule), now),
            Some(ExitReason::Stall)
        );
    }

    #[test]
    fn clock_gate_stall_outranks_time_stop() {
        let rule = rule_with(1000.0, 90.0, None, Some(100), Some(60), None);
        let pos = holding(1.0, rule.id);
        let trades = vec![buy(2.0, 2, 10)];
        let now = base_time() + Duration::seconds(300);
        assert_eq!(
            should_position_exit_on_clock(&pos, &walk(&pos, &trades), &LadderParams::from_rule(&rule), now),
            Some(ExitReason::Stall)
        );
    }

    #[test]
    fn clock_gate_inert_before_deadline() {
        let rule = rule_with(1000.0, 90.0, None, Some(300), Some(300), None);
        let pos = holding(1.0, rule.id);
        let trades = vec![buy(1.0, 2, 30)];
        let now = base_time() + Duration::seconds(100);
        assert_eq!(
            should_position_exit_on_clock(&pos, &walk(&pos, &trades), &LadderParams::from_rule(&rule), now),
            None
        );
    }

    #[test]
    fn clock_gate_inert_when_unconfigured() {
        let rule = rule_with(50.0, 20.0, Some(30.0), None, None, Some(50.0));
        let pos = holding(1.0, rule.id);
        let trades = vec![buy(1.0, 2, 30)];
        let now = base_time() + Duration::seconds(86_400);
        assert_eq!(
            should_position_exit_on_clock(&pos, &walk(&pos, &trades), &LadderParams::from_rule(&rule), now),
            None
        );
    }

    #[test]
    fn clock_gate_waits_for_recorded_entry() {
        let rule = rule_with(1000.0, 90.0, None, Some(60), None, None);
        let mut pos = holding(1.0, rule.id);
        pos.entry_time = None;
        let now = base_time() + Duration::seconds(600);
        assert_eq!(
            should_position_exit_on_clock(&pos, &walk(&pos, &[]), &LadderParams::from_rule(&rule), now),
            None
        );
    }

    // ── Incremental memoization (`CachedExitState`) ──────────────────────────

    #[test]
    fn cached_state_advance_matches_full_rebuild() {
        let entry = base_time();
        let all = vec![buy(2.0, 2, 10), buy(3.0, 3, 20), buy(2.5, 4, 30), buy(4.0, 5, 40)];

        // Incremental: seed from the first two trades, then fold the rest in two
        // steps the way the live trade path would as the cache vec grows. No
        // trimming here, so `base` stays 0 throughout.
        let mut cached = CachedExitState::build(&all[..2], 0, 1.0, entry);
        cached.advance(&all[..3], 0);
        cached.advance(&all, 0);

        let full = ExitWalkState::rebuild_from_trades(&all, 1.0, entry);
        assert!((cached.state.peak_price - full.peak_price).abs() < 1e-9);
        assert_eq!(cached.state.last_higher_high_time, full.last_higher_high_time);
    }

    #[test]
    fn cached_state_survives_front_trim() {
        // The peak (9.0) prints in an EARLY trade that is later trimmed out of the
        // retained window. Because it was folded before the trim, the memo must
        // keep it — and no in-between trade may be skipped as the window slides.
        let entry = base_time();
        let logical = vec![
            buy(2.0, 2, 10),
            buy(9.0, 3, 20), // peak — will be trimmed away below
            buy(3.0, 4, 30),
            buy(5.0, 5, 40),
            buy(4.0, 6, 50),
            buy(8.0, 7, 60),
        ];

        // Seed from the first window [0,2); base 0.
        let mut cached = CachedExitState::build(&logical[0..2], 0, 1.0, entry);
        // Window slides: 1 trimmed, window is logical[1..4]; folds absolute [2,4).
        cached.advance(&logical[1..4], 1);
        // Window slides again: 3 trimmed, window is logical[3..6]; folds [4,6).
        cached.advance(&logical[3..6], 3);

        let full = ExitWalkState::rebuild_from_trades(&logical, 1.0, entry);
        assert!((cached.state.peak_price - 9.0).abs() < 1e-9);
        assert!((cached.state.peak_price - full.peak_price).abs() < 1e-9);
        assert_eq!(cached.state.last_higher_high_time, full.last_higher_high_time);
    }

    #[test]
    fn cached_state_rebuilds_when_history_shrinks() {
        let entry = base_time();
        let all = vec![buy(2.0, 2, 10), buy(5.0, 3, 20)];
        let mut cached = CachedExitState::build(&all, 0, 1.0, entry);
        assert!((cached.state.peak_price - 5.0).abs() < 1e-9);

        // Token evicted + re-tracked from empty (base back to 0, short vec): the
        // stale cursor lands past the window end, so the state rebuilds and the
        // stale peak must not survive.
        let reset = vec![buy(2.0, 9, 100)];
        cached.advance(&reset, 0);
        assert!((cached.state.peak_price - 2.0).abs() < 1e-9);
    }
}
