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
use crate::state::local_state::SweepCorpusCache;
use crate::state::local_state::LocalState;
use crate::storage::repositories::grouped_sweep_repo::{GroupedSweepRepo, GroupedSweepTables};
use crate::sweep::aggregate::ComboMetrics;
use crate::lake::duck::LakeSource;
use crate::sweep::corpus::{sweep_per_mint_cap, CorpusSource, Selection};
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
    /// Notional (SOL) to price every simulated round-trip at. Affects the fixed
    /// per-leg cost (Jito tip + priority fee) as a fraction of the trade size —
    /// set this to the live `buy_amount_sol` so backtest PnL% matches live results.
    /// Omitted ⇒ 1.0 SOL (the server default).
    #[serde(default = "default_buy_amount_sol")]
    pub buy_amount_sol: f64,
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
fn default_buy_amount_sol() -> f64 {
    1.0
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
    /// Multi-key sort: an ordered, comma-joined `col:dir` list (index 0 = primary),
    /// e.g. `sort=n_fired:desc,win_rate:desc`. Each column is a frontend key
    /// (`"score"`, `"n_fired"`, `"p_exit_take_profit"`, …). This is the current
    /// wire format; `sort_col`/`sort_dir` below are the legacy single-key fallback.
    #[serde(default)]
    pub sort: Option<String>,
    /// Legacy single sort column. Omit to keep the default `score DESC`.
    #[serde(default)]
    pub sort_col: Option<String>,
    /// Legacy `"asc"` or `"desc"` (default `"desc"`).
    #[serde(default)]
    pub sort_dir: Option<String>,
}

/// Validated sort spec derived from `ResultsQuery`.
enum SortSpec {
    DirectCol { col: &'static str, dir: &'static str },
    ParamKey { key: String, dir: &'static str },
}

impl SortSpec {
    /// SQL `ORDER BY` fragment for this level (table-qualified, `NULLS LAST`).
    /// Every column/expression is allowlisted in `resolve_sort`, so the result
    /// is injection-safe.
    fn order_fragment(&self) -> String {
        match self {
            SortSpec::DirectCol { col, dir } => format!("r.{} {} NULLS LAST", col, dir),
            SortSpec::ParamKey { key, dir } => {
                format!("(c.params->>'{}')::numeric {} NULLS LAST", key, dir)
            }
        }
    }
}

/// Build the full `ORDER BY` body from the frontend sort params. Prefers the
/// multi-key `sort=col:dir,…` list; falls back to the legacy single
/// `sort_col`/`sort_dir`. Unresolvable levels are dropped. A stable
/// `r.combo_id ASC` tiebreak is always appended so equal rows keep a
/// deterministic order across pages/refetches. Empty ⇒ caller's default.
fn build_order_by(query: &ResultsQuery) -> String {
    order_by_from(
        query.sort.as_deref(),
        query.sort_col.as_deref(),
        query.sort_dir.as_deref(),
    )
}

/// Core of [`build_order_by`], split out so it's unit-testable without building
/// the full `ResultsQuery`.
fn order_by_from(sort: Option<&str>, sort_col: Option<&str>, sort_dir: Option<&str>) -> String {
    let mut levels: Vec<String> = Vec::new();
    if let Some(spec) = sort.filter(|s| !s.is_empty()) {
        for entry in spec.split(',') {
            let (col, dir) = entry.split_once(':').unwrap_or((entry, "desc"));
            if let Some(s) = resolve_sort(col.trim(), dir.trim()) {
                levels.push(s.order_fragment());
            }
        }
    } else if let Some(col) = sort_col {
        let dir = sort_dir.unwrap_or("desc");
        if let Some(s) = resolve_sort(col, dir) {
            levels.push(s.order_fragment());
        }
    }
    if levels.is_empty() {
        return String::new();
    }
    levels.push("r.combo_id ASC".to_string());
    levels.join(", ")
}

/// Allowlist of direct DB columns the frontend may sort by (maps frontend key
/// → DB column name, which happen to be identical here).
fn resolve_sort(col: &str, dir: &str) -> Option<SortSpec> {
    let dir_str: &'static str = match dir { "asc" => "ASC", _ => "DESC" };
    // Param column: frontend prefix is `p_`, DB expression is `(params->>'key')::numeric`.
    if let Some(param_key) = col.strip_prefix("p_") {
        // Only allow [a-z0-9_]+ to prevent injection.
        if param_key.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') && !param_key.is_empty() {
            return Some(SortSpec::ParamKey { key: param_key.to_string(), dir: dir_str });
        }
        return None;
    }
    let db_col: &'static str = match col {
        "score"               => "score",
        "n_fired"             => "n_fired",
        "n_closed"            => "n_closed",
        "n_open"              => "n_open",
        "win_rate"            => "win_rate",
        "total_pnl_sol"       => "total_pnl_sol",
        "mean_pnl_pct"        => "mean_pnl_pct",
        "median_pnl_pct"      => "median_pnl_pct",
        "p90_pnl_pct"         => "p90_pnl_pct",
        "best_pnl_pct"        => "best_pnl_pct",
        "worst_pnl_pct"       => "worst_pnl_pct",
        "std_pnl_pct"         => "std_pnl_pct",
        "profit_factor"       => "profit_factor",
        "expectancy_sol"      => "expectancy_sol",
        "avg_holding_secs"    => "avg_holding_secs",
        "median_holding_secs" => "median_holding_secs",
        "n_exit_take_profit"  => "n_exit_take_profit",
        "n_exit_stop_loss"    => "n_exit_stop_loss",
        "n_exit_trailing"     => "n_exit_trailing",
        "n_exit_stall"        => "n_exit_stall",
        "n_exit_time"         => "n_exit_time",
        "n_exit_liquidity"    => "n_exit_liquidity",
        "n_exit_next_kill"    => "n_exit_next_kill",
        "n_exit_dead"         => "n_exit_dead",
        "n_exit_open"         => "n_exit_open",
        _                     => return None,
    };
    Some(SortSpec::DirectCol { col: db_col, dir: dir_str })
}

