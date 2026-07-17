//! Strategy registry — the **one** place a strategy is wired into the grouped
//! sweep. It maps a `strategy_id` to (a) its per-strategy DB table triple and
//! (b) its concrete sweep entry point. The handler and repo are otherwise fully
//! generic (table-name- and data-driven), so adding "swing" later means: write a
//! `strategies/swing.rs` (`Strategy` + `ParamSpace` + `AxesSpec`), add its tables
//! + a match arm here, and a `lab/migrations/NNNN_*.sql` file — nothing else changes.
//!
//! The CPU-heavy sweep runs in a bounded rayon pool inside `spawn_blocking` so it
//! can never starve the live trading hot path (ingest / sell-confirm).

use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;

use crate::models::grouped_sweep::ComboTokenResult;
use crate::models::{Swing1Rule, Tpsl1Rule, Tpsl2Rule};
use crate::storage::repositories::grouped_sweep_repo::GroupedSweepTables;
use crate::sweep::corpus::{Corpus, CorpusToken};
use crate::sweep::engine::fill_outcomes;
use crate::sweep::grouped_engine::{run_grouped_with_refine, CoverageFloor, GroupResult, GroupSink};
use crate::sweep::grouping::GroupField;
use crate::sweep::progress::SweepObserver;
use crate::sweep::strategies::swing1::{
    AxesSpec as Swing1AxesSpec, Swing1Axes, Swing1Strategy,
};
use crate::sweep::strategies::tpsl1::{
    AxesSpec as Tpsl1AxesSpec, Tpsl1Axes, Tpsl1Strategy,
};
use crate::sweep::strategies::tpsl2::{AxesSpec, Tpsl2Axes, Tpsl2Strategy};
use crate::sweep::generic::{AxesModel, AxesRequest, GenericSweepStrategy};
use crate::sweep::strategy::{ExitCode, ParamSpace, RefineSpec, SweepMethod};

/// Notional (SOL) a grouped-sweep run prices every combo's round-trip at when the
/// request omits `buy_amount_sol` — the single fallback both `start_grouped_sweep`
/// (`default_buy_amount_sol`) and the token-results drill-in replay
/// (`run.buy_amount_sol.unwrap_or(..)`) share, so a stored run and its later
/// re-simulation can never quantize at two different notionals.
///
/// This is **not** wired to any saved `strategy_rules` row — a sweep run explores
/// many candidate combos, not one rule — so a sweep group and a single-rule
/// simulate of the "same" combo only produce identical PnL when the caller
/// explicitly sets this to the rule's `buy_amount_sol` (parity plan A2; see
/// `docs/plans/sweep/sweep-sim-parity.md`'s comparison-hygiene note until Phase 3
/// makes simulate replay the sweep group directly).
pub const SWEEP_DEFAULT_BUY_AMOUNT_SOL: f64 = 1.0;

/// Minimal no-op observer for single-combo re-simulation (no progress to report).
struct Noop;
impl SweepObserver for Noop {
    fn set_total(&self, _: usize) {}
    fn token_done(&self) {}
    fn cancelled(&self) -> bool { false }
}

/// Default cap on the param combos a single grouped sweep evaluates **per group**.
/// Bounds the `groups × combos × tokens` work; the handler rejects a full grid
/// whose product exceeds this before any sweep runs, and random/LHS draws are
/// clamped to it. A run may raise this per-request (up to [`HARD_MAX_COMBOS`]).
pub const MAX_COMBOS: usize = 100_000;

/// Absolute backstop on the per-request combo cap. A run can opt into more than
/// [`MAX_COMBOS`] (sweeps are infrequent and the caller accepts the wait), but
/// never past this — so a fat-fingered override still can't run away with the
/// `groups × combos × tokens` work or monopolise the bounded rayon pool.
pub const HARD_MAX_COMBOS: usize = 1_000_000;

/// Resolve the effective per-group combo cap for a run: the request override if
/// given, else the default, clamped to the hard backstop (and ≥ 1).
fn effective_cap(max_combos: Option<usize>) -> usize {
    max_combos.unwrap_or(MAX_COMBOS).clamp(1, HARD_MAX_COMBOS)
}

/// Default ceiling (MB) on the per-combo fold accumulators a single run may hold at
/// once. The sweep folds combos in batches of `combo_batch_size` so peak ≈
/// `threads × BATCH × sizeof(ComboAgg)`; this budget is what sizes `BATCH` (Phase
/// 2.5). A combo set too big to hold at once is swept in sequential batches rather
/// than rejected (the prior behaviour) — `HARD_MAX_COMBOS` still bounds the work.
/// Override with `SWEEP_MEMORY_BUDGET_MB`.
///
/// A bigger batch = fewer series-rebuild passes (`n_batches` ↓), and the series
/// precompute (not this budget) is the sweep's real memory cost, now bounded ∝
/// trades by the sparse grid (plan §P2). `lab` is the analysis workstation — it
/// never ships to the 4 GB EC2 box (only `live` does), so this is sized for the
/// workstation: 2 GB batches HARD_MAX_COMBOS (~600 MB of `ComboAgg` at 2 threads)
/// in a single pass. The old 256 MB default was conservatively sized against the
/// server, which this budget never runs on. Admission (plan §P4) accounts the
/// precompute separately via `SWEEP_ADMISSION_BUDGET_MB` — the two budgets are
/// deliberately not shared (reusing one caused the earlier false-OOM regression).
const DEFAULT_SWEEP_MEMORY_BUDGET_MB: usize = 2048;

