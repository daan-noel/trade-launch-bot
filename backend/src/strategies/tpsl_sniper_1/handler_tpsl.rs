use super::util::{ignore_zero_f64, ignore_zero_u64};
use crate::models::{Position, StrategyTPSLRule, Token};
use tracing::warn;
use uuid::Uuid;

/// TPSL (Take Profit Stop Loss) Strategy Handler
///
/// This strategy analyzes tokens based on defined rules and manages buy/sell positions.
/// - On token creation: Check if token matches buy entry rule criteria
/// - On trade events: Monitor positions for exit conditions (take profit or stop loss)
pub struct TPSL1StrategyHandler {
    rules: Vec<StrategyTPSLRule>,
}

impl TPSL1StrategyHandler {
    pub fn new(rules: Vec<StrategyTPSLRule>) -> Self {
        Self { rules }
    }

    /// Check if a new token should trigger a buy entry based on any active rule.
    /// Returns the rule_id if a match is found, None otherwise.
    pub fn check_buy_entry(&self, token: &Token) -> Option<Uuid> {
        for rule in &self.rules {
            if !rule.is_active {
                continue;
            }

            let p_initial_buy_sol = ignore_zero_f64(rule.p_initial_buy_sol);
            let tolerance_pct = ignore_zero_f64(Some(rule.tolerance_pct));
            let p_cu_limit = ignore_zero_u64(rule.p_cu_limit);
            let p_cu_price = ignore_zero_u64(rule.p_cu_price);
            let p_max_sol_cost = ignore_zero_f64(rule.p_max_sol_cost);
            let p_spendable_sol_in = ignore_zero_f64(rule.p_spendable_sol_in);
            let has_ix_labels = rule.p_ix_labels.as_array().map_or(false, |a| !a.is_empty());

            if p_initial_buy_sol.is_none()
                && tolerance_pct.is_none()
                && p_cu_limit.is_none()
                && p_cu_price.is_none()
                && p_max_sol_cost.is_none()
                && p_spendable_sol_in.is_none()
                && !has_ix_labels
            {
                warn!("All rule criteria are empty — simulation would match every token");
                continue;
            }

            // Check p_initial_buy_sol constraint (optional)
            if let Some(rule_initial_buy) = p_initial_buy_sol {
                if let Some(initial_buy) = token.initial_buy_sol {
                    let tol = rule_initial_buy.abs() * (rule.tolerance_pct * 0.01);
                    if (initial_buy - rule_initial_buy).abs() > tol + 1e-9 {
                        continue;
                    }
                } else {
                    continue;
                }
            }

            // Check p_cu_limit constraint (optional)
            if let Some(cu_limit_constraint) = p_cu_limit {
                if let Some(token_cu_limit) = token.cu_limit {
                    let token_value = token_cu_limit as f64;
                    let rule_value = cu_limit_constraint as f64;
                    let tol = rule_value.abs() * (rule.tolerance_pct * 0.01);
                    if (token_value - rule_value).abs() > tol + 1e-15 {
                        continue;
                    }
                } else {
                    continue;
                }
            }

            // Check p_cu_price constraint (optional)
            if let Some(cu_price_constraint) = p_cu_price {
                if let Some(token_cu_price) = token.cu_price {
                    let token_value = token_cu_price as f64;
                    let rule_value = cu_price_constraint as f64;
                    let tol = rule_value.abs() * (rule.tolerance_pct * 0.01);
                    if (token_value - rule_value).abs() > tol + 1e-9 {
                        continue;
                    }
                } else {
                    continue;
                }
            }

            // Check p_max_sol_cost constraint (optional)
            if let Some(max_sol_cost_constraint) = p_max_sol_cost {
                if let Some(ix) = &token.initial_buy_instruction {
                    let token_max_cost = ix.get("max_sol_cost").and_then(|v| {
                        v.as_u64()
                            .or_else(|| v.as_str().and_then(|s| s.parse::<u64>().ok()))
                    });
                    if let Some(token_max_cost) = token_max_cost {
                        const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;
                        let token_max_cost_sol = token_max_cost as f64 / LAMPORTS_PER_SOL;
                        let tol = max_sol_cost_constraint.abs() * (rule.tolerance_pct * 0.01);
                        if (token_max_cost_sol - max_sol_cost_constraint).abs() > tol + 1e-15 {
                            continue;
                        }
                    } else {
                        continue;
                    }
                } else {
                    continue;
                }
            }

            // Check p_spendable_sol_in constraint (optional)
            if let Some(spendable_sol_in_constraint) = p_spendable_sol_in {
                if let Some(ix) = &token.initial_buy_instruction {
                    let token_spendable_sol_in = ix.get("spendable_sol_in").and_then(|v| {
                        v.as_u64()
                            .or_else(|| v.as_str().and_then(|s| s.parse::<u64>().ok()))
                    });
                    if let Some(token_spendable_sol_in) = token_spendable_sol_in {
                        const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;
                        let token_spendable_sol_in_sol =
                            token_spendable_sol_in as f64 / LAMPORTS_PER_SOL;
                        let tol = spendable_sol_in_constraint.abs() * (rule.tolerance_pct * 0.01);
                        if (token_spendable_sol_in_sol - spendable_sol_in_constraint).abs()
                            > tol + 1e-15
                        {
                            continue;
                        }
                    } else {
                        continue;
                    }
                } else {
                    continue;
                }
            }

            // Check p_ix_labels constraint (optional, simple presence check)
            if !rule.p_ix_labels.is_null()
                && !rule.p_ix_labels.as_array().unwrap_or(&vec![]).is_empty()
            {
                if token.instruction_labels.is_null()
                    || token
                        .instruction_labels
                        .as_array()
                        .unwrap_or(&vec![])
                        .is_empty()
                {
                    continue;
                }
                // TODO: More sophisticated label matching logic
            }

            // All constraints matched!
            return Some(rule.id);
        }

        None
    }

