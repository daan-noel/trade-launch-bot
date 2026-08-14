//! The arm ledger's read endpoints — the Console **Arms** section.
//!
//! `GET /api/strategies/armed` answers "what is armed right now" from the in-RAM
//! `ArmedRegistry`; these answer "what was armed over a window" from
//! `strategy_arms`. Two readers of one fact, on purpose — the live lane must
//! patch per-event with no round trip, this one takes a date range.
//!
//! Both take the unified [`TableRequest`] so the Arms table's paging, sorting,
//! search, per-column filters and time window compose into one body, and the
//! funnel above it describes exactly the population the table pages.
//!
//! Plan: `docs/plans/strategies/arm-ledger.md`.

use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};

use trading_core::api::table_query::TableRequest;
use trading_core::models::ArmSummary;
use trading_core::storage::repositories::arm_repo::{ArmQuery, ArmRepo};

use crate::state::deploy_state::DeployState;

/// `POST /api/strategies/arms/query` — one page of arming episodes.
///
/// Reads the **api** pool (`state.db`): this is an interactive dashboard read,
/// and the ledger's writes ride the `hot` pool so a review query can never queue
/// behind the arm rate.
pub async fn query_arms(
    app_state: web::Data<Arc<DeployState>>,
    body: web::Json<TableRequest>,
) -> impl Responder {
    let repo = ArmRepo::new(app_state.db.clone());
    let body = body.into_inner();
    let (limit, offset) = body.pagination.bounds();
    let query = ArmQuery::from(body);
    match (repo.arms_paged(limit, offset, &query).await, repo.count_arms(&query).await) {
        // Bare array + `X-Total-Count`, matching every other server-paged table
        // on this stack — one response shape means one client-side page reader.
        (Ok(items), Ok(total)) => {
            HttpResponse::Ok().insert_header(("X-Total-Count", total.to_string())).json(items)
        }
        (Err(e), _) | (_, Err(e)) => {
            tracing::warn!("query_arms failed: {e}");
            HttpResponse::InternalServerError().json(serde_json::json!({ "error": e.to_string() }))
        }
    }
}

/// `POST /api/strategies/arms/summary` — the funnel over the same cohort, plus
/// the `unsatisfiable` breakdown.
///
/// Same body as [`query_arms`], aggregated in Postgres. Aggregating server-side
/// rather than folding the fetched page is what keeps the funnel exact past the
/// page size: a count taken from 25 visible rows would re-state itself on every
/// page turn.
///
/// The breakdown is a second statement (it groups by a per-row value, which a
/// fixed-shape aggregate cannot carry) issued **concurrently** on the same pool:
/// they answer one question for one screen, and running them in series would
/// make the strip wait on the slower of two identical scans.
pub async fn arms_summary(
    app_state: web::Data<Arc<DeployState>>,
    body: web::Json<TableRequest>,
) -> impl Responder {
    let repo = ArmRepo::new(app_state.db.clone());
    let query = ArmQuery::from(body.into_inner());
    match tokio::try_join!(repo.arm_funnel(&query), repo.arm_blocked_by(&query)) {
        Ok((funnel, blocked_by)) => HttpResponse::Ok().json(ArmSummary { funnel, blocked_by }),
        Err(e) => {
            tracing::warn!("arms_summary failed: {e}");
            HttpResponse::InternalServerError().json(serde_json::json!({ "error": e.to_string() }))
        }
    }
}
