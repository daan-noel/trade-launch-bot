//! Endpoints for **grouped** strategy param-sweeps (`/api/strategies/sweeps`).
//!
//! One generic handler set serves every strategy: the `strategy_id` resolves a
//! per-strategy table triple ([`registry::tables_for`]) and a sweep entry point
//! ([`registry::run_grouped`]). `start_grouped_sweep` selects tokens by a
//! `created_at` range, partitions them by the chosen fingerprint fields, sweeps
//! each surviving group, and persists run → groups → ranked combo rows. The
//! reads back the group-summary list and a group's ranked combo table for the
//! drill-in view.
//!
//! Shares the single-flight `sweep_running` gate with the live-cache TPSL2 sweep
//! (one CPU-heavy sweep at a time; the rayon pool is bounded inside the registry
//! so it can't starve the live trading hot path).

use std::sync::atomic::Ordering;
use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use futures_util::StreamExt as _;
use tokio_stream::wrappers::ReceiverStream;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::models::grouped_sweep::{GroupedSweepGroupWrite, GroupedSweepResult, GroupedSweepRun};
use crate::state::app_state::AppState;
use crate::storage::repositories::grouped_sweep_repo::{GroupedSweepRepo, GroupedSweepTables};
use crate::sweep::aggregate::ComboMetrics;
use crate::sweep::corpus::{attach_fingerprints, corpus_cache_dir, load_grouped_corpus, Selection};
use crate::sweep::grouped_engine::{CoverageFloor, GroupResult, GroupSink};
use crate::sweep::grouping::{group_key, normalize_label_vec, GroupField};
use crate::sweep::registry;
use crate::sweep::strategy::parse_method;

// ---------------------------------------------------------------------------
// Request / query bodies
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
pub struct StartGroupedSweepBody {
    /// Which strategy to sweep (`"tpsl2"`). Resolves the table set + entry point.
    pub strategy_id: String,
    /// Selection lower bound (inclusive). Omitted ⇒ no lower bound.
    #[serde(default)]
    pub created_after: Option<DateTime<Utc>>,
    /// Selection upper bound (exclusive). Omitted ⇒ no upper bound.
    #[serde(default)]
    pub created_before: Option<DateTime<Utc>>,
    /// Restrict to bonding-curve trades only (drop migrated-AMM legs).
    #[serde(default)]
    pub curve_only: bool,
    /// Fingerprint fields to group by (exact-value). Empty ⇒ one "ALL" group.
    #[serde(default)]
    pub group_by: Vec<GroupField>,
    /// Optional exact-set instruction-label filter: when set (and non-empty),
    /// restrict the corpus to tokens whose normalized `ix_labels` set EXACTLY
    /// equals these labels, then sweep that slice. The page offers this as the
    /// alternative to grouping by `ix_labels` — it's disabled there when the
    /// `IxLabels` group-by is selected. Empty/omitted ⇒ no filter.
    #[serde(default)]
    pub ix_labels_filter: Option<Vec<String>>,
    /// Drop groups with fewer than this many tokens before sweeping.
    #[serde(default = "default_min_tokens")]
    pub min_tokens: usize,
    /// Coverage floor — absolute minimum fired tokens a combo needs before it's
    /// eligible to be a group's headline pick (over-fit guard, finding #2).
    #[serde(default = "default_min_fired_abs")]
    pub min_fired_abs: u64,
    /// Coverage floor — fraction of the group's tokens a combo must fire on
    /// (the floor is `max(min_fired_abs, ceil(fire_frac · group_tokens))`).
    #[serde(default = "default_fire_frac")]
    pub fire_frac: f64,
    /// `grid` | `random:N` | `lhs:N` | `refine:N[:K]`. `refine` runs a coarse LHS
    /// pass of `N` draws then re-sweeps a neighborhood around each group's top-`K`
    /// combos (`K` default 3). Defaults to a full grid.
    #[serde(default)]
    pub method: Option<String>,
    /// Strategy-specific param axes (e.g. TPSL2's `AxesSpec`). Omitted axes fall
    /// back to that strategy's hardcoded defaults.
    #[serde(default = "default_axes")]
    pub axes: serde_json::Value,
    /// Hard cap on tokens loaded for the corpus (server-clamped).
    #[serde(default = "default_token_cap")]
    pub token_cap: usize,
    /// Per-group combo cap override. Omitted ⇒ the default `MAX_COMBOS`; clamped
    /// server-side to `HARD_MAX_COMBOS` so a typo can't run away.
    #[serde(default)]
    pub max_combos: Option<usize>,
    /// Bypass the corpus Parquet cache: force a fresh DB load (and rewrite the
    /// cache) even for a cacheable closed/settled window. Open/recent windows are
    /// never cached regardless. Default `false`.
    #[serde(default)]
    pub fresh: bool,
    /// Per-field value filters — restrict the corpus to tokens whose fingerprint
    /// value for the named field is in the allowed set. Map key = `GroupField`
    /// serde tag (e.g. `"cu_price"`); value = JSON array of allowed numbers.
    /// Empty array or absent key ⇒ no filter for that field. `"ix_labels"` is
    /// skipped here (use `ix_labels_filter` instead). Applied post-fingerprint,
    /// in-memory, so the unfiltered Parquet corpus cache is reused.
    #[serde(default)]
    pub field_filters: Option<std::collections::HashMap<String, Vec<serde_json::Value>>>,
}

fn default_min_tokens() -> usize {
    10
}
fn default_min_fired_abs() -> u64 {
    10
}
fn default_fire_frac() -> f64 {
    0.05
}
fn default_token_cap() -> usize {
    10_000
}
fn default_axes() -> serde_json::Value {
    serde_json::Value::Object(Default::default())
}

