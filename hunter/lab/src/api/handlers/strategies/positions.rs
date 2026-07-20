//! Lab (analysis-box) simulated-result table endpoints — server-side paging /
//! summary over a finished backtest's per-token rows, held in memory (lab is
//! single-user). Rendered so the frontend's simulate table + summary card consume
//! the same wire shape as the live/sweep surfaces. The legacy per-strategy
//! (`tpsl1`/`tpsl2`/`swing1`) paper-position read handlers were retired in Phase 7;
//! only the generic sim-result views remain.

use actix_web::HttpResponse;
use serde_json::{json, Value};
use uuid::Uuid;

use std::sync::Arc;

use trading_core::api::table_query::TableRequest;

use crate::state::local_state::LocalState;
use crate::state::sim_results::SimOutcome;

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
