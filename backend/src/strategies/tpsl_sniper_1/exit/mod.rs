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

use crate::models::trade::Trade;
use crate::models::{Position, PositionStatus, Tpsl1StrategyRule};

use super::util::{none_if_zero_f64, none_if_zero_u64};

/// The typed reason a position exited. Replaces the old stringly-typed reason
/// that was round-tripped through `&str`; the engine returns this directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitReason {
    TakeProfit,
    StopLoss,
    /// E1 · price fell `p_trailing_stop_pct`% below the peak-since-entry.
    TrailingStop,
    /// E3 · no new higher-high for `p_stall_secs`.
    Stall,
    /// E2 · held past the `p_time_stop_secs` deadline.
    TimeStop,
    /// E4 · virtual SOL reserves crashed `p_liquidity_drop_pct`% below their peak.
    LiquidityExit,
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
/// higher-high (drives the stall clock), and the peak virtual SOL reserves
/// (drives the liquidity-death exit). The clock-driven sweep rebuilds this from
/// history so it can measure Stall against `now` instead of the last trade.
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
        if let Some(reserves) = trade.virtual_sol_reserves {
            if reserves > self.peak_reserves {
                self.peak_reserves = reserves;
            }
        }
    }

    /// Replay the full post-entry history into a fresh state — the peaks as of
    /// the last trade. Used by the clock-driven sweep, which then measures the
    /// time features against wall-clock `now`.
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
    rule: &Tpsl1StrategyRule,
) -> Option<ExitFill> {
    if entry_price <= 0.0 {
        return None;
    }

    let take_profit_pct = rule.take_profit;
    let stop_loss_pct = rule.stop_loss;
    let trailing_stop_pct = none_if_zero_f64(rule.p_trailing_stop_pct); // E1 (None → off)
    let time_stop_secs = none_if_zero_u64(rule.p_time_stop_secs); // E2 (None → off)
    let stall_secs = none_if_zero_u64(rule.p_stall_secs); // E3 (None → off)
    let liquidity_drop_pct = none_if_zero_f64(rule.p_liquidity_drop_pct); // E4 (None → off)

    let trades_after_entry: Vec<&Trade> =
        trades.iter().filter(|t| t.block_time > entry_time).collect();

    let mut state = ExitWalkState::starting_at(entry_price, entry_time);

    for t in trades_after_entry.iter() {
        state.update_with_trade(t);
        let price = t.price_per_token;
        let pct = ((price - entry_price) / entry_price) * 100.0;

        // First feature that fires on this trade wins (ladder order).
        let reason: Option<ExitReason> = None
            .or_else(|| {
                // E4: reserves crash below the peak-since-entry. Highest priority —
                // a reserve crash leads the price move the others catch later.
                liquidity_drop_pct.and_then(|drop| {
                    t.virtual_sol_reserves.and_then(|reserves| {
                        (state.peak_reserves > 0.0
                            && reserves < state.peak_reserves * (1.0 - drop / 100.0))
                        .then_some(ExitReason::LiquidityExit)
                    })
                })
            })
            .or_else(|| (pct <= -stop_loss_pct).then_some(ExitReason::StopLoss))
            .or_else(|| (pct >= take_profit_pct).then_some(ExitReason::TakeProfit))
            .or_else(|| {
                // E1: bank the reversal once price falls `trail`% below the peak.
                trailing_stop_pct.and_then(|trail| {
                    (state.peak_price > 0.0 && price <= state.peak_price * (1.0 - trail / 100.0))
                        .then_some(ExitReason::TrailingStop)
                })
            })
            .or_else(|| {
                // E3: sell the flatline. Ranks above the time stop, below trailing.
                stall_secs.and_then(|secs| {
                    stall_triggered(state.last_higher_high_time, t.block_time, secs)
                        .then_some(ExitReason::Stall)
                })
            })
            .or_else(|| {
                // E2: cut once held past the deadline. Lowest priority.
                time_stop_secs.and_then(|secs| {
                    time_stop_triggered(entry_time, t.block_time, secs)
                        .then_some(ExitReason::TimeStop)
                })
            });

        let Some(reason) = reason else {
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
    rule: &Tpsl1StrategyRule,
    now: DateTime<Utc>,
) -> Option<ExitReason> {
    if let Some(secs) = none_if_zero_u64(rule.p_stall_secs) {
        if stall_triggered(state.last_higher_high_time, now, secs) {
            return Some(ExitReason::Stall);
        }
    }
    if let Some(secs) = none_if_zero_u64(rule.p_time_stop_secs) {
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
pub fn should_position_exit_on_trade(
    position: &Position,
    trades: &[Trade],
    rule: &Tpsl1StrategyRule,
) -> Option<ExitReason> {
    let entry_time = exit_clock_entry(position)?;
    find_trade_driven_exit(trades, entry_time, position.entry_price, rule).map(|fill| fill.reason)
}

/// Live **clock-driven** gate: should this Holding position exit on the
/// wall-clock sweep? Cheap-exits when the rule has no time features, else
/// rebuilds the walk state from history and checks the deadlines against `now`.
pub fn should_position_exit_on_clock(
    position: &Position,
    trades: &[Trade],
    rule: &Tpsl1StrategyRule,
    now: DateTime<Utc>,
) -> Option<ExitReason> {
    let entry_time = exit_clock_entry(position)?;
    if none_if_zero_u64(rule.p_time_stop_secs).is_none()
        && none_if_zero_u64(rule.p_stall_secs).is_none()
    {
        return None;
    }
    let state = ExitWalkState::rebuild_from_trades(trades, position.entry_price, entry_time);
    find_clock_driven_exit(&state, entry_time, rule, now)
}

/// Shared guard for both live gates: a position is evaluable only while Holding
/// with a recorded entry. A 0 entry price means the fill isn't indexed yet —
/// evaluating it would divide by ~0 and fire a phantom TakeProfit, flapping the
/// position ExitPending→Holding. Returns the entry time once those hold.
fn exit_clock_entry(position: &Position) -> Option<DateTime<Utc>> {
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

    /// A buy carrying explicit virtual SOL reserves (price still equals `price`).
    fn buy_resv(price: f64, slot: u64, secs: i64, reserves: f64) -> Trade {
        let mut t = buy(price, slot, secs);
        t.virtual_sol_reserves = Some(reserves);
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
    ) -> Tpsl1StrategyRule {
        Tpsl1StrategyRule::new(
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
            "TPSL1".into(),
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

        let legacy = legacy_fixed_take_profit_stop_loss(&trades, base_time(), 1.0, rule.take_profit, rule.stop_loss);
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

    #[test]
    fn clock_gate_fires_time_stop_after_silence() {
        let rule = rule_with(1000.0, 90.0, None, Some(300), None, None);
        let pos = holding(1.0, rule.id);
        let trades = vec![buy(1.0, 2, 30)]; // one trade then silence
        let now = base_time() + Duration::seconds(600);
        assert_eq!(
            should_position_exit_on_clock(&pos, &trades, &rule, now),
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
            should_position_exit_on_clock(&pos, &trades, &rule, now),
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
            should_position_exit_on_clock(&pos, &trades, &rule, now),
            Some(ExitReason::Stall)
        );
    }

    #[test]
    fn clock_gate_inert_before_deadline() {
        let rule = rule_with(1000.0, 90.0, None, Some(300), Some(300), None);
        let pos = holding(1.0, rule.id);
        let trades = vec![buy(1.0, 2, 30)];
        let now = base_time() + Duration::seconds(100);
        assert_eq!(should_position_exit_on_clock(&pos, &trades, &rule, now), None);
    }

    #[test]
    fn clock_gate_inert_when_unconfigured() {
        let rule = rule_with(50.0, 20.0, Some(30.0), None, None, Some(50.0));
        let pos = holding(1.0, rule.id);
        let trades = vec![buy(1.0, 2, 30)];
        let now = base_time() + Duration::seconds(86_400);
        assert_eq!(should_position_exit_on_clock(&pos, &trades, &rule, now), None);
    }

    #[test]
    fn clock_gate_waits_for_recorded_entry() {
        let rule = rule_with(1000.0, 90.0, None, Some(60), None, None);
        let mut pos = holding(1.0, rule.id);
        pos.entry_time = None;
        let now = base_time() + Duration::seconds(600);
        assert_eq!(should_position_exit_on_clock(&pos, &[], &rule, now), None);
    }
}
