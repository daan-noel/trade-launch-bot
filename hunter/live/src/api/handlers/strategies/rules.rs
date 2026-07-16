//! Unified strategy rule-CRUD + lifecycle edge (deploy box).
//!
//! One handler set over the core [`rules`](trading_core::strategies::rules) domain
//! and the unified [`StrategyService`], keyed by a `{strategy}` path segment
//! (`tpsl1`/`tpsl2` aliases or canonical ids). The domain validates + writes
//! `strategy_rules`; the service appends the live side effects (cache
//! `reload_rules` + `rules_changed` SSE) and owns the run lifecycle
//! (activate / pause / stop-and-close).
//!
//! Request/response shapes match the existing frontend contract: the flat `p_*`
//! field names line up 1:1 with the registry params structs, so a rule's params
//! JSONB *is* the request body (minus the universal columns), and a response is
//! the params object merged with the universal columns, the live runtime counters,
//! and a derived `lifecycle` label.

use actix_web::{web, HttpResponse, Responder};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::sync::Arc;
use uuid::Uuid;

use crate::state::deploy_state::DeployState;
use crate::strategies::PaperActivation;
use trading_core::models::StrategyRule;
use trading_core::strategies::registry::StrategyImpl;
use trading_core::storage::repositories::strategy_repo::RuleCounters;
use trading_core::strategies::rules::{params_to_value, RuleDraft, RuleError};
use trading_core::strategies::runtime_cache::StrategyRuntimeCache;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Resolve the `{strategy}` path segment to a [`StrategyImpl`], or a `400`.
fn resolve_strategy(seg: &str) -> Result<StrategyImpl, HttpResponse> {
    StrategyImpl::from_id(seg).ok_or_else(|| {
        HttpResponse::BadRequest().json(json!({"error": "Unknown strategy"}))
    })
}

/// Universal (typed-column) request keys — everything else in a body is a
/// strategy-specific param. The mid-run editable ("hot") set is a subset of these.
const RULE_NAME: &str = "rule_name";
const BUY_AMOUNT: &str = "buy_amount_sol";
const TRADE_MODE: &str = "trade_mode";
const MAX_CONCURRENT: &str = "p_max_concurrent_tokens";
const MAX_TOTAL: &str = "p_max_total_tokens";

/// Map a [`RuleError`] to its HTTP response.
fn rule_error(e: RuleError, ctx: &str) -> HttpResponse {
    match e {
        RuleError::Invalid(msg) => HttpResponse::BadRequest().json(json!({ "error": msg })),
        RuleError::Repo(err) => {
            tracing::error!("Failed to {ctx}: {err}");
            HttpResponse::InternalServerError().json(json!({"error": format!("Failed to {ctx}")}))
        }
    }
}

