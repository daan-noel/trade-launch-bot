//! swing1 exit ladder — symmetric to entry: the dev starting another kill is the
//! signal to flee. Ladder priority (first match on a trade wins):
//!
//!   NextKill → E4 LiquidityExit (real reserves) → StopLoss → TakeProfit →
//!   E1 TrailingStop → E3 Stall → E2 TimeStop.
//!
//! NextKill is the top arm and is the swing1-specific addition; the rest reuse
//! the same predicate shapes as tpsl1/tpsl2 (E4 uses **real** reserves like
//! tpsl2). Every feature is inert at `None`/`0`.
//!
//! NextKill detection is **causal**: a single forward scan over the post-entry
//! trades finds the first swing-LOW leg matching the next-kill profile (deep +
//! short); the position flees at the first trade at/after that leg's terminal
//! pivot. One scan up front → O(1) check per trade in the walk, so the batch
//! result is reproducible by the live-incremental machine (Phase 2).

use chrono::{DateTime, Duration, Utc};

use crate::config::constants::MAX_FILL_WAIT_SLOTS;
use crate::models::trade::TradeRow;
use crate::models::Swing1Rule;

use super::classifier::{LowFeatures, PhaseProfile};
use super::swing::{detect_swing_legs_raw, ScanState, SwingLeg, SwingParams, SwingType, Tx};
use super::{exit_next_kill_profile, swing_params_from_rule};

/// Canonical price for an exit decision: the shared GMGN spot
/// ([`chart_spot_price`](TradeRow::chart_spot_price), reserve pair → pool → exec
/// fallback), exactly as the leg detector ([`super::swing::sanitize_and_order`])
/// prices. The execution fallback covers rows without a reserve pair. This is the
/// counterpart of the entry's spot-priced fill — both avoid execution
/// `price_per_token`, which is **zeroed** in the Parquet-lake `SweepTrade` rows
/// swing1 sweeps over (it carries the reserve pair, not the exec price), so reading
/// it yielded a ~0 exit price and garbage PnL. Pricing off the spot keeps live and
/// sweep byte-identical (Step 0 canonical-price contract).
fn spot_price<T: TradeRow>(t: &T) -> f64 {
    t.chart_spot_price().unwrap_or_else(|| t.execution_price())
}

/// The typed reason a swing1 position exited. swing1-specific (intentional clone
/// of the tpsl ladders' reasons) so the **NextKill** top arm has a home; the
/// reason strings match the kernel's [`ExitCode::from_reason`] wire form.
///
/// [`ExitCode::from_reason`]: crate::strategies::kernel::ExitCode::from_reason
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitReason {
    /// Top priority — the dev started another kill leg (deep + short post-entry low).
    NextKill,
    TakeProfit,
    StopLoss,
    TrailingStop,
    Stall,
    TimeStop,
    LiquidityExit,
}

impl ExitReason {
    /// Stable wire/string form — must match the kernel's `from_reason` arms.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NextKill => "NextKill",
            Self::TakeProfit => "TakeProfit",
            Self::StopLoss => "StopLoss",
            Self::TrailingStop => "TrailingStop",
            Self::Stall => "Stall",
            Self::TimeStop => "TimeStop",
            Self::LiquidityExit => "LiquidityExit",
        }
    }
}

/// The resolved exit fill (price/tx/time + reason).
#[derive(Debug, Clone, PartialEq)]
pub struct ExitFill {
    pub price: f64,
    pub tx_signature: String,
    /// Slot of the fill trade — the unambiguous key the sweep drill-in uses to
    /// resolve this fill's real `tx_signature` from the `trades` table (the slim
    /// `SweepTrade` carries no signature, so the in-row `tx_signature` is empty
    /// on the sweep path). Live reads the signature directly and ignores this.
    pub slot: u64,
    pub block_time: DateTime<Utc>,
    pub reason: ExitReason,
}

