use actix_web::{web, HttpResponse, Responder};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{
    analyzers::swing_analyzer::{detect_swings, SwingLeg, SwingParams},
    state::app_state::AppState,
    storage::repositories::trade_repo::TradeRepo,
};

#[derive(Serialize)]
pub struct SwingResponse {
    pub mint: String,
    pub params: SwingParams,
    pub count: usize,
    pub swings: Vec<SwingLeg>,
}

/// Request body for batched, multi-token swing detection.
#[derive(Deserialize)]
pub struct SwingBatchRequest {
    /// Mints to run detection over, in the order results should be returned.
    pub mints: Vec<String>,
    /// Shared tunable parameters; omit to use defaults.
    #[serde(default)]
    pub params: SwingParams,
}

/// One token's swing ledger inside a batch response.
#[derive(Serialize)]
pub struct SwingBatchEntry {
    pub mint: String,
    pub count: usize,
    pub swings: Vec<SwingLeg>,
}

#[derive(Serialize)]
pub struct SwingBatchResponse {
    pub params: SwingParams,
    pub results: Vec<SwingBatchEntry>,
}

/// `POST /api/tokens/:mint/swings` — run swing detection over a token's trade
/// history (cache first, else DB). The (optional) JSON body carries tunable
/// `SwingParams`; send `{}` to use defaults.
pub async fn detect_token_swings(
    state: web::Data<Arc<AppState>>,
    path: web::Path<String>,
    body: web::Json<SwingParams>,
) -> impl Responder {
    let mint = path.into_inner();
    let params = body.into_inner();

    let trades = if let Some(entry) = state.token_cache.get(&mint) {
        entry.trades.clone()
    } else {
        let repo = TradeRepo::new(state.db.clone());
        match repo.find_by_mint_all(&mint).await {
            Ok(trades) => trades,
            Err(e) => {
                tracing::error!("DB error fetching trades for swing analysis {mint}: {e}");
                return HttpResponse::InternalServerError().json(serde_json::json!({
                    "error": "database error"
                }));
            }
        }
    };

    let swings = detect_swings(&trades, &params);
    HttpResponse::Ok().json(SwingResponse {
        mint,
        params,
        count: swings.len(),
        swings,
    })
}

/// `POST /api/tokens/swings/batch` — run swing detection over many tokens in a
/// single request, using one shared set of `SwingParams`. Trades come from the
/// in-memory cache when present, else from the DB (same as the single-token
/// endpoint). Mints whose trades fail to load are returned with an empty ledger
/// so the result aligns one-to-one with the requested mints.
pub async fn detect_tokens_swings_batch(
    state: web::Data<Arc<AppState>>,
    body: web::Json<SwingBatchRequest>,
) -> impl Responder {
    let SwingBatchRequest { mints, params } = body.into_inner();
    let repo = TradeRepo::new(state.db.clone());

    let mut results = Vec::with_capacity(mints.len());
    for mint in mints {
        let trades = if let Some(entry) = state.token_cache.get(&mint) {
            entry.trades.clone()
        } else {
            match repo.find_by_mint_all(&mint).await {
                Ok(trades) => trades,
                Err(e) => {
                    tracing::error!("DB error fetching trades for batch swing analysis {mint}: {e}");
                    Vec::new()
                }
            }
        };

        let swings = detect_swings(&trades, &params);
        results.push(SwingBatchEntry {
            mint,
            count: swings.len(),
            swings,
        });
    }

    HttpResponse::Ok().json(SwingBatchResponse { params, results })
}
