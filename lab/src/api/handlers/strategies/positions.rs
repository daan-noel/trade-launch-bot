//! Shared lab (analysis-box) position-read helpers for the per-strategy handlers
//! (`tpsl1` / `tpsl2` / `swing1`). The analysis box only ever runs a rule in paper
//! mode, so every read resolves the rule's **latest paper run** and serves/aggregates
//! that. Rendered through the shared `PositionResponse` (via `Position`) so the wire
//! shape is byte-for-byte the live endpoints — the frontend `useRulePositions` hook
//! consumes both identically.
//!
//! Kept in one place (rather than cloned three times) because the three strategy
//! handlers differ only by `strategy_id`; the paging/summary logic is identical.

use actix_web::HttpResponse;
use serde_json::json;
use uuid::Uuid;

use trading_core::api::table_query::TableRequest;
use trading_core::models::{Position, PositionResponse};
use trading_core::storage::repositories::strategy_repo::{PositionQuery, StrategyRepo};

/// Resolve the rule's latest paper run, scoped to `strategy_id`. Returns:
///   `Ok(Some(run_id))` — a paper run exists;
///   `Ok(None)`         — unknown / other-strategy rule, or no paper run yet (empty);
///   `Err(resp)`        — a DB error response the caller should return as-is.
async fn latest_paper_run(
    repo: &StrategyRepo,
    rule_id: Uuid,
    strategy_id: &str,
) -> Result<Option<Uuid>, HttpResponse> {
    match repo.find_rule(rule_id).await {
        Ok(Some(r)) if r.strategy_id == strategy_id => {}
        Ok(_) => return Ok(None),
        Err(e) => {
            tracing::error!("Failed to get rule {rule_id}: {e}");
            return Err(HttpResponse::InternalServerError().json(json!({"error": "Failed to get rule"})));
        }
    }
    match repo.latest_run(rule_id, "paper").await {
        Ok(Some(run)) => Ok(Some(run.id)),
        Ok(None) => Ok(None),
        Err(e) => {
            tracing::error!("Failed to load paper run for {rule_id}: {e}");
            Err(HttpResponse::InternalServerError().json(json!({"error": "Failed to load paper run"})))
        }
    }
}

/// POST `.../rules/{rule_id}/positions` — one page of the latest paper run's bag,
/// with the run-wide total on `X-Total-Count` (exposed to the browser fetch) so the
/// client pager can size itself. Empty (200-total 0) when the rule has no paper run.
/// The JSON body ([`TableRequest`]) carries paging + server-side sort/search/filter;
/// `pagination` → `(limit, offset)`, the rest → [`PositionQuery`] via `From`.
pub async fn positions_by_rule_paged(
    repo: &StrategyRepo,
    rule_id: Uuid,
    strategy_id: &str,
    req: TableRequest,
) -> HttpResponse {
    let run_id = match latest_paper_run(repo, rule_id, strategy_id).await {
        Ok(Some(id)) => id,
        Ok(None) => return positions_response(Vec::new(), 0),
        Err(resp) => return resp,
    };
    let (limit, offset) = req.pagination.bounds();
    let pq = PositionQuery::from(req);
    match (
        repo.find_positions_by_run_paged(run_id, limit, offset, &pq).await,
        repo.count_positions_by_run(run_id, &pq).await,
    ) {
        (Ok(positions), Ok(total)) => {
            let responses: Vec<PositionResponse> = positions
                .iter()
                .map(|p| PositionResponse::from(Position::from(p)))
                .collect();
            positions_response(responses, total)
        }
        (Err(e), _) | (_, Err(e)) => {
            tracing::error!("Failed to load positions for run {run_id}: {e}");
            HttpResponse::InternalServerError().json(json!({"error": "Failed to load positions"}))
        }
    }
}

/// GET `.../rules/{rule_id}/positions/summary` — run-wide aggregates for the
/// Positions Summary panel (latest paper run). Empty summary when no paper run.
pub async fn positions_summary_by_rule(
    repo: &StrategyRepo,
    rule_id: Uuid,
    strategy_id: &str,
) -> HttpResponse {
    let run_id = match latest_paper_run(repo, rule_id, strategy_id).await {
        Ok(Some(id)) => id,
        Ok(None) => {
            return HttpResponse::Ok()
                .json(trading_core::models::PositionsSummary::default())
        }
        Err(resp) => return resp,
    };
    match repo.positions_summary_by_run(run_id).await {
        Ok(summary) => HttpResponse::Ok().json(summary),
        Err(e) => {
            tracing::error!("Failed to load positions summary for run {run_id}: {e}");
            HttpResponse::InternalServerError().json(json!({"error": "Failed to load positions summary"}))
        }
    }
}

fn positions_response(responses: Vec<PositionResponse>, total: i64) -> HttpResponse {
    HttpResponse::Ok()
        .insert_header(("X-Total-Count", total.to_string()))
        .insert_header(("Access-Control-Expose-Headers", "X-Total-Count"))
        .json(responses)
}
