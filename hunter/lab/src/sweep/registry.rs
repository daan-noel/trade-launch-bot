//! Strategy registry — the **one** place a strategy is wired into the grouped
//! sweep. It maps a `strategy_id` to (a) its per-strategy DB table triple and
//! (b) its concrete sweep entry point. The handler and repo are otherwise fully
//! generic (table-name- and data-driven), so adding "swing" later means: write a
//! `strategies/swing.rs` (`Strategy` + `ParamSpace` + `AxesSpec`), add its tables
//! + a match arm here, and a `lab/migrations/NNNN_*.sql` file — nothing else changes.
//!
//! The CPU-heavy sweep runs in a bounded rayon pool inside `spawn_blocking` so it
//! can never starve the live trading hot path (ingest / sell-confirm).

use std::sync::atomic::{AtomicUsize, Ordering};
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
    fn set_total(&self, _total_tokens: usize, _combos_per_token: usize) {}
    fn token_done(&self, _combos_folded: usize) {}
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

/// Hard cap (MB) on fold-side peak memory (ComboAgg + in-flight TokenOutcome
/// buffers). Raised vs the old 256 MB so a 14 GB usable box can take larger
/// combo batches (fewer series rebuild passes).
const SWEEP_FOLD_BUDGET_CAP_MB: usize = 512;

/// Floor (MB) so a nearly-full box still makes progress with tiny batches.
const SWEEP_FOLD_BUDGET_FLOOR_MB: usize = 32;

/// Fold-accumulator + outcome-buffer budget in bytes. `usable/4` clamped to
/// 32..=512 MB, where usable = host free − desktop reserve.
pub(crate) fn sweep_memory_budget_bytes() -> u64 {
    let cap = (SWEEP_FOLD_BUDGET_CAP_MB as u64).saturating_mul(1024 * 1024);
    let floor = (SWEEP_FOLD_BUDGET_FLOOR_MB as u64).saturating_mul(1024 * 1024);
    match usable_host_bytes() {
        Some(usable) => (usable / 4).clamp(floor, cap),
        None => cap,
    }
}

/// Ceiling (MB) on the **series precompute** transient when host RAM is unreadable.
const DEFAULT_SWEEP_ADMISSION_BUDGET_MB: usize = 12 * 1024;

fn sweep_admission_budget_bytes() -> u64 {
    (DEFAULT_SWEEP_ADMISSION_BUDGET_MB as u64).saturating_mul(1024 * 1024)
}

/// Default host RAM (MB) to leave free for OS + desktop UI. Matches the
/// workstation contract: use almost everything, keep ~2 GB for local
/// interactivity. The run form can override it per run (see
/// [`set_ram_reserve_mb`]) — a headless box can give the sweep more, a box the
/// user is working on can keep more.
pub const DEFAULT_SWEEP_RAM_RESERVE_MB: usize = 2048;

/// Bounds on the per-run override. The floor keeps *some* headroom (a 0-reserve
/// run OOMs the box it runs on); the ceiling stops a typo from refusing every
/// run outright.
pub const MIN_SWEEP_RAM_RESERVE_MB: usize = 256;
pub const MAX_SWEEP_RAM_RESERVE_MB: usize = 32 * 1024;

/// Active desktop reserve (MB). Process-global rather than threaded through
/// every admission/shard/fold helper: the handler refuses concurrent sweeps
/// ("a sweep is already running"), so exactly one run's choice is live at a time
/// and the rayon workers can read it lock-free from any depth.
static RAM_RESERVE_MB: AtomicUsize = AtomicUsize::new(DEFAULT_SWEEP_RAM_RESERVE_MB);

/// Set the desktop reserve for the run about to start, clamped to
/// `MIN..=MAX_SWEEP_RAM_RESERVE_MB`. `None` restores the default. Call once at
/// admission, before any sizing/admission helper runs.
pub(crate) fn set_ram_reserve_mb(mb: Option<usize>) -> usize {
    let mb = mb
        .unwrap_or(DEFAULT_SWEEP_RAM_RESERVE_MB)
        .clamp(MIN_SWEEP_RAM_RESERVE_MB, MAX_SWEEP_RAM_RESERVE_MB);
    RAM_RESERVE_MB.store(mb, Ordering::Relaxed);
    mb
}

