use actix_web::{web, HttpResponse, Responder};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use trading_core::models::trade::TradeRow;

use crate::{
    analyzers::swing_analyzer::{detect_swings, SwingLeg, SwingParams},
    models::ingest::SseEvent,
    state::local_state::LocalState,
    state::job_progress::ProgressCell,
    state::swing_results::SwingOutcome,
    strategies::sim_fetch::{fetch_sim_histories, fetch_sim_history_one},
    sweep::projection::CorpusTrade,
};

#[derive(Serialize)]
pub struct SwingResponse {
    pub mint_address: String,
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
/// copy. Generic over [`TradeRow`] so it serves both the live `Trade` and the
/// lake's `CorpusTrade` off the same accessors.
pub(crate) fn filter_trades_to_window<T: TradeRow + Clone>(
    trades: &[T],
    window_start_ms: Option<i64>,
    window_end_ms: Option<i64>,
) -> Cow<'_, [T]> {
    if window_start_ms.is_none() && window_end_ms.is_none() {
        return Cow::Borrowed(trades);
    }
    let anchor = trades
        .iter()
        .filter(|t| t.amount_sol() > 0.0)
        .map(|t| t.block_time().timestamp_millis())
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
                let ts = t.block_time().timestamp_millis();
                lo.map_or(true, |lo| ts >= lo) && hi.map_or(true, |hi| ts <= hi)
            })
            .cloned()
            .collect(),
    )
}

/// One token's swing ledger inside a batch response.
#[derive(Serialize)]
pub struct SwingBatchEntry {
    pub mint_address: String,
    pub count: usize,
    pub swings: Vec<SwingLeg>,
}

#[derive(Serialize)]
pub struct SwingBatchResponse {
    pub params: SwingParams,
    pub results: Vec<SwingBatchEntry>,
}