#[derive(serde::Deserialize)]
pub struct RunsQuery {
    pub strategy_id: String,
    #[serde(default = "default_runs_limit")]
    pub limit: i64,
}

fn default_runs_limit() -> i64 {
    50
}

#[derive(serde::Deserialize)]
pub struct StrategyQuery {
    pub strategy_id: String,
}

#[derive(serde::Deserialize)]
pub struct PruneQuery {
    pub strategy_id: String,
    /// Required cutoff — runs created strictly before this are deleted.
    pub before: DateTime<Utc>,
}

#[derive(serde::Deserialize)]
pub struct ResultsQuery {
    pub strategy_id: String,
    /// Zero-based page index (default 0).
    #[serde(default)]
    pub page: Option<i64>,
    /// Rows per page, clamped to 1–1000 (default 200).
    #[serde(default)]
    pub limit: Option<i64>,
}

// ---------------------------------------------------------------------------
// POST /api/strategies/sweeps — start a grouped DB-range sweep
// ---------------------------------------------------------------------------

pub async fn start_grouped_sweep(
    state: web::Data<Arc<AppState>>,
    body: web::Json<StartGroupedSweepBody>,
) -> impl Responder {
    let b = body.into_inner();

    // Resolve the per-strategy table set (also validates the strategy id).
    let tables = match registry::tables_for(&b.strategy_id) {
        Some(t) => t,
        None => {
            return HttpResponse::BadRequest().json(serde_json::json!({
                "error": format!(
                    "unknown strategy_id '{}' (supported: {:?})",
                    b.strategy_id,
                    registry::strategy_ids()
                )
            }))
        }
    };

    // Single-flight: claim the shared sweep gate or reject. Claimed synchronously
    // here so a concurrent request gets its 409 immediately; the spawned job owns
    // the matching release (`run_grouped_sweep_job`'s `Gate`).
    if state
        .sweep_running
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return HttpResponse::Conflict()
            .json(serde_json::json!({"error": "a sweep is already running"}));
    }

    // Detach the run so a client disconnect (browser refresh / SPA navigation)
    // can't drop the request future mid-sweep. The job — its `sweep_running` +
    // progress snapshot and the persist step — must outlive the HTTP request so
    // `GET /api/jobs/status` can recover the in-flight bar after a reload (the
    // whole point of the global progress indicator). `rt::spawn` keeps it on the
    // worker (no `Send` bound, unlike `tokio::spawn`); awaiting the handle returns
    // the same response when the client stays connected, and dropping it on
    // disconnect never cancels the task.
    let state = state.get_ref().clone();
    actix_web::rt::spawn(run_grouped_sweep_job(state, b, tables))
        .await
        .unwrap_or_else(|_| {
            HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "sweep task ended unexpectedly"}))
        })
}

