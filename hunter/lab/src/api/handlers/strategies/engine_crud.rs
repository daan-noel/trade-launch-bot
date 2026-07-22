//! Rule-authoring CRUD + metric registry — the lab twin of the live engine
//! handlers (`hunter/live/.../strategies/engine.rs`). The lab bin serves
//! fingerprint/rule CRUD + the registry so the lab app can author + dry-run rules
//! on the workstation corpus (FE3).
//!
//! Unlike the live handlers there is NO running engine here, so these do **not**
//! call `engine.reload_rules()`: rules just persist to the lab DB; a synced or
//! restarted live bin picks them up on its next reload. All parsing + validation
//! is the shared SSOT in `trading_core::{models::Fingerprint, strategies::rules}`.

use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use uuid::Uuid;

use trading_core::api::handlers::strategies::rule_positions::{
    self, ScoreScope, ScoreScopeParam,
};
use trading_core::models::Fingerprint;
use trading_core::storage::repositories::fingerprint_repo::FingerprintRepo;
use trading_core::storage::repositories::rule_repo::RuleRepo;
use trading_core::strategies::rules::{self, apply_rule_update, RuleDraft, RuleError};

use crate::state::local_state::LocalState;
use crate::strategies::engine_sim::sim_cache_key_for_rule;

fn fp_repo(state: &LocalState) -> FingerprintRepo {
    FingerprintRepo::new(state.core.db.clone())
}
fn rule_repo(state: &LocalState) -> RuleRepo {
    RuleRepo::new(state.core.db.clone())
}

fn srv_err(ctx: &str, e: impl std::fmt::Display) -> HttpResponse {
    tracing::warn!("{ctx}: {e}");
    HttpResponse::InternalServerError().json(serde_json::json!({ "error": format!("{ctx} failed") }))
}
fn rule_err(e: RuleError, ctx: &str) -> HttpResponse {
    match e {
        RuleError::Invalid(m) => HttpResponse::BadRequest().json(serde_json::json!({ "error": m })),
        RuleError::Duplicate {
            existing_id,
            rule_name,
        } => HttpResponse::Conflict().json(serde_json::json!({
            "error": format!(
                "identical rule already exists: \"{rule_name}\" ({existing_id})"
            ),
            "existing_id": existing_id,
            "rule_name": rule_name,
        })),
        RuleError::Repo(err) => srv_err(ctx, err),
    }
}

/// GET `/api/meta/strategy-registry` (lab twin — pure, no state).
pub async fn strategy_registry() -> impl Responder {
    HttpResponse::Ok().json(hunter_engine::metrics::registry_json())
}

/// GET `/api/fingerprints` — with a folded-in `used_by` rule count (see live twin).
pub async fn list_fingerprints(app_state: web::Data<Arc<LocalState>>) -> impl Responder {
    let fps = match fp_repo(&app_state).list().await {
        Ok(v) => v,
        Err(e) => return srv_err("list fingerprints", e),
    };
    let mut usage: std::collections::HashMap<Uuid, i64> = std::collections::HashMap::new();
    match rule_repo(&app_state).list().await {
        Ok(rules) => {
            for r in &rules {
                *usage.entry(r.fingerprint_id).or_insert(0) += 1;
            }
        }
        Err(e) => return srv_err("list fingerprints (usage)", e),
    }
    let out: Vec<serde_json::Value> = fps
        .iter()
        .map(|fp| {
            let mut v = serde_json::to_value(fp).unwrap_or_else(|_| serde_json::json!({}));
            if let serde_json::Value::Object(map) = &mut v {
                map.insert(
                    "used_by".into(),
                    serde_json::json!(usage.get(&fp.id).copied().unwrap_or(0)),
                );
            }
            v
        })
        .collect();
    HttpResponse::Ok().json(out)
}

/// GET `/api/fingerprints/{id}`.
pub async fn get_fingerprint(
    app_state: web::Data<Arc<LocalState>>,
    path: web::Path<Uuid>,
) -> impl Responder {
    match fp_repo(&app_state).find(path.into_inner()).await {
        Ok(Some(fp)) => HttpResponse::Ok().json(fp),
        Ok(None) => {
            HttpResponse::NotFound().json(serde_json::json!({ "error": "fingerprint not found" }))
        }
        Err(e) => srv_err("get fingerprint", e),
    }
}

