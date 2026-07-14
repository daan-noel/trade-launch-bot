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

use super::registry::{StrategyImpl, StrategyParams, Swing1Params, Tpsl1Params, Tpsl2Params};

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
    pub buy_amount_sol: f64,
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
        StrategyParams::Swing1(p) => serde_json::to_value(p).unwrap_or(Value::Null),
    }
}

/// Validate a rule's strategy-specific params before persistence. Dispatches by
/// the params variant; `Err(message)` on the first violation.
pub fn validate(params: &StrategyParams) -> Result<(), String> {
    match params {
        StrategyParams::Tpsl1(p) => validate_tpsl1(p),
        StrategyParams::Tpsl2(p) => validate_tpsl2(p),
        StrategyParams::Swing1(p) => validate_swing1(p),
    }
}

/// Take Profit unbounded above (`> 0`); every other percent clamped to `0–100`
/// (`0` = disable sentinel, stays legal).
fn validate_tpsl1(p: &Tpsl1Params) -> Result<(), String> {
    if p.p_exit_take_profit <= 0.0 {
        return Err("Take Profit % must be greater than 0".into());
    }
    check_bucket_width(p.bucket_width_sol)?;
    check_bounded(&[
        ("Stop Loss %", p.p_exit_stop_loss),
        ("Trailing Stop %", p.p_exit_trailing_stop_pct.unwrap_or(0.0)),
        ("Liquidity Drop %", p.p_exit_liquidity_drop_pct.unwrap_or(0.0)),
    ])
}

