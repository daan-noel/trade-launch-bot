//! Unified rule-CRUD **domain**, keyed by `strategy_id` — validate → build a
//! [`StrategyRule`] → repo write. Replaces the hand-cloned `tpsl1`/`tpsl2` modules
//! in `api/handlers/strategies/tpsl_rules_core.rs`: one domain validates and
//! assembles every strategy's rule because the strategy-specific checks are
//! dispatched through the [`registry`](super::registry) params, not branched per
//! handler.
//!
//! Touches only validation + the `strategy_repo`; never the runtime cache, SSE,
//! or RPC. The calling edge (Phase 3 handlers) appends its side effects
//! (cache reload + `rules_changed` on `live`, nothing on `lab`) and owns the
//! request→draft mapping + the live-edit frozen-field guard.

use chrono::Utc;
use serde_json::Value;
use uuid::Uuid;

use crate::models::StrategyRule;
use crate::storage::repositories::strategy_repo::StrategyRepo;

use super::registry::{StrategyImpl, StrategyParams, Tpsl1Params, Tpsl2Params};

/// Outcome of a rule CRUD write, mapped to an HTTP status by the calling edge.
#[derive(Debug)]
pub enum RuleError {
    /// 400 — a validation check rejected the proposed rule.
    Invalid(String),
    /// 500 — a repository call failed.
    Repo(anyhow::Error),
}

/// The authored inputs for a new rule: the universal (typed-column) knobs plus
/// the strategy-specific [`StrategyParams`] brain.
#[derive(Debug, Clone)]
pub struct RuleDraft {
    pub strategy: StrategyImpl,
    pub rule_name: String,
    pub buy_amount: f64,
    pub trade_mode: String,
    pub max_concurrent_tokens: Option<i64>,
    pub max_total_tokens: Option<i64>,
    pub params: StrategyParams,
}

/// Serialize parsed params back to the JSONB stored on `strategy_rules.params`.
pub fn params_to_value(params: &StrategyParams) -> Value {
    match params {
        StrategyParams::Tpsl1(p) => serde_json::to_value(p).unwrap_or(Value::Null),
        StrategyParams::Tpsl2(p) => serde_json::to_value(p).unwrap_or(Value::Null),
    }
}

/// Validate a rule's strategy-specific params before persistence. Dispatches by
/// the params variant; `Err(message)` on the first violation.
pub fn validate(params: &StrategyParams) -> Result<(), String> {
    match params {
        StrategyParams::Tpsl1(p) => validate_tpsl1(p),
        StrategyParams::Tpsl2(p) => validate_tpsl2(p),
    }
}

/// Take Profit unbounded above (`> 0`); every other percent clamped to `0–100`
/// (`0` = disable sentinel, stays legal).
fn validate_tpsl1(p: &Tpsl1Params) -> Result<(), String> {
    if p.p_exit_take_profit <= 0.0 {
        return Err("Take Profit % must be greater than 0".into());
    }
    check_bounded(&[
        ("Stop Loss %", p.p_exit_stop_loss),
        ("Tolerance %", p.tolerance_pct),
        ("Trailing Stop %", p.p_exit_trailing_stop_pct.unwrap_or(0.0)),
        ("Liquidity Drop %", p.p_exit_liquidity_drop_pct.unwrap_or(0.0)),
    ])
}

