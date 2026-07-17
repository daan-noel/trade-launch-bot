//! HTTP handlers for the generic fingerprint + metrics engine (plan 4.8):
//! * `GET /api/meta/strategy-registry` — the metric registry the frontend renders
//!   its whole rule-authoring UI from (extensibility contract, plan §8).
//! * `GET /api/strategies/armed` — a live snapshot of armed (token, rule) pairs.
//! * `/api/fingerprints` — CRUD over the shared `fingerprints` rows.
//! * `/api/strategy-rules` — CRUD + activate/pause over the generic `strategy_rules`.
//!
//! Every rule/fingerprint mutation ends with `engine.reload_rules()` so the running
//! loop picks up the change (a `RulesReloaded` event) without a restart.

use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use chrono::Utc;
use serde_json::{json, Value};
use uuid::Uuid;

use trading_core::models::{Fingerprint, StrategyRule};
use trading_core::strategies::rules::{self, RuleDraft, RuleError};

use crate::state::deploy_state::DeployState;

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
    let fp = match parse_fingerprint(&body, Uuid::new_v4()) {
        Ok(fp) => fp,
        Err(e) => return HttpResponse::BadRequest().json(json!({"error": e})),
    };
    if !fp.has_any_criterion() {
        return HttpResponse::BadRequest()
            .json(json!({"error": "fingerprint must configure at least one match criterion"}));
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
    let fp = match parse_fingerprint(&body, id) {
        Ok(fp) => fp,
        Err(e) => return HttpResponse::BadRequest().json(json!({"error": e})),
    };
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
    let draft = match parse_rule_draft(&body) {
        Ok(d) => d,
        Err(e) => return HttpResponse::BadRequest().json(json!({"error": e})),
    };
    match rules::create(&app_state.rule_repo, &draft).await {
        Ok(rule) => {
            app_state.engine.reload_rules().await;
            HttpResponse::Created().json(rule)
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
    apply_rule_body(&mut rule, &body);
    match rules::save(&app_state.rule_repo, &rule).await {
        Ok(()) => {
            app_state.engine.reload_rules().await;
            HttpResponse::Ok().json(rule)
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
            HttpResponse::Ok().json(rule)
        }
        Err(e) => server_error("toggle rule active", e),
    }
}

// ── Parsing helpers ──────────────────────────────────────────────────────────

fn parse_fingerprint(body: &Value, id: Uuid) -> Result<Fingerprint, String> {
    let now = Utc::now();
    Ok(Fingerprint {
        id,
        name: body.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
        cu_limit: opt_i64(body, "cu_limit"),
        cu_price: opt_i64(body, "cu_price"),
        init_buy_lamports: opt_i64(body, "init_buy_lamports"),
        max_cost_lamports: opt_i64(body, "max_cost_lamports"),
        spendable_lamports_in: opt_i64(body, "spendable_lamports_in"),
        first_slot_buy_lamports: opt_i64(body, "first_slot_buy_lamports"),
        first_slot_sell_lamports: opt_i64(body, "first_slot_sell_lamports"),
        bucket_size_amount: body.get("bucket_size_amount").and_then(Value::as_f64).unwrap_or(0.1),
        ix_labels: body.get("ix_labels").and_then(|v| v.as_array()).map(|a| {
            a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()
        }),
        created_at: now,
        updated_at: now,
    })
}

fn parse_rule_draft(body: &Value) -> Result<RuleDraft, String> {
    let fingerprint_id = body
        .get("fingerprint_id")
        .and_then(Value::as_str)
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or("fingerprint_id is required (uuid)")?;
    Ok(RuleDraft {
        rule_name: body.get("rule_name").and_then(Value::as_str).unwrap_or("").to_string(),
        fingerprint_id,
        trade_mode: body.get("trade_mode").and_then(Value::as_str).unwrap_or("paper").to_string(),
        buy_amount_lamports: opt_i64(body, "buy_amount_lamports").unwrap_or(0),
        max_concurrent_tokens: opt_i64(body, "max_concurrent_tokens").unwrap_or(1),
        max_total_tokens: opt_i64(body, "max_total_tokens").unwrap_or(0),
        params: body.get("params").cloned().unwrap_or_else(|| json!({})),
    })
}

/// Overlay the mutable fields of an update body onto a loaded rule.
fn apply_rule_body(rule: &mut StrategyRule, body: &Value) {
    if let Some(v) = body.get("rule_name").and_then(Value::as_str) {
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
    if let Some(v) = body.get("trade_mode").and_then(Value::as_str) {
        rule.trade_mode = v.to_string();
    }
    if let Some(v) = body.get("params").cloned() {
        rule.params = v;
    }
    rule.updated_at = Utc::now();
}

/// Read an optional integer field (accepts a JSON number or numeric string).
fn opt_i64(body: &Value, key: &str) -> Option<i64> {
    body.get(key).and_then(|v| {
        v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))
    })
}

fn rule_error(e: RuleError, ctx: &str) -> HttpResponse {
    match e {
        RuleError::Invalid(msg) => HttpResponse::BadRequest().json(json!({"error": msg})),
        RuleError::Repo(err) => server_error(ctx, err),
    }
}

fn server_error(ctx: &str, e: impl std::fmt::Display) -> HttpResponse {
    tracing::warn!("{ctx}: {e}");
    HttpResponse::InternalServerError().json(json!({"error": format!("{ctx} failed")}))
}
