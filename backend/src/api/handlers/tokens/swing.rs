use actix_web::{web, HttpResponse, Responder};
use futures_util::stream::{self, StreamExt};
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

/// Request body for single-token swing detection. Mirrors the batch request's
/// trade-filtering options so the single-token panel can scope detection the
/// same way (time window + bonding-curve-only), independently of any batch run.
#[derive(Deserialize)]
pub struct SwingDetectRequest {
    /// Tunable parameters; omit to use defaults.
    #[serde(default)]
    pub params: SwingParams,
    /// Optional detection window, in milliseconds relative to the token's first
    /// trade. A `null`/omitted bound leaves that side open.
    #[serde(default)]
    pub window_start_ms: Option<i64>,
    #[serde(default)]
    pub window_end_ms: Option<i64>,
    /// When true, restrict detection to bonding-curve trades (`venue == "curve"`),
    /// i.e. the token-creation → migration phase. Applied before the window.
    #[serde(default)]
    pub curve_only: bool,
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
/// `params` plus the same `window_start_ms` / `window_end_ms` / `curve_only`
/// trade filters as the batch endpoint; send `{}` to use defaults over full
/// history.
pub async fn detect_token_swings(
    state: web::Data<Arc<AppState>>,
    path: web::Path<String>,
    body: web::Json<SwingDetectRequest>,
) -> impl Responder {
    let mint = path.into_inner();
    let SwingDetectRequest {
        params,
        window_start_ms,
        window_end_ms,
        curve_only,
    } = body.into_inner();

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

    let trades = if curve_only {
        trades.into_iter().filter(|t| t.venue == "curve").collect()
    } else {
        trades
    };
    let trades = filter_trades_to_window(trades, window_start_ms, window_end_ms);

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

    // Cap the request size: each mint can trigger a DB load + swing scan, so an
    // unbounded list could monopolize a connection-pool slice and the blocking
    // pool at once.
    const MAX_BATCH_MINTS: usize = 200;
    if mints.len() > MAX_BATCH_MINTS {
        return HttpResponse::BadRequest().json(serde_json::json!({
            "error": format!("too many mints: {} (max {MAX_BATCH_MINTS})", mints.len()),
        }));
    }

    // Max cache-miss DB loads in flight at once. Bounds the connection-pool
    // pressure a single batch can create while still overlapping the round-trips
    // instead of awaiting them one at a time.
    const BATCH_FETCH_CONCURRENCY: usize = 16;

    let app = state.get_ref().clone();

    // 1) Resolve each mint's trades concurrently — a cache hit clones in-memory
    //    (no await), a miss hits the DB. The index is carried so the results can
    //    be realigned to the requested order after the unordered fan-out.
    let fetches = mints.into_iter().enumerate().map(|(idx, mint)| {
        let app = app.clone();
        async move {
            let trades = if let Some(entry) = app.token_cache.get(&mint) {
                entry.trades.clone()
            } else {
                let repo = TradeRepo::new(app.db.clone());
                match repo.find_by_mint_all(&mint).await {
                    Ok(trades) => trades,
                    Err(e) => {
                        tracing::error!(
                            "DB error fetching trades for batch swing analysis {mint}: {e}"
                        );
                        Vec::new()
                    }
                }
            };
            (idx, mint, trades)
        }
    });
    let mut resolved: Vec<(usize, String, Vec<Trade>)> = stream::iter(fetches)
        .buffer_unordered(BATCH_FETCH_CONCURRENCY)
        .collect()
        .await;

    // 2) Run the (pure-CPU) window filter + swing scans off the HTTP worker.
    //    Doing them inline would pin one of the few workers for the whole batch.
    let resp_params = params.clone();
    let results = web::block(move || {
        resolved.sort_by_key(|(idx, _, _)| *idx);
        resolved
            .into_iter()
            .map(|(_, mint, trades)| {
                let trades = if curve_only {
                    trades.into_iter().filter(|t| t.venue == "curve").collect()
                } else {
                    trades
                };
                let trades = filter_trades_to_window(trades, window_start_ms, window_end_ms);
                let swings = detect_swings(&trades, &params);
                SwingBatchEntry {
                    mint,
                    count: swings.len(),
                    swings,
                }
            })
            .collect::<Vec<_>>()
    })
    .await;

    match results {
        Ok(results) => HttpResponse::Ok().json(SwingBatchResponse {
            params: resp_params,
            results,
        }),
        Err(e) => {
            tracing::error!("swing batch compute task panicked: {e}");
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "swing computation failed"
            }))
        }
    }
}
