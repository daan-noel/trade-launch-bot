use actix_web::{web, HttpResponse};
use serde::Deserialize;
use std::sync::Arc;

use crate::state::app_state::AppState;

use super::TokenSummary;

#[derive(Deserialize)]
pub struct TokenBatchQuery {
    pub mints: String,
}

/// `GET /api/tokens/batch?mints=a,b,...` — return full token records for an
/// explicit set of mints (comma-separated, capped at 500, duplicates removed).
/// Reuses `TokenSummary` (same shape as the `/api/tokens` list) so the frontend
/// can use the same `TokenRecord` type it already knows. Used by strategy pages
/// to enrich Matched / Simulated / Positions tables without touching the strategy
/// response structs.
pub async fn get_tokens_batch(
    state: web::Data<Arc<AppState>>,
    query: web::Query<TokenBatchQuery>,
) -> HttpResponse {
    let mints: Vec<String> = {
        let mut seen = std::collections::HashSet::new();
        query
            .mints
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && seen.insert(s.clone()))
            .take(500)
            .collect()
    };

    if mints.is_empty() {
        return HttpResponse::Ok().json(Vec::<TokenSummary>::new());
    }

    match state.token_repo().find_list_rows_for_mints(&mints).await {
        Ok(rows) => {
            let items: Vec<TokenSummary> = rows.into_iter().map(TokenSummary::from).collect();
            HttpResponse::Ok().json(items)
        }
        Err(e) => {
            tracing::error!("GET /api/tokens/batch failed: {e}");
            HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "batch query failed"}))
        }
    }
}
