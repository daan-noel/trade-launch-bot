//! Entry — the single place that decides **whether a new token is bought** and
//! **at what fill**.
//!
//! Two concerns:
//!   • criteria matching ([`token_matches_buy_rule`] / [`find_all_matching_buy_rules`])
//!     — does a token satisfy a rule's buy filters?
//!   • fill resolution ([`find_scalp_entry`], in [`scalp`]) — the first trade
//!     where every configured scalp gate holds is the entry.
//!
//! **To add an entry criterion:** write a `check_*` function returning a
//! [`CriterionOutcome`] and add it to the [`CRITERIA`] list. Every configured
//! criterion must be satisfied for a match, and a rule that configures *no*
//! criterion never matches (so a misconfigured rule can't buy every token).

use chrono::{DateTime, Utc};
use tracing::warn;
use uuid::Uuid;

use super::util::{none_if_zero_f64, none_if_zero_u64};
use crate::config::constants::{LAMPORTS_PER_SOL, MAX_SNIPE_AGE_SECS};
use crate::models::{Tpsl2Rule, Token};

mod scalp;
pub use scalp::{
    find_scalp_entry, find_scalp_entry_with_cohort_indexed, find_worst_case_paper_entry,
    find_worst_case_paper_entry_at, higher_low_confirmed_index, rule_configures_any_scalp_gate,
    scalp_cohort,
};

/// The result of testing one entry criterion against a token.
enum CriterionOutcome {
    /// The rule doesn't set this criterion (param None/zero) — inert, ignored.
    NotConfigured,
    /// Configured and the token satisfies it.
    Satisfied,
    /// Configured and the token fails it — the rule cannot match this token.
    Rejected,
}

/// Every **user-configured** entry criterion, evaluated in order. Adding a
/// filter = add its `check_*` here; nothing else changes. The `MAX_SNIPE_AGE_SECS`
/// freshness gate is deliberately **not** here — it's a live-only safety rail
/// (see [`token_is_fresh`]), so the shared matcher can be reused by the historical
/// matched/simulate scan without rejecting every non-live token.
const CRITERIA: &[fn(&Token, &Tpsl2Rule) -> CriterionOutcome] = &[
    check_initial_buy_sol,
    check_compute_unit_limit,
    check_compute_unit_price,
    check_max_sol_cost,
    check_spendable_sol_in,
    check_instruction_labels,
];

/// Whether a token satisfies a rule's buy criteria. A rule must configure at
/// least one criterion, and every configured criterion must be satisfied.
/// Shared by the live entry gate and the backtest.
pub fn token_matches_buy_rule(token: &Token, rule: &Tpsl2Rule) -> bool {
    let mut any_configured = false;
    for check in CRITERIA {
        match check(token, rule) {
            CriterionOutcome::NotConfigured => {}
            CriterionOutcome::Satisfied => any_configured = true,
            CriterionOutcome::Rejected => return false,
        }
    }
    any_configured
}

/// Whether a token satisfies every *configured* token-level criterion. Unlike
/// [`token_matches_buy_rule`], a rule that configures **no** token criterion
/// passes vacuously. Used as the token pre-filter for the scalp entry path,
/// where the trade-window gates ([`find_scalp_entry`]) do the real gating and a
/// rule may set no creation-instruction filter at all.
pub fn token_criteria_satisfied(token: &Token, rule: &Tpsl2Rule) -> bool {
    for check in CRITERIA {
        if let CriterionOutcome::Rejected = check(token, rule) {
            return false;
        }
    }
    true
}

/// Live-only freshness gate: reject tokens whose `created_at` is more than
/// `MAX_SNIPE_AGE_SECS` old. This is a hard safety rail (don't snipe an already-old
/// token / a gap-replayed create), **not** a user criterion — so it lives outside
/// [`CRITERIA`] and is applied only on the live entry path
/// ([`find_all_matching_buy_rules`] / `StrategyImpl::matches_entry`). The historical
/// matched/simulate scan intentionally skips it. Requires A3 (accurate `created_at`
/// on replayed creates) to work.
pub fn token_is_fresh(token: &Token) -> bool {
    Utc::now().signed_duration_since(token.created_at).num_seconds() <= MAX_SNIPE_AGE_SECS
}