/// The fold-accumulator memory budget in bytes (`SWEEP_MEMORY_BUDGET_MB` or the
/// default), saturating so a fat-fingered override can't wrap. `pub(crate)` so the
/// engine's combo-batching ([`crate::sweep::engine::combo_batch_size`]) sizes a
/// batch against the same budget this guard enforces.
pub(crate) fn sweep_memory_budget_bytes() -> u64 {
    std::env::var("SWEEP_MEMORY_BUDGET_MB")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|&m| m >= 1)
        .unwrap_or(DEFAULT_SWEEP_MEMORY_BUDGET_MB)
        .saturating_mul(1024 * 1024) as u64
}

/// Default ceiling (MB) on the **series precompute** transient a generic sweep may
/// hold — the peak is `threads × the largest token's sparse series`, which the
/// admission guard (plan §P4) estimates up front and rejects against rather than
/// letting a pathological rule×corpus abort mid-run. Deliberately **separate** from
/// the fold budget above: conflating the two is what caused the earlier false-OOM
/// regression. The sparse grid bounds a series ∝ trades, so at workstation scale
/// this is generous headroom — it only trips on a genuinely oversized run. Override
/// with `SWEEP_ADMISSION_BUDGET_MB`.
const DEFAULT_SWEEP_ADMISSION_BUDGET_MB: usize = 4096;

/// The generic-sweep precompute admission budget in bytes
/// (`SWEEP_ADMISSION_BUDGET_MB` or the default), saturating.
pub(crate) fn sweep_admission_budget_bytes() -> u64 {
    std::env::var("SWEEP_ADMISSION_BUDGET_MB")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|&m| m >= 1)
        .unwrap_or(DEFAULT_SWEEP_ADMISSION_BUDGET_MB)
        .saturating_mul(1024 * 1024) as u64
}

/// Default host RAM (MB) left free for the OS + desktop UI during series-precompute
/// admission. Override with `SWEEP_RAM_RESERVE_MB`.
const DEFAULT_SWEEP_RAM_RESERVE_MB: usize = 4096;

/// Host RAM reserve in bytes (`SWEEP_RAM_RESERVE_MB` or the default).
fn sweep_ram_reserve_bytes() -> u64 {
    std::env::var("SWEEP_RAM_RESERVE_MB")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|&m| m >= 1)
        .unwrap_or(DEFAULT_SWEEP_RAM_RESERVE_MB)
        .saturating_mul(1024 * 1024) as u64
}

/// Effective series-precompute admission ceiling: the configured
/// `SWEEP_ADMISSION_BUDGET_MB`, further capped by `available_ram − reserve` when
/// host memory is readable. Returns `(budget_bytes, host_total_mb, host_avail_mb)`.
fn effective_admission_budget_bytes() -> (u64, Option<u64>, Option<u64>) {
    let configured = sweep_admission_budget_bytes();
    match crate::sweep::obs::host_memory_bytes() {
        Some((total, available)) => {
            let reserve = sweep_ram_reserve_bytes();
            let host_ceiling = available.saturating_sub(reserve);
            let budget = configured.min(host_ceiling);
            (
                budget,
                Some(total / (1024 * 1024)),
                Some(available / (1024 * 1024)),
            )
        }
        None => (configured, None, None),
    }
}

/// Largest thread count `≤ preferred` whose estimated peak
/// (`threads × max_series_bytes`) fits `budget`. `None` if even 1 thread overflows.
fn threads_fitting_admission(preferred: usize, max_series_bytes: usize, budget: u64) -> Option<usize> {
    let preferred = preferred.max(1);
    if max_series_bytes == 0 {
        return Some(preferred);
    }
    let per = max_series_bytes as u64;
    if per > budget {
        return None;
    }
    let max_by_budget = (budget / per) as usize;
    Some(preferred.min(max_by_budget).max(1))
}

// ---------------------------------------------------------------------------
// Per-strategy wiring (the table set + the supported-id list)
// ---------------------------------------------------------------------------

/// TPSL2's own grouped-sweep tables (separate from every other strategy's, per
/// the per-strategy-tables design). Mirror these names in the `0004` migration.
const TPSL2_TABLES: GroupedSweepTables = GroupedSweepTables {
    runs: "tpsl2_grouped_sweep_runs",
    groups: "tpsl2_grouped_sweep_groups",
    results: "tpsl2_grouped_sweep_results",
    combos: "tpsl2_grouped_sweep_combos",
};

/// TPSL1's own grouped-sweep tables (same shape as TPSL2's, separate per the
/// per-strategy-tables design). Mirror these names in the `0004` migration.
const TPSL1_TABLES: GroupedSweepTables = GroupedSweepTables {
    runs: "tpsl1_grouped_sweep_runs",
    groups: "tpsl1_grouped_sweep_groups",
    results: "tpsl1_grouped_sweep_results",
    combos: "tpsl1_grouped_sweep_combos",
};

/// swing1's own grouped-sweep tables (same four-table shape, separate per the
/// per-strategy-tables design). Mirror these names in the `0002` migration. The
/// `_results` table additionally carries `n_exit_next_kill`.
const SWING1_TABLES: GroupedSweepTables = GroupedSweepTables {
    runs: "swing_1_grouped_sweep_runs",
    groups: "swing_1_grouped_sweep_groups",
    results: "swing_1_grouped_sweep_results",
    combos: "swing_1_grouped_sweep_combos",
};

/// The redesigned generic engine's single (unprefixed) grouped-sweep table set —
/// the one set the three legacy per-strategy sets collapse into (plan §5.4).
/// Created in `lab/migrations/0003_generic_grouped_sweep.sql`.
const GENERIC_TABLES: GroupedSweepTables = GroupedSweepTables {
    runs: "grouped_sweep_runs",
    groups: "grouped_sweep_groups",
    results: "grouped_sweep_results",
    combos: "grouped_sweep_combos",
};