/// POST `/api/fingerprints`.
pub async fn create_fingerprint(
    app_state: web::Data<Arc<LocalState>>,
    body: web::Json<serde_json::Value>,
) -> impl Responder {
    let fp = Fingerprint::from_json(&body, Uuid::new_v4(), chrono::Utc::now());
    if !fp.has_any_criterion() {
        return HttpResponse::BadRequest().json(
            serde_json::json!({ "error": "fingerprint must configure at least one match criterion" }),
        );
    }
    if let Err(e) = hunter_engine::metrics::flow_split::FlowPatterns::validate_metric_config(
        &fp.metric_config,
    ) {
        return HttpResponse::BadRequest().json(serde_json::json!({ "error": e }));
    }
    match fp_repo(&app_state).insert(&fp).await {
        Ok(()) => HttpResponse::Created().json(fp),
        Err(e) => srv_err("create fingerprint", e),
    }
}

/// PUT `/api/fingerprints/{id}`.
pub async fn update_fingerprint(
    app_state: web::Data<Arc<LocalState>>,
    path: web::Path<Uuid>,
    body: web::Json<serde_json::Value>,
) -> impl Responder {
    let fp = Fingerprint::from_json(&body, path.into_inner(), chrono::Utc::now());
    if let Err(e) = hunter_engine::metrics::flow_split::FlowPatterns::validate_metric_config(
        &fp.metric_config,
    ) {
        return HttpResponse::BadRequest().json(serde_json::json!({ "error": e }));
    }
    match fp_repo(&app_state).update(&fp).await {
        Ok(()) => {
            // Fingerprint content is not part of `sim_key` (only the id is), so a
            // content edit would otherwise leave a durable result looking current.
            app_state.sim_results.clear_for_fingerprint(fp.id);
            HttpResponse::Ok().json(fp)
        }
        Err(e) => srv_err("update fingerprint", e),
    }
}

/// DELETE `/api/fingerprints/{id}` (FK-guarded).
pub async fn delete_fingerprint(
    app_state: web::Data<Arc<LocalState>>,
    path: web::Path<Uuid>,
) -> impl Responder {
    match fp_repo(&app_state).delete(path.into_inner()).await {
        Ok(()) => HttpResponse::NoContent().finish(),
        Err(e) => HttpResponse::Conflict()
            .json(serde_json::json!({ "error": format!("cannot delete fingerprint (in use?): {e}") })),
    }
}

/// GET `/api/strategy-rules`.
/// `?score_scope=current|all` (default `all`) — same contract + same rollup as the
/// live twin, scored over the traded positions in the synced mirror. That's what
/// makes the lab Rules list a real scoreboard (PnL / Avg% / Win% / W-L / N) rather
/// than a bare list, so rules can be ranked and compared here off live results.
pub async fn list_rules(
    app_state: web::Data<Arc<LocalState>>,
    query: web::Query<ScoreScopeParam>,
) -> impl Responder {
    rule_positions::rules_with_counters(
        &app_state.core.strategy_repo(),
        &rule_repo(&app_state),
        query.into_inner().score_scope.unwrap_or(ScoreScope::All),
    )
    .await
}

/// GET `/api/strategy-rules/{id}`.
pub async fn get_rule(
    app_state: web::Data<Arc<LocalState>>,
    path: web::Path<Uuid>,
) -> impl Responder {
    match rule_repo(&app_state).find(path.into_inner()).await {
        Ok(Some(r)) => HttpResponse::Ok().json(r),
        Ok(None) => HttpResponse::NotFound().json(serde_json::json!({ "error": "rule not found" })),
        Err(e) => srv_err("get rule", e),
    }
}