/// All active rules whose criteria the token satisfies, in rule-list order. A
/// rule that configures no criterion is skipped with a warning rather than
/// matching every token. This is a **live** entry path, so the [`token_is_fresh`]
/// safety gate applies.
pub fn find_all_matching_buy_rules(token: &Token, rules: &[Tpsl2Rule]) -> Vec<Uuid> {
    let mut matched = Vec::new();
    if !token_is_fresh(token) {
        return matched;
    }
    for rule in rules {
        if !rule.is_active {
            continue;
        }
        if !rule_configures_any_criterion(rule) {
            warn!(
                "Rule {} has no entry criteria — skipping (would otherwise match every token)",
                rule.id
            );
            continue;
        }
        if token_matches_buy_rule(token, rule) {
            matched.push(rule.id);
        }
    }
    matched
}

/// Whether a rule sets at least one **token-level** entry criterion (used to
/// skip — and warn about — a misconfigured, match-everything rule). The scalp
/// trade-window gates are checked separately via [`rule_configures_any_scalp_gate`].
pub fn rule_configures_any_criterion(rule: &Tpsl2Rule) -> bool {
    none_if_zero_f64(rule.p_token_initial_buy_sol).is_some()
        || none_if_zero_u64(rule.p_token_cu_limit).is_some()
        || none_if_zero_u64(rule.p_token_cu_price).is_some()
        || none_if_zero_f64(rule.p_token_max_sol_cost).is_some()
        || none_if_zero_f64(rule.p_token_spendable_sol_in).is_some()
        || rule.p_token_ix_labels.as_array().map_or(false, |a| !a.is_empty())
}

/// True when `token_val` is within the rule's tolerance band around `rule_val`.
/// Public so swing1's fingerprint pre-filter reuses the exact tpsl2 semantics.
pub fn within_tolerance(token_val: f64, rule_val: f64, tolerance_pct: f64, eps: f64) -> bool {
    let tol = rule_val.abs() * (tolerance_pct * 0.01);
    (token_val - rule_val).abs() <= tol + eps
}

fn check_initial_buy_sol(token: &Token, rule: &Tpsl2Rule) -> CriterionOutcome {
    let Some(rule_val) = none_if_zero_f64(rule.p_token_initial_buy_sol) else {
        return CriterionOutcome::NotConfigured;
    };
    match token.initial_buy_sol {
        Some(v) if within_tolerance(v, rule_val, rule.tolerance_pct, 1e-9) => {
            CriterionOutcome::Satisfied
        }
        _ => CriterionOutcome::Rejected,
    }
}

fn check_compute_unit_limit(token: &Token, rule: &Tpsl2Rule) -> CriterionOutcome {
    let Some(rule_val) = none_if_zero_u64(rule.p_token_cu_limit) else {
        return CriterionOutcome::NotConfigured;
    };
    match token.cu_limit {
        Some(v) if v == rule_val => CriterionOutcome::Satisfied,
        _ => CriterionOutcome::Rejected,
    }
}

fn check_compute_unit_price(token: &Token, rule: &Tpsl2Rule) -> CriterionOutcome {
    let Some(rule_val) = none_if_zero_u64(rule.p_token_cu_price) else {
        return CriterionOutcome::NotConfigured;
    };
    match token.cu_price {
        Some(v) if v == rule_val => CriterionOutcome::Satisfied,
        _ => CriterionOutcome::Rejected,
    }
}

fn check_max_sol_cost(token: &Token, rule: &Tpsl2Rule) -> CriterionOutcome {
    let Some(rule_val) = none_if_zero_f64(rule.p_token_max_sol_cost) else {
        return CriterionOutcome::NotConfigured;
    };
    match instruction_arg_as_sol(token, "max_sol_cost") {
        Some(sol) if within_tolerance(sol, rule_val, rule.tolerance_pct, 1e-15) => {
            CriterionOutcome::Satisfied
        }
        _ => CriterionOutcome::Rejected,
    }
}

fn check_spendable_sol_in(token: &Token, rule: &Tpsl2Rule) -> CriterionOutcome {
    let Some(rule_val) = none_if_zero_f64(rule.p_token_spendable_sol_in) else {
        return CriterionOutcome::NotConfigured;
    };
    match instruction_arg_as_sol(token, "spendable_sol_in") {
        Some(sol) if within_tolerance(sol, rule_val, rule.tolerance_pct, 1e-15) => {
            CriterionOutcome::Satisfied
        }
        _ => CriterionOutcome::Rejected,
    }
}

