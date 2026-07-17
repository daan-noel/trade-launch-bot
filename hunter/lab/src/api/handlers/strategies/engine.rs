//! Generic engine simulate handlers (plan 5.2) — the one simulate surface that
//! replaces the per-strategy `tpsl1`/`tpsl2`/`swing1` simulate routes. A run is
//! keyed by a `run_id` (the rule id for a saved rule, a fresh id for a dry-run
//! draft); results land in [`SimResults`](crate::state::sim_results) and are served
//! by the strategy-agnostic [`super::positions`] result pagers.

use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use uuid::Uuid;

use trading_core::api::table_query::TableRequest;

use crate::state::local_state::LocalState;
use crate::strategies::engine_sim::{self, EngineSimRequest};

/// POST `/api/strategies/simulate` — start a generic engine simulation for a saved
/// rule (`{ "rule_id": ... }`) or an inline draft (`{ "draft": { ... } }`), over an
/// optional `{ since, until }` creation window. Returns `202 { run_id, started }`.
pub async fn simulate_engine(
    app_state: web::Data<Arc<LocalState>>,
    body: web::Json<EngineSimRequest>,
) -> impl Responder {
    engine_sim::spawn_engine_simulation(app_state, body.into_inner()).await
}

/// POST `/api/strategies/simulate/{run_id}/cancel` — cooperative cancel of an
/// in-flight run. A no-op when none is running for the id.
pub async fn cancel_engine_simulation(
    app_state: web::Data<Arc<LocalState>>,
    run_id: web::Path<Uuid>,
) -> impl Responder {
    let cancelling = match app_state.sim_cancels.get(&run_id.into_inner()) {
        Some(flag) => {
            flag.store(true, std::sync::atomic::Ordering::Release);
            true
        }
        None => false,
    };
    HttpResponse::Ok().json(serde_json::json!({ "cancelling": cancelling }))
}

/// POST `/api/strategies/simulate/{run_id}/result` — one page of a finished run's
/// per-token rows (shared server-side pager).
pub async fn engine_sim_result(
    app_state: web::Data<Arc<LocalState>>,
    run_id: web::Path<Uuid>,
    body: web::Json<TableRequest>,
) -> impl Responder {
    super::positions::sim_result_page(&app_state, run_id.into_inner(), body.into_inner())
}

/// POST `/api/strategies/simulate/{run_id}/result/summary` — aggregate rollup over a
/// finished run's filtered rows.
pub async fn engine_sim_result_summary(
    app_state: web::Data<Arc<LocalState>>,
    run_id: web::Path<Uuid>,
    body: web::Json<TableRequest>,
) -> impl Responder {
    super::positions::sim_result_summary(&app_state, run_id.into_inner(), body.into_inner())
}
