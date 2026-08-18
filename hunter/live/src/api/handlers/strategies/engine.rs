//! HTTP handlers for the generic fingerprint + metrics engine (plan 4.8):
//! * `GET /api/meta/strategy-registry` — the metric registry the frontend renders
//!   its whole rule-authoring UI from (extensibility contract, plan §8).
//! * `GET /api/strategies/armed` — a live snapshot of armed (token, rule) pairs.
//! * `/api/fingerprints` — CRUD over the shared `fingerprints` rows.
//! * `/api/strategy-rules` — CRUD + lifecycle over the generic `strategy_rules`:
//!   per-rule activate / pause / enable / disable / stop (stop force-closes the
//!   rule's open positions via the engine loop), plus `pause-all` / `stop-all`
//!   scoped by `?mode=real|paper`. `is_enabled` soft-archives inefficient rules
//!   without deleting them (orthogonal to Active/Idle).
//!
//! Every rule/fingerprint mutation schedules a background `engine.reload_rules()`
//! so the HTTP response returns after the PG write; `tpsl_rules_changed` SSE fires
//! once the loop acks.

use std::collections::HashSet;
use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use chrono::Utc;
use serde_json::{json, Value};
use trading_core::api::handlers::strategies::rule_positions::{
    self, ScoreScope, ScoreScopeParam,
};
use trading_core::models::Fingerprint;
use trading_core::services::veteran_roster;
use trading_core::storage::repositories::token_repo::TokenRepo;
use trading_core::strategies::rules::{self, apply_rule_update, RuleDraft, RuleError};
use uuid::Uuid;

use super::action_progress;
use crate::state::deploy_state::DeployState;

// ── Metadata ─────────────────────────────────────────────────────────────────

/// GET /api/meta/strategy-registry — the engine's self-describing metric registry.
pub async fn strategy_registry() -> impl Responder {
    HttpResponse::Ok().json(hunter_engine::metrics::registry_json())
}

/// GET /api/strategies/armed — currently-armed (token, rule) pairs (live monitor).
///
/// Filters out any (rule, mint) that already has an unsettled `strategy_positions`
/// row — belt-and-suspenders if the in-memory armed set lagged behind a buy.
pub async fn list_armed(app_state: web::Data<Arc<DeployState>>) -> impl Responder {
    let mut armed = app_state.armed.snapshot();
    if let Ok(open) = app_state.strategy_repo.find_open_positions().await {
        let occupied: HashSet<(Uuid, String)> = open
            .into_iter()
            .filter_map(|p| p.rule_id.map(|r| (r, p.mint_address)))
            .collect();
        if !occupied.is_empty() {
            armed.retain(|e| !occupied.contains(&(e.rule_id, e.mint_address.clone())));
        }
    }
    HttpResponse::Ok().json(armed)
}

// ── Fingerprints ─────────────────────────────────────────────────────────────

/// GET /api/fingerprints — each row annotated with `used_by` (how many rules
/// reference it), so the library can show "used by N" and guard deletion. The
/// count is folded in from the small rules list (no extra SQL / model change).
pub async fn list_fingerprints(app_state: web::Data<Arc<DeployState>>) -> impl Responder {
    let fps = match app_state.fingerprint_repo.list().await {
        Ok(v) => v,
        Err(e) => return server_error("list fingerprints", e),
    };
    let mut usage: std::collections::HashMap<Uuid, i64> = std::collections::HashMap::new();
    match app_state.rule_repo.list().await {
        Ok(rules) => {
            for r in &rules {
                *usage.entry(r.fingerprint_id).or_insert(0) += 1;
            }
        }
        Err(e) => return server_error("list fingerprints (usage)", e),
    }
    let out: Vec<Value> = fps
        .iter()
        .map(|fp| {
            let mut v = serde_json::to_value(fp).unwrap_or_else(|_| json!({}));
            if let Value::Object(map) = &mut v {
                map.insert("used_by".into(), json!(usage.get(&fp.id).copied().unwrap_or(0)));
            }
            v
        })
        .collect();
    HttpResponse::Ok().json(out)
}

/// GET /api/fingerprints/{id}
pub async fn get_fingerprint(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<Uuid>,
) -> impl Responder {
    match app_state.fingerprint_repo.find(path.into_inner()).await {
        Ok(Some(fp)) => HttpResponse::Ok().json(fp),
        Ok(None) => HttpResponse::NotFound().json(json!({"error": "fingerprint not found"})),
        Err(e) => server_error("get fingerprint", e),
    }
}