/// The detached body of a grouped sweep, spawned by [`start_grouped_sweep`] so
/// the work survives a client disconnect (the recovery contract behind
/// `/api/jobs/status`). Owns the single-flight release + progress reset + terminal
/// `SweepFinished` (via `Gate`), loads the corpus, runs the engine, and persists
/// run → groups → results. Assumes `sweep_running` was already claimed by the
/// caller.
async fn run_grouped_sweep_job(
    state: Arc<AppState>,
    b: StartGroupedSweepBody,
    tables: GroupedSweepTables,
) -> HttpResponse {
    // Releases the single-flight gate, resets the progress snapshot, and
    // broadcasts the terminal `SweepFinished` frame on EVERY exit path (done /
    // cancel / config-error / db-error) — putting the emit in Drop means no
    // early-return can forget it, so a global progress indicator always clears.
    struct Gate {
        running: Arc<std::sync::atomic::AtomicBool>,
        cancel: Arc<std::sync::atomic::AtomicBool>,
        progress: Arc<crate::state::job_progress::ProgressCell>,
        sse_tx: tokio::sync::broadcast::Sender<crate::models::ingest::SseEvent>,
        strategy_id: String,
    }
    impl Drop for Gate {
        fn drop(&mut self) {
            self.progress.reset();
            self.running.store(false, Ordering::Release);
            let _ = self.sse_tx.send(crate::models::ingest::SseEvent::SweepFinished {
                strategy_id: self.strategy_id.clone(),
                cancelled: self.cancel.load(Ordering::Acquire),
            });
        }
    }
    let _gate = Gate {
        running: state.sweep_running.clone(),
        cancel: state.sweep_cancel.clone(),
        progress: state.sweep_progress.clone(),
        sse_tx: state.sse_tx.clone(),
        strategy_id: b.strategy_id.clone(),
    };

    // Clear any stale cancel request + progress snapshot from a prior run before
    // this one starts.
    state.sweep_cancel.store(false, Ordering::Release);
    state.sweep_progress.reset();

    // Phase 0.3 — start the RSS/wall-clock trace at admission so every milestone
    // (corpus loaded, done) is measured from the same origin against the baseline.
    let clock = crate::sweep::obs::SweepClock::start();
    crate::sweep::obs::log_milestone(&clock, "admitted");

    let token_cap = b.token_cap.clamp(1, 100_000);
    let min_tokens = b.min_tokens.max(1);
    let floor = CoverageFloor {
        min_fired_abs: b.min_fired_abs,
        fire_frac: b.fire_frac.clamp(0.0, 1.0),
    };
    // `grid` | `random:N` | `lhs:N` | `refine:N[:K]`. A `refine` form runs a coarse
    // LHS pass then a per-group neighborhood refine (see `parse_method`).
    let (method, refine) = b
        .method
        .as_deref()
        .map(parse_method)
        .unwrap_or((crate::sweep::strategy::SweepMethod::Grid, None));
    // Stored run tag: the refine pass reports as its own method, else the sampler.
    let method_tag = if refine.is_some() { "refine".to_string() } else { method.tag().to_string() };

    let sel = Selection {
        mints: None,
        token_cap,
        created_after: b.created_after,
        created_before: b.created_before,
        // Sweep-specific cap (Phase 1.1): launch-window scalp entries decide on the
        // first minutes, so a few thousand trades/token is plenty — far below the
        // live `MAX_TRADES_RETAINED`. Override with `SWEEP_PER_MINT_CAP`.
        per_mint_cap: crate::sweep::corpus::sweep_per_mint_cap(),
        // Keep each over-cap token's launch window — the entry logic decides on the
        // first minutes, which a newest-first cap would drop (Rec 4).
        window: crate::sweep::corpus::TradeWindow::LaunchWindow,
        curve_only: b.curve_only,
    };

    // Load the corpus, reusing the selection-keyed Parquet cache for a closed/
    // settled window (Rec 3) so repeated sweeps over the same window skip the DB
    // load; an open/recent window always loads fresh. Fingerprints are attached
    // separately below (the trade cache is fingerprint-free) so caching trades
    // doesn't complicate grouping.
    let mut corpus = match load_grouped_corpus(state.db.clone(), &sel, &corpus_cache_dir(), b.fresh)
        .await
    {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("grouped sweep: corpus load failed: {e}");
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": e.to_string()}));
        }
    };
    if corpus.tokens.is_empty() {
        return HttpResponse::BadRequest()
            .json(serde_json::json!({"error": "no tokens in that date range — widen the selection"}));
    }
    if let Err(e) = attach_fingerprints(&state.db, &mut corpus).await {
        tracing::error!("grouped sweep: attach_fingerprints failed: {e}");
        return HttpResponse::InternalServerError()
            .json(serde_json::json!({"error": e.to_string()}));
    }
    // Optional exact-set ix_labels filter (applied post-fingerprint, in-memory so
    // the unfiltered Parquet corpus cache is reused across filter values): keep only
    // tokens whose label set equals the requested set. Normalize the request set the
    // same way the fingerprint is, so the `==` is order/dup-insensitive. An empty
    // request set is "no filter" (not "tokens with no labels").
    if let Some(want) = b
        .ix_labels_filter
        .as_ref()
        .filter(|f| !f.is_empty())
        .map(|f| crate::sweep::grouping::normalize_label_vec(f.clone()))
    {
        let before = corpus.tokens.len();
        corpus.tokens.retain(|t| t.fp.ix_labels == want);
        tracing::info!(
            kept = corpus.tokens.len(),
            dropped = before - corpus.tokens.len(),
            labels = ?want,
            "grouped sweep: ix_labels exact-set filter applied"
        );
        if corpus.tokens.is_empty() {
            return HttpResponse::BadRequest().json(serde_json::json!({
                "error": "no tokens match that instruction-label filter — adjust the labels or widen the selection"
            }));
        }
    }
    // Optional per-field numeric filters — restrict the corpus to tokens whose
    // fingerprint value for the named field is in the allowed set. Applied
    // post-fingerprint, in-memory, reusing the unfiltered Parquet cache.
    // Unknown keys and `ix_labels` (handled above) are skipped with a warning.
    if let Some(filters) = &b.field_filters {
        use crate::sweep::grouping::GroupField;
        for (field_str, allowed) in filters {
            if allowed.is_empty() {
                continue;
            }
            let field: GroupField = match serde_json::from_str(&format!("\"{field_str}\"")) {
                Ok(f) => f,
                Err(_) => {
                    tracing::warn!(key = %field_str, "grouped sweep: unknown field_filters key, skipping");
                    continue;
                }
            };
            if field == GroupField::IxLabels {
                tracing::warn!("grouped sweep: ix_labels in field_filters — use ix_labels_filter instead, skipping");
                continue;
            }
            let before = corpus.tokens.len();
            corpus.tokens.retain(|t| matches_field_filter(&t.fp, field, allowed));
            tracing::info!(
                field = %field_str,
                kept = corpus.tokens.len(),
                dropped = before - corpus.tokens.len(),
                "grouped sweep: field filter applied"
            );
            if corpus.tokens.is_empty() {
                return HttpResponse::BadRequest().json(serde_json::json!({
                    "error": format!(
                        "no tokens match the '{field_str}' field filter — adjust the values or widen the selection"
                    )
                }));
            }
        }
    }

    // Corpus is fully resident here — the Phase 0 baseline's first peak (every
    // streaming phase is judged against this RSS/seconds reading).
    tracing::info!(
        tokens = corpus.token_count(),
        trades = corpus.trade_count(),
        rss_mb = crate::sweep::obs::process_rss_mb(),
        elapsed_s = clock.elapsed_secs(),
        milestone = "corpus_loaded",
        "sweep: milestone"
    );

    let corpus_hash = corpus.hash.clone();
    let token_count = corpus.token_count() as i32;
    let grouping_spec = serde_json::to_value(&b.group_by).unwrap_or_else(|_| serde_json::json!([]));

    // Phase 4 — write the run header up front (`status = "running"`) BEFORE the
    // sweep, so the per-group `append_group` writes (FK → this row) can land
    // incrementally and a cancel/crash keeps whatever finished. `group_count` /
    // `combo_count` are placeholders (0) here, set by the engine's `begin` once
    // the surviving + combo sets are known; `axes_spec` is the raw request axes
    // until `finalize_completed` swaps in the resolved set. Counts/status/axes are
    // re-stamped at the terminal step.
    let run_id = Uuid::new_v4();
    let mut run = GroupedSweepRun {
        id: run_id,
        strategy_id: b.strategy_id.clone(),
        source: "db".to_string(),
        method: method_tag,
        created_after: b.created_after,
        created_before: b.created_before,
        curve_only: b.curve_only,
        grouping_spec,
        axes_spec: b.axes.clone(),
        min_tokens: min_tokens as i32,
        token_count,
        group_count: 0,
        combo_count: 0,
        corpus_hash: Some(corpus_hash),
        created_at: Utc::now(),
        status: "running".to_string(),
        groups_done: 0,
        // Persist the corpus filters + cap knobs exactly as submitted, so the
        // history panel can show what the run was for and a re-run can restore it.
        // Only store an active (non-empty) ix_labels filter; an empty/omitted one
        // means "no filter" and reads back as NULL.
        ix_labels_filter: b
            .ix_labels_filter
            .as_ref()
            .filter(|f| !f.is_empty())
            .and_then(|f| serde_json::to_value(f).ok()),
        field_filters: b
            .field_filters
            .as_ref()
            .filter(|f| !f.is_empty())
            .and_then(|f| serde_json::to_value(f).ok()),
        token_cap: Some(b.token_cap as i32),
        max_combos: b.max_combos.map(|v| v as i32),
        label: None,
    };
    let repo = GroupedSweepRepo::new(state.db.clone(), tables);
    if let Err(e) = repo.insert_run(&run).await {
        tracing::error!("grouped sweep: insert_run failed: {e}");
        return HttpResponse::InternalServerError()
            .json(serde_json::json!({"error": e.to_string()}));
    }

    // Single DB-writer task: the engine's per-group emits (which arrive out of
    // order from the cross-group small-group phase) are serialized through one
    // unbounded channel into one task, so concurrent folds never race the
    // connection. It drains until the sink (and thus the sender) drops when the
    // sweep ends, then returns the persisted-group tally. Unbounded so the sync
    // engine callback never blocks a rayon worker.
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<SweepWrite>();
    let writer = {
        let db = state.db.clone();
        let writer_sse_tx = state.sse_tx.clone();
        let writer_strategy_id = b.strategy_id.clone();
        actix_web::rt::spawn(async move {
            let repo = GroupedSweepRepo::new(db, tables);
            let mut groups_done = 0i32;
            let mut total_groups = 0i32;
            while let Some(msg) = rx.recv().await {
                match msg {
                    SweepWrite::Begin { group_count, combo_count } => {
                        total_groups = group_count;
                        if let Err(e) = repo.update_run_counts(run_id, group_count, combo_count).await
                        {
                            tracing::error!("grouped sweep: update_run_counts failed: {e}");
                        }
                        // Announce the saving phase so the frontend switches from
                        // the sweep bar to a fresh saving bar before any DB writes.
                        let _ = writer_sse_tx.send(crate::models::ingest::SseEvent::SweepProgress {
                            strategy_id: writer_strategy_id.clone(),
                            phase: "saving".to_string(),
                            processed: 0,
                            total: group_count as u64,
                        });
                    }
                    SweepWrite::Group(g) => match repo.append_group(run_id, &g).await {
                        Ok(()) => {
                            groups_done += 1;
                            let _ = writer_sse_tx.send(crate::models::ingest::SseEvent::SweepProgress {
                                strategy_id: writer_strategy_id.clone(),
                                phase: "saving".to_string(),
                                processed: groups_done as u64,
                                total: total_groups as u64,
                            });
                        }
                        Err(e) => tracing::error!("grouped sweep: append_group failed: {e}"),
                    },
                }
            }
            groups_done
        })
    };

    // Two phase-tagged observers: coarse_observer reports the coarse LHS pass
    // (only active for `refine` runs); observer reports the final sweep pass.
    // Both share the cancel flag; only `observer` writes the ProgressCell so
    // `/api/jobs/status` recovery shows the sweep-phase bar (the most useful one).
    let coarse_observer: Arc<dyn crate::sweep::progress::SweepObserver + Send> =
        Arc::new(crate::sweep::progress::SweepProgress::new(
            state.sse_tx.clone(),
            b.strategy_id.clone(),
            state.sweep_cancel.clone(),
            Arc::new(crate::state::job_progress::ProgressCell::default()),
            "coarse",
        ));
    let observer: Arc<dyn crate::sweep::progress::SweepObserver + Send> =
        Arc::new(crate::sweep::progress::SweepProgress::new(
            state.sse_tx.clone(),
            b.strategy_id.clone(),
            state.sweep_cancel.clone(),
            state.sweep_progress.clone(),
            "sweep",
        ));
    // The sink hands each fully-folded group to the writer task. `run_grouped`
    // takes ownership, so it (and its sender) drop when the sweep ends — closing
    // the channel and letting the writer task finish.
    let sink: Arc<dyn GroupSink + Send + Sync> = Arc::new(HandlerSink { tx });

    let result = registry::run_grouped(
        &b.strategy_id,
        b.axes.clone(),
        method,
        refine,
        corpus,
        b.group_by.clone(),
        min_tokens,
        floor,
        b.max_combos,
        coarse_observer,
        observer,
        sink,
    )
    .await;

    // The sweep is done — the sink dropped, so the writer drains and returns how
    // many groups actually persisted (the partial count on cancel/error).
    let groups_done = writer.await.unwrap_or(0);

    let output = match result {
        Ok(o) => o,
        Err(e) => {
            // A cooperative cancel surfaces as an engine error. Keep whatever
            // groups already committed and mark the run cancelled (honestly
            // partial) instead of discarding them.
            if state.sweep_cancel.load(Ordering::Acquire) {
                tracing::info!(groups_done, "grouped sweep: cancelled by user (partial kept)");
                if let Err(e) = repo.mark_cancelled(run_id).await {
                    tracing::error!("grouped sweep: mark_cancelled failed: {e}");
                }
                return HttpResponse::Ok().json(serde_json::json!({
                    "cancelled": true,
                    "run_id": run_id,
                    "groups_done": groups_done,
                }));
            }
            // Config errors (bad axes / over-cap grid) fail before any group
            // folds, leaving an empty placeholder run — drop it so it doesn't
            // litter the picker as a 0-group cancelled run.
            tracing::error!("grouped sweep failed: {e}");
            if let Err(del) = repo.delete_run(run_id).await {
                tracing::error!("grouped sweep: cleanup of failed run failed: {del}");
            }
            return HttpResponse::BadRequest().json(serde_json::json!({"error": e.to_string()}));
        }
    };

    // Full success — stamp the terminal status, authoritative counts, and the
    // resolved axes (only known now).
    if let Err(e) = repo
        .finalize_completed(
            run_id,
            output.groups.len() as i32,
            output.combo_count as i32,
            &output.axes_json,
        )
        .await
    {
        tracing::error!("grouped sweep: finalize failed: {e}");
        return HttpResponse::InternalServerError()
            .json(serde_json::json!({"error": e.to_string()}));
    }

    // Reflect the finalized state in the response body the client gets back.
    run.axes_spec = output.axes_json.clone();
    run.group_count = output.groups.len() as i32;
    run.combo_count = output.combo_count as i32;
    run.groups_done = groups_done;
    run.status = "completed".to_string();

    tracing::info!(
        run_id = %run.id,
        strategy = %run.strategy_id,
        tokens = run.token_count,
        groups = run.group_count,
        combos = run.combo_count,
        rss_mb = crate::sweep::obs::process_rss_mb(),
        elapsed_s = clock.elapsed_secs(),
        milestone = "done",
        "grouped sweep: done"
    );
    HttpResponse::Ok().json(run)
}

