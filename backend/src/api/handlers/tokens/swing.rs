use actix_web::{web, HttpResponse, Responder};
use futures_util::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::sync::Arc;

use crate::{
    analyzers::swing_analyzer::{detect_swings, SwingLeg, SwingParams},
    models::trade::Trade,
    state::app_state::AppState,
    state::token_cache::MAX_TRADES_RETAINED,
};

/// Bound for the cache-miss DB fallback. Matches the in-memory retention window
/// so a high-volume mint can't pull its entire (unbounded, growing) trade
/// history into a single HTTP response — the same launch→recent window the live
/// cache would have served, in chronological order.
const SWING_DB_TRADE_CAP: i64 = MAX_TRADES_RETAINED as i64;

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
    /// Optional client-generated id for the "Swing Detection All" run this chunk
    /// belongs to. When set, the raw legs are stashed in `state.swing_runs` under
    /// this id so the tokens list can sort by the chain columns. The frontend
    /// sends one fresh id per run across all its chunks; omit for a one-off batch.
    #[serde(default)]
    pub run_id: Option<String>,
}

/// Restrict a token's trades to a window measured relative to its first trade.
/// The anchor is the earliest `block_time` among trades that carry SOL — i.e.
/// what detection treats as the opening trade. With both bounds unset (or no
/// usable anchor) the trades are returned untouched.
/// Borrows the slice and returns it untouched (`Cow::Borrowed`) when no window
/// is set, allocating a filtered `Vec` only when a window actually clips it — so
/// the common no-window swing scan reads the Arc-shared buffer in place with zero
/// copy.
fn filter_trades_to_window<'a>(
    trades: &'a [Trade],
    window_start_ms: Option<i64>,
    window_end_ms: Option<i64>,
) -> Cow<'a, [Trade]> {
    if window_start_ms.is_none() && window_end_ms.is_none() {
        return Cow::Borrowed(trades);
    }
    let anchor = trades
        .iter()
        .filter(|t| t.sol_amount > 0.0)
        .map(|t| t.block_time.timestamp_millis())
        .min();
    let Some(anchor) = anchor else {
        return Cow::Borrowed(trades);
    };
    let lo = window_start_ms.map(|s| anchor + s);
    let hi = window_end_ms.map(|e| anchor + e);
    Cow::Owned(
        trades
            .iter()
            .filter(|t| {
                let ts = t.block_time.timestamp_millis();
                lo.map_or(true, |lo| ts >= lo) && hi.map_or(true, |hi| ts <= hi)
            })
            .cloned()
            .collect(),
    )
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

    // Read full `Trade` rows from the DB (not the slim cache window): swing
    // analysis is a cold, bounded, paginated path, and the live `TokenCache` now
    // retains only a slimmed `CachedTrade` projection. Bounded read — never
    // materialise an unbounded mint history.
    let repo = state.trade_repo();
    let trades: Arc<Vec<Trade>> = match repo.find_by_mint_paged(&mint, SWING_DB_TRADE_CAP, 0).await {
        Ok(trades) => Arc::new(trades),
        Err(e) => {
            tracing::error!("DB error fetching trades for swing analysis {mint}: {e}");
            return HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "database error"
            }));
        }
    };

    // Run the (pure-CPU) window filter + swing scan off the HTTP worker, matching
    // the batch path — doing it inline would pin a worker for the whole scan.
    let resp_params = params.clone();
    let result = web::block(move || {
        // Borrow the shared buffer; the curve filter allocates only the kept
        // trades, and the window filter borrows when unset — no full-history copy.
        let curve_filtered: Option<Vec<Trade>> = curve_only
            .then(|| trades.iter().filter(|t| t.venue == "curve").cloned().collect());
        let base: &[Trade] = curve_filtered.as_deref().unwrap_or(&trades);
        let windowed = filter_trades_to_window(base, window_start_ms, window_end_ms);
        detect_swings(&windowed, &params)
    })
    .await;

    match result {
        Ok(swings) => HttpResponse::Ok().json(SwingResponse {
            mint,
            params: resp_params,
            count: swings.len(),
            swings,
        }),
        Err(e) => {
            tracing::error!("swing compute task panicked for {mint}: {e}");
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "swing computation failed"
            }))
        }
    }
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
        run_id,
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

    // Resolve (creating if needed) the run this chunk feeds, so the swing scan
    // below can stash its raw legs for the tokens-list chain sort. Concurrent
    // chunks of the same run share one `SwingRun` (atomic get-or-create).
    let run = run_id
        .as_deref()
        .map(|id| app.swing_runs.get_or_create(id));

    // 1) Resolve each mint's trades concurrently — a cache hit clones in-memory
    //    (no await), a miss hits the DB. The index is carried so the results can
    //    be realigned to the requested order after the unordered fan-out.
    let fetches = mints.into_iter().enumerate().map(|(idx, mint)| {
        let app = app.clone();
        async move {
            // DB-only (see the single-mint path): the cache holds a slim
            // `CachedTrade` projection, and batch swing analysis is a cold bounded
            // path, so read full `Trade`s from Postgres.
            let repo = app.trade_repo();
            let trades: Arc<Vec<Trade>> =
                match repo.find_by_mint_paged(&mint, SWING_DB_TRADE_CAP, 0).await {
                    Ok(trades) => Arc::new(trades),
                    Err(e) => {
                        tracing::error!(
                            "DB error fetching trades for batch swing analysis {mint}: {e}"
                        );
                        Arc::new(Vec::new())
                    }
                };
            (idx, mint, trades)
        }
    });
    let mut resolved: Vec<(usize, String, Arc<Vec<Trade>>)> = stream::iter(fetches)
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
                // Borrow the shared buffer; allocate only what a filter retains.
                let curve_filtered: Option<Vec<Trade>> = curve_only
                    .then(|| trades.iter().filter(|t| t.venue == "curve").cloned().collect());
                let base: &[Trade] = curve_filtered.as_deref().unwrap_or(&trades);
                let windowed = filter_trades_to_window(base, window_start_ms, window_end_ms);
                let swings = detect_swings(&windowed, &params);
                // Stash the raw legs for this run so the tokens list can sort by
                // (and re-group at any latency) the chain columns.
                if let Some(run) = &run {
                    run.mints.insert(mint.clone(), swings.clone());
                }
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