/// POST /api/fingerprints/{id}/refresh-roster
///
/// Rebuild this fingerprint's `m_bundle` veteran roster now, rather than waiting for
/// the background refresher.
///
/// The bootstrap path. A roster is *derived* (a recurrence count over the
/// fingerprint's own launch history), never hand-authored - so a fingerprint created
/// today carries no roster at all, every `m_bundle` metric on it reads `NaN`, and a
/// rule reading one can never fire. Without this there is no way to fill it in except
/// activating the rule and waiting out the refresher's period.
pub async fn refresh_fingerprint_roster(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<Uuid>,
) -> impl Responder {
    let id = path.into_inner();
    let token_repo = TokenRepo::new(app_state.core.db.clone());
    match veteran_roster::refresh_roster(
        &app_state.core.db,
        &token_repo,
        &app_state.fingerprint_repo,
        id,
        veteran_roster::DEFAULT_LOOKBACK_DAYS,
    )
    .await
    {
        Ok(stats) => {
            // The roster lives on the fingerprint row the engine loads, so a live
            // rule reading it must see the new set without a restart.
            schedule_engine_reload(&app_state);
            HttpResponse::Ok().json(json!({
                "launches": stats.launches,
                "wallets": stats.wallets,
                "veterans": stats.veterans,
                "lookback_days": veteran_roster::DEFAULT_LOOKBACK_DAYS,
            }))
        }
        Err(e) => server_error("refresh veteran roster", e),
    }
}

/// POST /api/fingerprints
pub async fn create_fingerprint(
    app_state: web::Data<Arc<DeployState>>,
    body: web::Json<Value>,
) -> impl Responder {
    let mut fp = Fingerprint::from_json(&body, Uuid::new_v4(), Utc::now());
    fp.ensure_auto_name();
    if let Err(e) = fp.validate() {
        return HttpResponse::BadRequest().json(json!({ "error": e }));
    }
    if let Err(e) = hunter_engine::metrics::flow_split::FlowPatterns::validate_metric_config(
        &fp.metric_config,
    ) {
        return HttpResponse::BadRequest().json(json!({ "error": e }));
    }
    match app_state.fingerprint_repo.insert(&fp).await {
        Ok(()) => HttpResponse::Created().json(fp),
        Err(e) => server_error("create fingerprint", e),
    }
}

/// PUT /api/fingerprints/{id}
pub async fn update_fingerprint(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<Uuid>,
    body: web::Json<Value>,
) -> impl Responder {
    let id = path.into_inner();
    let mut fp = Fingerprint::from_json(&body, id, Utc::now());
    fp.ensure_auto_name();
    if let Err(e) = fp.validate() {
        return HttpResponse::BadRequest().json(json!({ "error": e }));
    }
    if let Err(e) = hunter_engine::metrics::flow_split::FlowPatterns::validate_metric_config(
        &fp.metric_config,
    ) {
        return HttpResponse::BadRequest().json(json!({ "error": e }));
    }
    match app_state.fingerprint_repo.update(&fp).await {
        Ok(()) => {
            schedule_engine_reload(&app_state);
            HttpResponse::Ok().json(fp)
        }
        Err(e) => server_error("update fingerprint", e),
    }
}

/// DELETE /api/fingerprints/{id} (FK-guarded — fails while any rule references it).
pub async fn delete_fingerprint(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<Uuid>,
) -> impl Responder {
    match app_state.fingerprint_repo.delete(path.into_inner()).await {
        Ok(()) => HttpResponse::NoContent().finish(),
        Err(e) => HttpResponse::Conflict()
            .json(json!({"error": format!("cannot delete fingerprint (in use?): {e}")})),
    }
}

// ── Rules ────────────────────────────────────────────────────────────────────

/// GET /api/strategy-rules
///
/// Each rule is enriched with DB-backed score fields (`total_positions`,
/// `win_rate`, `total_pnl_sol`, …).
///
/// `?score_scope=current|all` (default `all`):
/// - `all` — real = all-time positions; paper = latest run (legacy scoreboard)
/// - `current` — latest run for **both** modes (Rules Control keep/kill board)
///
/// `?score_mode=paper|real` scores **every** rule in that one mode instead of its own
/// `trade_mode`; with it, `all` is all-time on both sides.
pub async fn list_rules(
    app_state: web::Data<Arc<DeployState>>,
    query: web::Query<ScoreScopeParam>,
) -> impl Responder {
    // Shared with the lab bin — the scoreboard is a position rollup, so both apps
    // score a rule identically off whichever `strategy_positions` they can see.
    let q = query.into_inner();
    rule_positions::rules_with_counters(
        &app_state.strategy_repo,
        &app_state.rule_repo,
        q.score_scope.unwrap_or(ScoreScope::All),
        q.score_mode,
    )
    .await
}

