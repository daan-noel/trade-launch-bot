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
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::models::grouped_sweep::{GroupedSweepGroupWrite, GroupedSweepResult, GroupedSweepRun};
use crate::state::app_state::AppState;
use crate::storage::repositories::grouped_sweep_repo::{GroupedSweepRepo, GroupedSweepTables};
use crate::sweep::aggregate::ComboMetrics;
use crate::sweep::corpus::{attach_fingerprints, corpus_cache_dir, load_grouped_corpus, Selection};
use crate::sweep::grouped_engine::CoverageFloor;
use crate::sweep::grouping::GroupField;
use crate::sweep::registry::{self, GroupedSweepOutput};
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

    // Streams real `processed / total` token frames over SSE and carries the
    // shared cancel flag into the engine's hot loop.
    let observer: Arc<dyn crate::sweep::progress::SweepObserver + Send> =
        Arc::new(crate::sweep::progress::SweepProgress::new(
            state.sse_tx.clone(),
            b.strategy_id.clone(),
            state.sweep_cancel.clone(),
            state.sweep_progress.clone(),
        ));

    let output = match registry::run_grouped(
        &b.strategy_id,
        b.axes.clone(),
        method,
        refine,
        corpus,
        b.group_by.clone(),
        min_tokens,
        floor,
        b.max_combos,
        observer,
    )
    .await
    {
        Ok(o) => o,
        Err(e) => {
            // A cooperative cancel surfaces as an engine error; report it as a
            // benign cancellation (no run persisted) rather than a failure.
            if state.sweep_cancel.load(Ordering::Acquire) {
                tracing::info!("grouped sweep: cancelled by user");
                return HttpResponse::Ok().json(serde_json::json!({"cancelled": true}));
            }
            tracing::error!("grouped sweep failed: {e}");
            // Config errors (bad axes / over-cap grid) are client-fixable.
            return HttpResponse::BadRequest().json(serde_json::json!({"error": e.to_string()}));
        }
    };

    let run = GroupedSweepRun {
        id: Uuid::new_v4(),
        strategy_id: b.strategy_id.clone(),
        source: "db".to_string(),
        method: method_tag,
        created_after: b.created_after,
        created_before: b.created_before,
        curve_only: b.curve_only,
        grouping_spec,
        axes_spec: output.axes_json.clone(),
        min_tokens: min_tokens as i32,
        token_count,
        group_count: output.groups.len() as i32,
        combo_count: output.combo_count as i32,
        corpus_hash: Some(corpus_hash),
        created_at: Utc::now(),
    };

    let groups = build_group_writes(&output);
    if let Err(e) = GroupedSweepRepo::new(state.db.clone(), tables)
        .save_run(&run, &groups)
        .await
    {
        tracing::error!("grouped sweep: persist failed: {e}");
        return HttpResponse::InternalServerError()
            .json(serde_json::json!({"error": e.to_string()}));
    }

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

/// Flatten the sweep output into the repo's write units: one group per surviving
/// fingerprint, carrying its ranked combo rows. `group_index` is the deterministic
/// order the engine returned (largest group first). `fired_count` is the best
/// combo's `n_fired` — the sample size behind the group's headline pick.
fn build_group_writes(output: &GroupedSweepOutput) -> Vec<GroupedSweepGroupWrite> {
    let param_at = |id: u32| -> serde_json::Value {
        output
            .combo_params
            .get(id as usize)
            .cloned()
            .unwrap_or(serde_json::Value::Null)
    };

    output
        .groups
        .iter()
        .enumerate()
        .map(|(idx, g)| {
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
                group_index: idx as i32,
                group_key: g.key.to_json(),
                token_count: g.token_count as i32,
                fired_count,
                best_combo_id: g.best_combo_id as i32,
                best_score: g.best_score,
                best_expectancy_sol: g.best_expectancy_sol,
                best_params: param_at(g.best_combo_id),
                results,
            }
        })
        .collect()
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

/// `GET /api/strategies/sweeps/{run_id}/groups/{group_id}/results?strategy_id=tpsl2`
/// — every ranked combo row for one group (the drill-in table).
pub async fn list_results(
    state: web::Data<Arc<AppState>>,
    path: web::Path<(Uuid, Uuid)>,
    query: web::Query<StrategyQuery>,
) -> impl Responder {
    let tables = match registry::tables_for(&query.strategy_id) {
        Some(t) => t,
        None => return bad_strategy(&query.strategy_id),
    };
    let (run_id, group_id) = path.into_inner();
    match GroupedSweepRepo::new(state.db.clone(), tables)
        .list_results(run_id, group_id)
        .await
    {
        Ok(results) => HttpResponse::Ok().json(results),
        Err(e) => {
            tracing::error!("DB error listing grouped sweep results: {e}");
            HttpResponse::InternalServerError().json(serde_json::json!({"error": "database error"}))
        }
    }
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

fn bad_strategy(strategy_id: &str) -> HttpResponse {
    HttpResponse::BadRequest().json(serde_json::json!({
        "error": format!(
            "unknown strategy_id '{}' (supported: {:?})",
            strategy_id,
            registry::strategy_ids()
        )
    }))
}
