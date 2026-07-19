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
use serde_json::{json, Value};
use uuid::Uuid;

use std::sync::Arc;

use trading_core::api::table_query::TableRequest;
use trading_core::models::{Position, PositionResponse};
use trading_core::storage::repositories::strategy_repo::{PositionQuery, StrategyRepo};

use crate::state::local_state::LocalState;
use crate::state::sim_results::SimOutcome;

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

/// Run-split selector, mirroring the live `PositionScope`. Lab is paper-only, so
/// `Current` = latest paper run, `History` = every prior paper run. Absent = legacy
/// (latest paper run), so callers that don't opt into the split are unchanged.
#[derive(serde::Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PositionScope {
    Current,
    History,
}

#[derive(serde::Deserialize)]
pub struct ScopeParam {
    pub scope: Option<PositionScope>,
}

/// Attach shared token enrichment to a page of position responses via one bounded
/// batch fetch (`token_enrichment::fetch_by_mints` over the page's mints) — the same
/// SSOT the Matched / Simulated / Sweep tables use, so the positions table
/// sorts/filters/searches on token columns with no client merge. Sets the row-owned
/// `symbol` / `ath_price` off the row too. A fetch error is logged and leaves rows
/// un-enriched rather than failing the list.
async fn enrich_positions(repo: &StrategyRepo, responses: &mut [PositionResponse]) {
    if responses.is_empty() {
        return;
    }
    let mints: Vec<String> = responses.iter().map(|r| r.mint_address.clone()).collect();
    match trading_core::storage::token_enrichment::fetch_by_mints(repo.pool(), &mints).await {
        Ok(rows) => {
            let by_mint: std::collections::HashMap<String, _> =
                rows.into_iter().map(|r| (r.mint_address.clone(), r)).collect();
            for r in responses.iter_mut() {
                if let Some(row) = by_mint.get(&r.mint_address) {
                    r.symbol = row.symbol.clone();
                    r.ath_price = row.ath_price;
                    r.token = row.into();
                }
            }
        }
        Err(e) => tracing::warn!("positions enrichment fetch failed: {e}"),
    }
}

/// POST `.../rules/{rule_id}/positions` — one page of positions, with the total on
/// `X-Total-Count` (exposed to the browser fetch) so the client pager can size itself.
/// `scope` selects the run(s): `current`/absent → latest paper run; `history` → every
/// prior run, each row stamped with its `run_seq`. Empty (200-total 0) when there's no
/// matching run. The JSON body ([`TableRequest`]) carries paging + sort/search/filter.
pub async fn positions_by_rule_paged(
    repo: &StrategyRepo,
    rule_id: Uuid,
    strategy_id: &str,
    scope: Option<PositionScope>,
    req: TableRequest,
) -> HttpResponse {
    let (limit, offset) = req.pagination.bounds();
    let pq = PositionQuery::from(req);

    if matches!(scope, Some(PositionScope::History)) {
        // "Old runs": all paper runs except the latest, rows stamped with run_seq.
        let runs = match paper_runs_of(repo, rule_id, strategy_id).await {
            Ok(runs) => runs,
            Err(resp) => return resp,
        };
        let Some(&(latest_run_id, _)) = runs.first() else {
            return positions_response(Vec::new(), 0);
        };
        if runs.len() <= 1 {
            return positions_response(Vec::new(), 0);
        }
        let seq_map: std::collections::HashMap<Uuid, i64> = runs.into_iter().collect();
        return match (
            repo.find_positions_by_rule_excluding_run_paged(rule_id, latest_run_id, limit, offset, &pq)
                .await,
            repo.count_positions_by_rule_excluding_run(rule_id, latest_run_id, &pq).await,
        ) {
            (Ok(positions), Ok(total)) => {
                let mut responses: Vec<PositionResponse> = positions
                    .iter()
                    .map(|p| {
                        let mut r = PositionResponse::from(Position::from(p));
                        r.run_seq = seq_map.get(&p.run_id).copied();
                        r
                    })
                    .collect();
                enrich_positions(repo, &mut responses).await;
                positions_response(responses, total)
            }
            (Err(e), _) | (_, Err(e)) => {
                tracing::error!("Failed to load run-history positions for rule {rule_id}: {e}");
                HttpResponse::InternalServerError().json(json!({"error": "Failed to load positions"}))
            }
        };
    }

    // `current` / absent: the latest paper run's bag.
    let run_id = match latest_paper_run(repo, rule_id, strategy_id).await {
        Ok(Some(id)) => id,
        Ok(None) => return positions_response(Vec::new(), 0),
        Err(resp) => return resp,
    };
    match (
        repo.find_positions_by_run_paged(run_id, limit, offset, &pq).await,
        repo.count_positions_by_run(run_id, &pq).await,
    ) {
        (Ok(positions), Ok(total)) => {
            let mut responses: Vec<PositionResponse> = positions
                .iter()
                .map(|p| PositionResponse::from(Position::from(p)))
                .collect();
            enrich_positions(repo, &mut responses).await;
            positions_response(responses, total)
        }
        (Err(e), _) | (_, Err(e)) => {
            tracing::error!("Failed to load positions for run {run_id}: {e}");
            HttpResponse::InternalServerError().json(json!({"error": "Failed to load positions"}))
        }
    }
}