fn check_instruction_labels(token: &Token, rule: &Tpsl2Rule) -> CriterionOutcome {
    let Some(rule_labels) = rule.p_token_ix_labels.as_array() else {
        return CriterionOutcome::NotConfigured;
    };
    if rule_labels.is_empty() {
        return CriterionOutcome::NotConfigured;
    }
    let token_labels = match token.instruction_labels.as_array() {
        Some(a) => a,
        None => return CriterionOutcome::Rejected,
    };
    // Exact ordered match: same length, same string at every position.
    if rule_labels.len() == token_labels.len()
        && rule_labels.iter().zip(token_labels.iter()).all(|(r, t)| r == t)
    {
        CriterionOutcome::Satisfied
    } else {
        CriterionOutcome::Rejected
    }
}

/// Read a lamports field from the token's creation-instruction args (accepting a
/// number or a numeric string) and convert it to SOL. Public so swing1's
/// fingerprint pre-filter reads `max_sol_cost`/`spendable_sol_in` identically.
pub fn instruction_arg_as_sol(token: &Token, key: &str) -> Option<f64> {
    // Persisted `initial_buy_instruction` rows use camelCase keys (the ingest
    // writer's `buy_ix_to_json`), so accept both snake_case and camelCase.
    let obj = token.initial_buy_instruction.as_ref()?;
    let lamports = obj
        .get(key)
        .or_else(|| crate::grouping::buy_arg_camel(key).and_then(|camel| obj.get(camel)))
        .and_then(|v| {
            v.as_u64()
                .or_else(|| v.as_str().and_then(|s| s.parse::<u64>().ok()))
        })?;
    Some(lamports as f64 / LAMPORTS_PER_SOL as f64)
}

// ── Fill resolution ──────────────────────────────────────────────────────────

/// The resolved entry: the price/tx/time of the trade the position enters on.
#[derive(Debug, Clone, PartialEq)]
pub struct EntryFill {
    pub price: f64,
    /// Token amount of the resolving trade — carried so the target (trigger-trade)
    /// snapshot can persist the trade's own size, not just its price/tx/time.
    /// SOL is never stored; it's derived at display as `price × amount_tokens`.
    pub amount_tokens: f64,
    pub tx_signature: String,
    /// Slot of the fill trade — the unambiguous key the sweep drill-in uses to
    /// resolve this fill's real `tx_signature` from the `trades` table (the slim
    /// `SweepTrade` carries no signature, so the in-row `tx_signature` is empty
    /// on the sweep path). Live reads the signature directly and ignores this.
    pub slot: u64,
    pub block_time: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    // The `token_matches_buy_rule` matcher no longer applies a freshness gate
    // (that moved to the live-only `token_is_fresh`), so criteria tests are
    // timestamp-independent. `token_is_fresh` is exercised separately below.
    fn base_time() -> DateTime<Utc> {
        Utc::now()
    }

    fn token_with(
        initial_buy_sol: Option<f64>,
        cu_limit: Option<u64>,
        cu_price: Option<u64>,
        initial_buy_instruction: Option<Value>,
        instruction_labels: Value,
    ) -> Token {
        Token::new(
            "mint".into(),
            "creator".into(),
            "name".into(),
            "SYM".into(),
            None,
            None,
            None,
            initial_buy_sol,
            initial_buy_instruction,
            cu_limit,
            cu_price,
            false,
            false,
            instruction_labels,
            "create-sig".into(),
            None,
            base_time(),
        )
    }

    /// An active rule with the given entry criteria; exit params inert.
    fn rule_with_entry(
        p_token_initial_buy_sol: Option<f64>,
        p_token_cu_limit: Option<u64>,
        p_token_cu_price: Option<u64>,
        p_token_ix_labels: Value,
        p_token_max_sol_cost: Option<f64>,
        p_token_spendable_sol_in: Option<f64>,
        tolerance_pct: f64,
    ) -> Tpsl2Rule {
        let mut r = Tpsl2Rule::new(
            "test".into(),
            p_token_initial_buy_sol,
            p_token_cu_limit,
            p_token_cu_price,
            p_token_ix_labels,
            "paper".into(),
            1.0,
            50.0,
            20.0,
            p_token_max_sol_cost,
            p_token_spendable_sol_in,
            None,
            None,
            Some(tolerance_pct),
            None,
            None,
            None,
            None,
        );
        r.is_active = true;
        r
    }