/// The reserve the current run is sized against (MB).
pub(crate) fn ram_reserve_mb() -> usize {
    RAM_RESERVE_MB.load(Ordering::Relaxed)
}

fn sweep_ram_reserve_bytes() -> u64 {
    (ram_reserve_mb() as u64).saturating_mul(1024 * 1024)
}

/// `available − reserve`, or `None` if host memory is unreadable. `Some(0)` when
/// free RAM is already under the desktop reserve — callers must refuse or shrink.
pub(crate) fn usable_host_bytes() -> Option<u64> {
    let (_, available) = crate::sweep::obs::host_memory_bytes()?;
    Some(available.saturating_sub(sweep_ram_reserve_bytes()))
}

/// How much of currently-available host RAM may be used for the sweep peak.
/// Strict: `available − reserve` (no half-of-free degradation — that hid OOMs).
fn host_series_ceiling_bytes(available: u64, reserve: u64) -> u64 {
    available.saturating_sub(reserve)
}

/// Effective series+fold admission ceiling from host free RAM.
/// Returns `(budget_bytes, host_total_mb, host_avail_mb)`.
fn effective_admission_budget_bytes() -> (u64, Option<u64>, Option<u64>) {
    let fallback = sweep_admission_budget_bytes();
    match crate::sweep::obs::host_memory_bytes() {
        Some((total, available)) => {
            let reserve = sweep_ram_reserve_bytes();
            let host_ceiling = host_series_ceiling_bytes(available, reserve);
            if available <= reserve {
                tracing::warn!(
                    host_avail_mb = available / (1024 * 1024),
                    ram_reserve_mb = reserve / (1024 * 1024),
                    "generic sweep: host free RAM already under desktop reserve — \
                     usable budget is 0; run will be refused unless series is trivial"
                );
            }
            let budget = fallback.min(host_ceiling);
            (
                budget,
                Some(total / (1024 * 1024)),
                Some(available / (1024 * 1024)),
            )
        }
        None => (fallback, None, None),
    }
}

/// How many heavy [`Strategy::TokenState`]s (e.g. `MetricSeries`) may stay alive
/// together. `0`-byte estimates (legacy tpsl/swing `()`) → full thread pool.
/// Otherwise `min(threads, series_budget / max_series)`, with series_budget =
/// admission ceiling minus the fold budget so ComboAgg/outcome buffers still fit.
pub(crate) fn series_wave_size(max_series_bytes: usize, threads: usize) -> usize {
    let threads = threads.max(1);
    if max_series_bytes == 0 {
        return threads;
    }
    let (admission, _, _) = effective_admission_budget_bytes();
    let fold = sweep_memory_budget_bytes();
    let series_budget = admission.saturating_sub(fold);
    if series_budget < max_series_bytes as u64 {
        return 1; // at least one token; admission may still reject later
    }
    let by_budget = (series_budget / max_series_bytes as u64) as usize;
    by_budget.min(threads).max(1)
}

/// True when host free RAM is already ≤ the desktop reserve (degraded mode).
pub(crate) fn host_under_ram_reserve() -> bool {
    match crate::sweep::obs::host_memory_bytes() {
        Some((_, available)) => available <= sweep_ram_reserve_bytes(),
        None => false,
    }
}

/// Fold batch hard cap: full [`crate::sweep::engine::HARD_MAX_COMBO_BATCH`] when
/// usable RAM remains; **8192** when already under the desktop reserve.
pub(crate) fn hard_max_combo_batch() -> usize {
    if host_under_ram_reserve() {
        8_192
    } else {
        crate::sweep::engine::HARD_MAX_COMBO_BATCH
    }
}

/// Index-only combo vec (`GenericCombo { idx }`) + final `ComboMetrics` slot.
/// CompiledRules are bound per batch (not resident × N); combo JSON is deferred
/// to retained survivors only (~660/group).
const GENERIC_PER_COMBO_RESIDENT_BYTES: u64 = 280;

/// Slack for rayon stacks / allocator freelists inside the usable budget.
const SWEEP_ALLOC_SLACK_BYTES: u64 = 256 * 1024 * 1024;