// ---------------------------------------------------------------------------
// POST /api/strategies/sweeps — start a grouped DB-range sweep
// ---------------------------------------------------------------------------

pub async fn start_grouped_sweep(
    state: web::Data<Arc<LocalState>>,
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
    // can't drop the request future mid-sweep, AND so this POST returns as soon as
    // the run is admitted (run header persisted) instead of being held open for the
    // whole multi-minute sweep. Holding it open kept one of the browser's ~6
    // per-host HTTP/1.1 connections busy for the entire run, which could leave a
    // concurrent `POST .../sweeps/cancel` queued in the browser until the sweep
    // ended — so the cancel button appeared to do nothing. The job reports its
    // early result (admitted + `run_id`, or a pre-fold validation error) through
    // `early_tx`; the fold then runs detached, recoverable via `GET /api/jobs/status`
    // and SSE. `rt::spawn` keeps it on the worker (no `Send` bound, unlike
    // `tokio::spawn`); we drop the handle — dropping an `rt::spawn` task never
    // cancels it.
    let state = state.get_ref().clone();
    let (early_tx, early_rx) = tokio::sync::oneshot::channel::<HttpResponse>();
    actix_web::rt::spawn(run_grouped_sweep_job(state, b, tables, early_tx));
    early_rx.await.unwrap_or_else(|_| {
        HttpResponse::InternalServerError()
            .json(serde_json::json!({"error": "sweep task ended before reporting status"}))
    })
}