/// `POST /api/tokens/:mint/swings` — run swing detection over a token's full,
/// uncapped trade history read from the Parquet lake (the same corpus + fetch
/// path `swing1-detect` and the single-rule backtests use). The (optional) JSON
/// body carries tunable `params` plus the same `window_start_ms` /
/// `window_end_ms` / `curve_only` trade filters as the batch endpoint; send `{}`
/// to use defaults over full history.
pub async fn detect_token_swings(
    _state: web::Data<Arc<LocalState>>,
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

    // Uncapped, full-history read from the lake — `curve_only` is applied at load
    // (the projected `CorpusTrade` has no `venue`).
    let trades: Arc<Vec<CorpusTrade>> = match fetch_sim_history_one(&mint, curve_only).await {
        Ok(trades) => trades,
        Err(e) => {
            tracing::error!("lake trade fetch failed for swing analysis {mint}: {e}");
            return HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "lake trade fetch failed"
            }));
        }
    };

    // Run the (pure-CPU) window filter + swing scan off the HTTP worker, matching
    // the batch path — doing it inline would pin a worker for the whole scan.
    let resp_params = params.clone();
    let result = web::block(move || {
        let windowed = filter_trades_to_window(&trades, window_start_ms, window_end_ms);
        detect_swings(&windowed, &params)
    })
    .await;

    match result {
        Ok(swings) => HttpResponse::Ok().json(SwingResponse {
            mint_address: mint,
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

/// Sanity ceiling on a single run's mint count. The run is uncapped by design
/// (the whole filtered set can be thousands of tokens), but a wildly out-of-range
/// list — a client bug, a hostile caller — would still pin the blocking pool for
/// the whole scan, so reject the absurd while leaving every real run (≤ the
/// tokens-list ceiling) through.
const MAX_RUN_MINTS: usize = 100_000;

/// `POST /api/tokens/swings/batch` — start a "Swing Detection All" run as a
/// detached background job and return at once (`202 {"started": true}`).
///
/// The run scans the whole filtered token set (thousands of uncapped histories)
/// and can take minutes. The old design split the set into ≤200-mint chunks and
/// held one HTTP connection open per chunk until its scan finished; any mid-run
/// drop — dev proxy / browser idle cut / the ingest watchdog restarting the
/// process under load — severed that socket and surfaced on the client as a
/// `FETCH_ERROR`, even though the work was finishing fine. Instead the whole run
/// is one detached job: it fans the DB loads + CPU scans out internally, stores
/// its terminal outcome in [`LocalState::swing_results`], and the client collects it
/// via `GET /api/jobs/swings/{run_id}/result` once the `swing_detection_finished`
/// SSE fires — no long-held connection to cut.
///
/// `run_id` is now required (it keys the cancel flag, progress cell, result store,
/// and `swing_runs` legs cache). The detached scan still populates `swing_runs`
/// **before** storing the outcome, so the tokens-list chain-column sort reads a
/// fully-populated run the moment the client sees the result.
pub async fn detect_tokens_swings_batch(
    state: web::Data<Arc<LocalState>>,
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

    // The run is keyed end-to-end by the client run id — no id, nothing to key the
    // cancel/progress/result stores or the chain-sort legs cache by.
    let Some(run_id) = run_id else {
        return HttpResponse::BadRequest().json(serde_json::json!({
            "error": "run_id is required",
        }));
    };

    if mints.len() > MAX_RUN_MINTS {
        return HttpResponse::BadRequest().json(serde_json::json!({
            "error": format!("too many mints: {} (max {MAX_RUN_MINTS})", mints.len()),
        }));
    }

    // Register a cooperative cancel flag + a readable progress snapshot for this
    // run so the global jobs endpoints (`/api/jobs/status`, the swing cancel) can
    // observe and abort it. Both are removed on every exit path (RAII guard),
    // which also broadcasts the terminal `SwingDetectionFinished` so a global
    // progress indicator clears itself without polling. Registered synchronously
    // here so an immediate cancel finds the entry before the task is scheduled.
    // Drop any stale result so only this run's outcome is collectable.
    let cancel = Arc::new(AtomicBool::new(false));
    let cell = Arc::new(ProgressCell::default());
    cell.set_total(mints.len() as u64);
    state.swing_cancels.insert(run_id.clone(), cancel.clone());
    state.swing_progress.insert(run_id.clone(), cell.clone());
    state.swing_results.clear(&run_id);

    let app = state.get_ref().clone();

    // Detach the scan and return immediately. `rt::spawn` keeps the task on the
    // worker independent of this request; a client disconnect never cancels it.
    // The task stores its outcome BEFORE the guard drops (which fires
    // `SwingDetectionFinished`), so a client reacting to that SSE always finds the
    // result present.
    actix_web::rt::spawn(async move {
        struct SwingGuard {
            app: Arc<LocalState>,
            run_id: String,
            cancel: Arc<AtomicBool>,
        }
        impl Drop for SwingGuard {
            fn drop(&mut self) {
                self.app.swing_cancels.remove(&self.run_id);
                self.app.swing_progress.remove(&self.run_id);
                let _ = self.app.sse_tx.send(SseEvent::SwingDetectionFinished {
                    run_id: self.run_id.clone(),
                    cancelled: self.cancel.load(Ordering::Acquire),
                });
            }
        }
        let _guard = SwingGuard {
            app: app.clone(),
            run_id: run_id.clone(),
            cancel: cancel.clone(),
        };

        // Resolve (creating) the run that holds the raw legs for the tokens-list
        // chain sort.
        let run = app.swing_runs.get_or_create(&run_id);

        // 1) Resolve every requested mint's trades in a single lake read — the
        //    loader stages the whole mint list into one DuckDB temp table and
        //    scans once, so this is one lake query rather than one PG round trip
        //    per mint. `curve_only` is applied at load (the projected
        //    `CorpusTrade` has no `venue`). A lake-wide failure fails the whole
        //    run explicitly rather than degrading per-mint to an empty history.
        let mut histories = match fetch_sim_histories(&mints, curve_only).await {
            Ok(map) => map,
            Err(e) => {
                tracing::error!("lake trade fetch failed for batch swing analysis: {e}");
                app.swing_results.insert(
                    run_id,
                    SwingOutcome::Failed {
                        status: 500,
                        message: "lake trade fetch failed".to_string(),
                    },
                );
                return;
            }
        };

        // 2) Run the (pure-CPU) window filter + swing scans off the runtime, on
        //    the blocking pool — doing them inline would pin a worker for the whole
        //    run. Check the cancel flag between mints and bump the progress cell
        //    per completed mint; on cancel, break early and surface a partial set
        //    the caller discards. A mint absent from the lake result (no rows) is
        //    treated as an empty history.
        let resp_params = params.clone();
        let cancel_scan = cancel.clone();
        let cell_scan = cell.clone();
        let run_scan = run.clone();
        let scan = web::block(move || {
            let mut results = Vec::with_capacity(mints.len());
            for (processed, mint) in mints.into_iter().enumerate() {
                if cancel_scan.load(Ordering::Acquire) {
                    break;
                }
                let trades = histories.remove(&mint).unwrap_or_default();
                let windowed = filter_trades_to_window(&trades, window_start_ms, window_end_ms);
                let swings = detect_swings(&windowed, &params);
                // Stash the raw legs for this run so the tokens list can sort by
                // (and re-group at any latency) the chain columns.
                run_scan.mints.insert(mint.clone(), swings.clone());
                results.push(SwingBatchEntry {
                    mint_address: mint,
                    count: swings.len(),
                    swings,
                });
                cell_scan.set_processed(processed as u64 + 1);
            }
            results
        })
        .await;

        // Store the terminal outcome BEFORE `_guard` drops and fires the SSE.
        let outcome = if cancel.load(Ordering::Acquire) {
            // User-requested abort — benign, not a failure. The partial scan (if
            // any) is discarded.
            SwingOutcome::Cancelled
        } else {
            match scan {
                Ok(results) => {
                    let resp = SwingBatchResponse {
                        params: resp_params,
                        results,
                    };
                    // Serialize once here so the result endpoint serves the bytes
                    // verbatim (no re-encode of a potentially large payload).
                    match serde_json::to_string(&resp) {
                        Ok(json) => SwingOutcome::Done(json),
                        Err(e) => SwingOutcome::Failed {
                            status: 500,
                            message: format!("result serialization failed: {e}"),
                        },
                    }
                }
                Err(e) => {
                    tracing::error!("swing run compute task panicked: {e}");
                    SwingOutcome::Failed {
                        status: 500,
                        message: "swing computation failed".to_string(),
                    }
                }
            }
        };
        app.swing_results.insert(run_id, outcome);
    });

    HttpResponse::Accepted().json(serde_json::json!({ "started": true }))
}
