//! Rule-CRUD **domain** — validate → build a rule → repo write.
//!
//! [`RuleDraft`] → [`StrategyRule`] (`strategy_rules`): a fingerprint reference
//! plus [`RuleParams`] metric conditions — the one generic-engine rule shape (the
//! legacy per-strategy tpsl1/tpsl2/swing1 CRUD was retired in Phase 7).
//!
//! Touches only validation + the repo; never a runtime cache, SSE, or RPC. The
//! calling edge appends its side effects (`rules_changed` + an engine reload on
//! `live`, nothing on `lab`) and owns the request→draft mapping + the live-edit
//! frozen-field guard.

use chrono::Utc;
use serde_json::Value;
use uuid::Uuid;

use crate::models::fingerprint::opt_i64;
use crate::models::StrategyRule;
use crate::storage::repositories::rule_repo::RuleRepo;

use super::rule_params::RuleParams;

/// Outcome of a rule CRUD write, mapped to an HTTP status by the calling edge.
#[derive(Debug)]
pub enum RuleError {
    /// 400 — a validation check rejected the proposed rule.
    Invalid(String),
    /// 500 — a repository call failed.
    Repo(anyhow::Error),
}

// ═══════════════════════════════════════════════════════════════════════════
// Generic engine (fingerprint + metrics) rule CRUD
// ═══════════════════════════════════════════════════════════════════════════

/// The authored inputs for a new generic rule. `params` is the raw JSON body —
/// [`build_rule`] parses/validates it against the metric registry
/// ([`RuleParams::parse`]) and stores the canonical serialization.
#[derive(Debug, Clone)]
pub struct RuleDraft {
    pub rule_name: String,
    pub fingerprint_id: Uuid,
    pub trade_mode: String,
    pub buy_amount_lamports: i64,
    pub max_concurrent_tokens: i64,
    /// 0 = unlimited.
    pub max_total_tokens: i64,
    pub params: Value,
}

impl RuleDraft {
    /// Parse a create body from raw HTTP JSON — SSOT for the wire shape (shared
    /// by the live + lab CRUD handlers). `fingerprint_id` is required; every
    /// other field falls back to its default ([`build_rule`] / [`RuleParams::parse`]
    /// do the real validation). Amounts are lamports on the wire.
    pub fn from_json(body: &Value) -> Result<Self, String> {
        let fingerprint_id = body
            .get("fingerprint_id")
            .and_then(|v| v.as_str())
            .and_then(|s| Uuid::parse_str(s).ok())
            .ok_or("fingerprint_id is required (uuid)")?;
        Ok(RuleDraft {
            rule_name: body.get("rule_name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            fingerprint_id,
            trade_mode: body
                .get("trade_mode")
                .and_then(|v| v.as_str())
                .unwrap_or("paper")
                .to_string(),
            buy_amount_lamports: opt_i64(body, "buy_amount_lamports").unwrap_or(0),
            max_concurrent_tokens: opt_i64(body, "max_concurrent_tokens").unwrap_or(1),
            max_total_tokens: opt_i64(body, "max_total_tokens").unwrap_or(0),
            params: body.get("params").cloned().unwrap_or_else(|| serde_json::json!({})),
        })
    }
}

/// Overlay the mutable fields of a PUT body onto a loaded rule — SSOT for the
/// patch shape (shared by live + lab). `fingerprint_id` and `is_active` are
/// intentionally not patchable here (fingerprint frozen post-create; active
/// state only via the lifecycle endpoints).
pub fn apply_rule_update(rule: &mut StrategyRule, body: &Value) {
    if let Some(v) = body.get("rule_name").and_then(|v| v.as_str()) {
        rule.rule_name = v.to_string();
    }
    if let Some(v) = opt_i64(body, "buy_amount_lamports") {
        rule.buy_amount_lamports = v;
    }
    if let Some(v) = opt_i64(body, "max_concurrent_tokens") {
        rule.max_concurrent_tokens = v;
    }
    if let Some(v) = opt_i64(body, "max_total_tokens") {
        rule.max_total_tokens = v;
    }
    if let Some(v) = body.get("trade_mode").and_then(|v| v.as_str()) {
        rule.trade_mode = v.to_string();
    }
    if let Some(v) = body.get("params").cloned() {
        rule.params = v;
    }
    rule.updated_at = Utc::now();
}

/// Validate the typed-column knobs shared by create and edit.
fn validate_rule_fields(
    rule_name: &str,
    trade_mode: &str,
    buy_amount_lamports: i64,
    max_concurrent_tokens: i64,
    max_total_tokens: i64,
) -> Result<(), String> {
    if rule_name.trim().is_empty() {
        return Err("rule_name must not be empty".into());
    }
    if trade_mode != "paper" && trade_mode != "real" {
        return Err("trade_mode must be 'paper' or 'real'".into());
    }
    if buy_amount_lamports <= 0 {
        return Err("buy_amount_lamports must be > 0".into());
    }
    if max_concurrent_tokens < 1 {
        return Err("max_concurrent_tokens must be >= 1".into());
    }
    if max_total_tokens < 0 {
        return Err("max_total_tokens must be >= 0 (0 = unlimited)".into());
    }
    Ok(())
}

/// Validate + assemble a new (unpersisted) [`StrategyRule`] from a draft. The
/// new rule is inactive (`is_active = false`); a lifecycle endpoint activates
/// it. Params are stored in their canonical serialization
/// ([`RuleParams::to_value`]) so the JSONB shape can't drift by author.
pub fn build_rule(draft: &RuleDraft) -> Result<StrategyRule, String> {
    validate_rule_fields(
        &draft.rule_name,
        &draft.trade_mode,
        draft.buy_amount_lamports,
        draft.max_concurrent_tokens,
        draft.max_total_tokens,
    )?;
    let params = RuleParams::parse(&draft.params)?;
    let now = Utc::now();
    Ok(StrategyRule {
        id: Uuid::new_v4(),
        rule_name: draft.rule_name.clone(),
        fingerprint_id: draft.fingerprint_id,
        trade_mode: draft.trade_mode.clone(),
        is_active: false,
        buy_amount_lamports: draft.buy_amount_lamports,
        max_concurrent_tokens: draft.max_concurrent_tokens,
        max_total_tokens: draft.max_total_tokens,
        params: params.to_value(),
        created_at: now,
        updated_at: now,
    })
}

/// Validate, build, and persist a new generic rule.
pub async fn create(repo: &RuleRepo, draft: &RuleDraft) -> Result<StrategyRule, RuleError> {
    let rule = build_rule(draft).map_err(RuleError::Invalid)?;
    repo.insert(&rule).await.map_err(RuleError::Repo)?;
    Ok(rule)
}

/// Re-validate a fully-formed generic rule and persist the edit. The caller
/// merges its request patch into the loaded rule first (request-shaped,
/// edge-side) and enforces any live-edit frozen-field guard.
pub async fn save(repo: &RuleRepo, rule: &StrategyRule) -> Result<(), RuleError> {
    validate_rule_fields(
        &rule.rule_name,
        &rule.trade_mode,
        rule.buy_amount_lamports,
        rule.max_concurrent_tokens,
        rule.max_total_tokens,
    )
    .map_err(RuleError::Invalid)?;
    RuleParams::parse(&rule.params)
        .map_err(|e| RuleError::Invalid(format!("invalid params: {e}")))?;
    repo.update(rule).await.map_err(RuleError::Repo)?;
    Ok(())
}

#[cfg(test)]
mod generic_tests {
    use super::*;
    use serde_json::json;

