use actix_web::{web, HttpResponse, Responder};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{
    analyzers::swing_analyzer::{detect_swings, SwingLeg, SwingParams},
    models::trade::Trade,
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
    /// Optional detection window, in milliseconds relative to each token's first
    /// trade. A `null`/omitted bound leaves that side open. Trades outside the
    /// window are dropped before detection runs.
    #[serde(default)]
    pub window_start_ms: Option<i64>,
    #[serde(default)]
    pub window_end_ms: Option<i64>,
    /// When true, restrict detection to bonding-curve trades (`venue == "curve"`),
    /// i.e. the token-creation → migration phase. Applied before the window.
    #[serde(default)]
    pub curve_only: bool,
}

/// Restrict a token's trades to a window measured relative to its first trade.
/// The anchor is the earliest `block_time` among trades that carry SOL — i.e.
/// what detection treats as the opening trade. With both bounds unset (or no
/// usable anchor) the trades are returned untouched.
fn filter_trades_to_window(
    trades: Vec<Trade>,
    window_start_ms: Option<i64>,
    window_end_ms: Option<i64>,
) -> Vec<Trade> {
    if window_start_ms.is_none() && window_end_ms.is_none() {
        return trades;
    }
    let anchor = trades
        .iter()
        .filter(|t| t.sol_amount > 0.0)
        .map(|t| t.block_time.timestamp_millis())
        .min();
    let Some(anchor) = anchor else {
        return trades;
    };
    let lo = window_start_ms.map(|s| anchor + s);
    let hi = window_end_ms.map(|e| anchor + e);
    trades
        .into_iter()
        .filter(|t| {
            let ts = t.block_time.timestamp_millis();
            lo.map_or(true, |lo| ts >= lo) && hi.map_or(true, |hi| ts <= hi)
        })
        .collect()
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
    let SwingBatchRequest {
        mints,
        params,
        window_start_ms,
        window_end_ms,
        curve_only,
    } = body.into_inner();
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

        let trades = if curve_only {
            trades.into_iter().filter(|t| t.venue == "curve").collect()
        } else {
            trades
        };
        let trades = filter_trades_to_window(trades, window_start_ms, window_end_ms);
        let swings = detect_swings(&trades, &params);
        results.push(SwingBatchEntry {
            mint,
            count: swings.len(),
            swings,
        });
    }

    HttpResponse::Ok().json(SwingBatchResponse { params, results })
}
