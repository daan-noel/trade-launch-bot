//! `POST /api/tokens` + `POST /api/tokens/mints` — the token list, shared by both
//! bins (`docs/plans/frontend/token-list-backend.md`):
//!   - **Full list** (`tracked_only=false`, the default): filtered, sorted and paged
//!     **in Postgres** — `build_where_and_order` → `count_list` + `find_list_page`
//!     — so one request reads one page, never the whole universe.
//!   - **Tracked-only** (`tracked_only=true`, the "tracked" badge): served from the
//!     small in-RAM cache, no DB round-trip.
//!
//! `tracked` (the "tracked vs all" count) is always the in-RAM cache subset that
//! passes the same filter, so both paths report it consistently.

use std::sync::Arc;

use actix_web::{http::header, web, HttpRequest, HttpResponse, Responder};

use crate::api::table_query::TableRequest;
use crate::state::core_state::CoreState;

use super::{
    build_where_and_order, serialize_with_etag, tracked_count, tracked_mints,
    tracked_tokens_page, TokenQuery, TokenSummary, TokensListResponse,
};

/// Largest page one request may ask for. The Swing page pulls the whole filtered
/// set in one page, so this stays above `Page::bounds`'s 1000 clamp.
const MAX_PAGE_SIZE: i64 = 50_000;

/// `POST /api/tokens` — one page of the token list over the unified
/// [`TableRequest`] body.
pub async fn list_tokens(
    req: HttpRequest,
    state: web::Data<Arc<CoreState>>,
    body: web::Json<TableRequest>,
) -> impl Responder {
    let state = state.get_ref().clone();
    let body = body.into_inner();
    let (limit, offset) = page_bounds(&body);
    let q = TokenQuery::from_table_request(&body);

    let built = if body.tracked_only {
        web::block(move || serialize_with_etag(&tracked_tokens_page(&state, &q, limit as usize, offset as usize)))
            .await
            .map_err(|e| e.to_string())
    } else {
        full_list_page(state, q, limit, offset).await
    };
    respond(req, built)
}

/// `POST /api/tokens/mints` — the matched `mint_address` set for a
/// [`TableRequest`] filter, and nothing else. "Swing Detection All" fans out over
/// every filtered token; returning only the mints keeps that fetch tiny instead of
/// serializing whole rows. Order is irrelevant to the caller.
pub async fn list_token_mints(
    state: web::Data<Arc<CoreState>>,
    body: web::Json<TableRequest>,
) -> impl Responder {
    let state = state.get_ref().clone();
    let body = body.into_inner();
    let q = TokenQuery::from_table_request(&body);

    let mints = if body.tracked_only {
        web::block(move || tracked_mints(&state, &q)).await.map_err(|e| e.to_string())
    } else {
        let built = build_where_and_order(&q, chrono::Utc::now());
        state
            .token_repo()
            .find_list_mints(&built.where_sql, &built.args)
            .await
            .map_err(|e| e.to_string())
    };

    match mints {
        Ok(mints) => HttpResponse::Ok().json(serde_json::json!({ "mints": mints })),
        Err(e) => {
            tracing::error!("list_token_mints failed: {e}");
            HttpResponse::InternalServerError()
                .json(serde_json::json!({ "error": "failed to build token mints" }))
        }
    }
}

/// `(limit, offset)` from the request's 1-based page/pageSize.
fn page_bounds(req: &TableRequest) -> (i64, i64) {
    let page = req.pagination.page.max(1);
    let page_size = req.pagination.page_size.clamp(1, MAX_PAGE_SIZE);
    (page_size, (page - 1) * page_size)
}

/// One page of the full list from Postgres plus the in-RAM `tracked` count,
/// serialized + fingerprinted. The `COUNT` and the page query run concurrently,
/// so the response waits for the slower of the two, not their sum.
async fn full_list_page(
    state: Arc<CoreState>,
    q: TokenQuery,
    limit: i64,
    offset: i64,
) -> Result<(Vec<u8>, String), String> {
    let built = build_where_and_order(&q, chrono::Utc::now());
    let repo = state.token_repo();
    let (total, rows) = tokio::try_join!(
        repo.count_list(&built.where_sql, &built.args),
        repo.find_list_page(&built.where_sql, &built.order_sql, &built.args, limit, offset),
    )
    .map_err(|e| format!("token list query: {e}"))?;
    let items: Vec<TokenSummary> = rows.into_iter().map(TokenSummary::from).collect();

    web::block(move || {
        let tracked = tracked_count(&state, &q);
        serialize_with_etag(&TokensListResponse { total: total as usize, tracked, items })
    })
    .await
    .map_err(|e| format!("tracked count: {e}"))
}

/// ETag / If-None-Match → 304, else a full 200. `Cache-Control: no-cache` makes
/// the browser revalidate on every poll, so an unchanged page costs headers only.
fn respond(req: HttpRequest, built: Result<(Vec<u8>, String), String>) -> HttpResponse {
    let (body, etag) = match built {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("list_tokens failed: {e}");
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({ "error": "failed to build token list" }));
        }
    };

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