/// The DB table triple for a strategy's grouped sweeps, or `None` for an unknown
/// / not-yet-wired strategy.
pub fn tables_for(strategy_id: &str) -> Option<GroupedSweepTables> {
    match strategy_id {
        "generic" => Some(GENERIC_TABLES),
        "tpsl1" => Some(TPSL1_TABLES),
        "tpsl2" => Some(TPSL2_TABLES),
        "swing_1" => Some(SWING1_TABLES),
        _ => None,
    }
}

/// Strategy ids with a grouped-sweep implementation (for error messages / UI).
pub fn strategy_ids() -> &'static [&'static str] {
    &["generic", "tpsl1", "tpsl2", "swing_1"]
}

// ---------------------------------------------------------------------------
// Sweep output (generic — strategy-blind from here on)
// ---------------------------------------------------------------------------

/// The strategy-agnostic result of a grouped sweep, ready for the generic repo:
/// the per-group sweep results plus the resolved axes JSON to store on the run.
/// The combo→param-JSON map is no longer carried here — the engine's [`GroupSink`]
/// builds each group's write incrementally (Phase 4), so only the final combo
/// count survives to stamp on the run.
pub struct GroupedSweepOutput {
    /// Number of param combos evaluated per group.
    pub combo_count: usize,
    /// The resolved axes (after defaults + dedup) for storage / re-run.
    pub axes_json: Value,
    /// One entry per surviving group (≥ `min_tokens` tokens).
    pub groups: Vec<GroupResult>,
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

/// Run the grouped sweep for `strategy_id` over an already-loaded, already-
/// fingerprinted `corpus`. The sweep needs no DB rule — the base rule is
/// synthesized in-process (see `sweep_base_rule_*`); the CPU sweep runs on a
/// bounded blocking pool, which is the only reason this is async.
///
/// `coarse_observer` reports the coarse pass (only relevant for `refine` runs);
/// `observer` reports the final sweep pass. Pass separate observers with distinct
/// `phase` labels so the frontend can show one bar per phase.
// One thin dispatch fn called once from the handler — bundling these into a
// struct would only add an indirection for a single call site.
#[allow(clippy::too_many_arguments)]
pub async fn run_grouped(
    strategy_id: &str,
    axes_json: Value,
    method: SweepMethod,
    refine: Option<RefineSpec>,
    corpus: Corpus,
    fields: Vec<GroupField>,
    // Per-run bucket width (SOL) for the continuous SOL grouping fields — the same
    // width the created rule's matcher + the creation-stats dashboard use, so
    // "what you swept = what you run". Discrete fields ignore it.
    width: f64,
    min_tokens: usize,
    floor: CoverageFloor,
    max_combos: Option<usize>,
    buy_amount_sol: f64,
    coarse_observer: Arc<dyn SweepObserver + Send>,
    observer: Arc<dyn SweepObserver + Send>,
    sink: Arc<dyn GroupSink + Send + Sync>,
) -> Result<GroupedSweepOutput> {
    match strategy_id {
        "generic" => {
            sweep_generic(
                axes_json, method, refine, corpus, fields, width, min_tokens, floor, max_combos,
                buy_amount_sol, coarse_observer, observer, sink,
            )
            .await
        }
        "tpsl1" => {
            sweep_tpsl1(
                axes_json, method, refine, corpus, fields, width, min_tokens, floor, max_combos,
                buy_amount_sol, coarse_observer, observer, sink,
            )
            .await
        }
        "tpsl2" => {
            sweep_tpsl2(
                axes_json, method, refine, corpus, fields, width, min_tokens, floor, max_combos,
                buy_amount_sol, coarse_observer, observer, sink,
            )
            .await
        }
        "swing_1" => {
            sweep_swing1(
                axes_json, method, refine, corpus, fields, width, min_tokens, floor, max_combos,
                buy_amount_sol, coarse_observer, observer, sink,
            )
            .await
        }
        other => bail!(
            "strategy '{other}' has no grouped-sweep implementation yet (supported: {:?})",
            strategy_ids()
        ),
    }
}

/// Detected logical CPU count (fallback 1).
fn detect_cores() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}

/// Rayon threads for the in-process sweep. `lab` is the **analysis box** — it
/// runs no ingest/gRPC/trader, so a sweep never co-runs the live trading hot
/// path. Default is **half the logical cores** so the desktop OS/UI stay
/// responsive while a sweep runs (e.g. 8 on a 16-core workstation). Override
/// explicitly with `SWEEP_RAYON_THREADS` for walk-away max throughput (e.g.
/// `cores − 1`). Always ≥1. Host-RAM admission may still lower the count further.
fn bounded_threads() -> usize {
    if let Some(n) = std::env::var("SWEEP_RAYON_THREADS")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|&n| n >= 1)
    {
        return n;
    }
    (detect_cores() / 2).max(1)
}

// ---------------------------------------------------------------------------
// Generic (redesigned-engine) strategy entry point
// ---------------------------------------------------------------------------

