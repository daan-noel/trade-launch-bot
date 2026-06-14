//! Exit ladder — the single place that decides **when a held position exits**.
//!
//! Two triggers share one set of feature predicates so they can never drift:
//!   • trade-driven ([`find_trade_driven_exit`]) — re-evaluated when a trade prints.
//!   • clock-driven ([`find_clock_driven_exit`]) — re-evaluated on a wall-clock
//!     timer, so deadline exits (Stall / TimeStop) fire even while a token is
//!     silent and no trade is arriving.
//!
//! Ladder priority (first match on a trade wins):
//!   CohortExit → LiquidityExit → StopLoss → TakeProfit → TrailingStop → Stall → TimeStop.
//!
//! Every feature is inert by default: with all `p_*` exit params unset/zero the
//! trade walk reproduces the legacy fixed TP/SL behavior exactly.
//!
//! **To add an exit feature:** read its rule param near the top of
//! [`find_trade_driven_exit`], add one `.or_else(...)` arm at the right priority
//! in the ladder, and — if it is time-based — mirror it in
//! [`find_clock_driven_exit`] using a shared `*_triggered` predicate.

use chrono::{DateTime, Duration, Utc};

use crate::config::constants::RUGGED_EARLY_SLOT_WINDOW;
use crate::models::trade::{Trade, TradeType};
use crate::models::{Position, PositionStatus, Tpsl2Rule};