/// tpsl1's percent rules plus the scalp-continuation gates: pullback / cohort
/// percentages bounded, the entry window non-empty, and at least one scalp gate
/// configured (a tpsl2 rule's only entry path is the scalp trade-window gates).
fn validate_tpsl2(p: &Tpsl2Params) -> Result<(), String> {
    if p.p_exit_take_profit <= 0.0 {
        return Err("Take Profit % must be greater than 0".into());
    }
    check_bounded(&[
        ("Stop Loss %", p.p_exit_stop_loss),
        ("Tolerance %", p.tolerance_pct),
        ("Trailing Stop %", p.p_exit_trailing_stop_pct.unwrap_or(0.0)),
        ("Liquidity Drop %", p.p_exit_liquidity_drop_pct.unwrap_or(0.0)),
        ("Pullback %", p.p_entry_pullback_pct.unwrap_or(0.0)),
        ("Cohort Exit Ratio %", p.p_exit_cohort_ratio.unwrap_or(0.0)),
        ("Max Cohort Held %", p.p_entry_max_cohort_held.unwrap_or(0.0)),
    ])?;

    // Non-empty entry window: when both age bounds are set (nonzero), max > min.
    let nz = |v: Option<u64>| v.filter(|&x| x != 0);
    if let (Some(min), Some(max)) = (nz(p.p_entry_min_age_secs), nz(p.p_entry_max_age_secs)) {
        if max <= min {
            return Err(format!("Max Age ({max}s) must be greater than Min Age ({min}s)"));
        }
    }

    // At least one scalp entry gate — reuse the canonical check.
    if !super::tpsl_sniper_2::entry::rule_configures_any_scalp_gate(&p.to_rule()) {
        return Err("Rule configures no scalp entry gate".into());
    }
    Ok(())
}

fn check_bounded(fields: &[(&str, f64)]) -> Result<(), String> {
    for &(name, v) in fields {
        if !(0.0..=100.0).contains(&v) {
            return Err(format!("{name} must be between 0 and 100"));
        }
    }
    Ok(())
}

/// Validate + assemble a new (unpersisted) [`StrategyRule`] from a draft. The new
/// rule is inactive (`is_active = false`); a lifecycle endpoint activates it.
pub fn build_rule(draft: &RuleDraft) -> Result<StrategyRule, String> {
    if draft.params.strategy() != draft.strategy {
        return Err("params do not match the rule's strategy".into());
    }
    if draft.trade_mode != "paper" && draft.trade_mode != "real" {
        return Err("trade_mode must be 'paper' or 'real'".into());
    }
    validate(&draft.params)?;

    let now = Utc::now();
    Ok(StrategyRule {
        id: Uuid::new_v4(),
        strategy_id: draft.strategy.id().to_string(),
        rule_name: draft.rule_name.clone(),
        buy_amount: draft.buy_amount,
        trade_mode: draft.trade_mode.clone(),
        is_active: false,
        max_concurrent_tokens: draft.max_concurrent_tokens,
        max_total_tokens: draft.max_total_tokens,
        params: params_to_value(&draft.params),
        created_at: now,
        updated_at: now,
    })
}

// ── Repo orchestration (the calling edge appends cache reload / SSE) ───────────

/// Validate, build, and persist a new rule.
pub async fn create(repo: &StrategyRepo, draft: &RuleDraft) -> Result<StrategyRule, RuleError> {
    let rule = build_rule(draft).map_err(RuleError::Invalid)?;
    repo.insert_rule(&rule).await.map_err(RuleError::Repo)?;
    Ok(rule)
}