/// The swing1 exit-ladder params, resolved once from a [`Swing1Rule`] so the
/// per-trade predicate is a pure function of state + trade + params. `pub` (mirroring
/// [`tpsl_sniper_1::exit::LadderParams`](crate::strategies::tpsl_sniper_1::exit::LadderParams))
/// so the live enum dispatch ([`LadderParamsImpl`](crate::strategies::exit_state))
/// and the time-exit index can hold + query it.
#[derive(Clone)]
pub struct LadderParams {
    take_profit_pct: f64,
    stop_loss_pct: f64,
    trailing_stop_pct: Option<f64>, // E1
    time_stop_secs: Option<u64>,    // E2
    stall_secs: Option<u64>,        // E3
    liquidity_drop_pct: Option<f64>, // E4 (real reserves)
    /// Swing-scan params for the incremental next-kill machine (resolved from the
    /// same rule, so the live memo scans the identical legs the batch does).
    sparams: SwingParams,
    /// The next-kill kill-low profile; `None` when the arm is disabled.
    next_kill_profile: Option<PhaseProfile>,
}

impl LadderParams {
    pub fn from_rule(r: &Swing1Rule) -> Self {
        let nz_f64 = |v: Option<f64>| v.filter(|x| *x != 0.0);
        let nz_u64 = |v: Option<u64>| v.filter(|x| *x != 0);
        Self {
            take_profit_pct: r.p_exit_take_profit,
            stop_loss_pct: r.p_exit_stop_loss,
            trailing_stop_pct: nz_f64(r.p_exit_trailing_stop_pct),
            time_stop_secs: nz_u64(r.p_exit_time_stop_secs),
            stall_secs: nz_u64(r.p_exit_stall_secs),
            liquidity_drop_pct: nz_f64(r.p_exit_liquidity_drop_pct),
            sparams: swing_params_from_rule(r),
            next_kill_profile: exit_next_kill_profile(r),
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

    /// Whether the rule carries any wall-clock exit (TimeStop or Stall) — the
    /// time-exit secondary-index membership gate.
    pub fn has_time_exit(&self) -> bool {
        self.time_stop_secs.is_some() || self.stall_secs.is_some()
    }
}

/// Running peaks accumulated while walking post-entry trades.
#[derive(Clone, Copy)]
struct WalkState {
    peak_price: f64,
    last_higher_high_time: DateTime<Utc>,
    peak_reserves: f64,
}

impl WalkState {
    fn starting_at(entry_price: f64, entry_time: DateTime<Utc>) -> Self {
        Self {
            peak_price: entry_price,
            last_higher_high_time: entry_time,
            peak_reserves: 0.0,
        }
    }

    fn update_with_trade<T: TradeRow>(&mut self, t: &T) {
        let price = spot_price(t);
        if price > self.peak_price {
            self.peak_price = price;
            self.last_higher_high_time = t.block_time();
        }
        // E4 tracks **real** reserves (the tpsl2 variant).
        if let Some(r) = t.real_reserve_sol() {
            if r > self.peak_reserves {
                self.peak_reserves = r;
            }
        }
    }
}

/// The first post-entry timestamp (ms) at which a next-kill leg completes, or
/// `None` if next-kill is disabled / never fires. Causal single scan.
fn next_kill_fire_ms<T: TradeRow>(trades: &[T], rule: &Swing1Rule, entry_time: DateTime<Utc>) -> Option<i64> {
    let profile = exit_next_kill_profile(rule)?;
    let sparams = swing_params_from_rule(rule);
    // Only legs whose terminal pivot lands after entry can be a *next* kill.
    let entry_ms = entry_time.timestamp_millis();
    let legs = detect_swing_legs_raw(trades, &sparams);
    for leg in &legs {
        if leg.leg_type != SwingType::SwingLow || leg.end_at <= entry_ms {
            continue;
        }
        let f = LowFeatures::from_low(leg).expect("SwingLow checked");
        if profile.is_kill_low(&f) {
            return Some(leg.end_at.max(entry_ms + 1));
        }
    }
    None
}

/// Whether a finalized swing-LOW leg qualifies as a post-entry next-kill, and if so
/// the fire ms (`end_at.max(entry_ms + 1)`). Byte-identical to the per-leg body of
/// [`next_kill_fire_ms`], so the batch scan and the incremental memo agree leg-for-leg.
fn leg_next_kill_ms(leg: &super::swing::SwingLeg, profile: &PhaseProfile, entry_ms: i64) -> Option<i64> {
    if leg.leg_type != SwingType::SwingLow || leg.end_at <= entry_ms {
        return None;
    }
    let f = LowFeatures::from_low(leg).expect("SwingLow checked");
    profile.is_kill_low(&f).then(|| leg.end_at.max(entry_ms + 1))
}

/// Live-incremental analogue of [`find_trade_driven_exit_with_slot`]: an O(1)-per-trade
/// exit memo that reproduces the batch decision without re-walking a token's retained
/// history each ping. Mirrors [`tpsl_sniper_1::exit::CachedExitState`](crate::strategies::tpsl_sniper_1::exit::CachedExitState),
/// extended with the swing1-specific **incremental next-kill scan**.
///
/// The memo advances on **trades** (`&[T]`), folding each genuinely-new trade into:
///   1. the [`WalkState`] peaks (identical to tpsl1), AND
///   2. the shared swing [`ScanState`] machine — as a swing-LOW leg finalizes, the
///      causal next-kill predicate ([`leg_next_kill_ms`]) runs on it, reproducing the
///      batch [`next_kill_fire_ms`] one leg at a time.
///
/// `next_kill_ms` is the earliest qualifying **finalized** low (permanent once found).
/// The batch also considers the trailing *open* low at each window end, so
/// [`advance_and_find_exit`](Self::advance_and_find_exit) additionally checks the
/// current open leg via [`ScanState::snapshot_ledger`] — that part is transient (a
/// later trade may extend/merge the open leg), exactly as the batch recomputes it per
/// call. `consumed_abs` is the absolute fold cursor (front-trim safety, identical
/// arithmetic to tpsl1).
pub struct CachedSwingExitState {
    state: WalkState,
    entry_time: DateTime<Utc>,
    entry_price: f64,
    consumed_abs: u64,
    /// The scan machine + its `prev_post_spot` seed, folded incrementally.
    scan: ScanState,
    prev_post_spot: Option<f64>,
    /// Count of `scan.finalized()` legs already checked for next-kill.
    finalized_checked: usize,
    /// Swing params + next-kill profile resolved once from the rule. `profile` is
    /// `None` when next-kill is disabled (the arm is inert).
    sparams: SwingParams,
    profile: Option<PhaseProfile>,
    /// Earliest qualifying finalized low's fire ms (sticky once set).
    next_kill_ms_finalized: Option<i64>,
    entry_ms: i64,
}

impl CachedSwingExitState {
    /// An unfolded memo whose cursor sits at the window front (`base`), so the first
    /// [`advance_and_find_exit`](Self::advance_and_find_exit) folds the entire retained
    /// window (reproducing a full re-walk on the position's first ping). The swing scan
    /// params + next-kill profile come off the resolved [`LadderParams`].
    pub fn build_unfolded(
        base: u64,
        entry_price: f64,
        entry_time: DateTime<Utc>,
        params: &LadderParams,
    ) -> Self {
        Self {
            state: WalkState::starting_at(entry_price, entry_time),
            entry_time,
            entry_price,
            consumed_abs: base,
            scan: ScanState::new(),
            prev_post_spot: None,
            finalized_checked: 0,
            sparams: params.sparams.clone(),
            profile: params.next_kill_profile,
            next_kill_ms_finalized: None,
            entry_ms: entry_time.timestamp_millis(),
        }
    }

    /// A **fully-folded** memo seeded from the retained window (the clock-sweep's
    /// first-sight seed). Folds every window trade into the scan + peaks so a later
    /// clock check reads current peaks; the returned memo's cursor sits past the
    /// window so a following advance only folds genuinely-new trades.
    pub fn build(
        trades: &[impl TradeRow],
        base: u64,
        entry_price: f64,
        entry_time: DateTime<Utc>,
        params: &LadderParams,
    ) -> Self {
        let mut s = Self::build_unfolded(base, entry_price, entry_time, params);
        s.fold(trades, 0);
        s.consumed_abs = base + trades.len() as u64;
        s
    }

    /// Fold trades `[from..]` (window-relative) into the peaks + scan machine, keeping
    /// the finalized-leg next-kill cursor current. Used by the clock-seed [`build`]
    /// path, which needs current peaks but does not evaluate the ladder. The trade
    /// gate ([`advance_and_find_exit`]) instead folds the scan up front then folds the
    /// peaks step-by-step in the ladder walk (peak-based arms need the peak as-of each
    /// trade). Does NOT touch `consumed_abs` — the callers set it.
    fn fold(&mut self, trades: &[impl TradeRow], from: usize) {
        for t in &trades[from..] {
            if t.block_time() > self.entry_time {
                self.state.update_with_trade(t);
            }
            self.fold_scan(t);
        }
        self.check_new_finalized();
    }

    /// Fold ONE trade into the swing scan machine only (not the peaks). The scan folds
    /// ALL trades — the batch `next_kill_fire_ms` scans the whole window, then filters
    /// legs by `end_at > entry`.
    fn fold_scan(&mut self, t: &impl TradeRow) {
        if let Some(tx) = Tx::from_trade(t, self.prev_post_spot) {
            self.prev_post_spot = Some(tx.post_spot());
            self.scan.step(&tx, &self.sparams);
        }
    }

    /// Scan any newly-finalized legs for the earliest qualifying next-kill low.
    fn check_new_finalized(&mut self) {
        let Some(profile) = &self.profile else { return };
        if self.next_kill_ms_finalized.is_some() {
            self.finalized_checked = self.scan.finalized().len();
            return;
        }
        let legs = self.scan.finalized();
        for leg in &legs[self.finalized_checked..] {
            if let Some(ms) = leg_next_kill_ms(&leg.clone().finalize(), profile, self.entry_ms) {
                self.next_kill_ms_finalized = Some(ms);
                break;
            }
        }
        self.finalized_checked = legs.len();
    }

    /// The effective next-kill fire ms at the current window end: the earliest
    /// qualifying finalized low, else the trailing open low if IT qualifies (transient,
    /// recomputed each call — the batch does the same). Byte-identical to
    /// [`next_kill_fire_ms`] over the same window.
    fn effective_next_kill_ms(&self) -> Option<i64> {
        let profile = self.profile.as_ref()?;
        if let Some(ms) = self.next_kill_ms_finalized {
            return Some(ms);
        }
        // No finalized qualifying low yet: check the trailing open leg(s). The
        // snapshot ledger's finalized prefix is already known non-qualifying, so
        // only the trailing legs (beyond `finalized().len()`) are new to check.
        let ledger = self.scan.snapshot_ledger();
        let n_final = self.scan.finalized().len();
        ledger[n_final..]
            .iter()
            .find_map(|leg| leg_next_kill_ms(&leg.clone().finalize(), profile, self.entry_ms))
    }

    /// Wall-clock exit check (E2 TimeStop / E3 Stall) against the memoized peaks, for
    /// the per-second sweep — never re-walks the trade history. Mirrors
    /// [`tpsl_sniper_1::exit::find_clock_driven_exit`](crate::strategies::tpsl_sniper_1::exit::find_clock_driven_exit):
    /// Stall (measured from the last higher-high) outranks TimeStop, matching the
    /// trade-walk ladder order. Price/next-kill arms can't change between trades so
    /// they stay on [`advance_and_find_exit`](Self::advance_and_find_exit).
    pub fn clock_exit_reason(
        &self,
        entry_time: DateTime<Utc>,
        params: &LadderParams,
        now: DateTime<Utc>,
    ) -> Option<ExitReason> {
        if let Some(secs) = params.stall_secs {
            if (now - self.state.last_higher_high_time).num_seconds() >= secs as i64 {
                return Some(ExitReason::Stall);
            }
        }
        if let Some(secs) = params.time_stop_secs {
            if now >= entry_time + Duration::seconds(secs as i64) {
                return Some(ExitReason::TimeStop);
            }
        }
        None
    }

    /// Snapshot the swing scan as finalized [`SwingLeg`]s (finalized reversals plus
    /// the trailing open leg(s)) — for persisting onto the closed position's `extra`
    /// so the frontend chart can render the same legs the live memo saw, with no
    /// detector re-run. Pure read, does not mutate the memo.
    pub fn finalized_swing_legs(&self) -> Vec<SwingLeg> {
        self.scan.snapshot_ledger().into_iter().map(|leg| leg.finalize()).collect()
    }

    /// Fold the genuinely-new trades into the memo AND evaluate the exit ladder against
    /// each newly-folded post-entry trade, returning the first [`ExitReason`] that
    /// fires. Decision-equivalent to a full [`find_trade_driven_exit`] re-walk (peaks +
    /// scan fold identically; the ladder runs against the state as of each trade). `base`
    /// is the token's current `trades_base`. A cursor past the window end (history
    /// reset/over-trimmed) rebuilds the whole scan + peaks from the window.
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
            // Lost the cursor: re-fold the whole window from a fresh machine.
            self.state = WalkState::starting_at(self.entry_price, self.entry_time);
            self.scan = ScanState::new();
            self.prev_post_spot = None;
            self.finalized_checked = 0;
            self.next_kill_ms_finalized = None;
        }
        let from = if rebuild { 0 } else { start as usize };
        self.consumed_abs = base + trades.len() as u64;

        // Fold the swing scan over every new trade FIRST, so the next-kill ms reflects
        // the FULL window (the batch computes next-kill over the whole window, not just
        // up to the firing trade).
        for t in &trades[from..] {
            self.fold_scan(t);
        }
        self.check_new_finalized();
        let next_kill_ms = self.effective_next_kill_ms();

        // Then walk the new trades against the ladder, folding the PEAKS step-by-step
        // so each peak-based arm (trailing / liquidity) sees the peak as-of that trade
        // — exactly as the batch `find_trade_driven_exit` interleaves fold + evaluate.
        let mut fired = None;
        for t in &trades[from..] {
            if t.block_time() <= self.entry_time {
                continue;
            }
            self.state.update_with_trade(t);
            if fired.is_none() {
                fired = ladder_reason(
                    &self.state, t, self.entry_time, self.entry_price, params, next_kill_ms,
                );
            }
        }
        fired
    }
}