use super::cohort::{cohort_flow, early_cohort_wallets};
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
    /// E5 · the launch cohort's net holdings collapsed to ≤ `p_exit_cohort_ratio`
    /// of everything it bought (the multi-wallet rug dump). Top ladder priority.
    CohortExit,
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
            Self::CohortExit => "CohortExit",
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
    pub fn update_with_trade(&mut self, trade: &Trade) {
        if trade.price_per_token > self.peak_price {
            self.peak_price = trade.price_per_token;
            self.last_higher_high_time = trade.block_time;
        }
        if let Some(reserves) = trade.real_sol_reserves {
            if reserves > self.peak_reserves {
                self.peak_reserves = reserves;
            }
        }
    }

    /// Replay the full post-entry history into a fresh state — the peaks as of
    /// the last trade. Used to seed [`CachedExitState`] the first time a position
    /// is swept; afterwards the state is advanced incrementally, never rebuilt.
    pub fn rebuild_from_trades(
        trades: &[Trade],
        entry_price: f64,
        entry_time: DateTime<Utc>,
    ) -> Self {
        let mut state = Self::starting_at(entry_price, entry_time);
        for t in trades.iter().filter(|t| t.block_time > entry_time) {
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
#[derive(Debug, Clone)]
pub struct CachedExitState {
    pub state: ExitWalkState,
    entry_time: DateTime<Utc>,
    entry_price: f64,
    consumed_abs: u64,
    /// E5 launch-cohort memo (H4). `Some` once E5 is configured for the position;
    /// the cohort set + bag (denominator) are fixed at seed time and only
    /// `net` advances incrementally as cohort trades replay — replacing the
    /// per-ping HashSet rebuild + three full passes.
    cohort: Option<CohortMemo>,
}

/// Memoized E5 launch cohort: the wallet set + everything it ever bought (the
/// dump-ratio denominator), both fixed once the early-slot window has closed, plus
/// the cohort's running net holdings advanced trade-by-trade. Computed once at the
/// position's first sight (mirroring the peak memo); never rebuilt per ping.
#[derive(Debug, Clone)]
struct CohortMemo {
    wallets: std::collections::HashSet<String>,
    bought: f64,
    /// Net cohort holdings, current as of `consumed_abs`. Seeded to the net as of
    /// `entry_time`, then evolved by each post-entry cohort trade.
    net: f64,
}

impl CachedExitState {
    /// Seed from the retained post-entry history (one-time, at first sight of the
    /// position) and record the absolute fold cursor. `base` is the token's
    /// `trades_base` (count already trimmed from the front). No cohort memo — used
    /// by the clock sweep, which never needs E5.
    pub fn build(trades: &[Trade], base: u64, entry_price: f64, entry_time: DateTime<Utc>) -> Self {
        Self {
            state: ExitWalkState::rebuild_from_trades(trades, entry_price, entry_time),
            entry_time,
            entry_price,
            consumed_abs: base + trades.len() as u64,
            cohort: None,
        }
    }

    /// An empty (unfolded) state whose absolute cursor sits at the window front
    /// (`base`), so a following [`advance_and_find_exit`](Self::advance_and_find_exit)
    /// folds the entire retained window. `params` decides whether the E5 cohort
    /// memo is seeded (computed once here from the retained window). Used to seed
    /// the trade gate so its first pass reproduces a full re-walk while memoizing
    /// for subsequent incremental pings.
    pub fn build_unfolded(
        trades: &[Trade],
        base: u64,
        entry_price: f64,
        entry_time: DateTime<Utc>,
        params: &LadderParams,
    ) -> Self {
        let cohort = params.cohort_exit_ratio.map(|_| {
            // Fixed cohort + bag, computed once (H4). `net` seeds to the cohort's
            // net holdings as of entry; the post-entry walk evolves it from there.
            let wallets = early_cohort_wallets(trades, RUGGED_EARLY_SLOT_WINDOW);
            let bought = cohort_flow(trades, &wallets).bought_tokens;
            let net: f64 = trades
                .iter()
                .filter(|t| t.block_time <= entry_time && wallets.contains(&t.wallet_address))
                .map(signed_tokens)
                .sum();
            CohortMemo { wallets, bought, net }
        });
        Self {
            state: ExitWalkState::starting_at(entry_price, entry_time),
            entry_time,
            entry_price,
            consumed_abs: base,
            cohort,
        }
    }

    /// Lazily attach the E5 cohort memo if E5 is configured but the state was
    /// seeded without one (e.g. the clock sweep seeded it first via [`build`](Self::build)).
    /// `net` is seeded to the cohort's net holdings as of the current fold cursor
    /// (`consumed_abs`), so a following incremental advance continues correctly.
    /// No-op once a cohort memo is present or when E5 is off.
    pub fn ensure_cohort_seeded(&mut self, trades: &[Trade], base: u64, params: &LadderParams) {
        if self.cohort.is_some() || params.cohort_exit_ratio.is_none() {
            return;
        }
        // Window index of the fold cursor; clamp in case of an over-trim/reset.
        let cursor = self.consumed_abs.saturating_sub(base).min(trades.len() as u64) as usize;
        let wallets = early_cohort_wallets(trades, RUGGED_EARLY_SLOT_WINDOW);
        let bought = cohort_flow(trades, &wallets).bought_tokens;
        let net: f64 = trades[..cursor]
            .iter()
            .filter(|t| wallets.contains(&t.wallet_address))
            .map(signed_tokens)
            .sum();
        self.cohort = Some(CohortMemo { wallets, bought, net });
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
    pub fn advance(&mut self, trades: &[Trade], base: u64) {
        let start = self.consumed_abs.saturating_sub(base);
        if start > trades.len() as u64 {
            self.state =
                ExitWalkState::rebuild_from_trades(trades, self.entry_price, self.entry_time);
            self.consumed_abs = base + trades.len() as u64;
            return;
        }
        for t in &trades[start as usize..] {
            if t.block_time > self.entry_time {
                self.state.update_with_trade(t);
            }
        }
        self.consumed_abs = base + trades.len() as u64;
    }

    /// Incremental trade-gate: fold the genuinely-new trades into the running
    /// peaks (and the E5 cohort net) **and**, in the same pass, evaluate the exit
    /// ladder against each newly-folded post-entry trade, returning the first
    /// [`ExitReason`] that fires.
    ///
    /// Decision-equivalent to a full [`find_trade_driven_exit`] re-walk: peaks and
    /// the cohort net are accumulated by the identical fold, the ladder runs
    /// against the state as of each trade via the shared [`ladder_reason`]
    /// predicate, and only new trades can newly fire — an old trade that fired
    /// would already have exited the position. The rebuild branch (history
    /// reset/over-trimmed) re-walks the whole window from the seeded cohort net;
    /// old trades there are idempotent, so the first firing trade is unchanged.
    pub fn advance_and_find_exit(
        &mut self,
        trades: &[Trade],
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
            // The cohort memo (set + bag + entry-net) is fixed, so the cohort net
            // is re-derived by re-walking post-entry cohort trades from its seed.
            self.state = ExitWalkState::starting_at(self.entry_price, self.entry_time);
        }
        let from = if rebuild { 0 } else { start as usize };
        self.consumed_abs = base + trades.len() as u64;
        // On a rebuild the running cohort net is reset to the entry-net seed so the
        // re-walk re-derives it; re-seed it from the post-entry trades below.
        if rebuild {
            if let Some(c) = self.cohort.as_mut() {
                c.net = trades
                    .iter()
                    .filter(|t| t.block_time <= self.entry_time && c.wallets.contains(&t.wallet_address))
                    .map(signed_tokens)
                    .sum();
            }
        }

        let mut fired = None;
        for t in &trades[from..] {
            if t.block_time <= self.entry_time {
                continue;
            }
            self.state.update_with_trade(t);
            if let Some(c) = self.cohort.as_mut() {
                if c.wallets.contains(&t.wallet_address) {
                    c.net += signed_tokens(t);
                }
            }
            if fired.is_none() {
                let cohort_net = self.cohort.as_ref().map(|c| (c.bought, c.net));
                fired = ladder_reason(
                    &self.state,
                    t,
                    self.entry_time,
                    self.entry_price,
                    params,
                    cohort_net,
                );
            }
            // Keep folding the rest so the memoized peaks + cohort net stay current
            // for the clock sweep even after the gate has already decided to exit.
        }
        fired
    }
}

/// Signed token flow for a trade: +tokens on a buy, −tokens on a sell.
fn signed_tokens(t: &Trade) -> f64 {
    match t.trade_type {
        TradeType::Buy => t.token_amount,
        TradeType::Sell => -t.token_amount,
    }
}

/// The exit-ladder rule params, resolved once from a [`Tpsl2Rule`] so the per-trade
/// predicate ([`ladder_reason`]) is a pure function of state + trade + params. Lets
/// the full re-walk and the incremental [`CachedExitState::advance_and_find_exit`]
/// share one ladder definition that can never drift.
pub struct LadderParams {
    take_profit_pct: f64,
    stop_loss_pct: f64,
    trailing_stop_pct: Option<f64>,
    time_stop_secs: Option<u64>,
    stall_secs: Option<u64>,
    liquidity_drop_pct: Option<f64>,
    cohort_exit_ratio: Option<f64>,
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
            cohort_exit_ratio: none_if_zero_f64(rule.p_exit_cohort_ratio),       // E5
        }
    }
}