/// tpsl1's percent rules plus the scalp-continuation gates: pullback
/// percentage bounded, the entry window non-empty, and at least one scalp gate
/// configured (a tpsl2 rule's only entry path is the scalp trade-window gates).
fn validate_tpsl2(p: &Tpsl2Params) -> Result<(), String> {
    if p.p_exit_take_profit <= 0.0 {
        return Err("Take Profit % must be greater than 0".into());
    }
    check_bucket_width(p.bucket_width_sol)?;
    check_bounded(&[
        ("Stop Loss %", p.p_exit_stop_loss),
        ("Trailing Stop %", p.p_exit_trailing_stop_pct.unwrap_or(0.0)),
        ("Liquidity Drop %", p.p_exit_liquidity_drop_pct.unwrap_or(0.0)),
        ("Pullback %", p.p_entry_pullback_pct.unwrap_or(0.0)),
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

/// swing1 validation: percent params bounded `0–100`; fractional depth params
/// bounded `0–1`; require ≥1 swing **and** ≥1 kill/volume axis configured; and two
/// shape-sanity checks — a kill must be at least as deep as the volume ceiling, and
/// a volume low must last at least as long as the kill cap.
fn validate_swing1(p: &Swing1Params) -> Result<(), String> {
    if p.p_exit_take_profit <= 0.0 {
        return Err("Take Profit % must be greater than 0".into());
    }
    check_bucket_width(p.bucket_width_sol)?;
    check_bounded(&[
        ("Stop Loss %", p.p_exit_stop_loss),
        ("Trailing Stop %", p.p_exit_trailing_stop_pct.unwrap_or(0.0)),
        ("Liquidity Drop %", p.p_exit_liquidity_drop_pct.unwrap_or(0.0)),
        ("Pullback %", p.p_entry_pullback_pct.unwrap_or(0.0)),
    ])?;
    // Fractional depth params (0–1).
    for (name, v) in [
        ("Kill Depth Min", p.p_kill_depth_min_pct.unwrap_or(0.0)),
        ("Volume Depth Max", p.p_vol_depth_max_pct.unwrap_or(0.0)),
        ("Next-Kill Depth Min", p.p_exit_next_kill_depth_min_pct.unwrap_or(0.0)),
    ] {
        if !(0.0..=1.0).contains(&v) {
            return Err(format!("{name} (fractional) must be between 0 and 1"));
        }
    }

    // Require at least one swing-detection axis and one kill/volume axis so the
    // classifier has something to latch on (never a silent enter-everything path).
    let nz_f = |v: Option<f64>| v.filter(|&x| x != 0.0).is_some();
    let nz_i = |v: Option<i64>| v.filter(|&x| x != 0).is_some();
    let nz_u = |v: Option<u32>| v.filter(|&x| x != 0).is_some();
    let any_swing = nz_f(p.p_swing_high_to_low_sol)
        || nz_f(p.p_swing_high_to_low_pct)
        || nz_f(p.p_swing_low_to_high_sol)
        || nz_f(p.p_swing_low_to_high_pct)
        || nz_u(p.p_swing_min_leg_trades);
    let any_kill = nz_f(p.p_kill_depth_min_pct)
        || nz_i(p.p_kill_max_duration_ms)
        || nz_f(p.p_kill_min_net_flow_per_sec);
    let any_volume = nz_f(p.p_vol_depth_max_pct)
        || nz_i(p.p_vol_min_duration_ms)
        || nz_i(p.p_vol_min_up_duration_ms);
    if !any_swing {
        return Err("swing1 configures no swing-detection axis".into());
    }
    if !any_kill && !any_volume {
        return Err("swing1 configures no kill or volume profile axis".into());
    }
    // Entry needs a higher-low confirmation to fire.
    if !nz_f(p.p_entry_pullback_pct) {
        return Err("swing1 configures no entry higher-low (pullback %) gate".into());
    }

    // Shape sanity: a kill is deeper than the volume ceiling; a volume low lasts
    // at least as long as the kill cap. SSOT predicate shared with the lab
    // param-sweep so both reject the identical set of nonsensical combos.
    swing1_shape_sane(
        p.p_kill_depth_min_pct,
        p.p_vol_depth_max_pct,
        p.p_kill_max_duration_ms,
        p.p_vol_min_duration_ms,
    )?;
    Ok(())
}

/// The two swing1 shape-sanity invariants, shared by rule validation
/// ([`validate_swing1`]) and the lab param-sweep's combo filter so both reject
/// the identical set of nonsensical combos:
///   * a **kill** low must be at least as deep as the **volume** ceiling
///     (`kill_depth_min ≥ vol_depth_max`) — else the depth bands overlap and the
///     phase classifier can't separate a flush from accumulation;
///   * a **volume** low must last at least as long as the **kill** cap
///     (`vol_min_duration ≥ kill_max_duration`) — the same disjointness on time.
///
/// Each check fires only when both of its sides are set (nonzero); `0`/`None` =
/// "no bound", so an unconfigured axis never trips it.
pub fn swing1_shape_sane(
    kill_depth_min_pct: Option<f64>,
    vol_depth_max_pct: Option<f64>,
    kill_max_duration_ms: Option<i64>,
    vol_min_duration_ms: Option<i64>,
) -> Result<(), String> {
    if let (Some(kd), Some(vd)) = (
        kill_depth_min_pct.filter(|&x| x != 0.0),
        vol_depth_max_pct.filter(|&x| x != 0.0),
    ) {
        if kd < vd {
            return Err(format!(
                "Kill Depth Min ({kd}) must be ≥ Volume Depth Max ({vd})"
            ));
        }
    }
    if let (Some(km), Some(vm)) = (
        kill_max_duration_ms.filter(|&x| x != 0),
        vol_min_duration_ms.filter(|&x| x != 0),
    ) {
        if vm < km {
            return Err(format!(
                "Volume Min Duration ({vm}ms) must be ≥ Kill Max Duration ({km}ms)"
            ));
        }
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

/// Validate the fingerprint bucket width (SOL): finite and `> 0`, floored at
/// `MIN_BUCKET_WIDTH_SOL` so the `1e-9` ratio-epsilon in [`crate::grouping::bucket_index`]
/// stays valid and edge labels don't explode in decimals.
fn check_bucket_width(w: f64) -> Result<(), String> {
    if !w.is_finite() || w < crate::grouping::MIN_BUCKET_WIDTH_SOL {
        return Err(format!(
            "Bucket size (SOL) must be finite and ≥ {}",
            crate::grouping::MIN_BUCKET_WIDTH_SOL
        ));
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
        buy_amount_sol: draft.buy_amount_sol,
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
            p_token_first_slot_buy_sol: None,
            p_token_first_slot_sell_sol: None,
            p_token_ix_labels: json!([]),
            bucket_width_sol: 0.1,
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
            buy_amount_sol: 1.0,
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
            50.0, 20.0, None, None, None, None, Some(0.1), None, None, None, None,
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

    fn swing1_rule_min() -> crate::models::Swing1Rule {
        let mut r = crate::models::Swing1Rule::new(
            "r".into(), None, None, None, json!([]), "paper".into(), 1.0,
            50.0, 20.0, None, None, None, None, Some(0.1), None, None, None, None,
        );
        // A minimal valid config: one swing axis, kill+volume axes, entry pullback.
        r.p_swing_min_leg_trades = Some(2);
        r.p_kill_depth_min_pct = Some(0.6);
        r.p_kill_max_duration_ms = Some(5_000);
        r.p_vol_depth_max_pct = Some(0.3);
        r.p_vol_min_duration_ms = Some(10_000);
        r.p_entry_pullback_pct = Some(15.0);
        r
    }

    #[test]
    fn valid_swing1_rule_builds() {
        let p = StrategyParams::Swing1(Swing1Params::from_rule(&swing1_rule_min()));
        let d = draft(StrategyImpl::Swing1, p);
        let rule = build_rule(&d).expect("valid swing1");
        assert_eq!(rule.strategy_id, "swing_1");
    }

    #[test]
    fn swing1_requires_swing_and_profile_and_entry() {
        // No swing axis → rejected.
        let mut r = swing1_rule_min();
        r.p_swing_min_leg_trades = None;
        let d = draft(StrategyImpl::Swing1, StrategyParams::Swing1(Swing1Params::from_rule(&r)));
        assert!(matches!(build_rule(&d), Err(e) if e.contains("swing-detection")));

        // No kill/volume axis → rejected.
        let mut r = swing1_rule_min();
        r.p_kill_depth_min_pct = None;
        r.p_kill_max_duration_ms = None;
        r.p_vol_depth_max_pct = None;
        r.p_vol_min_duration_ms = None;
        r.p_vol_min_up_duration_ms = None;
        let d = draft(StrategyImpl::Swing1, StrategyParams::Swing1(Swing1Params::from_rule(&r)));
        assert!(matches!(build_rule(&d), Err(e) if e.contains("kill or volume")));

        // No entry pullback → rejected.
        let mut r = swing1_rule_min();
        r.p_entry_pullback_pct = None;
        let d = draft(StrategyImpl::Swing1, StrategyParams::Swing1(Swing1Params::from_rule(&r)));
        assert!(matches!(build_rule(&d), Err(e) if e.contains("higher-low")));
    }

    #[test]
    fn swing1_shape_sanity_kill_deeper_than_volume() {
        let mut r = swing1_rule_min();
        r.p_kill_depth_min_pct = Some(0.2); // shallower than the 0.3 volume ceiling
        r.p_vol_depth_max_pct = Some(0.3);
        let d = draft(StrategyImpl::Swing1, StrategyParams::Swing1(Swing1Params::from_rule(&r)));
        assert!(matches!(build_rule(&d), Err(e) if e.contains("Kill Depth Min")));
    }

    #[test]
    fn swing1_params_jsonb_round_trip() {
        let p = Swing1Params::from_rule(&swing1_rule_min());
        let v = serde_json::to_value(&p).unwrap();
        let back: Swing1Params = serde_json::from_value(v).unwrap();
        assert_eq!(p, back);
        // And through the registry.
        let reparsed = StrategyImpl::Swing1.parse_params(&params_to_value(&StrategyParams::Swing1(p.clone()))).unwrap();
        assert_eq!(reparsed, StrategyParams::Swing1(p));
    }

    #[test]
    fn tpsl2_empty_entry_window_rejected() {
        let mut rule = crate::models::Tpsl2Rule::new(
            "r".into(), None, None, None, json!([]), "paper".into(), 1.0,
            50.0, 20.0, None, None, None, None, Some(0.1), None, None, None, None,
        );
        rule.p_entry_min_age_secs = Some(60);
        rule.p_entry_max_age_secs = Some(30); // max <= min → empty window
        let d = draft(StrategyImpl::Tpsl2, StrategyParams::Tpsl2(Tpsl2Params::from_rule(&rule)));
        assert!(matches!(build_rule(&d), Err(e) if e.contains("Max Age")));
    }
}