/// The detached body of a grouped sweep, spawned by [`start_grouped_sweep`] so
/// the work survives a client disconnect (the recovery contract behind
/// `/api/jobs/status`). Owns the single-flight release + progress reset + terminal
/// `SweepFinished` (via `Gate`), loads the corpus, runs the engine, and persists
/// run → groups → results. Assumes `sweep_running` was already claimed by the
/// caller.
///
/// Reports its *early* result through `early_tx` — a pre-fold validation error
/// (bad range, no tokens, …) or the admitted run's `run_id` once the run header is
/// persisted — then runs the (multi-minute) fold detached. The caller returns that
/// early result immediately so the POST connection is freed before the fold,
/// keeping a concurrent cancel from queueing behind it (see [`start_grouped_sweep`]).
/// Post-admission outcomes (done / cancel / error) are surfaced via the DB run
/// status + the `SweepFinished` SSE frame, not this function's (now `()`) return.
async fn run_grouped_sweep_job(
    state: Arc<LocalState>,
    b: StartGroupedSweepBody,
    tables: GroupedSweepTables,
    early_tx: tokio::sync::oneshot::Sender<HttpResponse>,
) {
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
        // Uncapped by default (full history) — analysis prices over every trade, so
        // the sweep matches single-rule simulate for high-volume tokens. Set
        // `SWEEP_PER_MINT_CAP` (≥1) only to trade fidelity for a lighter/faster run.
        per_mint_cap: crate::sweep::corpus::sweep_per_mint_cap(),
        // Moot when uncapped (whole history loads); still keeps the launch prefix if a
        // `SWEEP_PER_MINT_CAP` override is set, which the entry logic decides on (Rec 4).
        window: crate::sweep::corpus::TradeWindow::LaunchWindow,
        curve_only: b.curve_only,
        // Sweep resolves the trigger by index, not signature — no `tx_signature` needed.
        with_signatures: false,
    };

    // Load the corpus from the immutable Parquet lake via DuckDB (the sole sweep
    // corpus source). The lake embeds grouping fingerprints (`has_fingerprints`), so
    // no separate `tokens` lookup is needed; trades arrive as slim, wallet-interned
    // `CorpusTrade` buffers in the same launch-window order the engine expects.
    let root = crate::lake::lake_root();
    let src = LakeSource::new(root.clone());

    // Reuse the in-memory corpus cache when the last sweep loaded the **same
    // selection against the same lake version** (the hash folds both). This skips a
    // full DuckDB Parquet re-read on the common tune-the-rule loop — iterating
    // strategy params/filters over one corpus. A fresh `lake-export` moves the lake
    // version → hash miss → reload, so a stale corpus can never be served. The cache
    // stores the **unfiltered** selection corpus, so this run's own field/label
    // filters below re-apply either way.
    let expected_hash = src.selection_hash(&sel);
    let cached = {
        let cache = state.sweep_corpus_cache.read().await;
        cache.as_ref().filter(|c| c.corpus_hash == expected_hash).map(|c| {
            crate::sweep::corpus::Corpus {
                tokens: (*c.tokens).clone(),
                hash: c.corpus_hash.clone(),
                has_fingerprints: true,
            }
        })
    };
    let mut corpus = if let Some(c) = cached {
        tracing::info!(lake = %root.display(),
            "grouped sweep: corpus cache hit (selection + lake-version match) — skipping DuckDB load");
        c
    } else {
        tracing::info!(lake = %root.display(), "grouped sweep: corpus source = Parquet lake (DuckDB)");
        match src.load(&sel).await {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("grouped sweep: lake corpus load failed: {e}");
                let _ = early_tx.send(HttpResponse::InternalServerError()
                    .json(serde_json::json!({"error": e.to_string()})));
                return;
            }
        }
    };
    if corpus.tokens.is_empty() {
        let _ = early_tx.send(HttpResponse::BadRequest()
            .json(serde_json::json!({"error": "no tokens in that date range — widen the selection"})));
        return;
    }

    // A cancel requested during the corpus load lands here. The lake read is a
    // synchronous DuckDB query (not itself mid-flight abortable), but this is the
    // longest pre-fold phase — and for swing1 by far: it has no token-creation gate,
    // so its corpus is the WHOLE window (the fingerprint/field filters below run
    // in-memory AFTER the load), where tpsl1/2 pre-prune to a fingerprint. Without
    // this poll a cancel clicked during that load looked completely dead: the engine
    // only starts polling once the fold begins. Bail the instant the load returns so
    // the user's cancel takes effect instead of folding a corpus they abandoned.
    // Pre-admission (no run row / `run_id` yet), so nothing to mark cancelled — the
    // `Gate` still releases the single-flight lock and broadcasts the terminal
    // `SweepFinished { cancelled: true }` frame that clears the global indicator.
    if state.sweep_cancel.load(Ordering::Acquire) {
        tracing::info!("grouped sweep: cancelled during corpus load (pre-admission)");
        let _ = early_tx.send(HttpResponse::Ok()
            .json(serde_json::json!({ "run_id": null, "status": "cancelled" })));
        return;
    }

    // Option A: stash the **unfiltered** selection corpus (trades + fingerprints) in
    // the in-memory cache so `list_token_results` calls right after the sweep skip
    // all I/O. This MUST happen BEFORE the in-memory ix_labels/field filters below:
    // the cache is keyed only by `corpus_hash` (selection mints + trade counts), which
    // is identical for two runs over the same window/caps regardless of their filters.
    // If we cached the post-filter subset, a later run with the same selection but a
    // different (or no) filter would hit this cache and `apply_filters` could not
    // recover the tokens this run's filter dropped — yielding an empty token-results
    // table. Caching unfiltered mirrors Option B (the Parquet corpus is also
    // unfiltered), so `apply_filters` re-applies *this* run's own filters either way.
    // Clone is cheap — trades/wallets are `Arc`, fp is a small struct.
    {
        let cached_tokens = Arc::new(corpus.tokens.clone());
        let corpus_hash = corpus.hash.clone();
        let mut cache = state.sweep_corpus_cache.write().await;
        *cache = Some(SweepCorpusCache { corpus_hash, tokens: cached_tokens });
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
            let _ = early_tx.send(HttpResponse::BadRequest().json(serde_json::json!({
                "error": "no tokens match that instruction-label filter — adjust the labels or widen the selection"
            })));
            return;
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
                let _ = early_tx.send(HttpResponse::BadRequest().json(serde_json::json!({
                    "error": format!(
                        "no tokens match the '{field_str}' field filter — adjust the values or widen the selection"
                    )
                })));
                return;
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
    let run = GroupedSweepRun {
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
        buy_amount_sol: Some(b.buy_amount_sol),
    };
    let repo = GroupedSweepRepo::new(state.batch_db.clone(), tables);
    if let Err(e) = repo.insert_run(&run).await {
        tracing::error!("grouped sweep: insert_run failed: {e}");
        let _ = early_tx.send(HttpResponse::InternalServerError()
            .json(serde_json::json!({"error": e.to_string()})));
        return;
    }

    // The run is admitted and its header is persisted — report "started" so the
    // POST returns NOW and the fold below runs detached (see `start_grouped_sweep`
    // for why holding the connection open blocked cancels). `run_id` lets the
    // client jump straight to the run, which fills in live via the per-group writes
    // + SSE progress frames; the terminal state arrives over the `SweepFinished`
    // SSE frame. A pre-fold config error (bad axes / over-cap grid) below now drops
    // this admitted run instead of returning a 400 — a rare trade for never holding
    // the connection across the fold.
    let _ = early_tx.send(HttpResponse::Accepted().json(serde_json::json!({
        "run_id": run_id,
        "status": "started",
    })));

    // Single DB-writer task: the engine's per-group emits (which arrive out of
    // order from the cross-group small-group phase) are serialized through one
    // unbounded channel into one task, so concurrent folds never race the
    // connection. It drains until the sink (and thus the sender) drops when the
    // sweep ends, then returns the persisted-group tally. Unbounded so the sync
    // engine callback never blocks a rayon worker.
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<SweepWrite>();
    let writer = {
        let db = state.batch_db.clone();
        let writer_sse_tx = state.sse_tx.clone();
        let writer_strategy_id = b.strategy_id.clone();
        actix_web::rt::spawn(async move {
            let repo = GroupedSweepRepo::new(db, tables);
            let mut groups_done = 0i32;
            let mut total_groups = 0i32;
            while let Some(msg) = rx.recv().await {
                match msg {
                    SweepWrite::Begin { group_count, combo_params } => {
                        total_groups = group_count;
                        if let Err(e) = repo
                            .update_run_counts(run_id, group_count, combo_params.len() as i32)
                            .await
                        {
                            tracing::error!("grouped sweep: update_run_counts failed: {e}");
                        }
                        // Write the per-run combo→params dictionary once, up front
                        // (0007 dedup) — every results row JOINs back to it.
                        if let Err(e) = repo.insert_combos(run_id, &combo_params).await {
                            tracing::error!("grouped sweep: insert_combos failed: {e}");
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
    // Option C: build group-key → mints map from the fully-filtered corpus so
    // group_done can attach mint lists to each write unit without re-scanning.
    let group_mints_map = {
        let mut map: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        for t in &corpus.tokens {
            let key_str = group_key(&t.fp, &b.group_by).to_json().to_string();
            map.entry(key_str).or_default().push(t.mint.clone());
        }
        Arc::new(map)
    };

    // The sink hands each fully-folded group to the writer task. `run_grouped`
    // takes ownership, so it (and its sender) drop when the sweep ends — closing
    // the channel and letting the writer task finish.
    let sink: Arc<dyn GroupSink + Send + Sync> =
        Arc::new(HandlerSink { tx, group_mints: group_mints_map });

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
        b.buy_amount_sol,
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
                // The client already holds `run_id` (admitted at start); the
                // partial run + its `cancelled` status reach it via the runs-list
                // refresh on the `SweepFinished` SSE frame.
                return;
            }
            // Config errors (bad axes / over-cap grid) fail before any group
            // folds, leaving an empty placeholder run — drop it so it doesn't
            // litter the picker as a 0-group cancelled run. (The client briefly
            // navigated to this now-deleted run; the runs-list refresh on
            // `SweepFinished` drops it back to the newest surviving run.)
            tracing::error!("grouped sweep failed: {e}");
            if let Err(del) = repo.delete_run(run_id).await {
                tracing::error!("grouped sweep: cleanup of failed run failed: {del}");
            }
            return;
        }
    };

    // Full success — stamp the terminal status, authoritative counts, and the
    // resolved axes (only known now). The client picks the finalized run up via
    // the runs-list refresh on the `SweepFinished` SSE frame.
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
        return;
    }

    tracing::info!(
        run_id = %run_id,
        strategy = %run.strategy_id,
        tokens = run.token_count,
        groups = output.groups.len(),
        combos = output.combo_count,
        rss_mb = crate::sweep::obs::process_rss_mb(),
        elapsed_s = clock.elapsed_secs(),
        milestone = "done",
        "grouped sweep: done"
    );
}

/// Message to the single per-run DB-writer task. `Begin` carries the engine's
/// fixed group/combo counts (one per run); `Group` carries one completed group's
/// write unit. Boxed so the enum stays small despite the large group payload.
enum SweepWrite {
    Begin {
        group_count: i32,
        /// The per-run combo→`params` dictionary (`combo_params[combo_id]`),
        /// persisted once via `insert_combos` (migration `0007` dedup).
        combo_params: Vec<serde_json::Value>,
    },
    Group(Box<GroupedSweepGroupWrite>),
}

/// The [`GroupSink`] bridge from the (sync, rayon-worker) engine to the (async)
/// DB-writer task: each emit is forwarded over the unbounded channel. Sends are
/// best-effort — a closed channel (writer already gone) just drops the emit,
/// which only happens on shutdown.
struct HandlerSink {
    tx: tokio::sync::mpsc::UnboundedSender<SweepWrite>,
    /// Option C: maps serialized group-key JSON → mints for that group.
    /// Built from the corpus before the sweep so the group_done callback can
    /// attach mints to each write unit without touching the corpus again.
    group_mints: Arc<std::collections::HashMap<String, Vec<String>>>,
}

impl GroupSink for HandlerSink {
    fn begin(&self, group_count: usize, combo_params: &[serde_json::Value]) {
        let _ = self.tx.send(SweepWrite::Begin {
            group_count: group_count as i32,
            combo_params: combo_params.to_vec(),
        });
    }

    fn group_done(&self, group_index: usize, group: &GroupResult, combo_params: &[serde_json::Value]) {
        let key_str = group.key.to_json().to_string();
        let mints = self.group_mints.get(&key_str).cloned().unwrap_or_default();
        let write = group_to_write(group_index, group, combo_params, mints);
        let _ = self.tx.send(SweepWrite::Group(Box::new(write)));
    }
}

/// Flatten one fully-folded group into the repo's write unit. `group_index` is the
/// engine's deterministic order (largest group first). `fired_count` is the best
/// combo's `n_fired` — the sample size behind the group's headline pick.
/// `combo_params[combo_id]` is each ranked combo's param JSON.
/// `mints` is the Option-C mint list for this group (empty for old-schema writes).
fn group_to_write(
    group_index: usize,
    g: &GroupResult,
    combo_params: &[serde_json::Value],
    mints: Vec<String>,
) -> GroupedSweepGroupWrite {
    let param_at = |id: u32| -> serde_json::Value {
        combo_params.get(id as usize).cloned().unwrap_or(serde_json::Value::Null)
    };
    let fired_count = g
        .metrics
        .get(g.best_combo_id as usize)
        .map(|m| m.n_fired as i64)
        .unwrap_or(0);
    // Storage retention: persist only the metric-extreme survivors per group (plus
    // best_combo). The engine already evaluated every combo, so this only trims
    // INSERT rows/binds — no sweep-eval cost. Same pure fn the compaction probe
    // uses, so existing and future data are selected identically.
    let cfg = crate::sweep::retention::RetentionCfg::default();
    let keep = crate::sweep::retention::retained_combo_ids(&g.metrics, g.best_combo_id, &cfg);
    let results = g
        .metrics
        .iter()
        .filter(|m| keep.contains(&m.combo_id))
        .map(metrics_to_result)
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
        mints,
    }
}

/// Map one combo's aggregated metrics into a persistable row. `params` is **not**
/// stored per row anymore (deduped into the per-run `_combos` dictionary, migration
/// `0007`) — it's left `Null` here and JOINed back on read, so this skips the
/// per-combo param-JSON clone the old signature did.
fn metrics_to_result(m: &ComboMetrics) -> GroupedSweepResult {
    GroupedSweepResult {
        combo_id: m.combo_id as i32,
        params: serde_json::Value::Null,
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
        n_exit_next_kill: m.n_exit_next_kill as i32,
        n_exit_dead: m.n_exit_dead as i32,
        n_exit_open: m.n_exit_open as i32,
    }
}

// ---------------------------------------------------------------------------
// GET endpoints
// ---------------------------------------------------------------------------

/// `GET /api/strategies/sweeps?strategy_id=tpsl2&limit=50` — runs, newest first.
pub async fn list_runs(
    state: web::Data<Arc<LocalState>>,
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
    state: web::Data<Arc<LocalState>>,
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
    state: web::Data<Arc<LocalState>>,
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

    let order_by = build_order_by(&query);

    let repo = GroupedSweepRepo::new(state.db.clone(), tables);

    let total = match repo.count_results(run_id, group_id).await {
        Ok(n) => n,
        Err(e) => {
            tracing::error!("DB error counting grouped sweep results: {e}");
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "database error"}));
        }
    };

    let results = match repo.list_results_paged(run_id, group_id, limit, offset, &order_by).await {
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
/// polls between groups / in the per-token loop and bails on (the run then ends as
/// a partial `cancelled` run, announced via the `SweepFinished` SSE frame). A no-op
/// when no sweep is running. Logs every hit so the request reaching the backend is
/// positively confirmable in the log (vs. a cancel stuck queued in the browser).
pub async fn cancel_grouped_sweep(state: web::Data<Arc<LocalState>>) -> impl Responder {
    let running = state.sweep_running.load(Ordering::Acquire);
    tracing::info!(running, "grouped sweep: cancel requested");
    if running {
        state.sweep_cancel.store(true, Ordering::Release);
    }
    HttpResponse::Ok().json(serde_json::json!({ "cancelling": running }))
}

/// `DELETE /api/strategies/sweeps/{run_id}?strategy_id=tpsl2` — drop one run.
/// Groups + results cascade. 404 if the id isn't found.
pub async fn delete_run(
    state: web::Data<Arc<LocalState>>,
    path: web::Path<Uuid>,
    query: web::Query<StrategyQuery>,
) -> impl Responder {
    let tables = match registry::tables_for(&query.strategy_id) {
        Some(t) => t,
        None => return bad_strategy(&query.strategy_id),
    };
    let run_id = path.into_inner();
    // Use batch_db: CASCADE deletes the combos table (can be hundreds of thousands
    // of rows, ~1.7 GB) and the API pool's 8s statement_timeout kills it.
    match GroupedSweepRepo::new(state.batch_db.clone(), tables).delete_run(run_id).await {
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
    state: web::Data<Arc<LocalState>>,
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
    state: web::Data<Arc<LocalState>>,
    query: web::Query<PruneQuery>,
) -> impl Responder {
    let tables = match registry::tables_for(&query.strategy_id) {
        Some(t) => t,
        None => return bad_strategy(&query.strategy_id),
    };
    // Use batch_db: same cascade volume as delete_run; API pool's 8s timeout kills it.
    match GroupedSweepRepo::new(state.batch_db.clone(), tables)
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
    state: web::Data<Arc<LocalState>>,
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

    // Fetch the params JSON for the requested combo (from the per-run `_combos`
    // dictionary — `params` is run-level, keyed by `(run_id, combo_id)`).
    let params_json = match repo.get_combo_params(run_id, query.combo_id).await {
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

    // -----------------------------------------------------------------------
    // Load the group's token corpus — A → B → C cascade.
    //
    // A: in-memory cache (warm path — zero I/O).
    // B: Parquet cache with embedded fingerprints (disk, no DB fingerprint query).
    // C: targeted DB load using the stored group-mint list (cold + uncacheable).
    // Fallback: full corpus load + attach_fingerprints (old groups without mints).
    // -----------------------------------------------------------------------

    // Parse grouping fields once — needed for re-keying on the A/B/fallback paths.
    let grouping_fields: Vec<GroupField> =
        match serde_json::from_value(run.grouping_spec.clone()) {
            Ok(f) => f,
            Err(e) => {
                tracing::error!("Bad grouping_spec in run: {e}");
                return HttpResponse::InternalServerError()
                    .json(serde_json::json!({"error": "invalid grouping spec"}));
            }
        };
    let target_key = group.group_key.clone();

    // Helper: apply the run's ix_labels + field filters to an owned token vec.
    let apply_filters = |mut tokens: Vec<crate::sweep::corpus::CorpusToken>| -> Vec<crate::sweep::corpus::CorpusToken> {
        if let Some(want) = run
            .ix_labels_filter
            .as_ref()
            .and_then(|v| serde_json::from_value::<Vec<String>>(v.clone()).ok())
            .filter(|f| !f.is_empty())
            .map(normalize_label_vec)
        {
            tokens.retain(|t| t.fp.ix_labels == want);
        }
        if let Some(filters) = run.field_filters.as_ref().and_then(|v| {
            serde_json::from_value::<std::collections::HashMap<String, Vec<serde_json::Value>>>(
                v.clone(),
            )
            .ok()
        }) {
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
                tokens.retain(|t| matches_field_filter(&t.fp, field, allowed));
            }
        }
        tokens.retain(|t| group_key(&t.fp, &grouping_fields).to_json() == target_key);
        tokens
    };

    // --- Option A: in-memory corpus cache ---
    let tokens = {
        let cache = state.sweep_corpus_cache.read().await;
        if cache.as_ref().map(|c| c.corpus_hash.as_str()) == run.corpus_hash.as_deref() {
            tracing::debug!("token-results: Option A cache hit");
            Some(apply_filters((*cache.as_ref().unwrap().tokens).clone()))
        } else {
            None
        }
    };

    let tokens = if let Some(t) = tokens {
        t
    } else {
        // Option A missed — reload from the Parquet lake (DuckDB), the sole corpus
        // source. Prefer the group's stored mints for a targeted load; otherwise fall
        // back to the run's full selection window. The lake embeds fingerprints, so no
        // separate `attach_fingerprints` pass is needed.
        let group_mints = repo.get_group_mints(group_id).await.unwrap_or(None);
        let sel = Selection {
            mints: group_mints.filter(|m| !m.is_empty()),
            created_after: run.created_after,
            created_before: run.created_before,
            curve_only: run.curve_only,
            token_cap: run.token_cap.map(|n| n as usize).unwrap_or(5_000),
            window: crate::sweep::corpus::TradeWindow::LaunchWindow,
            per_mint_cap: sweep_per_mint_cap(),
            with_signatures: false,
        };
        let root = crate::lake::lake_root();
        match LakeSource::new(root).load(&sel).await {
            Ok(c) => apply_filters(c.tokens),
            Err(e) => {
                tracing::error!("Failed to load corpus for token-results from lake: {e}");
                return HttpResponse::InternalServerError()
                    .json(serde_json::json!({"error": "corpus load failed"}));
            }
        }
    };

    if tokens.is_empty() {
        return HttpResponse::Ok().json(serde_json::json!([]));
    }

    // Re-simulate on a blocking thread (CPU-bound but short: one combo × N tokens).
    let strategy_id = query.strategy_id.clone();
    let buy_amount_sol = run.buy_amount_sol.unwrap_or(1.0);
    let result = tokio::task::spawn_blocking(move || {
        registry::simulate_one_combo(&strategy_id, &tokens, &params_json, buy_amount_sol)
    })
    .await;

    let mut rows = match result {
        Ok(Ok(rows)) => rows,
        Ok(Err(e)) => {
            tracing::error!("Single-combo simulation failed: {e}");
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": format!("simulation failed: {e}")}));
        }
        Err(e) => {
            tracing::error!("spawn_blocking panicked: {e}");
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "internal error"}));
        }
    };

    // Resolve the real base58 entry/exit signatures the chart/table need to link a
    // fill to its candle. The sweep walks a slim `CorpusTrade` with no signature, so
    // the fills only carry the slot; look the signatures up from the `trades` table
    // by (mint, slot, side). Entry fills are buys, exit fills sells.
    let fill_keys: Vec<(String, u64, bool)> = rows
        .iter()
        .flat_map(|r| {
            let entry = r.entry_slot.map(|s| (r.mint_address.clone(), s, true));
            let exit = r.exit_slot.map(|s| (r.mint_address.clone(), s, false));
            entry.into_iter().chain(exit)
        })
        .collect();
    if !fill_keys.is_empty() {
        let repo = crate::storage::repositories::trade_repo::TradeRepo::new(state.db.clone());
        match repo.resolve_fill_signatures(&fill_keys).await {
            Ok(sigs) => {
                for row in &mut rows {
                    if let Some(s) = row.entry_slot {
                        row.entry_tx = sigs.get(&(row.mint_address.clone(), s, true)).cloned();
                    }
                    if let Some(s) = row.exit_slot {
                        row.exit_tx = sigs.get(&(row.mint_address.clone(), s, false)).cloned();
                    }
                }
            }
            Err(e) => tracing::warn!("token-results: fill signature resolve failed: {e}"),
        }
    }

    // Batch-join token metadata via the shared enrichment SSOT (the same source the
    // Matched / Positions / Simulated tables use) so the frontend table shows the
    // full token column set with no per-row or client-side fetch. `created_at`/
    // `ath_price` are row-owned (excluded from `token`); set them off the row too.
    let mints: Vec<String> = rows.iter().map(|r| r.mint_address.clone()).collect();
    if !mints.is_empty() {
        match trading_core::storage::token_enrichment::fetch_by_mints(&state.db, &mints).await {
            Ok(meta_rows) => {
                let by_mint: std::collections::HashMap<String, _> =
                    meta_rows.into_iter().map(|r| (r.mint_address.clone(), r)).collect();
                for row in &mut rows {
                    if let Some(m) = by_mint.get(&row.mint_address) {
                        row.created_at = Some(m.token_created_at.to_rfc3339());
                        row.ath_price = m.ath_price;
                        row.token = m.into();
                    }
                }
            }
            Err(e) => tracing::warn!("token-results: enrichment fetch failed: {e}"),
        }
    }

    HttpResponse::Ok().json(rows)
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
/// `TokenProgramId` isn't offered in the frontend filter UI.
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
        MaxCostLamports => {
            let v = fp.max_cost_lamports;
            allowed.iter().any(|a| a.as_i64().map(|x| Some(x) == v).unwrap_or(false))
        }
        SpendableLamportsIn => {
            let v = fp.spendable_lamports_in;
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
        FirstSlotBuySol => {
            let v = fp.first_slot_buy_sol;
            allowed.iter().any(|a| {
                a.as_f64()
                    .map(|x| v.map(|y| (x - y).abs() < 1e-9).unwrap_or(false))
                    .unwrap_or(false)
            })
        }
        FirstSlotSellSol => {
            let v = fp.first_slot_sell_sol;
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
        TokenProgramId | IxLabels => false,
    }
}