    /// Check whether a holding position should exit. Runs the full exit ladder —
    /// fixed TP/SL plus E1–E4 (trailing / time / stall / liquidity) — over the
    /// token's post-entry trade history via [`simulate_exit`], so the live path
    /// honors exactly the same exits as the backtest. `trades` is the token's
    /// in-memory chronological history (from the cache). Returns the triggering
    /// reason, or `None` if no condition fires.
    pub fn check_exit(
        &self,
        position: &Position,
        trades: &[crate::models::trade::Trade],
        rule: &StrategyTPSLRule,
    ) -> Option<ExitReason> {
        if position.status != crate::models::PositionStatus::Holding {
            return None;
        }

        // Entry fill not recorded yet (entry_price/entry_time are set together,
        // asynchronously, after the buy is indexed). With no entry there is
        // nothing to evaluate against — and a 0 entry price would yield +inf and
        // fire a phantom TakeProfit, flapping the position ExitPending→Holding.
        if position.entry_price <= 0.0 {
            return None;
        }
        let entry_time = position.entry_time?;

        let exit = super::simulation_tpsl::simulate_exit(
            trades,
            entry_time,
            position.entry_price,
            rule,
        )?;
        ExitReason::from_sim_reason(&exit.3)
    }

    /// Wall-clock exit gate. Unlike [`check_exit`] — which only re-evaluates when
    /// a trade prints — this is driven by a timer and measured against `now`, so
    /// the deadline-style exits fire even when a token has gone silent and no new
    /// trade is arriving. It checks **only** the time-based exits, E2 TimeStop and
    /// E3 Stall, both measured from the position's recorded entry. Price-based
    /// exits (TP/SL/Trailing/Liquidity) can only change on a trade, so they stay
    /// on the trade-driven path and are deliberately excluded here — that keeps the
    /// two paths non-overlapping and avoids double-firing.
    ///
    /// Stall outranks TimeStop, matching `simulate_exit`'s ladder order. Returns
    /// the triggering reason, or `None` if no time-based condition is due.
    pub fn check_time_exit(
        &self,
        position: &Position,
        trades: &[crate::models::trade::Trade],
        rule: &StrategyTPSLRule,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Option<ExitReason> {
        if position.status != crate::models::PositionStatus::Holding {
            return None;
        }
        // No recorded entry yet ⇒ no clock to measure from (mirrors `check_exit`).
        if position.entry_price <= 0.0 {
            return None;
        }
        let entry_time = position.entry_time?;

        let time_stop_secs = ignore_zero_u64(rule.p_time_stop_secs);
        let stall_secs = ignore_zero_u64(rule.p_stall_secs);
        if time_stop_secs.is_none() && stall_secs.is_none() {
            return None;
        }

        // E3 · Stall: derive the last new-higher-high time from history, then
        // measure the flatline against `now` (not the last trade's time).
        if let Some(secs) = stall_secs {
            let (_, last_higher_high_time) = super::simulation_tpsl::peak_state_since_entry(
                trades,
                entry_time,
                position.entry_price,
            );
            if (now - last_higher_high_time).num_seconds() >= secs as i64 {
                return Some(ExitReason::Stall);
            }
        }

        // E2 · Time stop: held past the deadline. Lowest priority of the two.
        if let Some(secs) = time_stop_secs {
            if now >= entry_time + chrono::Duration::seconds(secs as i64) {
                return Some(ExitReason::TimeStop);
            }
        }

        None
    }

    /// Get a specific rule by ID.
    pub fn get_rule(&self, rule_id: Uuid) -> Option<&StrategyTPSLRule> {
        self.rules.iter().find(|r| r.id == rule_id)
    }
}

/// Check whether a token satisfies a single rule's entry criteria.
/// Unlike `check_buy_entry`, this does not require the rule to be active.
pub fn token_matches_rule(token: &Token, rule: &StrategyTPSLRule) -> bool {
    let p_initial_buy_sol = ignore_zero_f64(rule.p_initial_buy_sol);
    let p_cu_limit = ignore_zero_u64(rule.p_cu_limit);
    let p_cu_price = ignore_zero_u64(rule.p_cu_price);
    let p_max_sol_cost = ignore_zero_f64(rule.p_max_sol_cost);
    let p_spendable_sol_in = ignore_zero_f64(rule.p_spendable_sol_in);
    let has_ix_labels = rule.p_ix_labels.as_array().map_or(false, |a| !a.is_empty());

    if p_initial_buy_sol.is_none()
        && p_cu_limit.is_none()
        && p_cu_price.is_none()
        && p_max_sol_cost.is_none()
        && p_spendable_sol_in.is_none()
        && !has_ix_labels
    {
        return false;
    }

    if let Some(rule_val) = p_initial_buy_sol {
        match token.initial_buy_sol {
            Some(v) => {
                let tol = rule_val.abs() * (rule.tolerance_pct * 0.01);
                if (v - rule_val).abs() > tol + 1e-9 {
                    return false;
                }
            }
            None => return false,
        }
    }

    if let Some(rule_val) = p_cu_limit {
        match token.cu_limit {
            Some(v) => {
                let tol = rule_val as f64 * (rule.tolerance_pct * 0.01);
                if (v as f64 - rule_val as f64).abs() > tol + 1e-15 {
                    return false;
                }
            }
            None => return false,
        }
    }

    if let Some(rule_val) = p_cu_price {
        match token.cu_price {
            Some(v) => {
                let tol = rule_val as f64 * (rule.tolerance_pct * 0.01);
                if (v as f64 - rule_val as f64).abs() > tol + 1e-9 {
                    return false;
                }
            }
            None => return false,
        }
    }

    if let Some(rule_val) = p_max_sol_cost {
        const LAMPORTS: f64 = 1_000_000_000.0;
        let raw = token.initial_buy_instruction.as_ref().and_then(|ix| {
            ix.get("max_sol_cost")
                .and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
        });
        match raw {
            Some(v) => {
                let sol = v as f64 / LAMPORTS;
                let tol = rule_val.abs() * (rule.tolerance_pct * 0.01);
                if (sol - rule_val).abs() > tol + 1e-15 {
                    return false;
                }
            }
            None => return false,
        }
    }

    if let Some(rule_val) = p_spendable_sol_in {
        const LAMPORTS: f64 = 1_000_000_000.0;
        let raw = token.initial_buy_instruction.as_ref().and_then(|ix| {
            ix.get("spendable_sol_in")
                .and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
        });
        match raw {
            Some(v) => {
                let sol = v as f64 / LAMPORTS;
                let tol = rule_val.abs() * (rule.tolerance_pct * 0.01);
                if (sol - rule_val).abs() > tol + 1e-15 {
                    return false;
                }
            }
            None => return false,
        }
    }

    if has_ix_labels {
        if token.instruction_labels.is_null()
            || token.instruction_labels.as_array().map_or(true, |a| a.is_empty())
        {
            return false;
        }
    }

    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitReason {
    TakeProfit,
    StopLoss,
    /// E1 · price fell below the trailing peak.
    TrailingStop,
    /// E3 · no new higher-high for the stall window.
    Stall,
    /// E2 · held past the time-stop deadline.
    TimeStop,
    /// E4 · virtual SOL reserves crashed below their peak.
    LiquidityExit,
}

impl ExitReason {
    /// Map a [`simulate_exit`] reason string to the typed reason. Returns `None`
    /// for `"Open"` or any unrecognized value (i.e. no exit). Keeping this in
    /// sync with `simulate_exit`'s reason strings is what lets the live path and
    /// the simulation share one exit ladder.
    fn from_sim_reason(reason: &str) -> Option<Self> {
        match reason {
            "TakeProfit" => Some(Self::TakeProfit),
            "StopLoss" => Some(Self::StopLoss),
            "TrailingStop" => Some(Self::TrailingStop),
            "Stall" => Some(Self::Stall),
            "TimeStop" => Some(Self::TimeStop),
            "LiquidityExit" => Some(Self::LiquidityExit),
            _ => None,
        }
    }
}

impl std::fmt::Display for ExitReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::TakeProfit => "TakeProfit",
            Self::StopLoss => "StopLoss",
            Self::TrailingStop => "TrailingStop",
            Self::Stall => "Stall",
            Self::TimeStop => "TimeStop",
            Self::LiquidityExit => "LiquidityExit",
        };
        write!(f, "{s}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::trade::{Trade, TradeType};
    use crate::models::PositionStatus;
    use chrono::{DateTime, Duration, Utc};

    fn base_time() -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap()
    }

