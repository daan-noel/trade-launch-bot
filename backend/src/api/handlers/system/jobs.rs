//! Cross-cutting status + control for long-running background jobs (grouped
//! sweep, rule simulation), so a freshly-loaded or reconnecting dashboard can
//! recover the in-flight progress that SSE (future-only) can't replay.
//!
//! - `GET  /api/jobs/status` — what's running right now + its `processed/total`.
//! - `POST /api/jobs/simulations/{rule_id}/cancel` — strategy-agnostic cancel for
//!   a rule's backtest. `sim_cancels` is keyed by `rule_id` across both tpsl
//!   snipers, so one endpoint serves both (the per-strategy cancel routes remain
//!   for back-compat).

use std::sync::atomic::Ordering;
use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use serde::Serialize;
use uuid::Uuid;

use crate::state::app_state::AppState;

#[derive(Serialize)]
struct SweepStatus {
    processed: u64,
    total: u64,
}

#[derive(Serialize)]
struct SimulationStatus {
    rule_id: Uuid,
    processed: u64,
    total: u64,
}

#[derive(Serialize)]
struct JobsStatus {
    /// Present iff the single-flight grouped sweep is running.
    sweep: Option<SweepStatus>,
    /// One entry per in-flight rule simulation.
    simulations: Vec<SimulationStatus>,
}

/// `GET /api/jobs/status` — snapshot of every running background job.
pub async fn job_status(state: web::Data<Arc<AppState>>) -> impl Responder {
    let sweep = if state.sweep_running.load(Ordering::Acquire) {
        let (processed, total) = state.sweep_progress.snapshot();
        Some(SweepStatus { processed, total })
    } else {
        None
    };

    let simulations = state
        .sim_progress
        .iter()
        .map(|e| {
            let (processed, total) = e.value().snapshot();
            SimulationStatus {
                rule_id: *e.key(),
                processed,
                total,
            }
        })
        .collect();

    HttpResponse::Ok().json(JobsStatus { sweep, simulations })
}

/// `POST /api/jobs/simulations/{rule_id}/cancel` — strategy-agnostic cooperative
/// cancel for a rule's in-flight simulation. A no-op (`{"cancelling": false}`)
/// when no simulation is running for that rule.
pub async fn cancel_simulation(
    state: web::Data<Arc<AppState>>,
    rule_id: web::Path<Uuid>,
) -> impl Responder {
    let rid = rule_id.into_inner();
    let cancelling = match state.sim_cancels.get(&rid) {
        Some(flag) => {
            flag.store(true, Ordering::Release);
            true
        }
        None => false,
    };
    HttpResponse::Ok().json(serde_json::json!({ "cancelling": cancelling }))
}