/// The redesigned engine's grouped sweep (plan §5.4): resolve the request's axes
/// against the metric registry, enumerate combos into `RuleParams`, and run the
/// precompute-then-scan [`GenericSweepStrategy`] through the same partition +
/// persistence the legacy strategies use. Deadness is judged against the run's
/// wall-clock `now` (matching live / single-rule simulate).
#[allow(clippy::too_many_arguments)]
async fn sweep_generic(
    axes_json: Value,
    method: SweepMethod,
    refine: Option<RefineSpec>,
    corpus: Corpus,
    fields: Vec<GroupField>,
    width: f64,
    min_tokens: usize,
    floor: CoverageFloor,
    max_combos: Option<usize>,
    buy_amount_sol: f64,
    coarse_observer: Arc<dyn SweepObserver + Send>,
    observer: Arc<dyn SweepObserver + Send>,
    sink: Arc<dyn GroupSink + Send + Sync>,
) -> Result<GroupedSweepOutput> {
    let cap = effective_cap(max_combos);

    // Resolve + validate the axes against the metric registry (a typo'd group /
    // metric / operator, or a dynamic metric missing its window, is a hard error).
    let req: AxesRequest = serde_json::from_value(axes_json.clone())
        .context("invalid generic axes request")?;
    let model = AxesModel::resolve(&req).map_err(|e| anyhow!("axes: {e}"))?;

    // Grid guard: reject an explosive full grid before doing any sweep work.
    if matches!(method, SweepMethod::Grid) {
        let n = model.combo_count();
        if n > cap {
            bail!("grid has {n} combos, over the {cap} cap — narrow the axes, raise max_combos, or use random:N");
        }
    }
    // The (validated) request axes are echoed back for storage / re-run below.
    let strategy = GenericSweepStrategy::new(model, buy_amount_sol, chrono::Utc::now());
    let mut params = strategy.sample(method);
    if params.is_empty() {
        bail!("param space is empty");
    }
    if params.len() > cap {
        tracing::warn!(combos = params.len(), cap, "grouped sweep: clamping sampled combos to cap");
        params.truncate(cap);
    }

    let preferred_threads = bounded_threads();
    let cores = detect_cores();

    // Admission (plan §P4): the per-token series precompute — not the fold
    // accumulators — is the sweep's real memory cost. Estimate the worst-case
    // transient (`threads × the largest token's sparse series`) and either lower
    // threads to fit or reject up front. Budget = min(configured admission,
    // available host RAM − reserve) so the desktop stays usable.
    let max_series_bytes = corpus
        .tokens
        .iter()
        .map(|t| strategy.series_bytes_estimate(t))
        .max()
        .unwrap_or(0);
    let (admission_budget, host_total_mb, host_avail_mb) = effective_admission_budget_bytes();
    let mb = |b: u64| b / (1024 * 1024);
    let threads = match threads_fitting_admission(preferred_threads, max_series_bytes, admission_budget)
    {
        Some(n) => n,
        None => {
            bail!(
                "estimated series precompute for largest token ({} MB) exceeds the \
                 {} MB admission budget even at 1 thread — narrow the corpus or axes, \
                 or raise SWEEP_ADMISSION_BUDGET_MB / lower SWEEP_RAM_RESERVE_MB",
                mb(max_series_bytes as u64),
                mb(admission_budget)
            );
        }
    };
    if threads < preferred_threads {
        tracing::warn!(
            preferred_threads,
            threads,
            max_token_series_mb = mb(max_series_bytes as u64),
            budget_mb = mb(admission_budget),
            "generic sweep: lowered rayon threads to fit series-precompute admission budget"
        );
    }
    let peak_bytes = (threads as u64).saturating_mul(max_series_bytes as u64);
    tracing::info!(
        cores,
        preferred_threads,
        threads,
        max_token_series_mb = mb(max_series_bytes as u64),
        peak_precompute_mb = mb(peak_bytes),
        budget_mb = mb(admission_budget),
        configured_admission_mb = mb(sweep_admission_budget_bytes()),
        ram_reserve_mb = mb(sweep_ram_reserve_bytes()),
        host_total_mb,
        host_avail_mb,
        rss_mb = crate::sweep::obs::process_rss_mb(),
        "generic sweep: series-precompute admission estimate"
    );

    let (combo_count, groups) = tokio::task::spawn_blocking(
        move || -> Result<(usize, Vec<GroupResult>)> {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .thread_name(|i| format!("grouped-sweep-{i}"))
                .stack_size(8 * 1024 * 1024)
                .build()
                .map_err(|e| anyhow!("rayon pool build failed: {e}"))?;
            pool.install(|| {
                let (final_params, groups) = run_grouped_with_refine(
                    &strategy, params, refine, &corpus, &fields, width, min_tokens, floor, cap,
                    coarse_observer.as_ref(), observer.as_ref(), sink.as_ref(),
                )?;
                Ok((final_params.len(), groups))
            })
        },
    )
    .await??;

    Ok(GroupedSweepOutput { combo_count, axes_json, groups })
}

