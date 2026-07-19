//! `GET /api/tokens` — the local token-list handler. Lives in `backend` (not core)
//! because it takes `LocalState`; the filter/sort/page/ETag body is the core
//! `build_tokens_list`.

use std::sync::Arc;

use actix_web::{http::header, web, HttpRequest, HttpResponse, Responder};

use trading_core::api::table_query::TableRequest;

use crate::state::local_state::LocalState;

use super::{build_tokens_list, collect_filtered_mints, TokenQuery};

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
    // Token list keeps its large envelope (pageSize up to 50k pulls the full
    // filtered set), so we don't use `Page::bounds` (clamps to 1000).
    let page = body.pagination.page.max(1);
    let page_size = body.pagination.page_size.clamp(1, 50_000);
    let (limit_q, offset_q) = (page_size, (page - 1) * page_size);
    let tracked_only = body.tracked_only;
    let q = TokenQuery::from_table_request(&body);

    let built = web::block(move || {
        build_tokens_list(&state, &q, limit_q, offset_q, tracked_only)
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

/// `POST /api/tokens/mints` — the matched `mint_address` set for a
/// [`TableRequest`] filter, and nothing else. "Swing Detection All" fans out over
/// every filtered token; returning only the mints (not full rows) keeps that
/// "run over everything" fetch tiny even at the 50k list ceiling, instead of
/// serializing/transferring the whole page envelope just to read its mints. No
/// swing-sort branch — order is irrelevant to the caller.
pub async fn list_token_mints(
    state: web::Data<Arc<LocalState>>,
    body: web::Json<TableRequest>,
) -> impl Responder {
    let state = state.get_ref().clone();
    let body = body.into_inner();
    let tracked_only = body.tracked_only;
    let q = TokenQuery::from_table_request(&body);

    // Same blocking-pool discipline as `list_tokens`: the filter reduction over a
    // large snapshot is CPU work that mustn't stall the few async request threads.
    let mints = web::block(move || collect_filtered_mints(&state, &q, tracked_only)).await;

    match mints {
        Ok(mints) => HttpResponse::Ok().json(serde_json::json!({ "mints": mints })),
        Err(e) => {
            tracing::error!("list_token_mints blocking build failed: {e}");
            HttpResponse::InternalServerError()
                .json(serde_json::json!({ "error": "failed to build token mints" }))
        }
    }
}
