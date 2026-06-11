use crate::models::{Tpsl1StrategyRule, Token};
use uuid::Uuid;

/// Holds the active TPSL rule set for the live path and answers two questions on
/// token creation: which rule (if any) a token matches, and lookups by id. The
/// matching logic itself lives in [`super::entry`]; this is a thin holder.
pub struct TPSL1StrategyHandler {
    rules: Vec<Tpsl1StrategyRule>,
}

impl TPSL1StrategyHandler {
    pub fn new(rules: Vec<Tpsl1StrategyRule>) -> Self {
        Self { rules }
    }

    /// The first active rule whose buy criteria the token satisfies, or None.
    pub fn check_buy_entry(&self, token: &Token) -> Option<Uuid> {
        super::entry::find_first_matching_buy_rule(token, &self.rules)
    }

    /// Get a specific rule by ID.
    pub fn get_rule(&self, rule_id: Uuid) -> Option<&Tpsl1StrategyRule> {
        self.rules.iter().find(|r| r.id == rule_id)
    }
}