/// GET /api/strategy-rules/{id}/runs — run navigator for Rules Evidence.
pub async fn list_rule_runs(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<Uuid>,
) -> impl Responder {
    // Shared with the lab bin (which serves this off the synced mirror).
    rule_positions::rule_runs(
        &app_state.strategy_repo,
        &app_state.rule_repo,
        path.into_inner(),
    )
    .await
}

/// GET /api/strategy-rules/{id}
pub async fn get_rule(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<Uuid>,
) -> impl Responder {
    match app_state.rule_repo.find(path.into_inner()).await {
        Ok(Some(r)) => HttpResponse::Ok().json(r),
        Ok(None) => HttpResponse::NotFound().json(json!({"error": "rule not found"})),
        Err(e) => server_error("get rule", e),
    }
}

/// POST /api/strategy-rules
pub async fn create_rule(
    app_state: web::Data<Arc<DeployState>>,
    body: web::Json<Value>,
) -> impl Responder {
    let draft = match RuleDraft::from_json(&body) {
        Ok(d) => d,
        Err(e) => return HttpResponse::BadRequest().json(json!({"error": e})),
    };
    match rules::create_with_fp_check(
        &app_state.rule_repo,
        &app_state.fingerprint_repo,
        &draft,
    )
    .await
    {
        Ok((rule, warning)) => {
            schedule_engine_reload(&app_state);
            let mut body = serde_json::to_value(&rule).unwrap_or_default();
            if let Some(w) = warning {
                if let Some(obj) = body.as_object_mut() {
                    obj.insert("warning".into(), json!(w));
                }
            }
            HttpResponse::Created().json(body)
        }
        Err(e) => rule_error(e, "create rule"),
    }
}

/// PUT /api/strategy-rules/{id}
pub async fn update_rule(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<Uuid>,
    body: web::Json<Value>,
) -> impl Responder {
    let id = path.into_inner();
    let Ok(Some(mut rule)) = app_state.rule_repo.find(id).await else {
        return HttpResponse::NotFound().json(json!({"error": "rule not found"}));
    };
    apply_rule_update(&mut rule, &body);
    match rules::save_with_fp_check(
        &app_state.rule_repo,
        &app_state.fingerprint_repo,
        &mut rule,
    )
    .await
    {
        Ok(warning) => {
            schedule_engine_reload(&app_state);
            let mut body = serde_json::to_value(&rule).unwrap_or_default();
            if let Some(w) = warning {
                if let Some(obj) = body.as_object_mut() {
                    obj.insert("warning".into(), json!(w));
                }
            }
            HttpResponse::Ok().json(body)
        }
        Err(e) => rule_error(e, "update rule"),
    }
}

/// DELETE /api/strategy-rules/{id}
pub async fn delete_rule(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<Uuid>,
) -> impl Responder {
    match app_state.rule_repo.delete(path.into_inner()).await {
        Ok(()) => {
            schedule_engine_reload(&app_state);
            HttpResponse::NoContent().finish()
        }
        Err(e) => server_error("delete rule", e),
    }
}

/// POST /api/strategy-rules/{id}/activate
pub async fn activate_rule(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<Uuid>,
) -> impl Responder {
    set_active(&app_state, path.into_inner(), true).await
}

/// POST /api/strategy-rules/{id}/pause
pub async fn pause_rule(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<Uuid>,
) -> impl Responder {
    set_active(&app_state, path.into_inner(), false).await
}

/// POST /api/strategy-rules/{id}/enable — soft-unarchive (orthogonal to Active/Idle).
pub async fn enable_rule(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<Uuid>,
) -> impl Responder {
    set_enabled(&app_state, path.into_inner(), true).await
}

/// POST /api/strategy-rules/{id}/disable — soft-archive. Also pauses if Active so
/// the rule immediately leaves the live arm set.
pub async fn disable_rule(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<Uuid>,
) -> impl Responder {
    set_enabled(&app_state, path.into_inner(), false).await
}

