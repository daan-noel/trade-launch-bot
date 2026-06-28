use std::sync::Arc;

use trading_core::models::{Tpsl1Rule, Token};
use uuid::Uuid;

/// Holds the active TPSL rule set for the live path and answers two questions on
/// token creation: which rule (if any) a token matches, and lookups by id. The
/// matching logic itself lives in [`super::entry`]; this is a thin holder.
///
/// Rules are shared by `Arc`: a new handler is built on every token creation, so
/// the active set is cloned by pointer (one atomic bump) rather than deep-copied.
pub struct TPSL1StrategyHandler {
    rules: Arc<Vec<Tpsl1Rule>>,
}

impl TPSL1StrategyHandler {
    pub fn new(rules: Arc<Vec<Tpsl1Rule>>) -> Self {
        Self { rules }
    }

    /// All active rules whose buy criteria the token satisfies, in rule-list order.
    pub fn check_all_buy_entries(&self, token: &Token) -> Vec<Uuid> {
        super::entry::find_all_matching_buy_rules(token, &self.rules)
    }

    /// Get a specific rule by ID.
    pub fn get_rule(&self, rule_id: Uuid) -> Option<&Tpsl1Rule> {
        self.rules.iter().find(|r| r.id == rule_id)
    }
}
