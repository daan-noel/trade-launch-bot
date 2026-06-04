use actix_web::{web, HttpResponse, Responder};
use serde::Serialize;
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