/// The exit ladder for one trade. First feature that fires wins.
fn ladder_reason<T: TradeRow>(
    state: &WalkState,
    t: &T,
    entry_time: DateTime<Utc>,
    entry_price: f64,
    params: &LadderParams,
    next_kill_ms: Option<i64>,
) -> Option<ExitReason> {
    let price = spot_price(t);
    let block_time = t.block_time();
    let pct = ((price - entry_price) / entry_price) * 100.0;
    None
        .or_else(|| {
            // NextKill — top priority: the dev starting another kill.
            next_kill_ms
                .filter(|&ms| block_time.timestamp_millis() >= ms)
                .map(|_| ExitReason::NextKill)
        })
        .or_else(|| {
            // E4: real reserves crash below the peak-since-entry.
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
            params.trailing_stop_pct.and_then(|trail| {
                (state.peak_price > 0.0 && price <= state.peak_price * (1.0 - trail / 100.0))
                    .then_some(ExitReason::TrailingStop)
            })
        })
        .or_else(|| {
            params.stall_secs.and_then(|secs| {
                ((block_time - state.last_higher_high_time).num_seconds() >= secs as i64)
                    .then_some(ExitReason::Stall)
            })
        })
        .or_else(|| {
            params.time_stop_secs.and_then(|secs| {
                (block_time >= entry_time + Duration::seconds(secs as i64))
                    .then_some(ExitReason::TimeStop)
            })
        })
}

