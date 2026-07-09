//! `GET /api/tokens` — the local token-list handler. Lives in `backend` (not core)
//! because it takes `LocalState` and computes swing chain stats; the
//! filter/sort/page/ETag body is the core `build_tokens_list`.

use std::collections::HashMap;
use std::sync::Arc;

use actix_web::{http::header, web, HttpRequest, HttpResponse, Responder};

use trading_core::api::table_query::TableRequest;

use crate::analyzers::swing_analyzer::{compute_chain_stats, ChainStats};
use crate::state::local_state::LocalState;

use super::{build_tokens_list, is_swing_sort_col, TokenQuery};

/// `POST /api/tokens` — list tokens over the unified [`TableRequest`] body.
pub async fn list_tokens(
    req: HttpRequest,
    state: web::Data<Arc<LocalState>>,
    body: web::Json<TableRequest>,
) -> impl Responder {
    // Filtering + sorting the token set is CPU work that would otherwise block one
    // of the few (http_workers=2) async request threads. Run it on the blocking
    // pool so a large cache can't stall other requests.
    let state = state.get_ref().clone();
    let body = body.into_inner();
    // Token list keeps its large envelope (pageSize up to 50k — Swing pulls the full
    // filtered set), so we don't use `Page::bounds` (clamps to 1000).
    let page = body.pagination.page.max(1);
    let page_size = body.pagination.page_size.clamp(1, 50_000);
    let (limit_q, offset_q) = (page_size, (page - 1) * page_size);
    let tracked_only = body.tracked_only;
    let q = TokenQuery::from_table_request(&body);

    let built = web::block(move || {
        // Swing-dependent branch — the only part of the list build that touches
        // `swing_runs`. Any chain column among the sort levels? Compute each mint's
        // chain stats from the raw legs stashed under the run, grouped at the
        // requested latency (a single stats map serves every chain level, each
        // reading a different field via `swing_sort_value`). Kept here, out of the
        // core `build_tokens_list`, so a deploy build can pass `None`.
        let swing_stats: Option<HashMap<String, ChainStats>> =
            if q.sort_levels().iter().any(|(col, _)| is_swing_sort_col(col)) {
                q.swing_run_id()
                    .and_then(|id| state.swing_runs.get(id))
                    .map(|run| {
                        let mut stats = HashMap::with_capacity(run.mints.len());
                        for entry in run.mints.iter() {
                            stats.insert(
                                entry.key().clone(),
                                compute_chain_stats(entry.value(), q.swing_chain_latency_ms()),
                            );
                        }
                        stats
                    })
            } else {
                None
            };

        build_tokens_list(&state, &q, limit_q, offset_q, tracked_only, swing_stats.as_ref())
    })
    .await;

    let (body, etag) = match built {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("list_tokens blocking build failed: {e}");
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({ "error": "failed to build token list" }));
        }
    };

    // Conditional GET: the browser's HTTP cache echoes our last `ETag` back in
    // `If-None-Match`. `Cache-Control: no-cache` makes it revalidate on every poll
    // (never serve a stale page without asking), so when the tag still matches we
    // answer 304 with no body and the browser hands the cached page to the app —
    // the wire carries only headers. A changed page falls through to a full 200.
    let if_none_match_hit = req
        .headers()
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .map(|hdr| hdr.split(',').any(|t| t.trim() == etag))
        .unwrap_or(false);
    if if_none_match_hit {
        return HttpResponse::NotModified()
            .insert_header((header::ETAG, etag))
            .insert_header((header::CACHE_CONTROL, "no-cache"))
            .finish();
    }

    HttpResponse::Ok()
        .insert_header((header::ETAG, etag))
        .insert_header((header::CACHE_CONTROL, "no-cache"))
        .content_type("application/json")
        .body(body)
}