/// The exit ladder for a single trade `t`, given the running walk `state` (peaks as
/// of `t`, inclusive) and, when E5 is configured, the cohort `(bought, net)` as of
/// `t`. First feature that fires wins (ladder order). Shared by
/// [`find_trade_driven_exit`] and [`CachedExitState::advance_and_find_exit`].
fn ladder_reason(
    state: &ExitWalkState,
    t: &Trade,
    entry_time: DateTime<Utc>,
    entry_price: f64,
    params: &LadderParams,
    cohort: Option<(f64, f64)>,
) -> Option<ExitReason> {
    let price = t.price_per_token;
    let pct = ((price - entry_price) / entry_price) * 100.0;
    None
        .or_else(|| {
            // E5: the cohort dumped — its net collapsed to ≤ ratio of its bag.
            // Top priority: the insider cluster bailing is the biggest danger.
            params.cohort_exit_ratio.and_then(|ratio| {
                cohort.and_then(|(bought, net)| {
                    (bought > 0.0 && net <= bought * ratio).then_some(ExitReason::CohortExit)
                })
            })
        })
        .or_else(|| {
            // E4: reserves crash below the peak-since-entry. REAL reserves only.
            params.liquidity_drop_pct.and_then(|drop| {
                t.real_sol_reserves.and_then(|reserves| {
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
                stall_triggered(state.last_higher_high_time, t.block_time, secs)
                    .then_some(ExitReason::Stall)
            })
        })
        .or_else(|| {
            // E2: cut once held past the deadline. Lowest priority.
            params.time_stop_secs.and_then(|secs| {
                time_stop_triggered(entry_time, t.block_time, secs).then_some(ExitReason::TimeStop)
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
pub fn find_trade_driven_exit(
    trades: &[Trade],
    entry_time: DateTime<Utc>,
    entry_price: f64,
    rule: &Tpsl2Rule,
) -> Option<ExitFill> {
    if entry_price <= 0.0 {
        return None;
    }

    let params = LadderParams::from_rule(rule);

    // E5 precompute: the launch cohort, the bag it ever bought (the denominator),
    // and its net holdings as of entry. `cohort_net` then evolves causally as the
    // walk replays each post-entry cohort trade. Skipped entirely when E5 is off.
    let (cohort, cohort_bought, mut cohort_net) = match params.cohort_exit_ratio {
        Some(_) => {
            let cohort = early_cohort_wallets(trades, RUGGED_EARLY_SLOT_WINDOW);
            let bought = cohort_flow(trades, &cohort).bought_tokens;
            let net_at_entry: f64 = trades
                .iter()
                .filter(|t| t.block_time <= entry_time && cohort.contains(&t.wallet_address))
                .map(signed_tokens)
                .sum();
            (Some(cohort), bought, net_at_entry)
        }
        None => (None, 0.0, 0.0),
    };

    let trades_after_entry: Vec<&Trade> =
        trades.iter().filter(|t| t.block_time > entry_time).collect();

    let mut state = ExitWalkState::starting_at(entry_price, entry_time);

    for t in trades_after_entry.iter() {
        state.update_with_trade(t);
        // Evolve the cohort's net holdings as its trades replay (E5 only).
        if let Some(cohort) = cohort.as_ref() {
            if cohort.contains(&t.wallet_address) {
                cohort_net += signed_tokens(t);
            }
        }

        // First feature that fires on this trade wins (ladder order). Shared with
        // the incremental gate via `ladder_reason` so they can never drift.
        let cohort_arg = cohort.as_ref().map(|_| (cohort_bought, cohort_net));
        let Some(reason) = ladder_reason(&state, t, entry_time, entry_price, &params, cohort_arg)
        else {
            continue;
        };

        // Exit price: lowest price in the block where the exit condition met.
        let exit_slot = t.slot;
        let exit_trade = trades_after_entry
            .iter()
            .copied()
            .filter(|x| x.slot == exit_slot)
            .min_by(|a, b| {
                a.price_per_token
                    .partial_cmp(&b.price_per_token)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

        if let Some(et) = exit_trade {
            return Some(ExitFill {
                price: et.price_per_token,
                tx_signature: et.tx_signature.clone(),
                block_time: et.block_time,
                reason,
            });
        }
    }
    None
}

/// The time-based exits (E2 TimeStop / E3 Stall) measured against wall-clock
/// `now` rather than a trade timestamp, so they fire while a token is silent.
/// Covers ONLY the two clock features — price-based exits can't change between
/// trades and stay on [`find_trade_driven_exit`]. Stall outranks TimeStop,
/// matching the trade-walk ladder order.
pub fn find_clock_driven_exit(
    state: &ExitWalkState,
    entry_time: DateTime<Utc>,
    rule: &Tpsl2Rule,
    now: DateTime<Utc>,
) -> Option<ExitReason> {
    if let Some(secs) = none_if_zero_u64(rule.p_exit_stall_secs) {
        if stall_triggered(state.last_higher_high_time, now, secs) {
            return Some(ExitReason::Stall);
        }
    }
    if let Some(secs) = none_if_zero_u64(rule.p_exit_time_stop_secs) {
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
/// (decision-equivalent, no per-ping full re-walk or cohort rebuild).
#[cfg_attr(not(test), allow(dead_code))]
pub fn should_position_exit_on_trade(
    position: &Position,
    trades: &[Trade],
    rule: &Tpsl2Rule,
) -> Option<ExitReason> {
    let entry_time = clock_entry_time(position)?;
    find_trade_driven_exit(trades, entry_time, position.entry_price, rule).map(|fill| fill.reason)
}

/// Live **clock-driven** gate: should this Holding position exit on the
/// wall-clock sweep? Cheap-exits when the rule has no time features, else checks
/// the deadlines against `now`. `state` is the position's memoized walk state
/// ([`CachedExitState`]), kept current by the trade path — the sweep no longer
/// rebuilds it from history per tick.
pub fn should_position_exit_on_clock(
    position: &Position,
    state: &ExitWalkState,
    rule: &Tpsl2Rule,
    now: DateTime<Utc>,
) -> Option<ExitReason> {
    let entry_time = clock_entry_time(position)?;
    if none_if_zero_u64(rule.p_exit_time_stop_secs).is_none()
        && none_if_zero_u64(rule.p_exit_stall_secs).is_none()
    {
        return None;
    }
    find_clock_driven_exit(state, entry_time, rule, now)
}

/// Shared guard for both live gates: a position is evaluable only while Holding
/// with a recorded entry. A 0 entry price means the fill isn't indexed yet —
/// evaluating it would divide by ~0 and fire a phantom TakeProfit, flapping the
/// position ExitPending→Holding. Returns the entry time once those hold.
pub fn clock_entry_time(position: &Position) -> Option<DateTime<Utc>> {
    if position.status != PositionStatus::Holding || position.entry_price <= 0.0 {
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
            price, // sol_amount
            1.0,   // token_amount → price_per_token = price
            format!("sig-{slot}-{secs}"),
            slot,
            base_time() + Duration::seconds(secs),
        )
    }

    /// A buy carrying explicit **real** SOL reserves (price still equals `price`).
    /// E4 reads real reserves now (never virtual — wash trading can fake virtual).
    fn buy_resv(price: f64, slot: u64, secs: i64, reserves: f64) -> Trade {
        let mut t = buy(price, slot, secs);
        t.real_sol_reserves = Some(reserves);
        t
    }

    /// A trade by a specific wallet, with explicit side/sol/tokens/slot — for the
    /// cohort-dump (E5) tests where wallet identity and token flow matter.
    fn trade_w(wallet: &str, side: TradeType, sol: f64, tokens: f64, slot: u64, secs: i64) -> Trade {
        Trade::new(
            "mint".into(),
            wallet.into(),
            side,
            sol,
            tokens,
            format!("sig-{wallet}-{slot}-{secs}"),
            slot,
            base_time() + Duration::seconds(secs),
        )
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
            1.0, // buy_amount
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
        let mut p = Position::new(
            "mint".into(),
            "wallet".into(),
            entry_price,
            "entry-sig".into(),
            "TPSL2".into(),
            rule_id,
            1.0,
        );
        p.entry_time = Some(base_time());
        p
    }

    /// Test-only oracle for the legacy fixed TP/SL exit, retained to prove the
    /// ladder reproduces it exactly when E1–E4 are unset.
    fn legacy_fixed_take_profit_stop_loss(
        trades: &[Trade],
        entry_time: DateTime<Utc>,
        entry_price: f64,
        take_profit_pct: f64,
        stop_loss_pct: f64,
    ) -> Option<ExitFill> {
        let later: Vec<&Trade> = trades.iter().filter(|t| t.block_time > entry_time).collect();
        for t in later.iter() {
            if entry_price <= 0.0 {
                break;
            }
            let pct = ((t.price_per_token - entry_price) / entry_price) * 100.0;
            if pct < take_profit_pct && pct > -stop_loss_pct {
                continue;
            }
            let reason = if pct >= take_profit_pct {
                ExitReason::TakeProfit
            } else {
                ExitReason::StopLoss
            };
            let exit_slot = t.slot;
            let exit_trade = later
                .iter()
                .copied()
                .filter(|t| t.slot == exit_slot)
                .min_by(|a, b| {
                    a.price_per_token
                        .partial_cmp(&b.price_per_token)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
            if let Some(et) = exit_trade {
                return Some(ExitFill {
                    price: et.price_per_token,
                    tx_signature: et.tx_signature.clone(),
                    block_time: et.block_time,
                    reason,
                });
            }
        }
        None
    }

    // ── Trade-driven ladder (`find_trade_driven_exit`) ───────────────────────

    fn moonshot_then_reversal() -> Vec<Trade> {
        vec![
            buy(2.0, 2, 1), // +100%
            buy(3.0, 3, 2), // +200% (peak)
            buy(2.5, 4, 3), // -16.7% off peak (3.0*0.7 = 2.1 floor not hit)
            buy(2.0, 5, 4), // <= 2.1 → trailing fires here
        ]
    }

    #[test]
    fn trailing_stop_fires_on_reversal() {
        let trades = moonshot_then_reversal();
        let rule = rule_with(1000.0, 90.0, Some(30.0), None, None, None);

        let exit = find_trade_driven_exit(&trades, base_time(), 1.0, &rule)
            .expect("trailing stop should trigger an exit, not stay Open");

        assert_eq!(exit.reason, ExitReason::TrailingStop);
        assert!((exit.price - 2.0).abs() < 1e-9, "expected fill at the 2.0 trade, got {}", exit.price);
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
        let trades = vec![buy(3.0, 2, 1), buy(0.05, 3, 2)];
        let rule = rule_with(1000.0, 90.0, Some(30.0), None, None, None);
        let exit = find_trade_driven_exit(&trades, base_time(), 1.0, &rule).expect("should exit");
        assert_eq!(exit.reason, ExitReason::StopLoss);
    }

    #[test]
    fn ladder_matches_legacy_fixed_exit_when_features_off() {
        let trades = vec![buy(1.2, 2, 1), buy(0.7, 3, 2), buy(2.0, 4, 3)];
        let rule = rule_with(50.0, 20.0, None, None, None, None);

        let legacy = legacy_fixed_take_profit_stop_loss(&trades, base_time(), 1.0, rule.p_exit_take_profit, rule.p_exit_stop_loss);
        let walked = find_trade_driven_exit(&trades, base_time(), 1.0, &rule);
        assert_eq!(legacy, walked);
    }

    fn flat_series() -> Vec<Trade> {
        vec![buy(1.0, 2, 10), buy(1.0, 3, 20), buy(1.0, 4, 30), buy(1.0, 5, 40)]
    }

    #[test]
    fn time_stop_fires_at_deadline_trade() {
        let trades = flat_series();
        let rule = rule_with(1000.0, 90.0, None, Some(25), None, None);
        let exit = find_trade_driven_exit(&trades, base_time(), 1.0, &rule).expect("should exit");
        assert_eq!(exit.reason, ExitReason::TimeStop);
        assert_eq!(exit.block_time, base_time() + Duration::seconds(30));
    }

    #[test]
    fn price_exit_preempts_later_time_stop() {
        let trades = vec![buy(0.5, 2, 10), buy(0.5, 3, 30)];
        let rule = rule_with(1000.0, 20.0, None, Some(25), None, None);
        let exit = find_trade_driven_exit(&trades, base_time(), 1.0, &rule).expect("should exit");
        assert_eq!(exit.reason, ExitReason::StopLoss);
        assert_eq!(exit.block_time, base_time() + Duration::seconds(10));
    }

    fn peak_then_stall() -> Vec<Trade> {
        vec![buy(2.0, 2, 10), buy(2.0, 3, 20), buy(2.0, 4, 30), buy(2.0, 5, 40)]
    }

    #[test]
    fn stall_fires_after_flatline() {
        let trades = peak_then_stall();
        let rule = rule_with(1000.0, 90.0, None, None, Some(25), None);
        let exit = find_trade_driven_exit(&trades, base_time(), 1.0, &rule).expect("should exit");
        assert_eq!(exit.reason, ExitReason::Stall);
        assert_eq!(exit.block_time, base_time() + Duration::seconds(40));
    }

    #[test]
    fn steady_new_highs_do_not_stall() {
        let trades = vec![buy(2.0, 2, 10), buy(3.0, 3, 20), buy(4.0, 4, 30), buy(5.0, 5, 40)];
        let rule = rule_with(1000.0, 90.0, None, None, Some(15), None);
        assert!(find_trade_driven_exit(&trades, base_time(), 1.0, &rule).is_none());
    }

    #[test]
    fn trailing_outranks_stall_on_same_trade() {
        let trades = vec![buy(2.0, 2, 10), buy(1.5, 3, 20), buy(1.45, 4, 30), buy(1.3, 5, 40)];
        let rule = rule_with(1000.0, 90.0, Some(30.0), None, Some(25), None);
        let exit = find_trade_driven_exit(&trades, base_time(), 1.0, &rule).expect("should exit");
        assert_eq!(exit.reason, ExitReason::TrailingStop);
        assert_eq!(exit.block_time, base_time() + Duration::seconds(40));
    }

    fn rising_then_reserve_crash() -> Vec<Trade> {
        vec![
            buy_resv(1.0, 2, 10, 100.0),
            buy_resv(1.0, 3, 20, 120.0),
            buy_resv(1.0, 4, 30, 130.0),
            buy_resv(1.0, 5, 40, 50.0),
        ]
    }

    #[test]
    fn liquidity_exit_fires_on_reserve_crash() {
        let trades = rising_then_reserve_crash();
        let rule = rule_with(1000.0, 90.0, None, None, None, Some(50.0));
        let exit = find_trade_driven_exit(&trades, base_time(), 1.0, &rule).expect("should exit");
        assert_eq!(exit.reason, ExitReason::LiquidityExit);
        assert_eq!(exit.block_time, base_time() + Duration::seconds(40));
    }

    #[test]
    fn liquidity_exit_outranks_stop_loss() {
        let trades = vec![buy_resv(1.0, 2, 10, 100.0), buy_resv(0.05, 3, 20, 40.0)];
        let rule = rule_with(1000.0, 90.0, None, None, None, Some(50.0));
        let exit = find_trade_driven_exit(&trades, base_time(), 1.0, &rule).expect("should exit");
        assert_eq!(exit.reason, ExitReason::LiquidityExit);
    }

    // ── E5 cohort-dump (`p_exit_cohort_ratio`) ───────────────────────────────

    /// rule_with + an explicit E5 cohort-exit ratio.
    fn rule_cohort(cohort_exit_ratio: f64) -> Tpsl2Rule {
        let mut r = rule_with(1000.0, 99.0, None, None, None, None);
        r.p_exit_cohort_ratio = Some(cohort_exit_ratio);
        r
    }

    #[test]
    fn cohort_exit_fires_when_cohort_dumps() {
        // "dev" buys 100 tokens at launch (slot 1, pre-entry). Entry at base_time,
        // price 1.0. After entry an outside wallet trades (price ref), then "dev"
        // dumps 96 → net 4 / 100 = 0.04 ≤ 0.05 → CohortExit.
        let trades = vec![
            trade_w("dev", TradeType::Buy, 5.0, 100.0, 1, -2),
            trade_w("out", TradeType::Buy, 1.0, 1.0, 500, 1),
            trade_w("dev", TradeType::Sell, 4.5, 96.0, 501, 2),
        ];
        let rule = rule_cohort(0.05);
        let exit = find_trade_driven_exit(&trades, base_time(), 1.0, &rule).expect("should exit");
        assert_eq!(exit.reason, ExitReason::CohortExit);
        assert_eq!(exit.block_time, base_time() + Duration::seconds(2));
    }

    #[test]
    fn cohort_exit_does_not_fire_while_cohort_holds() {
        // "dev" keeps its bag (only a tiny trim) → net 95/100 = 0.95 > 0.05.
        let trades = vec![
            trade_w("dev", TradeType::Buy, 5.0, 100.0, 1, -2),
            trade_w("out", TradeType::Buy, 1.0, 1.0, 500, 1),
            trade_w("dev", TradeType::Sell, 0.2, 5.0, 501, 2),
        ];
        let rule = rule_cohort(0.05);
        assert!(find_trade_driven_exit(&trades, base_time(), 1.0, &rule).is_none());
    }

    #[test]
    fn cohort_exit_inert_when_unconfigured() {
        // Same 96-token dump, but E5 off → no CohortExit. Sell priced near entry
        // (≈0.94) so the high TP/SL don't fire either → position stays open.
        let trades = vec![
            trade_w("dev", TradeType::Buy, 5.0, 100.0, 1, -2),
            trade_w("out", TradeType::Buy, 1.0, 1.0, 500, 1),
            trade_w("dev", TradeType::Sell, 90.0, 96.0, 501, 2),
        ];
        let rule = rule_with(1000.0, 99.0, None, None, None, None);
        assert!(find_trade_driven_exit(&trades, base_time(), 1.0, &rule).is_none());
    }

    // ── Live trade gate (`should_position_exit_on_trade`) ────────────────────

    #[test]
    fn live_gate_fires_trailing_stop() {
        let rule = rule_with(1000.0, 90.0, Some(30.0), None, None, None);
        let pos = holding(1.0, rule.id);
        let trades = vec![buy(2.0, 2, 1), buy(3.0, 3, 2), buy(2.0, 4, 3)];
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
            pos.entry_price,
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
            should_position_exit_on_clock(&pos, &walk(&pos, &trades), &rule, now),
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
            should_position_exit_on_clock(&pos, &walk(&pos, &trades), &rule, now),
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
            should_position_exit_on_clock(&pos, &walk(&pos, &trades), &rule, now),
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
            should_position_exit_on_clock(&pos, &walk(&pos, &trades), &rule, now),
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
            should_position_exit_on_clock(&pos, &walk(&pos, &trades), &rule, now),
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
            should_position_exit_on_clock(&pos, &walk(&pos, &[]), &rule, now),
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
