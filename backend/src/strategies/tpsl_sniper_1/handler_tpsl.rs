use super::util::{ignore_zero_f64, ignore_zero_u64};
use crate::models::{Position, StrategyTPSLRule, Token};
use tracing::warn;
use uuid::Uuid;

/// TPSL (Take Profit Stop Loss) Strategy Handler
///
/// This strategy analyzes tokens based on defined rules and manages buy/sell positions.
/// - On token creation: Check if token matches buy entry rule criteria
/// - On trade events: Monitor positions for exit conditions (take profit or stop loss)
pub struct TPSLStrategyHandler {
    rules: Vec<StrategyTPSLRule>,
}

impl TPSLStrategyHandler {
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

    /// Check if a position should exit based on take profit or stop loss.
    /// Returns Some(ExitReason) if exit should occur, None otherwise.
    pub fn check_exit(
        &self,
        position: &Position,
        current_price: f64,
        rule: &StrategyTPSLRule,
    ) -> Option<ExitReason> {
        if position.status != crate::models::PositionStatus::Holding {
            return None;
        }

        let price_change_percent =
            ((current_price - position.entry_price) / position.entry_price) * 100.0;

        // Check take profit
        if price_change_percent >= rule.take_profit {
            return Some(ExitReason::TakeProfit);
        }

        // Check stop loss
        if price_change_percent <= -rule.stop_loss {
            return Some(ExitReason::StopLoss);
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
}

impl std::fmt::Display for ExitReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TakeProfit => write!(f, "TakeProfit"),
            Self::StopLoss => write!(f, "StopLoss"),
        }
    }
}