/// POST `.../rules/{rule_id}/positions/summary` — aggregates for the Positions Summary
/// panel over the same filtered population its table pages: `current`/absent → latest paper
/// run; `history` → every prior run. Empty summary when there's no matching run.
pub async fn positions_summary_by_rule(
    repo: &StrategyRepo,
    rule_id: Uuid,
    strategy_id: &str,
    scope: Option<PositionScope>,
    req: TableRequest,
) -> HttpResponse {
    let empty = || HttpResponse::Ok().json(trading_core::models::PositionsSummary::default());
    let pq = PositionQuery::from(req);

    // `lab` has no live runtime price cache (no ingest), so open positions can't be
    // marked to market here — `open_pnl_sol` stays 0. Analysis surfaces that *do*
    // report an unrealized figure (simulate, grouped sweep) mark against the replay's
    // last observed price instead; see `sim_query::summarize`.
    let no_prices = |_: &str| None;

    let result = if matches!(scope, Some(PositionScope::History)) {
        // Exclude the latest run; a lone run yields an empty (tokens=0) summary.
        match latest_paper_run(repo, rule_id, strategy_id).await {
            Ok(Some(latest_run_id)) => {
                repo.positions_summary_by_rule_excluding_run(
                    rule_id,
                    latest_run_id,
                    &pq,
                    no_prices,
                )
                .await
            }
            Ok(None) => return empty(),
            Err(resp) => return resp,
        }
    } else {
        match latest_paper_run(repo, rule_id, strategy_id).await {
            Ok(Some(run_id)) => repo.positions_summary_by_run(run_id, &pq, no_prices).await,
            Ok(None) => return empty(),
            Err(resp) => return resp,
        }
    };

    match result {
        Ok(summary) => HttpResponse::Ok().json(summary),
        Err(e) => {
            tracing::error!("Failed to load positions summary for rule {rule_id}: {e}");
            HttpResponse::InternalServerError().json(json!({"error": "Failed to load positions summary"}))
        }
    }
}

/// `(run_id, run_seq)` for every paper run of a rule, newest first — but only after
/// confirming the rule exists and belongs to `strategy_id` (so a mistyped strategy
/// segment can't read another strategy's runs). Returns `Ok(vec![])` for an unknown /
/// other-strategy rule (→ empty history).
async fn paper_runs_of(
    repo: &StrategyRepo,
    rule_id: Uuid,
    strategy_id: &str,
) -> Result<Vec<(Uuid, i64)>, HttpResponse> {
    match repo.find_rule(rule_id).await {
        Ok(Some(r)) if r.strategy_id == strategy_id => {}
        Ok(_) => return Ok(Vec::new()),
        Err(e) => {
            tracing::error!("Failed to get rule {rule_id}: {e}");
            return Err(HttpResponse::InternalServerError().json(json!({"error": "Failed to get rule"})));
        }
    }
    repo.run_seqs_for_rule(rule_id, "paper").await.map_err(|e| {
        tracing::error!("Failed to load paper runs for {rule_id}: {e}");
        HttpResponse::InternalServerError().json(json!({"error": "Failed to load paper runs"}))
    })
}

