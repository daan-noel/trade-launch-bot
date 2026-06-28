//! `GET /api/tokens` — token list for the live (deploy) bin.
//! Calls the core `build_tokens_list` with no swing-chain stats (deploy has no
//! swing runs). Filter/sort/page/ETag logic matches the lab handler exactly.

use std::sync::Arc;

use actix_web::{http::header, web, HttpRequest, HttpResponse, Responder};

use trading_core::{
    api::handlers::tokens::{build_tokens_list, PaginationParams, TokenQuery},
    state::core_state::CoreState,
};

/// `GET /api/tokens` — list all currently tracked tokens.
/// Swing-chain sort columns are accepted but sort matching rows last on the live
/// bin (no swing runs available), which is the correct no-data fallback.
pub async fn list_tokens(
    req: HttpRequest,
    state: web::Data<Arc<CoreState>>,
    query: web::Query<PaginationParams>,
) -> impl Responder {
    let state = state.get_ref().clone();
    let q = TokenQuery::from_params(&query);
    let limit_q = query.limit;
    let offset_q = query.offset;
    let tracked_only = query.tracked_only.unwrap_or(false);

    let built = web::block(move || {
        build_tokens_list(&state, &q, limit_q, offset_q, tracked_only, None)
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
