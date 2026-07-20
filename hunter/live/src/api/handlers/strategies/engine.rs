//! HTTP handlers for the generic fingerprint + metrics engine (plan 4.8):
//! * `GET /api/meta/strategy-registry` — the metric registry the frontend renders
//!   its whole rule-authoring UI from (extensibility contract, plan §8).
//! * `GET /api/strategies/armed` — a live snapshot of armed (token, rule) pairs.
//! * `/api/fingerprints` — CRUD over the shared `fingerprints` rows.
//! * `/api/strategy-rules` — CRUD + lifecycle over the generic `strategy_rules`:
//!   per-rule activate / pause / stop (stop force-closes the rule's open positions
//!   via the engine loop), plus `pause-all` / `stop-all` scoped by `?mode=real|paper`.
//!
//! Every rule/fingerprint mutation ends with `engine.reload_rules()` so the running
//! loop picks up the change (a `RulesReloaded` event) without a restart.

use std::collections::HashSet;
use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use chrono::Utc;
use serde_json::{json, Value};
use trading_core::models::ingest::SseEvent;
use trading_core::models::Fingerprint;
use trading_core::strategies::rules::{self, apply_rule_update, RuleDraft, RuleError};
use uuid::Uuid;

use super::action_progress;
use crate::state::deploy_state::DeployState;

/// Signal that the strategy-rules list changed so clients confirm optimistic
/// pause/activate without waiting on a refetch race. Legacy event name
/// (`tpsl_rules_changed`); payload `strategy` is `"generic"` for the unified engine.
fn emit_rules_changed(app_state: &DeployState) {
    let _ = app_state.sse_tx.send(SseEvent::TpslRulesChanged {
        strategy: "generic".into(),
    });
}

// ── Metadata ─────────────────────────────────────────────────────────────────

/// GET /api/meta/strategy-registry — the engine's self-describing metric registry.
pub async fn strategy_registry() -> impl Responder {
    HttpResponse::Ok().json(hunter_engine::metrics::registry_json())
}

/// GET /api/strategies/armed — currently-armed (token, rule) pairs (live monitor).
pub async fn list_armed(app_state: web::Data<Arc<DeployState>>) -> impl Responder {
    HttpResponse::Ok().json(app_state.armed.snapshot())
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

/// POST /api/fingerprints
pub async fn create_fingerprint(
    app_state: web::Data<Arc<DeployState>>,
    body: web::Json<Value>,
) -> impl Responder {
    let fp = Fingerprint::from_json(&body, Uuid::new_v4(), Utc::now());
    if !fp.has_any_criterion() {
        return HttpResponse::BadRequest()
            .json(json!({"error": "fingerprint must configure at least one match criterion"}));
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
    let fp = Fingerprint::from_json(&body, id, Utc::now());
    if let Err(e) = hunter_engine::metrics::flow_split::FlowPatterns::validate_metric_config(
        &fp.metric_config,
    ) {
        return HttpResponse::BadRequest().json(json!({ "error": e }));
    }
    match app_state.fingerprint_repo.update(&fp).await {
        Ok(()) => {
            app_state.engine.reload_rules().await;
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
pub async fn list_rules(app_state: web::Data<Arc<DeployState>>) -> impl Responder {
    match app_state.rule_repo.list().await {
        Ok(v) => HttpResponse::Ok().json(v),
        Err(e) => server_error("list rules", e),
    }
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
            app_state.engine.reload_rules().await;
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
            app_state.engine.reload_rules().await;
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
            app_state.engine.reload_rules().await;
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

async fn set_active(app_state: &DeployState, id: Uuid, active: bool) -> HttpResponse {
    let Ok(Some(mut rule)) = app_state.rule_repo.find(id).await else {
        return HttpResponse::NotFound().json(json!({"error": "rule not found"}));
    };
    rule.is_active = active;
    rule.updated_at = Utc::now();
    match app_state.rule_repo.update(&rule).await {
        Ok(()) => {
            app_state.engine.reload_rules().await;
            emit_rules_changed(app_state);
            HttpResponse::Ok().json(rule)
        }
        Err(e) => server_error("toggle rule active", e),
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
    let open = match app_state.strategy_repo.find_open_positions().await {
        Ok(all) => all
            .into_iter()
            .filter(|p| p.rule_id == Some(id))
            .collect::<Vec<_>>(),
        Err(e) => return server_error("stop: list open positions", e),
    };
    let total = open.len() as u64;
    let position_ids: HashSet<Uuid> = open.into_iter().map(|p| p.id).collect();
    let action_id = Uuid::new_v4();

    // Subscribe + start frame before close so we don't miss terminal updates.
    action_progress::spawn_stop_watcher(
        app_state.sse_tx.clone(),
        action_id,
        Some(id),
        position_ids,
    );
    app_state.engine.close_rule(id).await;

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

/// Deactivate every active rule of `mode`. Returns the count of rules paused, or a
/// `500` if the rule list / an update fails.
async fn pause_all_of_mode(app_state: &DeployState, mode: &str) -> HttpResponse {
    let rules = match app_state.rule_repo.list().await {
        Ok(v) => v,
        Err(e) => return server_error("pause-all: list rules", e),
    };
    let mut paused = 0usize;
    for mut rule in rules.into_iter().filter(|r| r.is_active && r.trade_mode == mode) {
        rule.is_active = false;
        rule.updated_at = Utc::now();
        if let Err(e) = app_state.rule_repo.update(&rule).await {
            return server_error("pause-all: update rule", e);
        }
        paused += 1;
    }
    app_state.engine.reload_rules().await;
    if paused > 0 {
        emit_rules_changed(app_state);
    }
    HttpResponse::Ok().json(json!({ "paused": paused }))
}

/// POST /api/strategy-rules/pause-all?mode=real|paper — entries off for every active
/// rule of `mode`; open positions are left to drain (same as the per-row Pause).
pub async fn pause_all_rules(
    app_state: web::Data<Arc<DeployState>>,
    query: web::Query<ModeParam>,
) -> impl Responder {
    pause_all_of_mode(&app_state, &query.mode).await
}

/// POST /api/strategy-rules/stop-all?mode=real|paper — force-close every open
/// position of `mode` (active or draining) **and** deactivate its active rules.
/// Returns 202 + `action_id`; closes stream over `action_progress`.
pub async fn stop_all_rules(
    app_state: web::Data<Arc<DeployState>>,
    query: web::Query<ModeParam>,
) -> impl Responder {
    let mode = query.mode.as_str();
    let open = match app_state.strategy_repo.find_open_positions().await {
        Ok(all) => all
            .into_iter()
            .filter(|p| p.mode == mode)
            .collect::<Vec<_>>(),
        Err(e) => return server_error("stop-all: list open positions", e),
    };
    let total = open.len() as u64;
    let position_ids: HashSet<Uuid> = open.into_iter().map(|p| p.id).collect();
    let action_id = Uuid::new_v4();

    action_progress::spawn_stop_watcher(app_state.sse_tx.clone(), action_id, None, position_ids);
    app_state.engine.close_mode(mode == "real").await;

    let pause_resp = pause_all_of_mode(&app_state, mode).await;
    if !pause_resp.status().is_success() {
        return pause_resp;
    }

    HttpResponse::Accepted().json(json!({
        "action_id": action_id,
        "kind": "stop",
        "mode": mode,
        "total": total,
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