/// Re-validate a fully-formed rule's params and persist the edit. The caller
/// merges its request patch into the loaded rule first (request-shaped, edge-side)
/// and enforces any live-edit frozen-field guard.
pub async fn save(repo: &StrategyRepo, rule: &StrategyRule) -> Result<(), RuleError> {
    let strategy = StrategyImpl::from_id(&rule.strategy_id)
        .ok_or_else(|| RuleError::Invalid(format!("unknown strategy_id '{}'", rule.strategy_id)))?;
    let params = strategy
        .parse_params(&rule.params)
        .map_err(|e| RuleError::Invalid(format!("invalid params: {e}")))?;
    validate(&params).map_err(RuleError::Invalid)?;
    repo.update_rule(rule).await.map_err(RuleError::Repo)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tpsl1_params(tp: f64, sl: f64) -> Tpsl1Params {
        Tpsl1Params {
            p_token_initial_buy_sol: Some(1.0),
            p_token_cu_limit: None,
            p_token_cu_price: None,
            p_token_max_sol_cost: None,
            p_token_spendable_sol_in: None,
            p_token_ix_labels: json!([]),
            tolerance_pct: 5.0,
            p_exit_take_profit: tp,
            p_exit_stop_loss: sl,
            p_exit_trailing_stop_pct: None,
            p_exit_time_stop_secs: None,
            p_exit_stall_secs: None,
            p_exit_liquidity_drop_pct: None,
        }
    }

    fn draft(strategy: StrategyImpl, params: StrategyParams) -> RuleDraft {
        RuleDraft {
            strategy,
            rule_name: "r".into(),
            buy_amount: 1.0,
            trade_mode: "paper".into(),
            max_concurrent_tokens: Some(3),
            max_total_tokens: None,
            params,
        }
    }

    #[test]
    fn valid_tpsl1_rule_builds() {
        let d = draft(StrategyImpl::Tpsl1, StrategyParams::Tpsl1(tpsl1_params(50.0, 20.0)));
        let rule = build_rule(&d).expect("valid");
        assert_eq!(rule.strategy_id, "tpsl_sniper_1");
        assert!(!rule.is_active);
        assert_eq!(rule.max_concurrent_tokens, Some(3));
        // Params JSONB round-trips back through the registry.
        let reparsed = StrategyImpl::Tpsl1.parse_params(&rule.params).unwrap();
        assert_eq!(reparsed, StrategyParams::Tpsl1(tpsl1_params(50.0, 20.0)));
    }

    #[test]
    fn take_profit_must_be_positive() {
        let d = draft(StrategyImpl::Tpsl1, StrategyParams::Tpsl1(tpsl1_params(0.0, 20.0)));
        assert!(matches!(build_rule(&d), Err(e) if e.contains("Take Profit")));
    }

    #[test]
    fn out_of_range_percent_rejected() {
        let d = draft(StrategyImpl::Tpsl1, StrategyParams::Tpsl1(tpsl1_params(50.0, 150.0)));
        assert!(matches!(build_rule(&d), Err(e) if e.contains("Stop Loss")));
    }

    #[test]
    fn mismatched_strategy_and_params_rejected() {
        // Tpsl2 strategy with Tpsl1 params.
        let d = draft(StrategyImpl::Tpsl2, StrategyParams::Tpsl1(tpsl1_params(50.0, 20.0)));
        assert!(matches!(build_rule(&d), Err(e) if e.contains("do not match")));
    }

    #[test]
    fn bad_trade_mode_rejected() {
        let mut d = draft(StrategyImpl::Tpsl1, StrategyParams::Tpsl1(tpsl1_params(50.0, 20.0)));
        d.trade_mode = "live".into();
        assert!(matches!(build_rule(&d), Err(e) if e.contains("trade_mode")));
    }

    #[test]
    fn tpsl2_requires_a_scalp_gate() {
        let mut rule = crate::models::Tpsl2Rule::new(
            "r".into(), None, None, None, json!([]), "paper".into(), 1.0,
            50.0, 20.0, None, None, None, None, Some(0.0), None, None, None, None,
        );
        rule.is_active = true;
        // No scalp gate configured → rejected.
        let no_gate = StrategyParams::Tpsl2(Tpsl2Params::from_rule(&rule));
        let d = draft(StrategyImpl::Tpsl2, no_gate);
        assert!(matches!(build_rule(&d), Err(e) if e.contains("scalp entry gate")));

        // With a scalp gate (min age) configured → builds.
        rule.p_entry_min_age_secs = Some(5);
        let with_gate = StrategyParams::Tpsl2(Tpsl2Params::from_rule(&rule));
        let d = draft(StrategyImpl::Tpsl2, with_gate);
        assert!(build_rule(&d).is_ok());
    }

    #[test]
    fn tpsl2_empty_entry_window_rejected() {
        let mut rule = crate::models::Tpsl2Rule::new(
            "r".into(), None, None, None, json!([]), "paper".into(), 1.0,
            50.0, 20.0, None, None, None, None, Some(0.0), None, None, None, None,
        );
        rule.p_entry_min_age_secs = Some(60);
        rule.p_entry_max_age_secs = Some(30); // max <= min → empty window
        let d = draft(StrategyImpl::Tpsl2, StrategyParams::Tpsl2(Tpsl2Params::from_rule(&rule)));
        assert!(matches!(build_rule(&d), Err(e) if e.contains("Max Age")));
    }
}
