//! CRUD for analysis-owned pattern sets — the Trader Analysis flow lens.
//! Lab-only (the table is in the lab-private migration set).
//!
//! A set is one vocabulary (`exact` ix_labels + fee pins, or `templates`
//! grain ids) with no fingerprint behind it, so a wallet study can classify
//! vol/non-vol on tokens that belong to no cohort. Kind is insert-only; the
//! picker is the switch. Promotion into a fingerprint (the only path to
//! something the engine trades) is a client-side copy through the existing
//! fingerprint PUT — nothing here writes `metric_config`.

use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use uuid::Uuid;

use crate::state::local_state::LocalState;
use crate::storage::repositories::ix_pattern_set_repo::{
    validate, IxPatternSetDraft, IxPatternSetRepo,
};

fn repo(state: &LocalState) -> IxPatternSetRepo {
    IxPatternSetRepo::new(state.core.db.clone())
}

fn srv_err(ctx: &str, e: impl std::fmt::Display) -> HttpResponse {
    tracing::warn!("{ctx}: {e}");
    HttpResponse::InternalServerError()
        .json(serde_json::json!({ "error": format!("{ctx} failed") }))
}

/// A name collision is the one expected write failure (unique index on
/// `lower(name)`), and it is the user's to fix — report it as a 409, not a 500.
fn write_err(ctx: &str, e: sqlx::Error) -> HttpResponse {
    if let sqlx::Error::Database(db) = &e {
        if db.constraint() == Some("idx_ix_pattern_sets_name") {
            return HttpResponse::Conflict().json(
                serde_json::json!({ "error": "a pattern set with this name already exists" }),
            );
        }
    }
    srv_err(ctx, e)
}

/// GET `/api/ix-pattern-sets` — every set, most-recently-updated first.
pub async fn list_sets(app_state: web::Data<Arc<LocalState>>) -> impl Responder {
    match repo(&app_state).list().await {
        Ok(v) => HttpResponse::Ok().json(v),
        Err(e) => srv_err("list ix pattern sets", e),
    }
}

/// POST `/api/ix-pattern-sets`.
pub async fn create_set(
    app_state: web::Data<Arc<LocalState>>,
    body: web::Json<IxPatternSetDraft>,
) -> impl Responder {
    let draft = body.into_inner();
    let sanitized = match validate(&draft) {
        Ok(s) => s,
        Err(e) => return HttpResponse::BadRequest().json(serde_json::json!({ "error": e })),
    };
    match repo(&app_state).insert(&draft, &sanitized).await {
        Ok(set) => HttpResponse::Ok().json(set),
        Err(e) => write_err("create ix pattern set", e),
    }
}

/// PUT `/api/ix-pattern-sets/{id}` — full replace of the writable half.
pub async fn update_set(
    app_state: web::Data<Arc<LocalState>>,
    path: web::Path<Uuid>,
    body: web::Json<IxPatternSetDraft>,
) -> impl Responder {
    let id = path.into_inner();
    let mut draft = body.into_inner();
    // Kind is insert-only. A PUT that omits it (or sends the other vocabulary)
    // must not re-sanitize the stored list as Exact and wipe templates.
    match repo(&app_state).find(id).await {
        Ok(Some(existing)) => draft.kind = existing.kind,
        Ok(None) => {
            return HttpResponse::NotFound()
                .json(serde_json::json!({ "error": "pattern set not found" }))
        }
        Err(e) => return srv_err("load ix pattern set", e),
    }
    let sanitized = match validate(&draft) {
        Ok(s) => s,
        Err(e) => return HttpResponse::BadRequest().json(serde_json::json!({ "error": e })),
    };
    match repo(&app_state).update(id, &draft, &sanitized).await {
        Ok(Some(set)) => HttpResponse::Ok().json(set),
        Ok(None) => {
            HttpResponse::NotFound().json(serde_json::json!({ "error": "pattern set not found" }))
        }
        Err(e) => write_err("update ix pattern set", e),
    }
}

/// DELETE `/api/ix-pattern-sets/{id}`.
pub async fn delete_set(
    app_state: web::Data<Arc<LocalState>>,
    path: web::Path<Uuid>,
) -> impl Responder {
    match repo(&app_state).delete(path.into_inner()).await {
        Ok(true) => HttpResponse::NoContent().finish(),
        Ok(false) => {
            HttpResponse::NotFound().json(serde_json::json!({ "error": "pattern set not found" }))
        }
        Err(e) => srv_err("delete ix pattern set", e),
    }
}
