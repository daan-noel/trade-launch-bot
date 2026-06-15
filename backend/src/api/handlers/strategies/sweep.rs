//! Read-only endpoints for strategy param-sweep results. A sweep is produced
//! offline by `backend -- sweep …`; these serve the persisted runs + their
//! ranked per-combo rows to the dashboard's per-strategy sweep page.
//!
//! Strategy-agnostic: `{strategy}` (e.g. `tpsl1`/`tpsl2`) is just a filter — the
//! same handlers back every strategy's sweep page.

use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use uuid::Uuid;

use crate::state::app_state::AppState;
use crate::storage::repositories::sweep_repo::SweepRepo;

#[derive(serde::Deserialize)]
pub struct RunsQuery {
    #[serde(default = "default_runs_limit")]
    pub limit: i64,
}

fn default_runs_limit() -> i64 {
    50
}

/// `GET /api/strategies/{strategy}/sweeps` — runs for a strategy, newest first.
pub async fn list_runs(
    state: web::Data<Arc<AppState>>,
    path: web::Path<String>,
    query: web::Query<RunsQuery>,
) -> impl Responder {
    let strategy = path.into_inner();
    let limit = query.limit.clamp(1, 200);
    match SweepRepo::new(state.db.clone()).list_runs(&strategy, limit).await {
        Ok(runs) => HttpResponse::Ok().json(runs),
        Err(e) => {
            tracing::error!("DB error listing sweep runs: {e}");
            HttpResponse::InternalServerError().json(serde_json::json!({"error": "database error"}))
        }
    }
}

/// `GET /api/strategies/{strategy}/sweeps/{run_id}/results` — every ranked
/// param-pair row for a run (bounded by combo count; the table sorts/filters
/// client-side).
pub async fn list_results(
    state: web::Data<Arc<AppState>>,
    path: web::Path<(String, Uuid)>,
) -> impl Responder {
    let (_strategy, run_id) = path.into_inner();
    match SweepRepo::new(state.db.clone()).list_results(run_id).await {
        Ok(results) => HttpResponse::Ok().json(results),
        Err(e) => {
            tracing::error!("DB error listing sweep results: {e}");
            HttpResponse::InternalServerError().json(serde_json::json!({"error": "database error"}))
        }
    }
}