/// Message to the single per-run DB-writer task. `Begin` carries the engine's
/// fixed group/combo counts (one per run); `Group` carries one completed group's
/// write unit. Boxed so the enum stays small despite the large group payload.
enum SweepWrite {
    Begin { group_count: i32, combo_count: i32 },
    Group(Box<GroupedSweepGroupWrite>),
}

/// The [`GroupSink`] bridge from the (sync, rayon-worker) engine to the (async)
/// DB-writer task: each emit is forwarded over the unbounded channel. Sends are
/// best-effort — a closed channel (writer already gone) just drops the emit,
/// which only happens on shutdown.
struct HandlerSink {
    tx: tokio::sync::mpsc::UnboundedSender<SweepWrite>,
}

impl GroupSink for HandlerSink {
    fn begin(&self, group_count: usize, combo_count: usize) {
        let _ = self.tx.send(SweepWrite::Begin {
            group_count: group_count as i32,
            combo_count: combo_count as i32,
        });
    }

    fn group_done(&self, group_index: usize, group: &GroupResult, combo_params: &[serde_json::Value]) {
        let write = group_to_write(group_index, group, combo_params);
        let _ = self.tx.send(SweepWrite::Group(Box::new(write)));
    }
}

/// Flatten one fully-folded group into the repo's write unit. `group_index` is the
/// engine's deterministic order (largest group first). `fired_count` is the best
/// combo's `n_fired` — the sample size behind the group's headline pick.
/// `combo_params[combo_id]` is each ranked combo's param JSON.
fn group_to_write(
    group_index: usize,
    g: &GroupResult,
    combo_params: &[serde_json::Value],
) -> GroupedSweepGroupWrite {
    let param_at = |id: u32| -> serde_json::Value {
        combo_params.get(id as usize).cloned().unwrap_or(serde_json::Value::Null)
    };
    let fired_count = g
        .metrics
        .get(g.best_combo_id as usize)
        .map(|m| m.n_fired as i64)
        .unwrap_or(0);
    let results = g
        .metrics
        .iter()
        .map(|m| metrics_to_result(m, param_at(m.combo_id)))
        .collect();
    GroupedSweepGroupWrite {
        group_index: group_index as i32,
        group_key: g.key.to_json(),
        token_count: g.token_count as i32,
        fired_count,
        best_combo_id: g.best_combo_id as i32,
        best_score: g.best_score,
        best_expectancy_sol: g.best_expectancy_sol,
        best_params: param_at(g.best_combo_id),
        results,
    }
}