/// Build the frontend rule shape: the params JSONB merged with the universal
/// columns, the run summary counters, and a derived lifecycle.
///
/// Counter source follows the row's display logic: an **active** rule shows its
/// **current run** live from the in-memory cache (ticks as positions arm/fill/close);
/// an **inactive** rule shows its **last run** from the DB (`db_counters`, the rule's
/// latest-run aggregate) so the row survives restarts and shows a stopped rule's
/// realized result — the cache only warms active + `Running` rules on boot. Both
/// resolve to the same run when active (latest == current), so the two agree. The
/// in-memory cache remains authoritative for entry-gating caps regardless.
/// `db_counters` is `None` at the mutation endpoints (the frontend refetches the
/// list, whose read path supplies them) and defaults to zero.
fn rule_to_json(
    rule: &StrategyRule,
    cache: &StrategyRuntimeCache,
    db_counters: Option<&RuleCounters>,
) -> Value {
    let mut obj: Map<String, Value> = match &rule.params {
        Value::Object(m) => m.clone(),
        _ => Map::new(),
    };

    obj.insert("id".into(), json!(rule.id));
    obj.insert("strategy_id".into(), json!(rule.strategy_id));
    obj.insert(RULE_NAME.into(), json!(rule.rule_name));
    obj.insert(BUY_AMOUNT.into(), json!(rule.buy_amount_sol));
    obj.insert(TRADE_MODE.into(), json!(rule.trade_mode));
    obj.insert("is_active".into(), json!(rule.is_active));
    obj.insert(MAX_CONCURRENT.into(), json!(rule.max_concurrent_tokens));
    obj.insert(MAX_TOTAL.into(), json!(rule.max_total_tokens));
    obj.insert("created_at".into(), json!(rule.created_at));
    obj.insert("updated_at".into(), json!(rule.updated_at));

    // Run summary counters: active → live current-run (cache); inactive → last-run (DB).
    let (open, pending, total, wins, losses, win_rate, avg_pnl_pct, total_pnl_sol) =
        if rule.is_active {
            let stats = cache.closed_stats_by_rule(rule.id);
            let closed = stats.closed();
            // Canonical capital-weighted return (sign-locked to `total_pnl_sol`).
            (
                cache.holding_count_by_rule(rule.id),
                cache.pending_count_by_rule(rule.id),
                cache.total_count_by_rule(rule.id),
                stats.wins,
                stats.losses,
                if closed > 0 { stats.wins as f64 / closed as f64 * 100.0 } else { 0.0 },
                stats.avg_pnl_pct(),
                stats.sum_pnl_sol,
            )
        } else if let Some(c) = db_counters {
            (
                c.open_positions,
                c.pending_positions,
                c.total_positions,
                c.win_count,
                c.loss_count,
                c.win_rate,
                c.avg_pnl_pct,
                c.total_pnl_sol,
            )
        } else {
            (0, 0, 0, 0, 0, 0.0, 0.0, 0.0)
        };

    obj.insert("open_positions".into(), json!(open));
    obj.insert("pending_positions".into(), json!(pending));
    obj.insert("total_positions".into(), json!(total));
    obj.insert("win_count".into(), json!(wins));
    obj.insert("loss_count".into(), json!(losses));
    obj.insert("win_rate".into(), json!(win_rate));
    obj.insert("avg_pnl_pct".into(), json!(avg_pnl_pct));
    obj.insert("total_pnl_sol".into(), json!(total_pnl_sol));

    // Lifecycle label (derived from `is_active` + the cache counters — no extra
    // query): Active → still entering; Draining → paused but holdings exiting;
    // Finished → ran and fully drained; Idle → never traded this run.
    let lifecycle = if rule.is_active {
        "Active"
    } else if open > 0 {
        "Draining"
    } else if total > 0 {
        "Finished"
    } else {
        "Idle"
    };
    obj.insert("lifecycle".into(), json!(lifecycle));

    Value::Object(obj)
}

fn cache(app_state: &DeployState) -> &Arc<StrategyRuntimeCache> {
    app_state.strategy.runtime()
}

// ---------------------------------------------------------------------------
// CRUD
// ---------------------------------------------------------------------------

/// GET /api/strategies/{strategy}/rules
pub async fn list_rules(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<String>,
) -> impl Responder {
    let strategy = match resolve_strategy(&path) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let repo = app_state.strategy.repo();
    match repo.find_rules_by_strategy(strategy.id()).await {
        Ok(rules) => {
            let cache = cache(&app_state);
            // One batched query for every inactive rule's last-run stats. A failure
            // just leaves inactive rows at zero (logged) rather than failing the list.
            let counters = repo
                .rule_counters_for_latest_runs(strategy.id())
                .await
                .unwrap_or_else(|e| {
                    tracing::warn!("rule counters fetch failed: {e}");
                    Default::default()
                });
            let out: Vec<Value> = rules
                .iter()
                .map(|r| rule_to_json(r, cache, counters.get(&r.id)))
                .collect();
            HttpResponse::Ok().json(out)
        }
        Err(e) => {
            tracing::error!("Failed to list rules: {e}");
            HttpResponse::InternalServerError().json(json!({"error": "Failed to list rules"}))
        }
    }
}

