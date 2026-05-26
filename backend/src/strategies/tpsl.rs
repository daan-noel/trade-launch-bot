use crate::models::{Position, StrategyTPSLRule, Token};
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

            // Check p_initial_buy_sol constraint
            if let Some(initial_buy) = token.initial_buy_sol {
                if (initial_buy - rule.p_initial_buy_sol).abs() > 0.01 {
                    continue;
                }
            } else {
                continue;
            }

            // Check p_cu_limit constraint (optional)
            if let Some(cu_limit_constraint) = rule.p_cu_limit {
                if let Some(token_cu_limit) = token.cu_limit {
                    if token_cu_limit != cu_limit_constraint {
                        continue;
                    }
                } else {
                    continue;
                }
            }

            // Check p_cu_price constraint (optional)
            if let Some(cu_price_constraint) = rule.p_cu_price {
                if let Some(token_cu_price) = token.cu_price {
                    if token_cu_price != cu_price_constraint {
                        continue;
                    }
                } else {
                    continue;
                }
            }

            // Check p_ix_labels constraint (optional, simple presence check)
            if !rule.p_ix_labels.is_null() && !rule.p_ix_labels.as_array().unwrap_or(&vec![]).is_empty() {
                if token.instruction_labels.is_null() || token.instruction_labels.as_array().unwrap_or(&vec![]).is_empty() {
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
    pub fn check_exit(&self, position: &Position, current_price: f64, rule: &StrategyTPSLRule) -> Option<ExitReason> {
        if position.status != crate::models::PositionStatus::Holding {
            return None;
        }

        let price_change_percent = ((current_price - position.entry_price) / position.entry_price) * 100.0;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tpsl_buy_entry() {
        let rule = StrategyTPSLRule::new(
            "Test Rule".to_string(),
            0.5,
            None,
            None,
            serde_json::json!([]),
            1.0,
            50.0,
            20.0,
        );

        let handler = TPSLStrategyHandler::new(vec![rule]);

        // Token matching the rule
        let token = Token::new(
            "ABC123".to_string(),
            "creator".to_string(),
            "Test Token".to_string(),
            "TEST".to_string(),
            None,
            Some(1_000_000),
            Some(0.5),
            None,
            None,
            false,
            serde_json::json!([]),
            "tx_sig".to_string(),
            chrono::Utc::now(),
        );

        assert!(handler.check_buy_entry(&token).is_some());
    }
}