/// Map one combo's aggregated metrics + its param JSON into a persistable row.
fn metrics_to_result(m: &ComboMetrics, params: serde_json::Value) -> GroupedSweepResult {
    GroupedSweepResult {
        combo_id: m.combo_id as i32,
        params,
        n_fired: m.n_fired as i64,
        n_open: m.n_open as i64,
        n_closed: m.n_closed as i64,
        win_rate: m.win_rate,
        total_pnl_sol: m.total_pnl_sol,
        mean_pnl_pct: m.mean_pnl_pct,
        median_pnl_pct: m.median_pnl_pct,
        p90_pnl_pct: m.p90_pnl_pct,
        best_pnl_pct: m.best_pnl_pct,
        worst_pnl_pct: m.worst_pnl_pct,
        std_pnl_pct: m.std_pnl_pct,
        profit_factor: m.profit_factor,
        score: m.score,
        expectancy_sol: m.expectancy_sol,
        avg_holding_secs: m.avg_holding_secs,
        median_holding_secs: m.median_holding_secs,
        n_exit_take_profit: m.n_exit_take_profit as i32,
        n_exit_stop_loss: m.n_exit_stop_loss as i32,
        n_exit_trailing: m.n_exit_trailing as i32,
        n_exit_stall: m.n_exit_stall as i32,
        n_exit_time: m.n_exit_time as i32,
        n_exit_liquidity: m.n_exit_liquidity as i32,
        n_exit_cohort: m.n_exit_cohort as i32,
        n_exit_open: m.n_exit_open as i32,
    }
}