#[cfg(test)]
mod order_by_tests {
    use super::order_by_from;

    #[test]
    fn multi_key_builds_all_levels_with_stable_tiebreak() {
        // The reported bug: the secondary key never reached SQL, so rows tied on
        // the primary (all FIRED = 6) fell back to heap order. Both levels must
        // appear, in order, followed by the stable `combo_id` tiebreak.
        let order = order_by_from(Some("n_fired:desc,win_rate:desc"), None, None);
        assert_eq!(
            order,
            "r.n_fired DESC NULLS LAST, r.win_rate DESC NULLS LAST, r.combo_id ASC"
        );
    }

    #[test]
    fn param_column_level_uses_jsonb_expression() {
        let order = order_by_from(Some("p_exit_take_profit:asc,score:desc"), None, None);
        assert_eq!(
            order,
            "(c.params->>'exit_take_profit')::numeric ASC NULLS LAST, \
             r.score DESC NULLS LAST, r.combo_id ASC"
        );
    }

    #[test]
    fn unknown_levels_are_dropped() {
        let order = order_by_from(Some("bogus:desc,n_closed:asc"), None, None);
        assert_eq!(order, "r.n_closed ASC NULLS LAST, r.combo_id ASC");
        // All-unknown → empty (repo applies its score default).
        assert!(order_by_from(Some("bogus:desc"), None, None).is_empty());
    }

    #[test]
    fn legacy_single_pair_fallback() {
        let order = order_by_from(None, Some("win_rate"), Some("asc"));
        assert_eq!(order, "r.win_rate ASC NULLS LAST, r.combo_id ASC");
    }

    #[test]
    fn empty_sort_yields_empty_default() {
        assert!(order_by_from(None, None, None).is_empty());
        assert!(order_by_from(Some(""), None, None).is_empty());
    }
}