// ---------------------------------------------------------------------------
// TPSL2 strategy entry point
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
async fn sweep_tpsl2(
    axes_json: Value,
    method: SweepMethod,
    refine: Option<RefineSpec>,
    corpus: Corpus,
    fields: Vec<GroupField>,
    // Per-run bucket width (SOL) for the continuous SOL grouping fields (see
    // `run_grouped`). Threaded verbatim into the engine's `partition`.
    width: f64,
    min_tokens: usize,
    floor: CoverageFloor,
    max_combos: Option<usize>,
    buy_amount_sol: f64,
    coarse_observer: Arc<dyn SweepObserver + Send>,
    observer: Arc<dyn SweepObserver + Send>,
    sink: Arc<dyn GroupSink + Send + Sync>,
) -> Result<GroupedSweepOutput> {
    let cap = effective_cap(max_combos);

    // Resolve the page-supplied axes (omitted/empty axes fall back to defaults).
    let spec: AxesSpec = serde_json::from_value(axes_json).context("invalid TPSL2 axes spec")?;
    let axes = Tpsl2Axes::from_spec(&spec);

    // Grid guard: reject an explosive full grid before doing any sweep work.
    if matches!(method, SweepMethod::Grid) {
        let n = axes.combo_count();
        if n > cap {
            bail!("grid has {n} combos, over the {cap} cap — narrow the axes, raise max_combos, or use random:N");
        }
    }
    // Store the resolved axes (post-defaults/dedup) so the run is reproducible.
    let axes_json = serde_json::to_value(&axes).context("serializing resolved TPSL2 axes")?;

    // Judge deadness against the run's wall-clock `now` (matching live), captured once
    // so it's uniform across the whole run. Over the sealed lake `now` ≫ every last
    // trade, so a token that stopped trading is booked `Dead` not `Open`.
    let strategy = Tpsl2Strategy::new(sweep_base_rule_tpsl2(buy_amount_sol), axes)
        .with_as_of(chrono::Utc::now());
    let mut params = strategy.sample(method);
    if params.is_empty() {
        bail!("param space is empty");
    }
    // Random/LHS draws can request any `n`; clamp to the cap (grid is pre-checked).
    if params.len() > cap {
        tracing::warn!(
            combos = params.len(),
            cap,
            "grouped sweep: clamping sampled combos to cap"
        );
        params.truncate(cap);
    }

    let threads = bounded_threads();
    // No longer rejects on a large combo set: the engine sweeps the combos in
    // budget-sized batches (Phase 2.5, `combo_batch_size`), so accumulator memory is
    // bounded by `threads × BATCH × ComboAgg` regardless of total combo count. `cap`
    // (≤ HARD_MAX_COMBOS) still bounds the *work*.

    // The coarse→refine driver may grow the combo set (a neighborhood around each
    // group's survivors), so the final combo count is only known after the sweep.
    // The per-combo param JSON is emitted by the sink during the fold (Phase 4), so
    // the task only needs to surface the final count here.
    let (combo_count, groups) = tokio::task::spawn_blocking(
        move || -> Result<(usize, Vec<GroupResult>)> {
            // Bound the pool so the sweep can't saturate every core in the live
            // process; `install` makes the inner `par_iter` use this pool.
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .thread_name(|i| format!("grouped-sweep-{i}"))
                // Default Windows PE stack (1 MB) is too small for the hot loop:
                // fill_outcomes → resolve_entry/exit → exit ladder fns stack up
                // enough frames to overflow on large corpora. 8 MB matches the
                // Linux thread default and gives comfortable headroom.
                .stack_size(8 * 1024 * 1024)
                .build()
                .map_err(|e| anyhow!("rayon pool build failed: {e}"))?;
            pool.install(|| {
                let (final_params, groups) = run_grouped_with_refine(
                    &strategy,
                    params,
                    refine,
                    &corpus,
                    &fields,
                    width,
                    min_tokens,
                    floor,
                    cap,
                    coarse_observer.as_ref(),
                    observer.as_ref(),
                    sink.as_ref(),
                )?;
                Ok((final_params.len(), groups))
            })
        },
    )
    .await??;

    Ok(GroupedSweepOutput { combo_count, axes_json, groups })
}

/// Synthetic TPSL2 base rule the swept params overlay. Only `buy_amount_sol` is
/// meaningful (the caller's request value, [`SWEEP_DEFAULT_BUY_AMOUNT_SOL`] when
/// omitted — see its doc for the parity caveat); every other field is either
/// overwritten by the swept axes or unused in the grouped sweep, so we build it
/// in-memory instead of requiring a DB template rule.
fn sweep_base_rule_tpsl2(buy_amount_sol: f64) -> Tpsl2Rule {
    Tpsl2Rule::new(
        "sweep-synthetic-base".into(),
        None,                  // p_token_initial_buy_sol — unused in sweep
        None,                  // p_token_cu_limit        — unused in sweep
        None,                  // p_token_cu_price        — unused in sweep
        serde_json::json!([]), // p_token_ix_labels       — unused in sweep
        "paper".into(),        // trade_mode              — unused in sweep
        buy_amount_sol,
        0.0,                   // p_exit_take_profit      — overlaid per combo
        0.0,                   // p_exit_stop_loss        — overlaid per combo
        None,                  // p_token_max_sol_cost    — unused in sweep
        None,                  // p_token_spendable_sol_in — unused in sweep
        None,                  // p_max_concurrent_tokens — unused in grouped sweep
        None,                  // p_max_total_tokens      — unused in grouped sweep
        None,                  // bucket_width_sol           — default 0.1; run width wired Stage 3 in sweep
        None,                  // p_exit_trailing_stop_pct — overlaid per combo
        None,                  // p_exit_time_stop_secs    — overlaid per combo
        None,                  // p_exit_stall_secs        — overlaid per combo
        None,                  // p_exit_liquidity_drop_pct — overlaid per combo
    )
}