// ---------------------------------------------------------------------------
// GET endpoints
// ---------------------------------------------------------------------------

/// `GET /api/strategies/sweeps?strategy_id=tpsl2&limit=50` — runs, newest first.
pub async fn list_runs(
    state: web::Data<Arc<AppState>>,
    query: web::Query<RunsQuery>,
) -> impl Responder {
    let tables = match registry::tables_for(&query.strategy_id) {
        Some(t) => t,
        None => return bad_strategy(&query.strategy_id),
    };
    let limit = query.limit.clamp(1, 200);
    match GroupedSweepRepo::new(state.db.clone(), tables).list_runs(limit).await {
        Ok(runs) => HttpResponse::Ok().json(runs),
        Err(e) => {
            tracing::error!("DB error listing grouped sweep runs: {e}");
            HttpResponse::InternalServerError().json(serde_json::json!({"error": "database error"}))
        }
    }
}

/// `GET /api/strategies/sweeps/{run_id}/groups?strategy_id=tpsl2` — group
/// summaries for a run (best expectancy first).
pub async fn list_groups(
    state: web::Data<Arc<AppState>>,
    path: web::Path<Uuid>,
    query: web::Query<StrategyQuery>,
) -> impl Responder {
    let tables = match registry::tables_for(&query.strategy_id) {
        Some(t) => t,
        None => return bad_strategy(&query.strategy_id),
    };
    let run_id = path.into_inner();
    match GroupedSweepRepo::new(state.db.clone(), tables).list_groups(run_id).await {
        Ok(groups) => HttpResponse::Ok().json(groups),
        Err(e) => {
            tracing::error!("DB error listing grouped sweep groups: {e}");
            HttpResponse::InternalServerError().json(serde_json::json!({"error": "database error"}))
        }
    }
}

/// `GET /api/strategies/sweeps/{run_id}/groups/{group_id}/results?strategy_id=tpsl2&page=0&limit=200`
///
/// Streams one page of ranked combo rows as NDJSON (one JSON object per line).
/// Best-score combos come first (`ORDER BY score DESC NULLS LAST`).
/// The `X-Total-Count` response header carries the unfiltered group total so the
/// frontend can render a correct page count without a second round-trip.
pub async fn list_results(
    state: web::Data<Arc<AppState>>,
    path: web::Path<(Uuid, Uuid)>,
    query: web::Query<ResultsQuery>,
) -> HttpResponse {
    let tables = match registry::tables_for(&query.strategy_id) {
        Some(t) => t,
        None => return bad_strategy(&query.strategy_id),
    };
    let (run_id, group_id) = path.into_inner();
    let page = query.page.unwrap_or(0).max(0);
    let limit = query.limit.unwrap_or(200).clamp(1, 1000);
    let offset = page * limit;

    let repo = GroupedSweepRepo::new(state.db.clone(), tables);

    let total = match repo.count_results(run_id, group_id).await {
        Ok(n) => n,
        Err(e) => {
            tracing::error!("DB error counting grouped sweep results: {e}");
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "database error"}));
        }
    };

    let results = match repo.list_results_paged(run_id, group_id, limit, offset).await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("DB error listing grouped sweep results: {e}");
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "database error"}));
        }
    };

    // Serialize each row as a newline-delimited JSON line and stream them through
    // a channel so actix uses chunked transfer — the frontend reader sees rows
    // arrive progressively rather than waiting for the full payload.
    // `actix_web::Error` is !Send so we send plain `Bytes` and wrap in `Ok` below.
    let (tx, rx) = tokio::sync::mpsc::channel::<actix_web::web::Bytes>(64);
    tokio::spawn(async move {
        for result in results {
            match serde_json::to_vec(&result) {
                Ok(mut bytes) => {
                    bytes.push(b'\n');
                    if tx.send(actix_web::web::Bytes::from(bytes)).await.is_err() {
                        break; // client disconnected
                    }
                }
                Err(e) => {
                    tracing::error!("NDJSON serialization error: {e}");
                    break;
                }
            }
        }
    });

    let stream = ReceiverStream::new(rx).map(Ok::<_, actix_web::Error>);

    HttpResponse::Ok()
        .content_type("application/x-ndjson")
        .insert_header(("X-Total-Count", total.to_string()))
        .insert_header(("Access-Control-Expose-Headers", "X-Total-Count"))
        .streaming(stream)
}

/// `POST /api/strategies/sweeps/cancel` — request cancellation of the in-flight
/// grouped sweep. Cooperative: flips the shared cancel flag, which the engine
/// polls between groups / in the per-token loop and bails on (the in-flight
/// `start_grouped_sweep` request then returns `{cancelled: true}`). A no-op when
/// no sweep is running.
pub async fn cancel_grouped_sweep(state: web::Data<Arc<AppState>>) -> impl Responder {
    let running = state.sweep_running.load(Ordering::Acquire);
    if running {
        state.sweep_cancel.store(true, Ordering::Release);
    }
    HttpResponse::Ok().json(serde_json::json!({ "cancelling": running }))
}

