//! `GET /api/tokens` — token list for the live (deploy) bin.
//!
//! Two paths, matching the live/lab cache split (see `tokens-db-pagination-plan.md`):
//!   - **Full list** (`tracked_only=false`, the default): paged **from Postgres**.
//!     The live in-RAM cache holds only tracking tokens, so the full token universe
//!     (100K+) is filtered/sorted/paged in SQL via `build_where_and_order` +
//!     `find_list_page`/`count_list` — no RAM ceiling, no 25K cap.
//!   - **Tracked-only** (`tracked_only=true`, the "tracked" badge): served from the
//!     small in-RAM cache via the core `build_tokens_list`, which is cheap and needs
//!     no DB round-trip.
//!
//! `tracked` (the "tracked vs all" count) is always the in-RAM cache subset that
//! passes the same filter, so both paths report it consistently.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use actix_web::{http::header, web, HttpRequest, HttpResponse, Responder};

use trading_core::{
    api::handlers::tokens::{
        build_tokens_list, build_where_and_order, PaginationParams, TokenQuery, TokenSummary,
        TokensListResponse,
    },
    state::core_state::CoreState,
};

/// `GET /api/tokens` — list tokens.
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

    // Tracked-only view: small resident set, served from the in-RAM cache exactly
    // as before (no SQL). Keeps the "tracked" badge click instant.
    if tracked_only {
        let state2 = state.clone();
        let built = web::block(move || {
            build_tokens_list(&state2, &q, limit_q, offset_q, true, None)
        })
        .await;
        return respond(req, built.map_err(|e| e.to_string()));
    }

    // Full list: page the whole universe from Postgres.
    let built = build_full_list_sql(&state, &q, limit_q, offset_q).await;
    respond(req, built)
}

/// Build one page of the full token list from Postgres, plus the in-RAM `tracked`
/// count, and serialize+fingerprint it into `(body, etag)` — the same response
/// shape and ETag scheme as the in-RAM `build_tokens_list`.
async fn build_full_list_sql(
    state: &Arc<CoreState>,
    q: &TokenQuery,
    limit_q: i64,
    offset_q: i64,
) -> Result<(Vec<u8>, String), String> {
    let now = chrono::Utc::now();
    let built = build_where_and_order(q, now);
    let limit = limit_q.max(1).min(50_000);
    let offset = offset_q.max(0);

    let repo = state.token_repo();
    let total = repo
        .count_list(&built.where_sql, &built.args)
        .await
        .map_err(|e| format!("count_list: {e}"))? as usize;
    let rows = repo
        .find_list_page(&built.where_sql, &built.order_sql, &built.args, limit, offset)
        .await
        .map_err(|e| format!("find_list_page: {e}"))?;
    let items: Vec<TokenSummary> = rows.into_iter().map(TokenSummary::from).collect();

    // `tracked` = the in-RAM (cache-tracked) subset passing the same filter. The
    // resident set is small, so this stays a cheap in-memory scan off the hot loop.
    let state2 = state.clone();
    let q2 = q.clone_for_tracked();
    let tracked = web::block(move || {
        let snapshot = state2.token_list.get(&state2.token_cache, now);
        snapshot.tracked_filtered_count(|t| q2.matches_public(t, now))
    })
    .await
    .map_err(|e| format!("tracked count: {e}"))?;

    let resp = TokensListResponse { total, tracked, items };
    let body = serde_json::to_vec(&resp).unwrap_or_default();
    let mut hasher = DefaultHasher::new();
    body.hash(&mut hasher);
    let etag = format!("\"{:016x}\"", hasher.finish());
    Ok((body, etag))
}

/// Shared response finalization: ETag / If-None-Match → 304 or 200.
fn respond(req: HttpRequest, built: Result<(Vec<u8>, String), String>) -> HttpResponse {
    let (body, etag) = match built {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("list_tokens build failed: {e}");
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