/// GET /api/strategies/{strategy}/rules/{rule_id}
pub async fn get_rule(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<(String, Uuid)>,
) -> impl Responder {
    let (strategy, rule_id) = path.into_inner();
    if let Err(resp) = resolve_strategy(&strategy) {
        return resp;
    }
    let repo = app_state.strategy.repo();
    match repo.find_rule(rule_id).await {
        Ok(Some(rule)) => {
            // Inactive rules read their last-run stats from the DB; fetch the strategy's
            // batched counters and pick this rule's (unused for an active rule).
            let counters = repo
                .rule_counters_for_latest_runs(&rule.strategy_id)
                .await
                .unwrap_or_default();
            HttpResponse::Ok().json(rule_to_json(&rule, cache(&app_state), counters.get(&rule.id)))
        }
        Ok(None) => HttpResponse::NotFound().json(json!({"error": "Rule not found"})),
        Err(e) => {
            tracing::error!("Failed to get rule {rule_id}: {e}");
            HttpResponse::InternalServerError().json(json!({"error": "Failed to get rule"}))
        }
    }
}

/// Pull `Some(i64)` from a JSON value that is a number, `None` for JSON null.
fn opt_i64(v: &Value) -> Option<i64> {
    v.as_i64()
}

/// POST /api/strategies/{strategy}/rules
///
/// The body is the flat rule shape (params `p_*` + universal columns). The params
/// deserialize straight off the body (the param structs ignore the universal keys);
/// the universal columns are read out explicitly.
pub async fn create_rule(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<String>,
    body: web::Json<Value>,
) -> impl Responder {
    let strategy = match resolve_strategy(&path) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let body = body.into_inner();

    let params = match strategy.parse_params(&body) {
        Ok(p) => p,
        Err(e) => {
            return HttpResponse::BadRequest().json(json!({"error": format!("invalid params: {e}")}))
        }
    };

    let rule_name = body.get(RULE_NAME).and_then(Value::as_str).unwrap_or("").to_string();
    let buy_amount_sol = body.get(BUY_AMOUNT).and_then(Value::as_f64).unwrap_or(0.0);
    let trade_mode = body.get(TRADE_MODE).and_then(Value::as_str).unwrap_or("paper").to_string();
    let draft = RuleDraft {
        strategy,
        rule_name,
        buy_amount_sol,
        trade_mode,
        max_concurrent_tokens: body.get(MAX_CONCURRENT).and_then(opt_i64),
        max_total_tokens: body.get(MAX_TOTAL).and_then(opt_i64),
        params,
    };

    match app_state.strategy.create_rule(&draft).await {
        Ok(rule) => HttpResponse::Created().json(rule_to_json(&rule, cache(&app_state), None)),
        Err(e) => rule_error(e, "create rule"),
    }
}

/// PUT /api/strategies/{strategy}/rules/{rule_id}
///
/// Partial update: present body keys overlay the rule's params (explicit JSON null
/// clears an optional param); universal columns update if present. A **live**
/// (active) rule freezes everything but the hot set (`rule_name`, `buy_amount_sol`,
/// concurrency caps) — `trade_mode` is frozen by value.
pub async fn update_rule(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<(String, Uuid)>,
    body: web::Json<Value>,
) -> impl Responder {
    let (strategy_seg, rule_id) = path.into_inner();
    let strategy = match resolve_strategy(&strategy_seg) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let body = body.into_inner();

    let mut rule = match app_state.strategy.repo().find_rule(rule_id).await {
        Ok(Some(r)) => r,
        Ok(None) => return HttpResponse::NotFound().json(json!({"error": "Rule not found"})),
        Err(e) => {
            tracing::error!("Failed to get rule {rule_id}: {e}");
            return HttpResponse::InternalServerError().json(json!({"error": "Failed to get rule"}));
        }
    };

    // Live-edit freeze guard: while the rule is active only the hot set is editable.
    if rule.is_active {
        if let Value::Object(m) = &body {
            if let Some(tm) = m.get(TRADE_MODE).and_then(Value::as_str) {
                if tm != rule.trade_mode {
                    return HttpResponse::BadRequest()
                        .json(json!({"error": "trade_mode is frozen while the rule is live"}));
                }
            }
            for k in m.keys() {
                let editable = matches!(
                    k.as_str(),
                    RULE_NAME | BUY_AMOUNT | MAX_CONCURRENT | MAX_TOTAL | TRADE_MODE | "is_active" | "id"
                );
                if !editable {
                    return HttpResponse::BadRequest().json(json!({
                        "error": format!("'{k}' cannot be edited while the rule is live")
                    }));
                }
            }
        }
    }

    // Merge present body keys onto the existing params, then re-parse (validates
    // types) and clean back to the canonical params JSONB (drops universal keys).
    let mut merged: Map<String, Value> = match &rule.params {
        Value::Object(m) => m.clone(),
        _ => Map::new(),
    };
    if let Value::Object(m) = &body {
        for (k, v) in m {
            merged.insert(k.clone(), v.clone());
        }
    }
    let params = match strategy.parse_params(&Value::Object(merged)) {
        Ok(p) => p,
        Err(e) => {
            return HttpResponse::BadRequest().json(json!({"error": format!("invalid params: {e}")}))
        }
    };
    rule.params = params_to_value(&params);

    // Universal columns (apply only the keys present in the body).
    if let Value::Object(m) = &body {
        if let Some(name) = m.get(RULE_NAME).and_then(Value::as_str) {
            rule.rule_name = name.to_string();
        }
        if let Some(amount) = m.get(BUY_AMOUNT).and_then(Value::as_f64) {
            rule.buy_amount_sol = amount;
        }
        if let Some(mode) = m.get(TRADE_MODE).and_then(Value::as_str) {
            rule.trade_mode = mode.to_string();
        }
        if let Some(v) = m.get(MAX_CONCURRENT) {
            rule.max_concurrent_tokens = opt_i64(v);
        }
        if let Some(v) = m.get(MAX_TOTAL) {
            rule.max_total_tokens = opt_i64(v);
        }
    }

    match app_state.strategy.save_rule(&rule).await {
        Ok(()) => HttpResponse::Ok().json(rule_to_json(&rule, cache(&app_state), None)),
        Err(e) => rule_error(e, "update rule"),
    }
}