/// `DELETE /api/strategies/sweeps/{run_id}?strategy_id=tpsl2` — drop one run.
/// Groups + results cascade. 404 if the id isn't found.
pub async fn delete_run(
    state: web::Data<Arc<AppState>>,
    path: web::Path<Uuid>,
    query: web::Query<StrategyQuery>,
) -> impl Responder {
    let tables = match registry::tables_for(&query.strategy_id) {
        Some(t) => t,
        None => return bad_strategy(&query.strategy_id),
    };
    let run_id = path.into_inner();
    match GroupedSweepRepo::new(state.db.clone(), tables).delete_run(run_id).await {
        Ok(0) => HttpResponse::NotFound().json(serde_json::json!({"error": "run not found"})),
        Ok(n) => HttpResponse::Ok().json(serde_json::json!({"deleted": n})),
        Err(e) => {
            tracing::error!("DB error deleting grouped sweep run: {e}");
            HttpResponse::InternalServerError().json(serde_json::json!({"error": "database error"}))
        }
    }
}

/// Body for [`rename_run`]. A blank/whitespace-only label clears the name
/// (stored as NULL → the UI falls back to the timestamp + grouping hint).
#[derive(serde::Deserialize)]
pub struct RenameBody {
    pub label: String,
}

/// `PATCH /api/strategies/sweeps/{run_id}?strategy_id=tpsl2` — set or clear a
/// run's user-given name. 404 if the id isn't found.
pub async fn rename_run(
    state: web::Data<Arc<AppState>>,
    path: web::Path<Uuid>,
    query: web::Query<StrategyQuery>,
    body: web::Json<RenameBody>,
) -> impl Responder {
    let tables = match registry::tables_for(&query.strategy_id) {
        Some(t) => t,
        None => return bad_strategy(&query.strategy_id),
    };
    let run_id = path.into_inner();
    let trimmed = body.label.trim();
    let label = (!trimmed.is_empty()).then_some(trimmed);
    match GroupedSweepRepo::new(state.db.clone(), tables)
        .update_label(run_id, label)
        .await
    {
        Ok(0) => HttpResponse::NotFound().json(serde_json::json!({"error": "run not found"})),
        Ok(_) => HttpResponse::Ok().json(serde_json::json!({"label": label})),
        Err(e) => {
            tracing::error!("DB error renaming grouped sweep run: {e}");
            HttpResponse::InternalServerError().json(serde_json::json!({"error": "database error"}))
        }
    }
}

/// `DELETE /api/strategies/sweeps?strategy_id=tpsl2&before=<rfc3339>` — prune all
/// runs created strictly before `before` (groups + results cascade). `before` is
/// required so this can't accidentally wipe everything.
pub async fn prune_runs(
    state: web::Data<Arc<AppState>>,
    query: web::Query<PruneQuery>,
) -> impl Responder {
    let tables = match registry::tables_for(&query.strategy_id) {
        Some(t) => t,
        None => return bad_strategy(&query.strategy_id),
    };
    match GroupedSweepRepo::new(state.db.clone(), tables)
        .delete_runs_before(query.before)
        .await
    {
        Ok(n) => HttpResponse::Ok().json(serde_json::json!({"deleted": n})),
        Err(e) => {
            tracing::error!("DB error pruning grouped sweep runs: {e}");
            HttpResponse::InternalServerError().json(serde_json::json!({"error": "database error"}))
        }
    }
}

// ---------------------------------------------------------------------------
// Token-results drill-in (re-simulate one combo on its group's corpus slice)
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
pub struct TokenResultsQuery {
    pub strategy_id: String,
    pub combo_id: i32,
}