    /// A buy whose `price_per_token` equals `price` (token_amount = 1.0), at
    /// `base_time() + secs`.
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

    /// Minimal rule with explicit TP/SL + optional E1/E2/E3/E4; else inert.
    fn rule_with(
        take_profit: f64,
        stop_loss: f64,
        trailing: Option<f64>,
        time_stop_secs: Option<u64>,
        stall_secs: Option<u64>,
        liquidity_drop_pct: Option<f64>,
    ) -> StrategyTPSLRule {
        StrategyTPSLRule::new(
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

    fn check(pos: &Position, trades: &[Trade], rule: &StrategyTPSLRule) -> Option<ExitReason> {
        TPSL1StrategyHandler::new(vec![rule.clone()]).check_exit(pos, trades, rule)
    }

    // E1 fires in the live gate, not just the sim: moon to 3.0 then reverse to
    // 2.0 (≤ peak·0.7 = 2.1) trips the 30% trailing stop.
    #[test]
    fn live_gate_fires_trailing_stop() {
        let rule = rule_with(1000.0, 90.0, Some(30.0), None, None, None);
        let pos = holding(1.0, rule.id);
        let trades = vec![buy(2.0, 2, 1), buy(3.0, 3, 2), buy(2.0, 4, 3)];
        assert_eq!(check(&pos, &trades, &rule), Some(ExitReason::TrailingStop));
    }

    // The legacy fixed exits still fire through the same path.
    #[test]
    fn live_gate_fires_take_profit() {
        let rule = rule_with(50.0, 90.0, None, None, None, None);
        let pos = holding(1.0, rule.id);
        let trades = vec![buy(2.0, 2, 1)]; // +100% ≥ 50
        assert_eq!(check(&pos, &trades, &rule), Some(ExitReason::TakeProfit));
    }

    // All exit params unset + a flat series ⇒ no exit (inert by default).
    #[test]
    fn live_gate_inert_when_nothing_triggers() {
        let rule = rule_with(1000.0, 90.0, None, None, None, None);
        let pos = holding(1.0, rule.id);
        let trades = vec![buy(1.0, 2, 1), buy(1.0, 3, 2)];
        assert_eq!(check(&pos, &trades, &rule), None);
    }

    // Entry not recorded yet (entry_time None) ⇒ the gate can't evaluate, even
    // on a crash that would otherwise stop-loss.
    #[test]
    fn live_gate_waits_for_recorded_entry() {
        let rule = rule_with(50.0, 20.0, None, None, None, None);
        let mut pos = holding(1.0, rule.id);
        pos.entry_time = None;
        let trades = vec![buy(0.1, 2, 1)]; // -90%
        assert_eq!(check(&pos, &trades, &rule), None);
    }

    // A position already exiting is never re-triggered.
    #[test]
    fn live_gate_skips_non_holding() {
        let rule = rule_with(50.0, 20.0, None, None, None, None);
        let mut pos = holding(1.0, rule.id);
        pos.status = PositionStatus::ExitPending;
        let trades = vec![buy(0.1, 2, 1)];
        assert_eq!(check(&pos, &trades, &rule), None);
    }

    // ── Time-driven exit gate (`check_time_exit`) ────────────────────────────

    fn check_time(
        pos: &Position,
        trades: &[Trade],
        rule: &StrategyTPSLRule,
        now: DateTime<Utc>,
    ) -> Option<ExitReason> {
        TPSL1StrategyHandler::new(vec![rule.clone()]).check_time_exit(pos, trades, rule, now)
    }

    // The core bug fix: a token whose last trade was long ago still time-stops
    // out, because the deadline is measured against `now`, not the last trade.
    // Last trade at +30s; time stop 300s; now = entry + 10 min ⇒ TimeStop.
    #[test]
    fn time_exit_fires_after_silence() {
        let rule = rule_with(1000.0, 90.0, None, Some(300), None, None);
        let pos = holding(1.0, rule.id);
        let trades = vec![buy(1.0, 2, 30)]; // one trade then silence
        let now = base_time() + Duration::seconds(600);
        assert_eq!(check_time(&pos, &trades, &rule, now), Some(ExitReason::TimeStop));
    }

    // Stall measured against `now`: peak at +10s, then silence; with stall 60s
    // and now = entry + 5 min, the flatline trips even with no further trades.
    #[test]
    fn time_exit_stall_fires_during_silence() {
        let rule = rule_with(1000.0, 90.0, None, None, Some(60), None);
        let pos = holding(1.0, rule.id);
        let trades = vec![buy(2.0, 2, 10)]; // new high at +10s, then quiet
        let now = base_time() + Duration::seconds(300);
        assert_eq!(check_time(&pos, &trades, &rule, now), Some(ExitReason::Stall));
    }

    // Stall outranks TimeStop when both are due on the same sweep (matches the
    // `simulate_exit` ladder order).
    #[test]
    fn time_exit_stall_outranks_time_stop() {
        let rule = rule_with(1000.0, 90.0, None, Some(100), Some(60), None);
        let pos = holding(1.0, rule.id);
        let trades = vec![buy(2.0, 2, 10)];
        let now = base_time() + Duration::seconds(300);
        assert_eq!(check_time(&pos, &trades, &rule, now), Some(ExitReason::Stall));
    }

    // Before either deadline, nothing fires.
    #[test]
    fn time_exit_inert_before_deadline() {
        let rule = rule_with(1000.0, 90.0, None, Some(300), Some(300), None);
        let pos = holding(1.0, rule.id);
        let trades = vec![buy(1.0, 2, 30)];
        let now = base_time() + Duration::seconds(100);
        assert_eq!(check_time(&pos, &trades, &rule, now), None);
    }

    // No time-based params set ⇒ the gate is a no-op regardless of elapsed time.
    #[test]
    fn time_exit_inert_when_unconfigured() {
        let rule = rule_with(50.0, 20.0, Some(30.0), None, None, Some(50.0));
        let pos = holding(1.0, rule.id);
        let trades = vec![buy(1.0, 2, 30)];
        let now = base_time() + Duration::seconds(86_400);
        assert_eq!(check_time(&pos, &trades, &rule, now), None);
    }

    // Entry not recorded yet (entry_time None) ⇒ no clock, no exit.
    #[test]
    fn time_exit_waits_for_recorded_entry() {
        let rule = rule_with(1000.0, 90.0, None, Some(60), None, None);
        let mut pos = holding(1.0, rule.id);
        pos.entry_time = None;
        let now = base_time() + Duration::seconds(600);
        assert_eq!(check_time(&pos, &[], &rule, now), None);
    }
}
