//! Generic engine simulate handlers (plan 5.2) — the one simulate surface that
//! replaces the per-strategy `tpsl1`/`tpsl2`/`swing1` simulate routes. A run is
//! keyed by a `run_id` (the rule id for a saved rule, a fresh id for a dry-run
//! draft); results land in [`SimResults`](crate::state::sim_results) and are served
//! by the strategy-agnostic [`super::positions`] result pagers.

use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::json;
use uuid::Uuid;

use trading_core::api::table_query::TableRequest;
use trading_core::storage::token_enrichment::TokenEnrichment;

use crate::state::local_state::LocalState;
use crate::strategies::engine_sim::{self, EngineSimRequest};

/// A matched token sent to the page. Carries the **full** shared token enrichment
/// (`#[serde(flatten)] token`) in the response body — no client-side merge — plus
/// the row-owned identity/divergent fields (`mint`/`symbol`/`ath_price`/`created_at`)
/// the shared struct excludes. Field names match the frontend's `MatchedTokenRecord`.
#[derive(Serialize)]
pub struct MatchedTokenResult {
    pub mint_address: String,
    pub symbol: String,
    /// Token creation time (from `tokens.created_at`).
    pub created_at: DateTime<Utc>,
    pub ath_price: Option<f64>,
    #[serde(flatten)]
    pub token: TokenEnrichment,
}

#[derive(Serialize)]
pub struct MatchedTokensResponse {
    pub tokens: Vec<MatchedTokenResult>,
}

/// One server-side page of the tokens in a matched mint set, carrying the shared
/// token enrichment. The mint set is produced upstream (the rule's fingerprint
/// scan); this only pages + enriches it.
async fn matched_page_response(
    repo: trading_core::storage::repositories::strategy_repo::StrategyRepo,
    mints: &[String],
    req: TableRequest,
) -> HttpResponse {
    use trading_core::storage::repositories::strategy_repo::PositionQuery;
    let (limit, offset) = req.pagination.bounds();
    let pq = PositionQuery::from(req);
    match (
        repo.find_tokens_by_mints_paged(mints, limit, offset, &pq).await,
        repo.count_tokens_by_mints(mints, &pq).await,
    ) {
        (Ok(rows), Ok(total)) => {
            let tokens: Vec<MatchedTokenResult> = rows
                .iter()
                .map(|r| MatchedTokenResult {
                    mint_address: r.mint_address.clone(),
                    symbol: r.symbol.clone(),
                    created_at: r.token_created_at,
                    ath_price: r.ath_price,
                    token: TokenEnrichment::from(r),
                })
                .collect();
            HttpResponse::Ok()
                .insert_header(("X-Total-Count", total.to_string()))
                .insert_header(("Access-Control-Expose-Headers", "X-Total-Count"))
                .json(MatchedTokensResponse { tokens })
        }
        (Err(e), _) | (_, Err(e)) => {
            tracing::error!("matched-token page query failed: {e}");
            HttpResponse::InternalServerError().json(json!({"error": "Failed to build matched tokens"}))
        }
    }
}

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

/// POST `/api/strategies/simulate/{run_id}/result/time-summary` — hold + wall-clock
/// bins for the Temporal summary band (filter body matches `/result/summary`).
pub async fn engine_sim_result_time_summary(
    app_state: web::Data<Arc<LocalState>>,
    run_id: web::Path<Uuid>,
    query: web::Query<std::collections::HashMap<String, String>>,
    body: web::Json<TableRequest>,
) -> impl Responder {
    let wall_field = query
        .get("wall_field")
        .map(String::as_str)
        .unwrap_or("entry_time");
    super::positions::sim_result_time_summary(
        &app_state,
        run_id.into_inner(),
        body.into_inner(),
        wall_field,
    )
}

/// POST `/api/strategies/simulate/{run_id}/matched` — one page of the tokens whose
/// creation axes match the rule's fingerprint (the **matched** candidate pool the
/// run's positions are a subset of). `run_id` is the saved rule's id. The scan is
/// shared with the backtest via the candidate cache, and the mint set is paged
/// through the same shared token query the tpsl matched route uses.
pub async fn engine_matched_tokens(
    app_state: web::Data<Arc<LocalState>>,
    run_id: web::Path<Uuid>,
    body: web::Json<TableRequest>,
) -> HttpResponse {
    let rule_id = run_id.into_inner();
    let req = body.into_inner();
    let (from, to) = req.range.as_ref().map_or((None, None), |r| (r.from, r.to));

    let fp = match engine_sim::load_rule_fingerprint(&app_state, rule_id).await {
        Ok(fp) => fp,
        Err(resp) => return resp,
    };
    let mints: Vec<String> = match engine_sim::scan_matched_candidates(&app_state, &fp, from, to).await
    {
        Ok(tokens) => tokens.iter().map(|t| t.mint_address.clone()).collect(),
        Err(e) => {
            tracing::error!("engine matched-token scan failed: {e}");
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({ "error": "Failed to build matched tokens" }));
        }
    };
    matched_page_response(app_state.strategy_repo(), &mints, req).await
}