/// DELETE /api/strategies/{strategy}/rules/{rule_id}
pub async fn delete_rule(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<(String, Uuid)>,
) -> impl Responder {
    let (strategy, rule_id) = path.into_inner();
    if let Err(resp) = resolve_strategy(&strategy) {
        return resp;
    }
    match app_state.strategy.delete_rule(rule_id).await {
        Ok(true) => HttpResponse::NoContent().finish(),
        Ok(false) => HttpResponse::NotFound().json(json!({"error": "Rule not found"})),
        Err(e) => {
            tracing::error!("Failed to delete rule {rule_id}: {e}");
            HttpResponse::InternalServerError().json(json!({"error": "Failed to delete rule"}))
        }
    }
}

// ---------------------------------------------------------------------------
// Lifecycle
// ---------------------------------------------------------------------------

/// Body for the activate endpoint: a paper rule chooses a fresh run vs continuing
/// the prior one (ignored for real rules). Absent ⇒ fresh.
#[derive(Deserialize, Default)]
pub struct ActivateBody {
    #[serde(default)]
    pub paper_run: PaperRun,
}

#[derive(Deserialize, Default, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum PaperRun {
    #[default]
    Fresh,
    Continue,
}

impl From<PaperRun> for PaperActivation {
    fn from(p: PaperRun) -> Self {
        match p {
            PaperRun::Fresh => PaperActivation::Fresh,
            PaperRun::Continue => PaperActivation::Continue,
        }
    }
}

/// POST /api/strategies/{strategy}/rules/{rule_id}/activate
pub async fn activate_rule(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<(String, Uuid)>,
    body: Option<web::Json<ActivateBody>>,
) -> impl Responder {
    let (strategy, rule_id) = path.into_inner();
    if let Err(resp) = resolve_strategy(&strategy) {
        return resp;
    }
    let activation = body.map(|b| b.into_inner().paper_run).unwrap_or_default().into();
    match app_state.strategy.activate_rule(rule_id, activation).await {
        Ok(rule) => HttpResponse::Ok().json(rule_to_json(&rule, cache(&app_state), None)),
        Err(e) => lifecycle_error(rule_id, "activate", e),
    }
}

/// POST /api/strategies/{strategy}/rules/{rule_id}/pause
pub async fn pause_rule(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<(String, Uuid)>,
) -> impl Responder {
    let (strategy, rule_id) = path.into_inner();
    if let Err(resp) = resolve_strategy(&strategy) {
        return resp;
    }
    match app_state.strategy.pause_rule(rule_id).await {
        Ok(rule) => HttpResponse::Ok().json(rule_to_json(&rule, cache(&app_state), None)),
        Err(e) => lifecycle_error(rule_id, "pause", e),
    }
}