    #[test]
    fn matches_initial_buy_sol_within_tolerance() {
        let rule = rule_with_entry(Some(1.0), None, None, json!([]), None, None, 10.0);
        let near = token_with(Some(1.05), None, None, None, json!([]));
        let far = token_with(Some(1.2), None, None, None, json!([]));
        assert!(token_matches_buy_rule(&near, &rule));
        assert!(!token_matches_buy_rule(&far, &rule));
    }

    #[test]
    fn freshness_is_live_only_not_a_matcher_criterion() {
        use chrono::Duration;
        // An old token still matches the pure criteria (analysis/backtest path)…
        let rule = rule_with_entry(Some(1.0), None, None, json!([]), None, None, 10.0);
        let mut old = token_with(Some(1.0), None, None, None, json!([]));
        old.created_at = Utc::now() - Duration::seconds(MAX_SNIPE_AGE_SECS + 60);
        assert!(token_matches_buy_rule(&old, &rule));
        // …but the live-only freshness gate rejects it.
        assert!(!token_is_fresh(&old));
        let fresh = token_with(Some(1.0), None, None, None, json!([]));
        assert!(token_is_fresh(&fresh));
        // find_all_matching (a live path) drops the stale token entirely.
        assert!(find_all_matching_buy_rules(&old, std::slice::from_ref(&rule)).is_empty());
        assert_eq!(
            find_all_matching_buy_rules(&fresh, std::slice::from_ref(&rule)),
            vec![rule.id]
        );
    }

    #[test]
    fn rejects_when_token_field_missing() {
        let rule = rule_with_entry(None, Some(100_000), None, json!([]), None, None, 10.0);
        let token = token_with(Some(1.0), None, None, None, json!([])); // cu_limit None
        assert!(!token_matches_buy_rule(&token, &rule));
    }

    #[test]
    fn max_sol_cost_read_from_instruction_args() {
        let rule = rule_with_entry(None, None, None, json!([]), Some(1.0), None, 0.0);
        let ix = json!({ "max_sol_cost": 1_000_000_000u64 }); // 1 SOL
        let token = token_with(None, None, None, Some(ix), json!([]));
        assert!(token_matches_buy_rule(&token, &rule));
    }

    #[test]
    fn instruction_labels_exact_ordered_match() {
        let rule = rule_with_entry(None, None, None, json!(["A", "B", "C"]), None, None, 0.0);
        // Exact match passes.
        let exact = token_with(None, None, None, None, json!(["A", "B", "C"]));
        assert!(token_matches_buy_rule(&exact, &rule));
        // Wrong order rejected.
        let reordered = token_with(None, None, None, None, json!(["A", "C", "B"]));
        assert!(!token_matches_buy_rule(&reordered, &rule));
        // Subset rejected.
        let subset = token_with(None, None, None, None, json!(["A", "B"]));
        assert!(!token_matches_buy_rule(&subset, &rule));
        // Superset rejected.
        let superset = token_with(None, None, None, None, json!(["A", "B", "C", "D"]));
        assert!(!token_matches_buy_rule(&superset, &rule));
        // Empty token labels rejected.
        let empty = token_with(None, None, None, None, json!([]));
        assert!(!token_matches_buy_rule(&empty, &rule));
        // Different label rejected.
        let diff = token_with(None, None, None, None, json!(["A", "B", "X"]));
        assert!(!token_matches_buy_rule(&diff, &rule));
    }

    // The degenerate-case fix: a rule with only a tolerance and no real criteria
    // must NOT match every token (the old inline check_buy_entry did).
    #[test]
    fn rule_with_no_criteria_never_matches() {
        let rule = rule_with_entry(None, None, None, json!([]), None, None, 10.0);
        let token = token_with(Some(1.0), Some(100_000), None, None, json!([]));
        assert!(!token_matches_buy_rule(&token, &rule));
        assert!(find_all_matching_buy_rules(&token, std::slice::from_ref(&rule)).is_empty());
    }

    #[test]
    fn find_all_matching_skips_inactive_and_returns_all_matches() {
        let token = token_with(Some(1.0), None, None, None, json!([]));
        let mut inactive = rule_with_entry(Some(1.0), None, None, json!([]), None, None, 10.0);
        inactive.is_active = false;
        let active1 = rule_with_entry(Some(1.0), None, None, json!([]), None, None, 10.0);
        let active2 = rule_with_entry(Some(1.0), None, None, json!([]), None, None, 10.0);

        let rules = vec![inactive, active1.clone(), active2.clone()];
        assert_eq!(
            find_all_matching_buy_rules(&token, &rules),
            vec![active1.id, active2.id]
        );
    }
}