/// POST `/api/strategy-rules`.
pub async fn create_rule(
    app_state: web::Data<Arc<LocalState>>,
    body: web::Json<serde_json::Value>,
) -> impl Responder {
    let draft = match RuleDraft::from_json(&body) {
        Ok(d) => d,
        Err(e) => return HttpResponse::BadRequest().json(serde_json::json!({ "error": e })),
    };
    match rules::create_with_fp_check(&rule_repo(&app_state), &fp_repo(&app_state), &draft).await {
        Ok((rule, warning)) => {
            let mut body = serde_json::to_value(&rule).unwrap_or_default();
            if let Some(w) = warning {
                if let Some(obj) = body.as_object_mut() {
                    obj.insert("warning".into(), serde_json::json!(w));
                }
            }
            HttpResponse::Created().json(body)
        }
        Err(e) => rule_err(e, "create rule"),
    }
}

/// PUT `/api/strategy-rules/{id}`.
pub async fn update_rule(
    app_state: web::Data<Arc<LocalState>>,
    path: web::Path<Uuid>,
    body: web::Json<serde_json::Value>,
) -> impl Responder {
    let repo = rule_repo(&app_state);
    let Ok(Some(mut rule)) = repo.find(path.into_inner()).await else {
        return HttpResponse::NotFound().json(serde_json::json!({ "error": "rule not found" }));
    };
    apply_rule_update(&mut rule, &body);
    match rules::save_with_fp_check(&repo, &fp_repo(&app_state), &mut rule).await {
        Ok(warning) => {
            // Drop the durable last-sim when trading config changed (`sim_key`).
            // Rename / enable toggles that leave the key intact keep the result.
            if let Ok(key) = sim_cache_key_for_rule(&rule) {
                app_state.sim_results.invalidate_unless_key(&rule.id, &key);
            } else {
                app_state.sim_results.clear(&rule.id);
            }
            let mut body = serde_json::to_value(&rule).unwrap_or_default();
            if let Some(w) = warning {
                if let Some(obj) = body.as_object_mut() {
                    obj.insert("warning".into(), serde_json::json!(w));
                }
            }
            HttpResponse::Ok().json(body)
        }
        Err(e) => rule_err(e, "update rule"),
    }
}

/// DELETE `/api/strategy-rules/{id}`.
pub async fn delete_rule(
    app_state: web::Data<Arc<LocalState>>,
    path: web::Path<Uuid>,
) -> impl Responder {
    let id = path.into_inner();
    match rule_repo(&app_state).delete(id).await {
        Ok(()) => {
            app_state.sim_results.clear(&id);
            HttpResponse::NoContent().finish()
        }
        Err(e) => srv_err("delete rule", e),
    }
}

async fn set_rule_active(state: &LocalState, id: Uuid, active: bool) -> HttpResponse {
    let repo = rule_repo(state);
    let Ok(Some(mut rule)) = repo.find(id).await else {
        return HttpResponse::NotFound().json(serde_json::json!({ "error": "rule not found" }));
    };
    if active && !rule.is_enabled {
        return HttpResponse::BadRequest().json(serde_json::json!({
            "error": "rule is disabled; enable it before activating"
        }));
    }
    rule.is_active = active;
    rule.updated_at = chrono::Utc::now();
    match repo.update(&rule).await {
        Ok(()) => HttpResponse::Ok().json(rule),
        Err(e) => srv_err("toggle rule active", e),
    }
}

async fn set_rule_enabled(state: &LocalState, id: Uuid, enabled: bool) -> HttpResponse {
    let repo = rule_repo(state);
    let Ok(Some(mut rule)) = repo.find(id).await else {
        return HttpResponse::NotFound().json(serde_json::json!({ "error": "rule not found" }));
    };
    rule.is_enabled = enabled;
    // Disable ⇒ pause so Active/Idle cannot disagree with the archive flag.
    if !enabled {
        rule.is_active = false;
    }
    rule.updated_at = chrono::Utc::now();
    match repo.update(&rule).await {
        Ok(()) => HttpResponse::Ok().json(rule),
        Err(e) => srv_err("toggle rule enabled", e),
    }
}

/// POST `/api/strategy-rules/{id}/activate`.
pub async fn activate_rule(
    app_state: web::Data<Arc<LocalState>>,
    path: web::Path<Uuid>,
) -> impl Responder {
    set_rule_active(&app_state, path.into_inner(), true).await
}

/// POST `/api/strategy-rules/{id}/pause`.
pub async fn pause_rule(
    app_state: web::Data<Arc<LocalState>>,
    path: web::Path<Uuid>,
) -> impl Responder {
    set_rule_active(&app_state, path.into_inner(), false).await
}