/// POST /api/strategies/{strategy}/rules/{rule_id}/stop
pub async fn stop_rule(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<(String, Uuid)>,
) -> impl Responder {
    let (strategy, rule_id) = path.into_inner();
    if let Err(resp) = resolve_strategy(&strategy) {
        return resp;
    }
    match app_state.strategy.stop_and_close_rule(rule_id).await {
        Ok((rule, _count)) => HttpResponse::Ok().json(rule_to_json(&rule, cache(&app_state), None)),
        Err(e) => lifecycle_error(rule_id, "stop", e),
    }
}

/// Map a lifecycle error to its HTTP response (404 for a missing rule, else 500).
fn lifecycle_error(rule_id: Uuid, action: &str, e: anyhow::Error) -> HttpResponse {
    if e.to_string().contains("Rule not found") {
        return HttpResponse::NotFound().json(json!({"error": "Rule not found"}));
    }
    tracing::error!("Failed to {action} rule {rule_id}: {e}");
    HttpResponse::InternalServerError().json(json!({"error": format!("Failed to {action} rule")}))
}

// ---------------------------------------------------------------------------
// Bulk lifecycle (Pause All / Stop All — scoped to one `trade_mode` at a time,
// mirroring the Real/Paper section split in the live rules pages)
// ---------------------------------------------------------------------------

/// `?mode=real|paper` query selector for the bulk lifecycle endpoints (mirrors
/// the `PositionScope`/`ScopeParam` pattern in `positions.rs`).
#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum TradeModeFilter {
    Real,
    Paper,
}

impl TradeModeFilter {
    fn as_str(self) -> &'static str {
        match self {
            Self::Real => "real",
            Self::Paper => "paper",
        }
    }
}

#[derive(Deserialize)]
pub struct ModeParam {
    pub mode: TradeModeFilter,
}

/// Split a batch of per-rule lifecycle results into the updated rules (as the
/// frontend rule shape) and the failed ids + error strings.
fn bulk_response<T>(
    results: Vec<(Uuid, anyhow::Result<T>)>,
    to_json: impl Fn(&T) -> Value,
) -> HttpResponse {
    let mut updated = Vec::new();
    let mut failed = Vec::new();
    for (id, res) in results {
        match res {
            Ok(v) => updated.push(to_json(&v)),
            Err(e) => failed.push(json!({ "rule_id": id, "error": e.to_string() })),
        }
    }
    HttpResponse::Ok().json(json!({ "updated": updated, "failed": failed }))
}

/// POST /api/strategies/{strategy}/rules/pause-all?mode=real|paper
///
/// Pauses every currently active rule of `mode` (entries off; open positions
/// left to drain via the exit ladder — same as the per-row Pause button).
pub async fn pause_all_rules(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<String>,
    query: web::Query<ModeParam>,
) -> impl Responder {
    let strategy = match resolve_strategy(&path) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let cache = cache(&app_state);
    match app_state.strategy.pause_all_rules(strategy.id(), query.mode.as_str()).await {
        Ok(results) => bulk_response(results, |r| rule_to_json(r, cache, None)),
        Err(e) => {
            tracing::error!("Failed to pause all rules: {e}");
            HttpResponse::InternalServerError().json(json!({"error": "Failed to pause all rules"}))
        }
    }
}

/// POST /api/strategies/{strategy}/rules/stop-all?mode=real|paper
///
/// Stops and force-closes every rule of `mode` that is active or still holding
/// open positions (same as the per-row Stop button, applied to a set).
pub async fn stop_all_rules(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<String>,
    query: web::Query<ModeParam>,
) -> impl Responder {
    let strategy = match resolve_strategy(&path) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let cache = cache(&app_state);
    match app_state.strategy.stop_all_rules(strategy.id(), query.mode.as_str()).await {
        Ok(results) => bulk_response(results, |(r, _count)| rule_to_json(r, cache, None)),
        Err(e) => {
            tracing::error!("Failed to stop all rules: {e}");
            HttpResponse::InternalServerError().json(json!({"error": "Failed to stop all rules"}))
        }
    }
}