pub(crate) fn estimate_generic_combo_side_bytes(n_combos: usize) -> u64 {
    (n_combos as u64).saturating_mul(GENERIC_PER_COMBO_RESIDENT_BYTES)
}

/// Refuse when estimated peak cannot fit in `usable = free − desktop reserve`
/// (minus slack). Under-reserve (`usable == 0`) always refuses non-trivial runs.
fn admit_generic_combo_side(n_combos: usize, series_peak: u64, fold: u64) -> Result<()> {
    let combo_side = estimate_generic_combo_side_bytes(n_combos);
    let need = combo_side
        .saturating_add(series_peak)
        .saturating_add(fold);
    let Some(usable_raw) = usable_host_bytes() else {
        return Ok(());
    };
    let usable = usable_raw.saturating_sub(SWEEP_ALLOC_SLACK_BYTES);
    if need > usable {
        let mb = |b: u64| b / (1024 * 1024);
        let avail = crate::sweep::obs::host_memory_bytes().map(|(_, a)| a).unwrap_or(0);
        bail!(
            "estimated resident peak ~{} MB ({} combos × ~{} B + fold {} MB + series {} MB) \
             exceeds usable RAM {} MB (host free {} MB − {} MB desktop reserve − {} MB slack) — \
             use random:N, narrow axes, tighten token/date filters, or free RAM before retrying",
            mb(need),
            n_combos,
            GENERIC_PER_COMBO_RESIDENT_BYTES,
            mb(fold),
            mb(series_peak),
            mb(usable),
            mb(avail),
            ram_reserve_mb(),
            mb(SWEEP_ALLOC_SLACK_BYTES),
        );
    }
    Ok(())
}