// ---------------------------------------------------------------------------
// swing1 strategy entry point
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
async fn sweep_swing1(
    axes_json: Value,
    method: SweepMethod,
    refine: Option<RefineSpec>,
    corpus: Corpus,
    fields: Vec<GroupField>,
    // Per-run bucket width (SOL) for the continuous SOL grouping fields (see
    // `run_grouped`). Threaded verbatim into the engine's `partition`.
    width: f64,
    min_tokens: usize,
    floor: CoverageFloor,
    max_combos: Option<usize>,
    buy_amount_sol: f64,
    coarse_observer: Arc<dyn SweepObserver + Send>,
    observer: Arc<dyn SweepObserver + Send>,
    sink: Arc<dyn GroupSink + Send + Sync>,
) -> Result<GroupedSweepOutput> {
    let cap = effective_cap(max_combos);

    // Resolve the page-supplied axes (omitted/empty axes fall back to defaults).
    let spec: Swing1AxesSpec =
        serde_json::from_value(axes_json).context("invalid swing1 axes spec")?;
    let axes = Swing1Axes::from_spec(&spec);

    // Grid guard: reject an explosive full grid before doing any sweep work. swing1
    // has ~25 axes — a full grid is almost never sane (use LHS/refine); this guard
    // surfaces that instead of churning.
    if matches!(method, SweepMethod::Grid) {
        let n = axes.combo_count();
        if n > cap {
            bail!("grid has {n} combos, over the {cap} cap — narrow the axes, raise max_combos, or use random:N / lhs:N");
        }
    }
    // Store the resolved axes (post-defaults/dedup) so the run is reproducible.
    let axes_json = serde_json::to_value(&axes).context("serializing resolved swing1 axes")?;

    // Judge deadness against the run's wall-clock `now` (matching live), captured once
    // so it's uniform across the whole run. Over the sealed lake `now` ≫ every last
    // trade, so a token that stopped trading is booked `Dead` not `Open`.
    let strategy = Swing1Strategy::new(sweep_base_rule_swing1(buy_amount_sol), axes)
        .with_as_of(chrono::Utc::now());
    let mut params = strategy.sample(method);
    if params.is_empty() {
        bail!("param space is empty");
    }
    // Random/LHS draws can request any `n`; clamp to the cap (grid is pre-checked).
    if params.len() > cap {
        tracing::warn!(
            combos = params.len(),
            cap,
            "grouped sweep: clamping sampled combos to cap"
        );
        params.truncate(cap);
    }

    let threads = bounded_threads();

    let (combo_count, groups) = tokio::task::spawn_blocking(
        move || -> Result<(usize, Vec<GroupResult>)> {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .thread_name(|i| format!("grouped-sweep-{i}"))
                .stack_size(8 * 1024 * 1024)
                .build()
                .map_err(|e| anyhow!("rayon pool build failed: {e}"))?;
            pool.install(|| {
                let (final_params, groups) = run_grouped_with_refine(
                    &strategy,
                    params,
                    refine,
                    &corpus,
                    &fields,
                    width,
                    min_tokens,
                    floor,
                    cap,
                    coarse_observer.as_ref(),
                    observer.as_ref(),
                    sink.as_ref(),
                )?;
                Ok((final_params.len(), groups))
            })
        },
    )
    .await??;

    Ok(GroupedSweepOutput { combo_count, axes_json, groups })
}

/// Synthetic swing1 base rule the swept params overlay. As with tpsl1/2, only
/// `buy_amount_sol` is meaningful — every other field is overwritten by the swept axes
/// or unused in the grouped sweep. swing1 has no token-creation gate, so the
/// token-filter fields stay inert.
fn sweep_base_rule_swing1(buy_amount_sol: f64) -> Swing1Rule {
    Swing1Rule::new(
        "sweep-synthetic-base".into(),
        None,                  // p_token_initial_buy_sol — unused (no creation gate)
        None,                  // p_token_cu_limit        — unused
        None,                  // p_token_cu_price        — unused
        serde_json::json!([]), // p_token_ix_labels       — unused
        "paper".into(),        // trade_mode              — unused in sweep
        buy_amount_sol,
        0.0,                   // p_exit_take_profit      — overlaid per combo
        0.0,                   // p_exit_stop_loss        — overlaid per combo
        None,                  // p_token_max_sol_cost    — unused
        None,                  // p_token_spendable_sol_in — unused
        None,                  // p_max_concurrent_tokens — unused in grouped sweep
        None,                  // p_max_total_tokens      — unused in grouped sweep
        None,                  // bucket_width_sol           — default 0.1; run width wired Stage 3
        None,                  // p_exit_trailing_stop_pct — overlaid per combo
        None,                  // p_exit_time_stop_secs    — overlaid per combo
        None,                  // p_exit_stall_secs        — overlaid per combo
        None,                  // p_exit_liquidity_drop_pct — overlaid per combo
    )
}

// ---------------------------------------------------------------------------
// TPSL1 strategy entry point
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
async fn sweep_tpsl1(
    axes_json: Value,
    method: SweepMethod,
    refine: Option<RefineSpec>,
    corpus: Corpus,
    fields: Vec<GroupField>,
    // Per-run bucket width (SOL) for the continuous SOL grouping fields (see
    // `run_grouped`). Threaded verbatim into the engine's `partition`.
    width: f64,
    min_tokens: usize,
    floor: CoverageFloor,
    max_combos: Option<usize>,
    buy_amount_sol: f64,
    coarse_observer: Arc<dyn SweepObserver + Send>,
    observer: Arc<dyn SweepObserver + Send>,
    sink: Arc<dyn GroupSink + Send + Sync>,
) -> Result<GroupedSweepOutput> {
    let cap = effective_cap(max_combos);

    // Resolve the page-supplied axes (omitted/empty axes fall back to defaults).
    let spec: Tpsl1AxesSpec =
        serde_json::from_value(axes_json).context("invalid TPSL1 axes spec")?;
    let axes = Tpsl1Axes::from_spec(&spec);

    // Grid guard: reject an explosive full grid before doing any sweep work.
    if matches!(method, SweepMethod::Grid) {
        let n = axes.combo_count();
        if n > cap {
            bail!("grid has {n} combos, over the {cap} cap — narrow the axes, raise max_combos, or use random:N");
        }
    }
    // Store the resolved axes (post-defaults/dedup) so the run is reproducible.
    let axes_json = serde_json::to_value(&axes).context("serializing resolved TPSL1 axes")?;

    // Judge deadness against the run's wall-clock `now` (matching live), captured once
    // so it's uniform across the whole run. Over the sealed lake `now` ≫ every last
    // trade, so a token that stopped trading is booked `Dead` not `Open`.
    let strategy = Tpsl1Strategy::new(sweep_base_rule_tpsl1(buy_amount_sol), axes)
        .with_as_of(chrono::Utc::now());
    let mut params = strategy.sample(method);
    if params.is_empty() {
        bail!("param space is empty");
    }
    // Random/LHS draws can request any `n`; clamp to the cap (grid is pre-checked).
    if params.len() > cap {
        tracing::warn!(
            combos = params.len(),
            cap,
            "grouped sweep: clamping sampled combos to cap"
        );
        params.truncate(cap);
    }

    let threads = bounded_threads();
    // Combos are swept in budget-sized batches (Phase 2.5), so a large combo set is
    // bounded in memory rather than rejected — see `sweep_tpsl2` for the rationale.

    // The coarse→refine driver may grow the combo set, so the final combo count is
    // only known after the sweep; the per-combo param JSON is emitted by the sink
    // during the fold (Phase 4).
    let (combo_count, groups) = tokio::task::spawn_blocking(
        move || -> Result<(usize, Vec<GroupResult>)> {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .thread_name(|i| format!("grouped-sweep-{i}"))
                .stack_size(8 * 1024 * 1024)
                .build()
                .map_err(|e| anyhow!("rayon pool build failed: {e}"))?;
            pool.install(|| {
                let (final_params, groups) = run_grouped_with_refine(
                    &strategy,
                    params,
                    refine,
                    &corpus,
                    &fields,
                    width,
                    min_tokens,
                    floor,
                    cap,
                    coarse_observer.as_ref(),
                    observer.as_ref(),
                    sink.as_ref(),
                )?;
                Ok((final_params.len(), groups))
            })
        },
    )
    .await??;

    Ok(GroupedSweepOutput { combo_count, axes_json, groups })
}

