//! Cross-cutting status + control for long-running background jobs (grouped
//! sweep, flow-discovery, metric-discovery, rule-search, family-search, rule
//! simulation), so a
//! freshly-loaded or reconnecting dashboard can recover the in-flight progress
//! that SSE (future-only) can't replay.
//!
//! - `GET  /api/jobs/status` — what's running right now + its `processed/total`.
//! - `POST /api/jobs/simulations/{rule_id}/cancel` — strategy-agnostic cancel for
//!   a rule's backtest. `sim_cancels` is keyed by `rule_id` across both tpsl
//!   snipers, so one endpoint serves both (the per-strategy cancel routes remain
//!   for back-compat).

use std::sync::atomic::Ordering;
use std::sync::Arc;

use actix_web::http::StatusCode;
use actix_web::{web, HttpResponse, Responder};
use serde::Serialize;
use uuid::Uuid;

use crate::state::local_state::LocalState;
use crate::state::sim_results::SimOutcome;

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
    /// Present iff the single-flight flow-discovery job is running.
    discovery: Option<SweepStatus>,
    /// Present iff the single-flight metric-discovery pipeline is running.
    metric_discovery: Option<SweepStatus>,
    /// Present iff the single-flight rule-search job is running.
    rule_search: Option<SweepStatus>,
    /// Present iff the single-flight family-search job is running.
    family_search: Option<SweepStatus>,
}

/// `GET /api/jobs/status` — snapshot of every running background job.
pub async fn job_status(state: web::Data<Arc<LocalState>>) -> impl Responder {
    let sweep = if state.sweep_running.load(Ordering::Acquire) {
        let (processed, total) = state.sweep_progress.snapshot();
        Some(SweepStatus { processed, total })
    } else {
        None
    };

    let discovery = if state.discovery_running.load(Ordering::Acquire) {
        let (processed, total) = state.discovery_progress.snapshot();
        Some(SweepStatus { processed, total })
    } else {
        None
    };

    let metric_discovery = if state.metric_discovery_running.load(Ordering::Acquire) {
        let (processed, total) = state.metric_discovery_progress.snapshot();
        Some(SweepStatus { processed, total })
    } else {
        None
    };

    let rule_search = if state.rule_search_running.load(Ordering::Acquire) {
        let (processed, total) = state.rule_search_progress.snapshot();
        Some(SweepStatus { processed, total })
    } else {
        None
    };

    let family_search = if state.family_search_running.load(Ordering::Acquire) {
        let (processed, total) = state.family_search_progress.snapshot();
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

    HttpResponse::Ok().json(JobsStatus {
        sweep,
        simulations,
        discovery,
        metric_discovery,
        rule_search,
        family_search,
    })
}

/// `POST /api/jobs/simulations/{rule_id}/cancel` — strategy-agnostic cooperative
/// cancel for a rule's in-flight simulation. A no-op (`{"cancelling": false}`)
/// when no simulation is running for that rule.
pub async fn cancel_simulation(
    state: web::Data<Arc<LocalState>>,
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

/// `GET /api/jobs/simulations/{rule_id}/result` — collect the terminal outcome of
/// a finished simulation (started via the per-strategy `POST .../simulate`). The
/// detached run stores its result in [`LocalState::sim_results`] on completion and
/// the client fetches it here after the `simulation_finished` SSE, so a long
/// backtest's result is never tied to the lifetime of the starting request —
/// the structural source of the old `FETCH_ERROR`. Strategy-agnostic (keyed by
/// `rule_id` like the cancel route). Ephemeral (draft/cancel/fail) entries are
/// single-delivery; durable disk results are left in place for the paged table.
///
/// - 200 + `[BacktestTokenResult, …]` — success
/// - 200 + `{"cancelled": true}` — the run was cancelled
/// - 400 / 404 / 500 + `{"error": …}` — the run failed (status mirrors the cause)
/// - 404 + `{"error": …}` — no result (still running, not started, or cleared)
pub async fn simulation_result(
    state: web::Data<Arc<LocalState>>,
    rule_id: web::Path<Uuid>,
) -> impl Responder {
    let rid = rule_id.into_inner();
    match state.sim_results.take(&rid) {
        // Legacy whole-blob collector — serialize the parsed rows back to a JSON
        // array. (The Simulated table now prefers the paged POST endpoint; this
        // stays for backward compatibility during the frontend transition.)
        Some(SimOutcome::Done(rows)) => HttpResponse::Ok().json(&*rows),
        Some(SimOutcome::Cancelled) => {
            HttpResponse::Ok().json(serde_json::json!({ "cancelled": true }))
        }
        Some(SimOutcome::Failed { status, message }) => {
            let code =
                StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            HttpResponse::build(code).json(serde_json::json!({ "error": message }))
        }
        None => HttpResponse::NotFound().json(serde_json::json!({
            "error": "no simulation result (still running, not started, or cleared)"
        })),
    }
}