async fn set_active(app_state: &DeployState, id: Uuid, active: bool) -> HttpResponse {
    let Ok(Some(mut rule)) = app_state.rule_repo.find(id).await else {
        return HttpResponse::NotFound().json(json!({"error": "rule not found"}));
    };
    if active && !rule.is_enabled {
        return HttpResponse::BadRequest()
            .json(json!({"error": "rule is disabled; enable it before activating"}));
    }
    // Refresh the veteran roster BEFORE the rule goes live, not after. The roster is
    // a stored snapshot on the fingerprint row; a rule activated against a stale or
    // absent one reads `NaN`/0 and quietly enters nothing, which looks like a bad
    // rule rather than a missing input. Synchronous on purpose - activation is a
    // deliberate operator action, and seconds here beat an hour of silence.
    if active && hunter_engine::metrics::bundle::params_reference_bundle(&rule.params) {
        let token_repo = TokenRepo::new(app_state.core.db.clone());
        if let Err(e) = veteran_roster::refresh_roster(
            &app_state.core.db,
            &token_repo,
            &app_state.fingerprint_repo,
            rule.fingerprint_id,
            veteran_roster::DEFAULT_LOOKBACK_DAYS,
        )
        .await
        {
            tracing::warn!("veteran-roster refresh on activate ({}): {e}", rule.fingerprint_id);
        }
    }
    rule.is_active = active;
    rule.updated_at = Utc::now();
    match app_state.rule_repo.update(&rule).await {
        Ok(()) => {
            schedule_engine_reload(app_state);
            HttpResponse::Ok().json(rule)
        }
        Err(e) => server_error("toggle rule active", e),
    }
}

async fn set_enabled(app_state: &DeployState, id: Uuid, enabled: bool) -> HttpResponse {
    let Ok(Some(mut rule)) = app_state.rule_repo.find(id).await else {
        return HttpResponse::NotFound().json(json!({"error": "rule not found"}));
    };
    rule.is_enabled = enabled;
    // Disable ⇒ pause so the live engine drops the rule immediately.
    if !enabled {
        rule.is_active = false;
    }
    rule.updated_at = Utc::now();
    match app_state.rule_repo.update(&rule).await {
        Ok(()) => {
            schedule_engine_reload(app_state);
            HttpResponse::Ok().json(rule)
        }
        Err(e) => server_error("toggle rule enabled", e),
    }
}

/// POST /api/strategy-rules/{id}/stop — deactivate the rule **and** force-close its
/// open positions now (the per-row Stop, vs Pause which leaves positions to drain).
/// Returns 202 + `action_id` immediately; position closes stream over
/// `action_progress` / `strategy_position_update`.
pub async fn stop_rule(
    app_state: web::Data<Arc<DeployState>>,
    path: web::Path<Uuid>,
) -> impl Responder {
    let id = path.into_inner();
    // `find_open_positions` is "not End/EntryFailed", which also returns rows the
    // engine already handed off (`ExitStuck`/`ExitUnconfirmed`). Those are open in
    // the attention lane but settled as far as a stop is concerned — counting them
    // gave a `total` the action could never reach.
    let open = match app_state.strategy_repo.find_open_positions().await {
        Ok(all) => all
            .into_iter()
            .filter(|p| p.rule_id == Some(id) && action_progress::stop_in_flight(&p.status))
            .collect::<Vec<_>>(),
        Err(e) => return server_error("stop: list open positions", e),
    };
    let total = open.len() as u64;
    let position_ids: HashSet<Uuid> = open.into_iter().map(|p| p.id).collect();
    let action_id = Uuid::new_v4();

    // Subscribe + start frame before close so we don't miss terminal updates.
    action_progress::spawn_stop_watcher(
        app_state.sse_tx.clone(),
        app_state.strategy_repo.clone(),
        action_id,
        Some(id),
        position_ids,
    );
    if !app_state.engine.close_rule(id).await {
        return engine_unavailable("stop", "engine channel closed");
    }

    // Deactivate after close is queued (positions already ExitPending-bound).
    let rule_resp = set_active(&app_state, id, false).await;
    if !rule_resp.status().is_success() {
        return rule_resp;
    }

    HttpResponse::Accepted().json(json!({
        "action_id": action_id,
        "kind": "stop",
        "rule_id": id,
        "total": total,
        "closing": true,
    }))
}

// ── Bulk lifecycle (Pause All / Stop All — one `trade_mode` at a time) ─────────

/// `?mode=real|paper` selector for the bulk lifecycle endpoints.
#[derive(serde::Deserialize)]
pub struct ModeParam {
    pub mode: String,
}

