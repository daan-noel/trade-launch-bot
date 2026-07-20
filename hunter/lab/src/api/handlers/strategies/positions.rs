//! Lab (analysis-box) simulated-result table endpoints — server-side paging /
//! summary over a finished backtest's per-token rows. Durable results live on
//! disk under `$SWEEP_LAKE_DIR/sim-results/`; the pager hydrates at most one
//! rule's rows into the RAM working set. Rendered so the frontend's simulate
//! table + summary card consume the same wire shape as the live/sweep surfaces.

use actix_web::HttpResponse;
use serde_json::{json, Map, Value};
use uuid::Uuid;

use std::sync::Arc;

use trading_core::api::table_query::TableRequest;

use crate::state::local_state::LocalState;
use crate::state::sim_results::SimOutcome;

// ---------------------------------------------------------------------------
// Simulated-result table (server-side page over hydrated working-set rows)
// ---------------------------------------------------------------------------

/// Borrow a rule's finished sim rows from the store (disk → working set on miss),
/// mapping the non-success / absent states to the response the caller should
/// return as-is. `Ok(rows)` on success; `Err(resp)` for cancelled / failed /
/// not-yet-available.
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
            "error": "no simulation result (still running, not started, or cleared)"
        }))),
    }
}

/// Flattened `RunSummary` + sim-only extras (`computed_at`, `n_migrated`) — the
/// wire shape the Simulate table columns and summary card both consume.
fn summary_json_from_rows(state: &LocalState, rule_id: Uuid, rows: &[Value]) -> Value {
    let metrics = crate::strategies::sim_query::summarize(rows);
    // Graduated-to-AMM count over **fired** rows only (NoEntry is matched-but-
    // never-entered — matching the live card's "migrated among entered" and the
    // sweep drill-in's `fired && is_migrated` filter).
    let migrated = rows
        .iter()
        .filter(|r| {
            let fired = r.get("fired").and_then(Value::as_bool).unwrap_or_else(|| {
                r.get("exit_reason").and_then(Value::as_str) != Some("NoEntry")
            });
            fired && r.get("is_migrated").and_then(Value::as_bool).unwrap_or(false)
        })
        .count();
    let mut body = serde_json::to_value(&metrics).unwrap_or_else(|_| json!({}));
    if let Some(obj) = body.as_object_mut() {
        obj.insert("computed_at".into(), json!(state.sim_results.computed_at(&rule_id)));
        obj.insert("n_migrated".into(), json!(migrated));
    }
    body
}

/// POST `.../rules/{rule_id}/simulate/result` — one page of the latest finished
/// backtest's per-token rows, sorted/filtered/searched server-side **in memory**
/// after a working-set hydrate, with the full match count on `X-Total-Count`.
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
/// the Simulated summary card.
pub fn sim_result_summary(state: &LocalState, rule_id: Uuid, req: TableRequest) -> HttpResponse {
    let rows = match peek_sim_rows(state, rule_id) {
        Ok(rows) => rows,
        Err(resp) => return resp,
    };
    let filtered = crate::strategies::sim_query::filter_rows(&rows, &req);
    HttpResponse::Ok().json(summary_json_from_rows(state, rule_id, &filtered))
}

/// POST `/api/strategies/simulate/summaries` — unfiltered rollups for many rules
/// in one round-trip (Simulate page hydrate). Reads **meta only** (no row load).
/// Rules with no durable `Done` result are omitted.
pub fn sim_result_summaries(state: &LocalState, rule_ids: &[Uuid]) -> HttpResponse {
    let mut summaries = Map::new();
    for &rule_id in rule_ids {
        if let Some(body) = state.sim_results.cached_summary_json(&rule_id) {
            summaries.insert(rule_id.to_string(), body);
        }
    }
    HttpResponse::Ok().json(json!({ "summaries": summaries }))
}

/// POST `.../rules/{rule_id}/simulate/result/time-summary` — hold-duration +
/// wall-clock bins over the filtered sim cohort (same filter grammar as
/// [`sim_result_summary`]). Query: `wall_field` = `entry_time`|`created_at`,
/// `wall_grain` = `auto`|`30m`|`1h`|`2h`|`4h`|`day`,
/// `hold_scheme` = `auto`|`dense_15s`|`dense_60s`|`mid_5m`|`mid_30m`|`wide_2h`|`wide_day`.
pub fn sim_result_time_summary(
    state: &LocalState,
    rule_id: Uuid,
    req: TableRequest,
    wall_field: &str,
    wall_grain: Option<&str>,
    hold_scheme: Option<&str>,
) -> HttpResponse {
    let rows = match peek_sim_rows(state, rule_id) {
        Ok(rows) => rows,
        Err(resp) => return resp,
    };
    let filtered = crate::strategies::sim_query::filter_rows(&rows, &req);
    let field = crate::strategies::sim_query::WallTimeField::parse(wall_field);
    let body =
        crate::strategies::sim_query::time_summary(&filtered, field, wall_grain, hold_scheme);
    HttpResponse::Ok().json(body)
}