// ---------------------------------------------------------------------------
// Single-combo re-simulation (for the token-results drill-in endpoint)
// ---------------------------------------------------------------------------

/// Re-simulate a single stored combo on a pre-filtered token slice.
///
/// Runs sequentially (no rayon): one combo per token is O(tokens) trivially cheap,
/// far less than the DB round-trips that preceded it. Returns one result per token,
/// always including non-fired tokens so the caller can display the full group slice.
pub fn simulate_one_combo(
    strategy_id: &str,
    tokens: &[CorpusToken],
    params_json: &Value,
    buy_amount_sol: f64,
) -> Result<Vec<ComboTokenResult>> {
    match strategy_id {
        "tpsl2" => simulate_tpsl2_one_combo(tokens, params_json, buy_amount_sol),
        "tpsl1" => simulate_tpsl1_one_combo(tokens, params_json, buy_amount_sol),
        "swing_1" => simulate_swing1_one_combo(tokens, params_json, buy_amount_sol),
        other => bail!(
            "strategy '{other}' has no single-combo simulation (supported: {:?})",
            strategy_ids()
        ),
    }
}

fn exit_label(code: ExitCode) -> &'static str {
    match code {
        ExitCode::NoEntry => "NoEntry",
        ExitCode::Open => "Open",
        ExitCode::TakeProfit => "TakeProfit",
        ExitCode::StopLoss => "StopLoss",
        ExitCode::TrailingStop => "TrailingStop",
        ExitCode::Stall => "Stall",
        ExitCode::TimeStop => "TimeStop",
        ExitCode::LiquidityExit => "LiquidityExit",
        ExitCode::NextKill => "NextKill",
        ExitCode::Dead => "Dead",
        ExitCode::Metrics => "Metrics",
    }
}

fn simulate_tpsl2_one_combo(
    tokens: &[CorpusToken],
    params_json: &Value,
    buy_amount_sol: f64,
) -> Result<Vec<ComboTokenResult>> {
    // Same death-close "present" contract as the grouped sweep: judge against run
    // time (matching live). For the sealed lake this resolves the same Dead/Open
    // verdict a sweep did, so the drill-in stays PnL-consistent with the stored row.
    let strategy = Tpsl2Strategy::for_replay(sweep_base_rule_tpsl2(buy_amount_sol))
        .with_as_of(chrono::Utc::now());
    let combo = strategy.combo_from_params_json(params_json)?;
    let params = std::slice::from_ref(&combo);
    let noop = Noop;
    let mut results = Vec::with_capacity(tokens.len());
    for tt in tokens {
        let mut outs = Vec::with_capacity(1);
        let _ = fill_outcomes(&strategy, params, tt, &noop, &mut outs);
        let o = outs
            .into_iter()
            .next()
            .unwrap_or_else(crate::sweep::strategy::TokenOutcome::no_entry);
        results.push(ComboTokenResult {
            mint_address: tt.mint.clone(),
            symbol: tt.symbol.clone(),
            fired: o.fired,
            pnl_sol: o.pnl_sol,
            pnl_pct: o.pnl_percent,
            holding_secs: o.holding_secs,
            exit: exit_label(o.exit).to_string(),
            entry_time: o.entry_time.map(|t| t.to_rfc3339()),
            entry_price: o.entry_price,
            // tx left null here; the handler resolves the real signature from the
            // `trades` table by (mint, slot, side) — the sweep row carries none.
            entry_tx: None,
            entry_slot: o.entry_slot,
            exit_time: o.exit_time.map(|t| t.to_rfc3339()),
            exit_price: o.exit_price,
            exit_tx: None,
            exit_slot: o.exit_slot,
            created_at: None,
            ath_price: None,
            token: Default::default(),
        });
    }
    Ok(results)
}

