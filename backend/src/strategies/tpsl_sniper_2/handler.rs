use std::sync::Arc;

use crate::models::{Tpsl2StrategyRule, Token};
use uuid::Uuid;

/// Holds the active TPSL rule set for the live path and answers two questions on
/// token creation: which rule (if any) a token matches, and lookups by id. The
/// matching logic itself lives in [`super::entry`]; this is a thin holder.
///
/// Rules are shared by `Arc`: a new handler is built on every token creation, so
/// the active set is cloned by pointer (one atomic bump) rather than deep-copied.
pub struct TPSL2StrategyHandler {
    rules: Arc<Vec<Tpsl2StrategyRule>>,
}

impl TPSL2StrategyHandler {
    pub fn new(rules: Arc<Vec<Tpsl2StrategyRule>>) -> Self {
        Self { rules }
    }

    /// The first active rule whose buy criteria the token satisfies, or None.
    pub fn check_buy_entry(&self, token: &Token) -> Option<Uuid> {
        super::entry::find_first_matching_buy_rule(token, &self.rules)
    }

    /// Get a specific rule by ID.
    pub fn get_rule(&self, rule_id: Uuid) -> Option<&Tpsl2StrategyRule> {
        self.rules.iter().find(|r| r.id == rule_id)
    }
}
