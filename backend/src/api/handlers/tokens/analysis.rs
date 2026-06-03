use actix_web::{web, HttpResponse, Responder};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::sync::Arc;

use crate::{state::app_state::AppState, storage::repositories::analysis_repo::AnalysisRepo};

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct AnalysisResultResponse {
    pub analyzer_name: String,
    pub score: f64,
    pub indicators: Vec<String>,
    pub computed_at: DateTime<Utc>,
}

#[derive(Serialize)]
pub struct CreatorResponse {
    pub wallet_address: String,
    pub persisted_profile: Option<PersistedCreatorProfile>,
}

#[derive(Serialize)]
pub struct PersistedCreatorProfile {
    pub tokens_created: i32,
    pub total_volume_sol: f64,
    pub suspiciousness_score: f64,
    pub wash_trade_score: f64,
    pub last_analyzed_at: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GET /api/tokens/:mint/analysis` — all analysis results for a token.
pub async fn get_token_analysis(
    state: web::Data<Arc<AppState>>,
    path: web::Path<String>,
) -> impl Responder {
    let mint = path.into_inner();
    let repo = AnalysisRepo::new(state.db.clone());

    match repo.find_by_mint(&mint).await {
        Ok(results) => {
            let response: Vec<AnalysisResultResponse> = results
                .into_iter()
                .map(|r| AnalysisResultResponse {
                    analyzer_name: r.analyzer_name,
                    score: r.score,
                    indicators: r.indicators,
                    computed_at: r.computed_at,
                })
                .collect();
            HttpResponse::Ok().json(response)
        }
        Err(e) => {
            tracing::error!("DB error fetching analysis for {mint}: {e}");
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "database error"
            }))
        }
    }
}

/// `GET /api/creators/:wallet` — persisted creator profile from DB.
pub async fn get_creator(
    state: web::Data<Arc<AppState>>,
    path: web::Path<String>,
) -> impl Responder {
    let wallet = path.into_inner();
    let repo = AnalysisRepo::new(state.db.clone());

    match repo.find_creator_profile(&wallet).await {
        Ok(Some(cp)) => HttpResponse::Ok().json(CreatorResponse {
            wallet_address: wallet,
            persisted_profile: Some(PersistedCreatorProfile {
                tokens_created: cp.tokens_created,
                total_volume_sol: cp.total_volume_sol,
                suspiciousness_score: cp.suspiciousness_score,
                wash_trade_score: cp.wash_trade_score,
                last_analyzed_at: cp.last_analyzed_at,
            }),
        }),
        Ok(None) => HttpResponse::NotFound().json(serde_json::json!({
            "error": "creator not found",
            "wallet": wallet
        })),
        Err(e) => {
            tracing::error!("DB error fetching creator profile for {wallet}: {e}");
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "database error"
            }))
        }
    }
}

// ---------------------------------------------------------------------------
// List endpoints (paginated)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct AnalysisListResponse {
    pub total: i64,
    pub items: Vec<AnalysisResultResponse>,
}

#[derive(Serialize)]
pub struct CreatorSummary {
    pub wallet_address: String,
    pub tokens_created: i32,
    pub total_volume_sol: f64,
    pub suspiciousness_score: f64,
    pub wash_trade_score: f64,
    pub last_analyzed_at: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
pub struct CreatorListResponse {
    pub total: i64,
    pub items: Vec<CreatorSummary>,
}

#[derive(serde::Deserialize)]
pub struct ListParams {
    #[serde(default = "default_list_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_list_limit() -> i64 {
    50
}

/// `GET /api/analysis?limit=&offset=` — all analysis results, newest first.
pub async fn list_analysis_results(
    state: web::Data<Arc<AppState>>,
    query: web::Query<ListParams>,
) -> impl Responder {
    let repo = AnalysisRepo::new(state.db.clone());
    let limit = query.limit.max(1).min(500);
    let offset = query.offset.max(0);

    match repo.list_results(limit, offset).await {
        Ok((total, results)) => {
            let items = results
                .into_iter()
                .map(|r| AnalysisResultResponse {
                    analyzer_name: r.analyzer_name,
                    score: r.score,
                    indicators: r.indicators,
                    computed_at: r.computed_at,
                })
                .collect();
            HttpResponse::Ok().json(AnalysisListResponse { total, items })
        }
        Err(e) => {
            tracing::error!("DB error listing analysis results: {e}");
            HttpResponse::InternalServerError().json(serde_json::json!({"error": "database error"}))
        }
    }
}

/// `GET /api/creators?limit=&offset=` — all creator profiles, most suspicious first.
pub async fn list_creators(
    state: web::Data<Arc<AppState>>,
    query: web::Query<ListParams>,
) -> impl Responder {
    let repo = AnalysisRepo::new(state.db.clone());
    let limit = query.limit.max(1).min(500);
    let offset = query.offset.max(0);

    match repo.list_creator_profiles(limit, offset).await {
        Ok((total, profiles)) => {
            let items = profiles
                .into_iter()
                .map(|p| CreatorSummary {
                    wallet_address: p.wallet_address,
                    tokens_created: p.tokens_created,
                    total_volume_sol: p.total_volume_sol,
                    suspiciousness_score: p.suspiciousness_score,
                    wash_trade_score: p.wash_trade_score,
                    last_analyzed_at: p.last_analyzed_at,
                })
                .collect();
            HttpResponse::Ok().json(CreatorListResponse { total, items })
        }
        Err(e) => {
            tracing::error!("DB error listing creator profiles: {e}");
            HttpResponse::InternalServerError().json(serde_json::json!({"error": "database error"}))
        }
    }
}