    fn generic_draft(params: Value) -> RuleDraft {
        RuleDraft {
            rule_name: "r".into(),
            fingerprint_id: Uuid::new_v4(),
            trade_mode: "paper".into(),
            buy_amount_lamports: 1_000_000_000, // 1 SOL
            max_concurrent_tokens: 3,
            max_total_tokens: 0,
            params,
        }
    }

    fn valid_params() -> Value {
        json!({
            "take_profit": 100,
            "stop_loss": 30,
            "entry": {
                "m_snapshot": {
                    "time": [
                        {"operator": ">", "value": 10},
                        {"operator": "<", "value": 30}
                    ]
                }
            }
        })
    }

    #[test]
    fn valid_generic_rule_builds_inactive_with_canonical_params() {
        let rule = build_rule(&generic_draft(valid_params())).expect("valid");
        assert!(!rule.is_active);
        assert_eq!(rule.buy_amount_lamports, 1_000_000_000);
        assert!((rule.buy_amount_sol() - 1.0).abs() < 1e-12);
        // Stored params are the canonical serialization — re-parsing and
        // re-serializing is a fixed point.
        let reparsed = RuleParams::parse(&rule.params).unwrap();
        assert_eq!(reparsed.to_value(), rule.params);
    }

    #[test]
    fn generic_field_validation() {
        let mut d = generic_draft(valid_params());
        d.trade_mode = "live".into();
        assert!(matches!(build_rule(&d), Err(e) if e.contains("trade_mode")));

        let mut d = generic_draft(valid_params());
        d.buy_amount_lamports = 0;
        assert!(matches!(build_rule(&d), Err(e) if e.contains("buy_amount_lamports")));

        let mut d = generic_draft(valid_params());
        d.max_concurrent_tokens = 0;
        assert!(matches!(build_rule(&d), Err(e) if e.contains("max_concurrent_tokens")));

        let mut d = generic_draft(valid_params());
        d.rule_name = "  ".into();
        assert!(matches!(build_rule(&d), Err(e) if e.contains("rule_name")));
    }

    #[test]
    fn generic_params_registry_checked() {
        // A typo'd metric can't silently no-op — it fails the save.
        let d = generic_draft(json!({
            "entry": {"m_snapshot": {"tyme": [{"operator": ">", "value": 1}]}}
        }));
        assert!(matches!(build_rule(&d), Err(e) if e.contains("tyme")));

        // Contradictory conditions fail the save.
        let d = generic_draft(json!({
            "entry": {"m_snapshot": {"time": [
                {"operator": ">", "value": 30},
                {"operator": "<", "value": 10}
            ]}}
        }));
        assert!(matches!(build_rule(&d), Err(e) if e.contains("contradictory")));

        // Fingerprint-only rule (empty params) is legal: enter on arm, TP/SL off.
        assert!(build_rule(&generic_draft(json!({}))).is_ok());
    }
}