fn positions_response(responses: Vec<PositionResponse>, total: i64) -> HttpResponse {
    HttpResponse::Ok()
        .insert_header(("X-Total-Count", total.to_string()))
        .insert_header(("Access-Control-Expose-Headers", "X-Total-Count"))
        .json(responses)
}

// ---------------------------------------------------------------------------
// Simulated-result table (server-side, in-memory over the finished sim's rows)
// ---------------------------------------------------------------------------

/// Borrow a rule's finished sim rows from the in-memory store, mapping the
/// non-success / absent states to the response the caller should return as-is.
/// `Ok(rows)` on success; `Err(resp)` for cancelled / failed / not-yet-available.
fn peek_sim_rows(state: &LocalState, rule_id: Uuid) -> Result<Arc<Vec<serde_json::Value>>, HttpResponse> {
    match state.sim_results.peek(&rule_id) {
        Some(SimOutcome::Done(rows)) => Ok(rows),
        Some(SimOutcome::Cancelled) => {
            Err(HttpResponse::Ok().json(json!({ "cancelled": true })))
        }
        Some(SimOutcome::Failed { status, message }) => {
            let code = actix_web::http::StatusCode::from_u16(status)
                .unwrap_or(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR);
            Err(HttpResponse::build(code).json(json!({ "error": message })))
        }
        None => Err(HttpResponse::NotFound().json(json!({
            "error": "no simulation result (still running, not started, or expired)"
        }))),
    }
}

/// POST `.../rules/{rule_id}/simulate/result` — one page of the latest finished
/// backtest's per-token rows, sorted/filtered/searched server-side **in memory**
/// (the results are already resident — lab is single-user), with the full match
/// count on `X-Total-Count`. Shared by tpsl1/tpsl2/swing1.
pub fn sim_result_page(state: &LocalState, rule_id: Uuid, req: TableRequest) -> HttpResponse {
    let rows = match peek_sim_rows(state, rule_id) {
        Ok(rows) => rows,
        Err(resp) => return resp,
    };
    let (page, total) = crate::strategies::sim_query::query(&rows, &req);
    HttpResponse::Ok()
        .insert_header(("X-Total-Count", total.to_string()))
        .insert_header(("Access-Control-Expose-Headers", "X-Total-Count"))
        .json(json!({ "tokens": page }))
}

/// POST `.../rules/{rule_id}/simulate/result/summary` — aggregate over the finished
/// sim's rows matching the request's search + filters (pagination/sort ignored) for
/// the Simulated summary card. Shared by tpsl1/tpsl2/swing1.
pub fn sim_result_summary(state: &LocalState, rule_id: Uuid, req: TableRequest) -> HttpResponse {
    let rows = match peek_sim_rows(state, rule_id) {
        Ok(rows) => rows,
        Err(resp) => return resp,
    };
    let filtered = crate::strategies::sim_query::filter_rows(&rows, &req);
    let metrics = crate::strategies::sim_query::summarize(&filtered);
    // Graduated-to-AMM count over the same filtered cohort. A token attribute
    // (`is_migrated`), not a kernel PnL figure, so it rides on the response body
    // beside `computed_at` rather than inside `RunSummary`. Every sim row is an
    // entered position, so this counts among `n_fired` — matching the live
    // card's "migrated among entered".
    let migrated = filtered
        .iter()
        .filter(|r| r.get("is_migrated").and_then(Value::as_bool).unwrap_or(false))
        .count();
    // Flattened `RunMetrics` — the same field names the grouped sweep and a
    // live/paper run emit, so one frontend component renders all three (parity
    // plan B4). `computed_at` / `n_migrated` are the sim-specific additions.
    let mut body = serde_json::to_value(&metrics).unwrap_or_else(|_| json!({}));
    if let Some(obj) = body.as_object_mut() {
        // When the run finished — the Run column renders this as relative time.
        obj.insert("computed_at".into(), json!(state.sim_results.computed_at(&rule_id)));
        obj.insert("n_migrated".into(), json!(migrated));
    }
    HttpResponse::Ok().json(body)
}