/// `GET /api/strategies/sweeps/{run_id}/groups/{group_id}/token-results`
///
/// Re-simulates a single stored combo against the tokens that belong to the
/// given group and returns per-token PnL / exit / holding-time. The corpus is
/// loaded from the Parquet cache (near-instant for settled windows) so this
/// endpoint is cheap to call on repeat.
pub async fn list_token_results(
    state: web::Data<Arc<AppState>>,
    path: web::Path<(Uuid, Uuid)>,
    query: web::Query<TokenResultsQuery>,
) -> impl Responder {
    let tables = match registry::tables_for(&query.strategy_id) {
        Some(t) => t,
        None => return bad_strategy(&query.strategy_id),
    };
    let (run_id, group_id) = path.into_inner();
    let repo = GroupedSweepRepo::new(state.db.clone(), tables);

    // Load the run header for the selection params.
    let run = match repo.get_run(run_id).await {
        Ok(Some(r)) => r,
        Ok(None) => {
            return HttpResponse::NotFound().json(serde_json::json!({"error": "run not found"}))
        }
        Err(e) => {
            tracing::error!("DB error fetching grouped sweep run: {e}");
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "database error"}));
        }
    };

    // Load the group header for its stored group_key.
    let group = match repo.get_group(group_id).await {
        Ok(Some(g)) => g,
        Ok(None) => {
            return HttpResponse::NotFound().json(serde_json::json!({"error": "group not found"}))
        }
        Err(e) => {
            tracing::error!("DB error fetching grouped sweep group: {e}");
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "database error"}));
        }
    };

    // Fetch the params JSON for the requested combo.
    let params_json = match repo.get_combo_params(group_id, query.combo_id).await {
        Ok(Some(p)) => p,
        Ok(None) => {
            return HttpResponse::NotFound().json(serde_json::json!({"error": "combo not found"}))
        }
        Err(e) => {
            tracing::error!("DB error fetching combo params: {e}");
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "database error"}));
        }
    };

    // Build the same Selection the original sweep used.
    let sel = Selection {
        created_after: run.created_after,
        created_before: run.created_before,
        curve_only: run.curve_only,
        token_cap: run.token_cap.map(|n| n as usize).unwrap_or(5_000),
        window: crate::sweep::corpus::TradeWindow::LaunchWindow,
        ..Default::default()
    };

    // Load corpus (Parquet cache for settled windows).
    let mut corpus = match load_grouped_corpus(state.db.clone(), &sel, &corpus_cache_dir(), false).await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("Failed to load corpus for token-results: {e}");
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "corpus load failed"}));
        }
    };
    if let Err(e) = attach_fingerprints(&state.db, &mut corpus).await {
        tracing::error!("Failed to attach fingerprints: {e}");
        return HttpResponse::InternalServerError()
            .json(serde_json::json!({"error": "fingerprint error"}));
    }

    // Re-apply the ix_labels_filter the sweep used.
    if let Some(want) = run
        .ix_labels_filter
        .as_ref()
        .and_then(|v| serde_json::from_value::<Vec<String>>(v.clone()).ok())
        .filter(|f| !f.is_empty())
        .map(|f| normalize_label_vec(f))
    {
        corpus.tokens.retain(|t| t.fp.ix_labels == want);
    }

    // Re-apply per-field value filters.
    if let Some(filters) = run
        .field_filters
        .as_ref()
        .and_then(|v| {
            serde_json::from_value::<std::collections::HashMap<String, Vec<serde_json::Value>>>(
                v.clone(),
            )
            .ok()
        })
    {
        for (field_str, allowed) in &filters {
            if allowed.is_empty() {
                continue;
            }
            let field: GroupField = match serde_json::from_str(&format!("\"{field_str}\"")) {
                Ok(f) => f,
                Err(_) => continue,
            };
            if field == GroupField::IxLabels {
                continue;
            }
            corpus.tokens.retain(|t| matches_field_filter(&t.fp, field, allowed));
        }
    }

    // Parse the grouping fields so we can recompute each token's group_key.
    let grouping_fields: Vec<GroupField> =
        match serde_json::from_value(run.grouping_spec.clone()) {
            Ok(f) => f,
            Err(e) => {
                tracing::error!("Bad grouping_spec in run: {e}");
                return HttpResponse::InternalServerError()
                    .json(serde_json::json!({"error": "invalid grouping spec"}));
            }
        };

    // Filter to tokens that belong to this group by recomputing their key.
    let target_key = group.group_key.clone();
    corpus
        .tokens
        .retain(|t| group_key(&t.fp, &grouping_fields).to_json() == target_key);

    if corpus.tokens.is_empty() {
        return HttpResponse::Ok().json(serde_json::json!([]));
    }

    // Re-simulate on a blocking thread (CPU-bound but short: one combo × N tokens).
    let strategy_id = query.strategy_id.clone();
    let tokens = corpus.tokens;
    let result = tokio::task::spawn_blocking(move || {
        registry::simulate_one_combo(&strategy_id, &tokens, &params_json)
    })
    .await;

    match result {
        Ok(Ok(rows)) => HttpResponse::Ok().json(rows),
        Ok(Err(e)) => {
            tracing::error!("Single-combo simulation failed: {e}");
            HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": format!("simulation failed: {e}")}))
        }
        Err(e) => {
            tracing::error!("spawn_blocking panicked: {e}");
            HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "internal error"}))
        }
    }
}

fn bad_strategy(strategy_id: &str) -> HttpResponse {
    HttpResponse::BadRequest().json(serde_json::json!({
        "error": format!(
            "unknown strategy_id '{}' (supported: {:?})",
            strategy_id,
            registry::strategy_ids()
        )
    }))
}

/// Return `true` if the token's fingerprint value for `field` is in `allowed`.
/// `IxLabels` is handled by the `ix_labels_filter` path and never reaches here.
/// `CreatorWallet` / `TokenProgramId` aren't offered in the frontend filter UI.
fn matches_field_filter(
    fp: &crate::sweep::grouping::TokenFingerprint,
    field: crate::sweep::grouping::GroupField,
    allowed: &[serde_json::Value],
) -> bool {
    use crate::sweep::grouping::GroupField::*;
    match field {
        CuLimit => {
            let v = fp.cu_limit;
            allowed.iter().any(|a| a.as_i64().map(|x| Some(x) == v).unwrap_or(false))
        }
        CuPrice => {
            let v = fp.cu_price;
            allowed.iter().any(|a| a.as_i64().map(|x| Some(x) == v).unwrap_or(false))
        }
        MaxSolCost => {
            let v = fp.max_sol_cost;
            allowed.iter().any(|a| a.as_i64().map(|x| Some(x) == v).unwrap_or(false))
        }
        SpendableSolIn => {
            let v = fp.spendable_sol_in;
            allowed.iter().any(|a| a.as_i64().map(|x| Some(x) == v).unwrap_or(false))
        }
        InitialBuySol => {
            let v = fp.initial_buy_sol;
            allowed.iter().any(|a| {
                a.as_f64()
                    .map(|x| v.map(|y| (x - y).abs() < 1e-9).unwrap_or(false))
                    .unwrap_or(false)
            })
        }
        IsCashbackEnabled => {
            let v = fp.is_cashback_enabled;
            allowed.iter().any(|a| a.as_bool().map(|b| b == v).unwrap_or(false))
        }
        CreatorWallet | TokenProgramId | IxLabels => false,
    }
}