fn simulate_tpsl1_one_combo(
    tokens: &[CorpusToken],
    params_json: &Value,
    buy_amount_sol: f64,
) -> Result<Vec<ComboTokenResult>> {
    let strategy = Tpsl1Strategy::for_replay(sweep_base_rule_tpsl1(buy_amount_sol))
        .with_as_of(chrono::Utc::now());
    let combo = strategy.combo_from_params_json(params_json)?;
    let params = std::slice::from_ref(&combo);
    let noop = Noop;
    let mut results = Vec::with_capacity(tokens.len());
    for tt in tokens {
        let mut outs = Vec::with_capacity(1);
        let _ = fill_outcomes(&strategy, params, tt, &noop, &mut outs);
        let o = outs
            .into_iter()
            .next()
            .unwrap_or_else(crate::sweep::strategy::TokenOutcome::no_entry);
        results.push(ComboTokenResult {
            mint_address: tt.mint.clone(),
            symbol: tt.symbol.clone(),
            fired: o.fired,
            pnl_sol: o.pnl_sol,
            pnl_pct: o.pnl_percent,
            holding_secs: o.holding_secs,
            exit: exit_label(o.exit).to_string(),
            entry_time: o.entry_time.map(|t| t.to_rfc3339()),
            entry_price: o.entry_price,
            // tx left null here; the handler resolves the real signature from the
            // `trades` table by (mint, slot, side) — the sweep row carries none.
            entry_tx: None,
            entry_slot: o.entry_slot,
            exit_time: o.exit_time.map(|t| t.to_rfc3339()),
            exit_price: o.exit_price,
            exit_tx: None,
            exit_slot: o.exit_slot,
            created_at: None,
            ath_price: None,
            token: Default::default(),
        });
    }
    Ok(results)
}

fn simulate_swing1_one_combo(
    tokens: &[CorpusToken],
    params_json: &Value,
    buy_amount_sol: f64,
) -> Result<Vec<ComboTokenResult>> {
    let strategy = Swing1Strategy::for_replay(sweep_base_rule_swing1(buy_amount_sol))
        .with_as_of(chrono::Utc::now());
    let combo = strategy.combo_from_params_json(params_json)?;
    let params = std::slice::from_ref(&combo);
    let noop = Noop;
    let mut results = Vec::with_capacity(tokens.len());
    for tt in tokens {
        let mut outs = Vec::with_capacity(1);
        let _ = fill_outcomes(&strategy, params, tt, &noop, &mut outs);
        let o = outs
            .into_iter()
            .next()
            .unwrap_or_else(crate::sweep::strategy::TokenOutcome::no_entry);
        results.push(ComboTokenResult {
            mint_address: tt.mint.clone(),
            symbol: tt.symbol.clone(),
            fired: o.fired,
            pnl_sol: o.pnl_sol,
            pnl_pct: o.pnl_percent,
            holding_secs: o.holding_secs,
            exit: exit_label(o.exit).to_string(),
            entry_time: o.entry_time.map(|t| t.to_rfc3339()),
            entry_price: o.entry_price,
            // tx left null here; the handler resolves the real signature from the
            // `trades` table by (mint, slot, side) — the sweep row carries none.
            entry_tx: None,
            entry_slot: o.entry_slot,
            exit_time: o.exit_time.map(|t| t.to_rfc3339()),
            exit_price: o.exit_price,
            exit_tx: None,
            exit_slot: o.exit_slot,
            created_at: None,
            ath_price: None,
            token: Default::default(),
        });
    }
    Ok(results)
}

/// Synthetic TPSL1 base rule the swept params overlay. As with TPSL2, only
/// `buy_amount_sol` is meaningful (see [`SWEEP_DEFAULT_BUY_AMOUNT_SOL`]) — every
/// other field is overwritten by the swept exit ladder or unused in the grouped
/// sweep.
fn sweep_base_rule_tpsl1(buy_amount_sol: f64) -> Tpsl1Rule {
    Tpsl1Rule::new(
        "sweep-synthetic-base".into(),
        None,                  // p_token_initial_buy_sol — unused in sweep
        None,                  // p_token_cu_limit        — unused in sweep
        None,                  // p_token_cu_price        — unused in sweep
        serde_json::json!([]), // p_token_ix_labels       — unused in sweep
        "paper".into(),        // trade_mode              — unused in sweep
        buy_amount_sol,
        0.0,                   // p_exit_take_profit      — overlaid per combo
        0.0,                   // p_exit_stop_loss        — overlaid per combo
        None,                  // p_token_max_sol_cost    — unused in sweep
        None,                  // p_token_spendable_sol_in — unused in sweep
        None,                  // p_max_concurrent_tokens — unused in grouped sweep
        None,                  // p_max_total_tokens      — unused in grouped sweep
        None,                  // bucket_width_sol           — default 0.1; run width wired Stage 3 in sweep
        None,                  // p_exit_trailing_stop_pct — overlaid per combo
        None,                  // p_exit_time_stop_secs    — overlaid per combo
        None,                  // p_exit_stall_secs        — overlaid per combo
        None,                  // p_exit_liquidity_drop_pct — overlaid per combo
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threads_fitting_keeps_preferred_when_under_budget() {
        // 100 MB/token × 8 threads = 800 MB < 2048 MB
        assert_eq!(
            threads_fitting_admission(8, 100 * 1024 * 1024, 2048 * 1024 * 1024),
            Some(8)
        );
    }

    #[test]
    fn threads_fitting_lowers_to_fit_budget() {
        // 500 MB/token × 8 = 4000 MB > 2048 MB → max threads = 2048/500 = 4
        assert_eq!(
            threads_fitting_admission(8, 500 * 1024 * 1024, 2048 * 1024 * 1024),
            Some(4)
        );
    }

    #[test]
    fn threads_fitting_rejects_when_one_thread_overflows() {
        assert_eq!(
            threads_fitting_admission(8, 3000 * 1024 * 1024, 2048 * 1024 * 1024),
            None
        );
    }

    #[test]
    fn threads_fitting_zero_series_keeps_preferred() {
        assert_eq!(threads_fitting_admission(8, 0, 0), Some(8));
    }

    #[test]
    fn default_threads_are_half_cores() {
        // Without SWEEP_RAYON_THREADS, default is cores/2 (floor 1).
        std::env::remove_var("SWEEP_RAYON_THREADS");
        let cores = detect_cores();
        assert_eq!(bounded_threads(), (cores / 2).max(1));
    }
}
