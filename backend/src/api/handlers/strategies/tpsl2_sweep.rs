//! Endpoints for TPSL2 param-sweeps. `list_runs`/`list_results` serve the
//! persisted runs + ranked per-combo rows to the dashboard's TPSL2 sweep page.
//! `start_sweep` runs a sweep **in-process against the live `TokenCache`** (zero
//! DB reads for the corpus, bounded rayon pool, single-flight) — produced by the
//! `tpsl2-sweep` CLI otherwise.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use uuid::Uuid;

use crate::state::app_state::AppState;
use crate::storage::repositories::tpsl2_sweep_repo::Tpsl2SweepRepo;
use crate::tpsl2_sweep::cli::{parse_method, run_cache_sweep, CacheSweepConfig};
use crate::tpsl2_sweep::strategy::SweepMethod;

#[derive(serde::Deserialize)]
pub struct RunsQuery {
    #[serde(default = "default_runs_limit")]
    pub limit: i64,
}

fn default_runs_limit() -> i64 {
    50
}

#[derive(serde::Deserialize)]
pub struct StartSweepBody {
    /// Base rule the swept params overlay. Omitted ⇒ the first TPSL2 rule.
    #[serde(default)]
    pub rule_id: Option<Uuid>,
    /// Token cap for the cache corpus (most-recent N in the live window).
    #[serde(default = "default_sweep_tokens")]
    pub tokens: usize,
    /// `grid` | `random:N` | `lhs:N`. Defaults to a full grid.
    #[serde(default)]
    pub method: Option<String>,
    /// Restrict to bonding-curve trades only (drop migrated-AMM legs).
    #[serde(default)]
    pub curve_only: bool,
}

fn default_sweep_tokens() -> usize {
    5_000
}

/// `POST /api/strategies/tpsl2/sweeps` — run a sweep against the **live** token
/// cache and persist it. Single-flight: 409 if one is already running. Returns
/// the persisted run on completion.
pub async fn start_sweep(
    state: web::Data<Arc<AppState>>,
    body: web::Json<StartSweepBody>,
) -> impl Responder {
    // Single-flight: claim the gate or reject. The guard resets it on every exit
    // path (success, error, or early return) so a failed sweep can't wedge it.
    if state
        .tpsl2_sweep_running
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return HttpResponse::Conflict()
            .json(serde_json::json!({"error": "a TPSL2 sweep is already running"}));
    }
    struct Gate(Arc<std::sync::atomic::AtomicBool>);
    impl Drop for Gate {
        fn drop(&mut self) {
            self.0.store(false, Ordering::Release);
        }
    }
    let _gate = Gate(state.tpsl2_sweep_running.clone());

    let b = body.into_inner();
    let cfg = CacheSweepConfig {
        rule_id: b.rule_id,
        tokens: b.tokens.clamp(1, 50_000),
        method: b.method.as_deref().map(parse_method).unwrap_or(SweepMethod::Grid),
        curve_only: b.curve_only,
    };

    match run_cache_sweep(state.db.clone(), state.token_cache.clone(), cfg).await {
        Ok(run) => HttpResponse::Ok().json(run),
        Err(e) => {
            tracing::error!("tpsl2 cache sweep failed: {e}");
            HttpResponse::InternalServerError().json(serde_json::json!({"error": e.to_string()}))
        }
    }
}

/// `GET /api/strategies/tpsl2/sweeps` — TPSL2 sweep runs, newest first.
pub async fn list_runs(
    state: web::Data<Arc<AppState>>,
    query: web::Query<RunsQuery>,
) -> impl Responder {
    let limit = query.limit.clamp(1, 200);
    match Tpsl2SweepRepo::new(state.db.clone()).list_runs(limit).await {
        Ok(runs) => HttpResponse::Ok().json(runs),
        Err(e) => {
            tracing::error!("DB error listing tpsl2 sweep runs: {e}");
            HttpResponse::InternalServerError().json(serde_json::json!({"error": "database error"}))
        }
    }
}

/// `GET /api/strategies/tpsl2/sweeps/{run_id}/results` — every ranked param-pair
/// row for a run (bounded by combo count; the table sorts/filters client-side).
pub async fn list_results(
    state: web::Data<Arc<AppState>>,
    path: web::Path<Uuid>,
) -> impl Responder {
    let run_id = path.into_inner();
    match Tpsl2SweepRepo::new(state.db.clone()).list_results(run_id).await {
        Ok(results) => HttpResponse::Ok().json(results),
        Err(e) => {
            tracing::error!("DB error listing tpsl2 sweep results: {e}");
            HttpResponse::InternalServerError().json(serde_json::json!({"error": "database error"}))
        }
    }
}