/// Walk the post-entry trades and return the first exit the ladder fires, or
/// `None` if still open. Mirrors tpsl1's worst-case fill window.
pub fn find_trade_driven_exit<T: TradeRow>(
    trades: &[T],
    entry_time: DateTime<Utc>,
    entry_price: f64,
    rule: &Swing1Rule,
) -> Option<ExitFill> {
    find_trade_driven_exit_with_slot(trades, entry_time, entry_price, rule).map(|(f, _)| f)
}

/// [`find_trade_driven_exit`] that also returns the **firing slot** (for the live
/// paper fill-poll window), matching the tpsl2 resolver shape.
pub fn find_trade_driven_exit_with_slot<T: TradeRow>(
    trades: &[T],
    entry_time: DateTime<Utc>,
    entry_price: f64,
    rule: &Swing1Rule,
) -> Option<(ExitFill, u64)> {
    if entry_price <= 0.0 {
        return None;
    }
    let params = LadderParams::from_rule(rule);
    let next_kill_ms = next_kill_fire_ms(trades, rule, entry_time);
    let mut state = WalkState::starting_at(entry_price, entry_time);

    for (fire_idx, t) in trades.iter().enumerate().filter(|(_, t)| t.block_time() > entry_time) {
        state.update_with_trade(t);

        let Some(reason) = ladder_reason(&state, t, entry_time, entry_price, &params, next_kill_ms)
        else {
            continue;
        };

        // Worst-case fill window: trigger slot F + the next observed slot within
        // MAX_FILL_WAIT_SLOTS; fill = lowest price in the window (worst for a sell).
        let exit_slot = t.slot();
        let post = &trades[fire_idx + 1..];
        let next_slot = post
            .iter()
            .filter(|x| x.block_time() > entry_time && x.slot() > exit_slot)
            .map(|x| x.slot())
            .next();
        let exit_trade = post
            .iter()
            .filter(|x| {
                x.block_time() > entry_time && {
                    let s = x.slot();
                    s == exit_slot
                        || next_slot.is_some_and(|ns| s == ns && ns <= exit_slot + MAX_FILL_WAIT_SLOTS)
                }
            })
            .min_by(|a, b| spot_price(*a).total_cmp(&spot_price(*b)));

        if let Some(et) = exit_trade {
            return Some((
                ExitFill {
                    price: spot_price(et),
                    tx_signature: et.tx_signature().to_string(),
                    slot: et.slot(),
                    block_time: et.block_time(),
                    reason,
                },
                exit_slot,
            ));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::trade::{Trade, TradeType};
    use chrono::Utc;

    fn base() -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap()
    }

    fn tr(kind: TradeType, secs: i64, price: f64, slot: u64) -> Trade {
        // price = sol/token; set token_amount=1 so price_per_token == amount_sol.
        Trade::new(
            "mint".into(),
            "w".into(),
            kind,
            price,
            1,
            format!("sig-{secs}-{slot}"),
            slot,
            base() + Duration::seconds(secs),
        )
    }

    fn rule_tp_sl(tp: f64, sl: f64) -> Swing1Rule {
        let mut r = Swing1Rule::new(
            "r".into(), None, None, None, serde_json::json!([]), "paper".into(),
            1.0, tp, sl, None, None, None, None, None, None, None, None, None,
        );
        r.is_active = true;
        r
    }

    #[test]
    fn take_profit_fires_without_next_kill() {
        let rule = rule_tp_sl(50.0, 90.0);
        // entry @1.0; price jumps to 2.0 (>+50%) at slot 3 (the TP trigger). The
        // fill window needs a trade AFTER the trigger in the same/next slot — add a
        // slot-4 trade so the worst-case fill resolves.
        let trades = vec![
            tr(TradeType::Buy, 1, 1.0, 2),
            tr(TradeType::Buy, 2, 2.0, 3), // TP trigger
            tr(TradeType::Buy, 3, 1.9, 4), // fill window
        ];
        let f = find_trade_driven_exit(&trades, base(), 1.0, &rule).expect("exit");
        assert_eq!(f.reason, ExitReason::TakeProfit);
    }

    #[test]
    fn next_kill_fires_on_post_entry_kill_leg() {
        // Configure a next-kill profile (≥50% drop, ≤5s). With TP/SL out of reach,
        // a deep+short post-entry low must flee via NextKill.
        let mut rule = rule_tp_sl(500.0, 95.0); // TP/SL never hit by the path below
        rule.p_exit_next_kill_depth_min_pct = Some(0.5);
        rule.p_exit_next_kill_max_duration_ms = Some(5_000);
        // Reversal thresholds small so the swing scan splits legs.
        rule.p_swing_high_to_low_sol = Some(0.5);
        rule.p_swing_low_to_high_sol = Some(0.5);
        rule.p_swing_min_leg_trades = Some(1);
        // entry @1.0; pump to 3.0, then a deep fast dump to 1.2 (-60% off the 3.0
        // peak within ~2s → kill low), then a reversal that completes the low leg
        // and provides the flee fill.
        let trades = vec![
            tr(TradeType::Buy, 1, 1.0, 2),
            tr(TradeType::Buy, 2, 3.0, 3),  // up-leg
            tr(TradeType::Sell, 4, 1.2, 4), // deep dump within ~2s → kill low
            tr(TradeType::Buy, 6, 1.3, 5),  // reversal up → completes the low leg
            tr(TradeType::Buy, 7, 1.3, 6),  // flee fill window
        ];
        let f = find_trade_driven_exit(&trades, base(), 1.0, &rule).expect("exit");
        assert_eq!(f.reason, ExitReason::NextKill);
    }

    // ── Incremental memo parity (Phase A DoD gate) ───────────────────────────

    /// Reason-only reference walk: the first ladder-firing reason across the full
    /// window, WITHOUT the fill-window resolution. This is what the incremental memo
    /// returns (the live path resolves the fill separately, exactly like tpsl1's
    /// `advance_and_find_exit`). Byte-identical firing logic to
    /// [`find_trade_driven_exit_with_slot`] up to the fill lookup.
    fn reference_reason(
        trades: &[Trade],
        entry_time: DateTime<Utc>,
        entry_price: f64,
        rule: &Swing1Rule,
    ) -> Option<ExitReason> {
        if entry_price <= 0.0 {
            return None;
        }
        let params = LadderParams::from_rule(rule);
        let next_kill_ms = next_kill_fire_ms(trades, rule, entry_time);
        let mut state = WalkState::starting_at(entry_price, entry_time);
        for t in trades.iter().filter(|t| t.block_time() > entry_time) {
            state.update_with_trade(t);
            if let Some(r) =
                ladder_reason(&state, t, entry_time, entry_price, &params, next_kill_ms)
            {
                return Some(r);
            }
        }
        None
    }

    /// A trade whose spot price AND swing volume are both `sol` (token_amount = 1, no
    /// reserves ⇒ `execution_price() == amount_sol == sol`). In this simplified model
    /// price and volume are coupled, exactly as in [`tr`] and the batch tests.
    fn trx(kind: TradeType, secs: i64, sol: f64, slot: u64) -> Trade {
        Trade::new(
            "mint".into(), "w".into(), kind, sol, 1, format!("s-{secs}-{slot}"), slot,
            base() + Duration::seconds(secs),
        )
    }

    fn rule_next_kill() -> Swing1Rule {
        let mut r = rule_tp_sl(500.0, 95.0); // TP/SL out of the way
        r.p_exit_next_kill_depth_min_pct = Some(0.5);
        r.p_exit_next_kill_max_duration_ms = Some(5_000);
        r.p_swing_high_to_low_sol = Some(0.5);
        r.p_swing_low_to_high_sol = Some(0.5);
        r.p_swing_min_leg_trades = Some(1);
        r
    }

    /// The memo, fed the full window in one advance from an unfolded seed, fires the
    /// same reason as the reference walk — for every prefix of the sequence.
    fn assert_memo_matches_reference_over_prefixes(trades: &[Trade], rule: &Swing1Rule) {
        let entry = base();
        let params = LadderParams::from_rule(rule);
        for k in 1..=trades.len() {
            let prefix = &trades[..k];
            let expected = reference_reason(prefix, entry, 1.0, rule);
            let mut memo = CachedSwingExitState::build_unfolded(0, 1.0, entry, &params);
            let got = memo.advance_and_find_exit(prefix, 0, &params);
            assert_eq!(
                got, expected,
                "prefix len {k}: memo {got:?} != reference {expected:?}"
            );
        }
    }

    /// The proven kill-low sequence (mirrors `next_kill_fires_on_post_entry_kill_leg`):
    /// pump to 3.0, deep fast dump to 1.2 (−60% in ≤5s → kill low), then a reversal
    /// that finalizes the low and prints trades past its pivot (the flee window).
    fn next_kill_seq() -> Vec<Trade> {
        vec![
            trx(TradeType::Buy, 1, 1.0, 2),
            trx(TradeType::Buy, 2, 3.0, 3),  // up-leg
            trx(TradeType::Sell, 4, 1.2, 4), // deep dump → kill low
            trx(TradeType::Buy, 6, 1.3, 5),  // reversal completes the low
            trx(TradeType::Buy, 7, 1.3, 6),
            trx(TradeType::Buy, 9, 1.35, 7),
        ]
    }

    #[test]
    fn memo_matches_reference_next_kill() {
        assert_memo_matches_reference_over_prefixes(&next_kill_seq(), &rule_next_kill());
    }

    #[test]
    fn memo_matches_reference_tp_sl_trailing() {
        let mut rule = rule_tp_sl(50.0, 40.0);
        rule.p_exit_trailing_stop_pct = Some(30.0);
        let trades = vec![
            trx(TradeType::Buy, 1, 1.0, 2),
            trx(TradeType::Buy, 2, 1.6, 3), // +60% → TP fires here
            trx(TradeType::Buy, 3, 1.5, 4),
            trx(TradeType::Sell, 4, 0.5, 5),
        ];
        assert_memo_matches_reference_over_prefixes(&trades, &rule);
    }

    /// A cheap deterministic LCG so the property test is reproducible without a
    /// rand dependency (Date/rand are unavailable in this crate's test harness).
    fn lcg(seed: &mut u64) -> u64 {
        *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        *seed >> 33
    }

    #[test]
    fn memo_property_matches_reference_random_sequences() {
        let rule = rule_next_kill();
        let mut seed = 0x5EED_1234_ABCDu64;
        for _ in 0..200 {
            let n = 3 + (lcg(&mut seed) % 12) as usize;
            let mut trades = Vec::with_capacity(n);
            let mut slot = 2u64;
            for i in 0..n {
                slot += 1 + (lcg(&mut seed) % 2); // mostly monotone slots
                let is_buy = lcg(&mut seed) % 2 == 0;
                // sol (= spot price = swing volume) in [0.2, 4.0]: some sub-threshold,
                // some big, spanning deep dumps and shallow moves.
                let sol = 0.2 + (lcg(&mut seed) % 380) as f64 / 100.0;
                let kind = if is_buy { TradeType::Buy } else { TradeType::Sell };
                trades.push(trx(kind, (i as i64) + 1, sol, slot));
            }
            assert_memo_matches_reference_over_prefixes(&trades, &rule);
        }
    }

    /// Incremental multi-step fold (seed from a prefix, advance in steps) matches a
    /// single full-window advance — and a front-trim (window slides, `base` grows)
    /// preserves a next-kill leg detected before the trim.
    #[test]
    fn memo_incremental_and_front_trim() {
        let rule = rule_next_kill();
        let entry = base();
        let params = LadderParams::from_rule(&rule);
        let trades = next_kill_seq();

        // Find the first prefix at which the reference fires NextKill — the memo,
        // fed incrementally, must fire it by the same prefix (never later, never a
        // different reason). This pins the incremental fold to the batch without
        // hard-coding which trade forms the leg.
        let first_fire = (1..=trades.len())
            .find(|&k| reference_reason(&trades[..k], entry, 1.0, &rule) == Some(ExitReason::NextKill))
            .expect("reference fires NextKill somewhere");

        let mut inc = CachedSwingExitState::build_unfolded(0, 1.0, entry, &params);
        let mut fired_at = None;
        for k in 1..=trades.len() {
            if let Some(r) = inc.advance_and_find_exit(&trades[k - 1..k], (k - 1) as u64, &params) {
                fired_at = Some((k, r));
                break;
            }
        }
        assert_eq!(
            fired_at,
            Some((first_fire, ExitReason::NextKill)),
            "incremental fold must fire NextKill at the same prefix as the batch"
        );

        // Front-trim: fold up to (but not including) the firing trade, then trim the
        // whole window away and feed ONLY the firing trade with an advanced `base`.
        // The next-kill ms was already memoized, so NextKill still fires post-trim.
        let mut trimmed = CachedSwingExitState::build_unfolded(0, 1.0, entry, &params);
        trimmed.advance_and_find_exit(&trades[..first_fire - 1], 0, &params);
        let tail = &trades[first_fire - 1..first_fire];
        let after = trimmed.advance_and_find_exit(tail, (first_fire - 1) as u64, &params);
        assert_eq!(after, Some(ExitReason::NextKill), "next-kill survives a front trim");
    }
}
