use actix_web::{web, HttpResponse, Responder};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::sync::Arc;

use crate::{
    models::trade::Trade, state::app_state::AppState,
    storage::repositories::analysis_repo::AnalysisRepo,
};

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
    /// Tokens created this session (from in-memory cache)
    pub live_created_tokens: Vec<String>,
    /// Number of trades in the in-memory sliding window
    pub live_trade_window_size: usize,
    /// Current suspiciousness score from cache
    pub live_suspiciousness_score: f64,
    /// Trades in the creator's current sliding window (most recent N)
    pub recent_trades: Vec<Trade>,
    /// Persisted creator profile, if available
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

/// `GET /api/creators/:wallet` — live creator state from cache + persisted profile.
pub async fn get_creator(
    state: web::Data<Arc<AppState>>,
    path: web::Path<String>,
) -> impl Responder {
    let wallet = path.into_inner();

    // Always check DB for a persisted profile
    let repo = AnalysisRepo::new(state.db.clone());
    let persisted_profile = match repo.find_creator_profile(&wallet).await {
        Ok(p) => p.map(|cp| PersistedCreatorProfile {
            tokens_created: cp.tokens_created,
            total_volume_sol: cp.total_volume_sol,
            suspiciousness_score: cp.suspiciousness_score,
            wash_trade_score: cp.wash_trade_score,
            last_analyzed_at: cp.last_analyzed_at,
        }),
        Err(e) => {
            tracing::error!("DB error fetching creator profile for {wallet}: {e}");
            return HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "database error"
            }));
        }
    };

    // Enrich with live data from cache if the creator is tracked this session
    if let Some(creator_state) = state.creator_cache.get(&wallet) {
        let recent_trades: Vec<Trade> = creator_state
            .trade_history
            .iter()
            .rev()
            .take(20)
            .cloned()
            .collect();

        return HttpResponse::Ok().json(CreatorResponse {
            wallet_address: wallet.clone(),
            live_created_tokens: creator_state.created_tokens.clone(),
            live_trade_window_size: creator_state.trade_history.len(),
            live_suspiciousness_score: creator_state.suspiciousness_score,
            recent_trades,
            persisted_profile,
        });
    }

    // Creator not in cache — return whatever the DB has
    if persisted_profile.is_some() {
        return HttpResponse::Ok().json(CreatorResponse {
            wallet_address: wallet,
            live_created_tokens: vec![],
            live_trade_window_size: 0,
            live_suspiciousness_score: 0.0,
            recent_trades: vec![],
            persisted_profile,
        });
    }

    HttpResponse::NotFound().json(serde_json::json!({
        "error": "creator not found",
        "wallet": wallet
    }))
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