/// Planned combo count for admission (before `sample` materialises CompiledRules).
fn planned_combo_count(method: SweepMethod, model_combos: usize, cap: usize) -> Result<usize> {
    if model_combos == 0 {
        return Ok(0);
    }
    if model_combos == usize::MAX {
        bail!("grid combo_count overflowed — refuse to sample");
    }
    Ok(match method {
        SweepMethod::Grid => model_combos.min(cap),
        SweepMethod::Random { n, .. } | SweepMethod::LatinHypercube { n, .. } => {
            n.min(model_combos).min(cap)
        }
    })
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

/// Rayon threads for the in-process sweep. Leaves **2 logical CPUs** for the
/// desktop (e.g. 14 on a 16-core workstation). Always ≥1. Host-RAM admission may
/// still lower the count further at sweep start.
fn bounded_threads() -> usize {
    detect_cores().saturating_sub(2).max(1)
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

    // Grid guard: reject an explosive full grid before doing any sweep work. A full
    // grid runs exactly as chosen (capped by `cap` = the caller's Max combos/group);
    // we do NOT silently swap it for a coarse LHS + refine. That auto-convert only
    // ever triggered when the caller had explicitly raised `max_combos` ≥ 200k (the
    // default 100k cap can't reach the threshold), i.e. it overrode an explicit choice
    // every time — so it's gone. A caller who wants coarse+refine asks for it via
    // `refine:N:K`; one whose grid is too big for RAM is refused by admission below
    // with actionable guidance.
    if matches!(method, SweepMethod::Grid) {
        let n = model.combo_count();
        if n > cap {
            bail!("grid has {n} combos, over the {cap} cap — narrow the axes, raise max_combos, or use random:N");
        }
    }

    let preferred_threads = bounded_threads();
    let cores = detect_cores();

    // Admission BEFORE sample. Peak is one **shard** (sharding + spill handle the
    // rest of N), not the full combo dictionary.
    let planned = planned_combo_count(method, model.combo_count(), cap)?;
    if planned == 0 {
        bail!("param space is empty");
    }
    let strategy = GenericSweepStrategy::new(model, buy_amount_sol, chrono::Utc::now());
    let max_series_bytes = corpus
        .tokens
        .iter()
        .map(|t| strategy.series_bytes_estimate(t))
        .max()
        .unwrap_or(0);
    let (admission_raw, host_total_mb, host_avail_mb) = effective_admission_budget_bytes();
    let fold_budget = sweep_memory_budget_bytes();
    let admission_budget = admission_raw.saturating_sub(fold_budget);
    let mb = |b: u64| b / (1024 * 1024);
    let threads = match threads_fitting_admission(preferred_threads, max_series_bytes, admission_budget)
    {
        Some(n) => n,
        None => {
            bail!(
                "estimated series precompute for largest token ({} MB) exceeds the \
                 {} MB series budget (admission {} MB − fold {} MB) even at 1 thread — \
                 narrow the corpus or axes (token_cap / date range / fewer combos)",
                mb(max_series_bytes as u64),
                mb(admission_budget),
                mb(admission_raw),
                mb(fold_budget)
            );
        }
    };
    let wave = series_wave_size(max_series_bytes, threads);
    // Series peak = `threads × one token's series`. The cross-group Phase-2 driver
    // runs one small group per worker, each building→folding→dropping a single
    // `MetricSeries` at a time (grouped_engine::sweep_group_serial), so `threads`
    // series are resident at the pool's peak; Phase-1's wave (≤ threads) fits the
    // same envelope. (`wave == threads` here, but model it on `threads` directly so
    // the guard stays honest if the wave sizing changes.) Pre-fix this path cached a
    // whole group's series per worker — ~4·threads² resident — which this estimate
    // never modeled and which OOM'd 16 GB boxes.
    let series_peak = (threads as u64).saturating_mul(max_series_bytes as u64);
    // Fold buffers (ComboAgg accumulators + one TokenOutcome scratch vec) are resident
    // once PER WORKER under the cross-group `par_iter`, not once globally — so multiply
    // the per-worker fold footprint at the actual batch by `threads`. `planned` (≥ the
    // realised sample) keeps this a conservative upper bound on `batch`, hence on the
    // footprint. NB: use the true residency, not `combo_batch_size`'s inflight sizing
    // model — see `fold_footprint_bytes`.
    let admit_batch = crate::sweep::engine::combo_batch_size(planned, threads);
    let fold_peak =
        (threads as u64).saturating_mul(crate::sweep::engine::fold_footprint_bytes(admit_batch));
    let shard_peak = crate::sweep::shard::max_combos_per_shard(wave, max_series_bytes).min(planned);
    admit_generic_combo_side(shard_peak, series_peak, fold_peak)?;

    if threads < preferred_threads || wave < threads {
        tracing::warn!(
            preferred_threads,
            threads,
            wave,
            max_token_series_mb = mb(max_series_bytes as u64),
            budget_mb = mb(admission_budget),
            "generic sweep: lowered concurrency to fit series+fold admission budget"
        );
    }
    tracing::info!(
        cores,
        preferred_threads,
        threads,
        wave,
        planned_combos = planned,
        shard_peak_combos = shard_peak,
        combo_side_mb = mb(estimate_generic_combo_side_bytes(shard_peak)),
        max_token_series_mb = mb(max_series_bytes as u64),
        peak_precompute_mb = mb(series_peak),
        budget_mb = mb(admission_budget),
        fold_budget_mb = mb(fold_budget),
        fold_peak_mb = mb(fold_peak),
        admission_cap_mb = mb(sweep_admission_budget_bytes()),
        ram_reserve_mb = mb(sweep_ram_reserve_bytes()),
        host_total_mb,
        host_avail_mb,
        rss_mb = crate::sweep::obs::process_rss_mb(),
        "generic sweep: series+combo-side admission estimate"
    );

    // The (validated) request axes are echoed back for storage / re-run below.
    let mut params = strategy.sample(method);
    if params.is_empty() {
        bail!("param space is empty");
    }
    if params.len() > cap {
        tracing::warn!(combos = params.len(), cap, "grouped sweep: clamping sampled combos to cap");
        params.truncate(cap);
    }
    // Re-check shard peak against realised sample size. `fold_peak` was estimated at
    // `planned` (≥ the realised sample), so it stays a valid conservative bound here.
    let realised_shard = crate::sweep::shard::max_combos_per_shard(wave, max_series_bytes)
        .min(params.len());
    if realised_shard != shard_peak {
        admit_generic_combo_side(realised_shard, series_peak, fold_peak)?;
    }

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
        "generic" => simulate_generic_one_combo(tokens, params_json, buy_amount_sol),
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

/// Re-simulate a single stored **generic** (redesigned-engine) combo per token —
/// the drill-in counterpart of [`sweep_generic`]. The stored `params_json` is a
/// canonical [`RuleParams`] object (what [`GenericSweepStrategy::params_json`]
/// emits), so we compile it straight to a [`CompiledRule`] and run the same
/// precompute-then-scan the grouped sweep used — no axes model needed. Deadness is
/// judged against wall-clock `now`, matching the sweep / live.
///
/// The scan carries each fill row's `entry_slot`/`exit_slot` (the slot of the real
/// trade the fill executes against — the fill row's own trade, or the next print when
/// the fill lands on a tick), so the handler's fill → `tx_signature` lookup resolves
/// the chart's entry/exit tx markers here the same way it does for tpsl combos.
fn simulate_generic_one_combo(
    tokens: &[CorpusToken],
    params_json: &Value,
    buy_amount_sol: f64,
) -> Result<Vec<ComboTokenResult>> {
    use hunter_engine::arm::CompiledRule;
    use hunter_engine::event::{LoadedRule, RuleId, TradeMode};
    use hunter_engine::fingerprint::FingerprintId;
    use hunter_engine::rule_params::RuleParams;
    use trading_core::config::constants::sol_to_lamports;
    use trading_core::strategies::kernel::CostModel;

    use crate::sweep::generic::strategy::{build_series, columns_for, scan, sparse_grid_for};

    let params = RuleParams::parse(params_json)
        .map_err(|e| anyhow!("invalid generic combo params: {e}"))?;
    // Dummy fingerprint + unlimited caps: like the sweep, a single-combo drill-in
    // judges each token independently and never models cross-token concurrency.
    let loaded = LoadedRule {
        id: RuleId(uuid::Uuid::nil()),
        fingerprint_id: FingerprintId(uuid::Uuid::nil()),
        trade_mode: TradeMode::Paper,
        buy_amount_lamports: sol_to_lamports(buy_amount_sol).max(0) as u64,
        max_concurrent_tokens: u32::MAX,
        max_total_tokens: 0,
        params,
    };
    let compiled = CompiledRule::compile(&loaded);
    let columns = columns_for(&compiled);
    let grid = sparse_grid_for(&compiled);
    let as_of = chrono::Utc::now();
    let cost = CostModel::pumpfun_default();

    let mut results = Vec::with_capacity(tokens.len());
    for tt in tokens {
        let series = build_series(tt, columns.clone(), &grid, as_of);
        let o = scan(&series, &compiled, buy_amount_sol, &cost);
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
            // Resolved from `entry_slot`/`exit_slot` by the handler post-sim.
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
    fn combo_side_estimate_scales_with_combos() {
        assert_eq!(estimate_generic_combo_side_bytes(0), 0);
        assert_eq!(
            estimate_generic_combo_side_bytes(1_000),
            1_000 * GENERIC_PER_COMBO_RESIDENT_BYTES
        );
        // Index-only: ~945k × 280 B ≈ 250 MB — not multi-GB.
        let approx = estimate_generic_combo_side_bytes(944_784);
        assert!(approx > 200 * 1024 * 1024 && approx < 400 * 1024 * 1024);
    }

    #[test]
    fn default_threads_leave_two_cores() {
        let cores = detect_cores();
        assert_eq!(bounded_threads(), cores.saturating_sub(2).max(1));
    }

    #[test]
    fn host_series_ceiling_subtracts_reserve_when_room() {
        let reserve = 2u64 * 1024 * 1024 * 1024;
        let available = 8u64 * 1024 * 1024 * 1024;
        assert_eq!(
            host_series_ceiling_bytes(available, reserve),
            6u64 * 1024 * 1024 * 1024
        );
    }

    #[test]
    fn host_series_ceiling_is_zero_when_under_reserve() {
        let reserve = 2u64 * 1024 * 1024 * 1024;
        let available = 1u64 * 1024 * 1024 * 1024;
        assert_eq!(host_series_ceiling_bytes(available, reserve), 0);
    }
}