/// Deactivate every active rule of `mode`. Returns the count paused.
async fn pause_all_of_mode(app_state: &DeployState, mode: &str) -> Result<usize, HttpResponse> {
    let rules = app_state
        .rule_repo
        .list()
        .await
        .map_err(|e| server_error("pause-all: list rules", e))?;
    let mut paused = 0usize;
    for mut rule in rules.into_iter().filter(|r| r.is_active && r.trade_mode == mode) {
        rule.is_active = false;
        rule.updated_at = Utc::now();
        app_state
            .rule_repo
            .update(&rule)
            .await
            .map_err(|e| server_error("pause-all: update rule", e))?;
        paused += 1;
    }
    schedule_engine_reload(app_state);
    Ok(paused)
}

/// POST /api/strategy-rules/pause-all?mode=real|paper — entries off for every active
/// rule of `mode`; open positions are left to drain (same as the per-row Pause).
pub async fn pause_all_rules(
    app_state: web::Data<Arc<DeployState>>,
    query: web::Query<ModeParam>,
) -> impl Responder {
    match pause_all_of_mode(&app_state, &query.mode).await {
        Ok(paused) => HttpResponse::Ok().json(json!({ "paused": paused })),
        Err(resp) => resp,
    }
}

/// POST /api/strategy-rules/stop-all?mode=real|paper — force-close every open
/// position of `mode` (active or draining) **and** deactivate its active rules.
/// Returns 202 + `action_id`; closes stream over `action_progress`.
pub async fn stop_all_rules(
    app_state: web::Data<Arc<DeployState>>,
    query: web::Query<ModeParam>,
) -> impl Responder {
    let mode = query.mode.as_str();
    // Same "already handed off" filter as the per-rule stop — see `stop_rule`.
    let open = match app_state.strategy_repo.find_open_positions().await {
        Ok(all) => all
            .into_iter()
            .filter(|p| p.mode == mode && action_progress::stop_in_flight(&p.status))
            .collect::<Vec<_>>(),
        Err(e) => return server_error("stop-all: list open positions", e),
    };
    let total = open.len() as u64;
    let position_ids: HashSet<Uuid> = open.into_iter().map(|p| p.id).collect();
    let action_id = Uuid::new_v4();

    action_progress::spawn_stop_watcher(
        app_state.sse_tx.clone(),
        app_state.strategy_repo.clone(),
        action_id,
        None,
        position_ids,
    );
    if !app_state.engine.close_mode(mode == "real").await {
        return engine_unavailable("stop-all", "engine channel closed");
    }

    let paused = match pause_all_of_mode(&app_state, mode).await {
        Ok(n) => n,
        Err(resp) => return resp,
    };

    if paused == 0 && total == 0 {
        return HttpResponse::Conflict().json(json!({
            "error": "no active rules or open positions to stop for this mode",
            "paused": paused,
            "total": total,
        }));
    }

    HttpResponse::Accepted().json(json!({
        "action_id": action_id,
        "kind": "stop",
        "mode": mode,
        "total": total,
        "paused": paused,
        "closing": true,
    }))
}

// ── Error helpers ────────────────────────────────────────────────────────────

fn rule_error(e: RuleError, ctx: &str) -> HttpResponse {
    match e {
        RuleError::Invalid(msg) => HttpResponse::BadRequest().json(json!({"error": msg})),
        RuleError::Duplicate {
            existing_id,
            rule_name,
        } => HttpResponse::Conflict().json(json!({
            "error": format!(
                "identical rule already exists: \"{rule_name}\" ({existing_id})"
            ),
            "existing_id": existing_id,
            "rule_name": rule_name,
        })),
        RuleError::Repo(err) => server_error(ctx, err),
    }
}

fn server_error(ctx: &str, e: impl std::fmt::Display) -> HttpResponse {
    tracing::warn!("{ctx}: {e}");
    HttpResponse::InternalServerError().json(json!({"error": format!("{ctx} failed")}))
}

fn schedule_engine_reload(app_state: &DeployState) {
    app_state.engine.schedule_reload(app_state.sse_tx.clone());
}

fn engine_unavailable(ctx: &str, detail: impl std::fmt::Display) -> HttpResponse {
    tracing::warn!("{ctx}: {detail}");
    HttpResponse::ServiceUnavailable().json(json!({
        "error": format!("{ctx}: engine unavailable"),
        "detail": detail.to_string(),
    }))
}