/// POST `/api/strategy-rules/{id}/enable` — soft-unarchive.
pub async fn enable_rule(
    app_state: web::Data<Arc<LocalState>>,
    path: web::Path<Uuid>,
) -> impl Responder {
    set_rule_enabled(&app_state, path.into_inner(), true).await
}

/// POST `/api/strategy-rules/{id}/disable` — soft-archive (+ pause if Active).
pub async fn disable_rule(
    app_state: web::Data<Arc<LocalState>>,
    path: web::Path<Uuid>,
) -> impl Responder {
    set_rule_enabled(&app_state, path.into_inner(), false).await
}

/// `?mode=real|paper` selector for bulk pause (lab twin of the live handler).
#[derive(serde::Deserialize)]
pub struct ModeParam {
    pub mode: String,
}

/// POST `/api/strategy-rules/pause-all?mode=real|paper` — flip `is_active=false`
/// for every active rule of `mode`. Lab has no engine, so this is a pure DB flag
/// flip (live also reloads the engine + emits SSE).
pub async fn pause_all_rules(
    app_state: web::Data<Arc<LocalState>>,
    query: web::Query<ModeParam>,
) -> impl Responder {
    pause_all_of_mode(&app_state, query.mode.as_str()).await
}

async fn pause_all_of_mode(state: &LocalState, mode: &str) -> HttpResponse {
    let rules = match rule_repo(state).list().await {
        Ok(v) => v,
        Err(e) => return srv_err("pause-all: list rules", e),
    };
    let mut paused = 0usize;
    for mut rule in rules
        .into_iter()
        .filter(|r| r.is_active && r.trade_mode == mode)
    {
        rule.is_active = false;
        rule.updated_at = chrono::Utc::now();
        if let Err(e) = rule_repo(state).update(&rule).await {
            return srv_err("pause-all: update rule", e);
        }
        paused += 1;
    }
    HttpResponse::Ok().json(serde_json::json!({ "paused": paused }))
}

/// Emit a terminal `action_progress` so the Rules UI clears its Stopping spinner
/// (lab has no positions to close — `total` is always 0).
fn emit_stop_done(
    state: &LocalState,
    action_id: Uuid,
    rule_id: Option<Uuid>,
) {
    let _ = state.sse_tx.send(trading_core::models::ingest::SseEvent::ActionProgress {
        action_id,
        mint_address: None,
        rule_id,
        kind: "stop".into(),
        status: "done".into(),
        done: 0,
        total: 0,
        error: None,
    });
}

/// POST `/api/strategy-rules/{id}/stop` — lab twin of the live Stop. No engine /
/// open positions here, so this is deactivate + an immediate `action_progress`
/// done frame (same 202 wire shape the shared RulesView expects).
pub async fn stop_rule(
    app_state: web::Data<Arc<LocalState>>,
    path: web::Path<Uuid>,
) -> impl Responder {
    let id = path.into_inner();
    let pause_resp = set_rule_active(&app_state, id, false).await;
    if !pause_resp.status().is_success() {
        return pause_resp;
    }
    let action_id = Uuid::new_v4();
    emit_stop_done(&app_state, action_id, Some(id));
    HttpResponse::Accepted().json(serde_json::json!({
        "action_id": action_id,
        "kind": "stop",
        "rule_id": id,
        "total": 0,
        "closing": true,
    }))
}

/// POST `/api/strategy-rules/stop-all?mode=real|paper` — deactivate every active
/// rule of `mode` (no positions to force-close on lab).
pub async fn stop_all_rules(
    app_state: web::Data<Arc<LocalState>>,
    query: web::Query<ModeParam>,
) -> impl Responder {
    let mode = query.mode.as_str();
    let pause_resp = pause_all_of_mode(&app_state, mode).await;
    if !pause_resp.status().is_success() {
        return pause_resp;
    }
    let action_id = Uuid::new_v4();
    emit_stop_done(&app_state, action_id, None);
    HttpResponse::Accepted().json(serde_json::json!({
        "action_id": action_id,
        "kind": "stop",
        "mode": mode,
        "total": 0,
        "closing": true,
    }))
}
