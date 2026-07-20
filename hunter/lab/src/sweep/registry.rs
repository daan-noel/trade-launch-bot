//! Strategy registry — the seam between `strategy_id` and the grouped sweep. Since
//! the tpsl/swing retirement (Phase 7) there is exactly one arm, `"generic"`, mapping
//! to the unprefixed `grouped_sweep_*` table triple and the `GenericSweepStrategy`
//! entry point (fingerprint + metric-condition axes, scanned over precomputed
//! `MetricSeries`). The handler and repo stay fully table-name-/data-driven, so the
//! registry is the one place that would grow an arm if a second sweep family ever
//! returned — but the generic engine is designed to cover the space with one.
//!
//! The CPU-heavy sweep runs in a bounded rayon pool inside `spawn_blocking` so it
//! can never starve the live trading hot path (ingest / sell-confirm).

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;

use crate::models::grouped_sweep::ComboTokenResult;
use crate::storage::repositories::grouped_sweep_repo::GroupedSweepTables;
use crate::sweep::corpus::{Corpus, CorpusToken};
use crate::sweep::grouped_engine::{run_grouped_with_refine, CoverageFloor, GroupResult, GroupSink};
use crate::sweep::grouping::GroupField;
use crate::sweep::progress::SweepObserver;
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

/// Per-run ceiling (bytes) on the fold budget, chosen by [`plan_sweep_sizing`]'s
/// degradation ladder. `0` = no override (auto sizing). Applied as a **ceiling**,
/// not a fixed value, so the live `usable/4` sizing below still shrinks further
/// when free RAM drops mid-run — the plan bounds the peak, it doesn't pin it.
///
/// Process-global for the same reason as [`RAM_RESERVE_MB`]: single-flight means
/// exactly one run's plan is live, and the rayon workers read it lock-free.
static FOLD_BUDGET_CEILING: AtomicU64 = AtomicU64::new(0);

/// Install the planned fold-budget ceiling for the run about to start (`None`
/// clears it). Called once by [`plan_sweep_sizing`].
pub(crate) fn set_fold_budget_ceiling(bytes: Option<u64>) {
    FOLD_BUDGET_CEILING.store(bytes.unwrap_or(0), Ordering::Relaxed);
}

/// The run's **permanent** resident set (bytes) — process RSS captured once the
/// corpus is fully loaded, before any fold buffer is allocated. `0` = not yet
/// captured (pre-load, or RSS unreadable).
///
/// This is the linchpin of the mid-run headroom read (see [`usable_host_bytes`]).
/// A grouped sweep's own RSS is dominated by the resident corpus (millions of
/// `SweepTrade`s), which stays for the whole run. A *live* `available` reading
/// therefore collapses toward the desktop reserve as the sweep allocates — not
/// because the box is out of RAM, but because the sweep is holding it — which
/// starved the small-group fold onto its slow series-rebuild path (token-outer
/// fired on 0 of 405 groups, measured 2026-07-19). Anchoring to this baseline lets
/// the mid-run read treat the corpus as consumed (it is) but the sweep's own
/// *transient* fold buffers as reusable headroom (they are — that budget is what
/// they're drawn from).
///
/// Process-global for the same single-flight reason as [`RAM_RESERVE_MB`].
static RESIDENT_BASELINE_BYTES: AtomicU64 = AtomicU64::new(0);

/// Record the run's permanent resident set. Call **once**, at the `corpus_loaded`
/// milestone — the corpus is fully resident and no fold buffer exists yet, so this
/// RSS is exactly the non-reclaimable floor every later mid-run read anchors to.
pub(crate) fn set_resident_baseline_bytes(bytes: Option<u64>) {
    RESIDENT_BASELINE_BYTES.store(bytes.unwrap_or(0), Ordering::Relaxed);
}

/// Fold-accumulator + outcome-buffer budget in bytes. `usable/4` clamped to
/// 32..=512 MB, where usable = host free − desktop reserve, then capped by the
/// run's planned ceiling (see [`FOLD_BUDGET_CEILING`]).
///
/// Re-read on every call by `combo_batch_size` / `max_combos_per_shard` /
/// `max_parallel_shards` / `full_combo_aggs_fit`, so a run that started on a free
/// box automatically thins its batches when RAM tightens mid-run.
pub(crate) fn sweep_memory_budget_bytes() -> u64 {
    let cap = (SWEEP_FOLD_BUDGET_CAP_MB as u64).saturating_mul(1024 * 1024);
    let floor = (SWEEP_FOLD_BUDGET_FLOOR_MB as u64).saturating_mul(1024 * 1024);
    let live = match usable_host_bytes() {
        Some(usable) => (usable / 4).clamp(floor, cap),
        None => cap,
    };
    match FOLD_BUDGET_CEILING.load(Ordering::Relaxed) {
        0 => live,
        // The ceiling never drops below the floor — a plan that tight still has to
        // make progress with tiny batches rather than allocate nothing.
        ceiling => live.min(ceiling).max(floor),
    }
}

/// Ceiling (MB) on the **series precompute** transient when host RAM is unreadable.
const DEFAULT_SWEEP_ADMISSION_BUDGET_MB: usize = 12 * 1024;

fn sweep_admission_budget_bytes() -> u64 {
    (DEFAULT_SWEEP_ADMISSION_BUDGET_MB as u64).saturating_mul(1024 * 1024)
}

/// Default host RAM (MB) to leave free for OS + desktop UI.
///
/// **This is a preference, not a cliff.** Since sizing degrades down a ladder
/// ([`plan_sweep_sizing`]) instead of refusing, a tight reserve costs wall-clock
/// (fewer threads, smaller batches) rather than failing the run — so the default
/// is 1 GB rather than the old 2 GB, which was the single biggest source of
/// spurious refusals on a workstation with a browser open. The run form can still
/// override it per run (see [`set_ram_reserve_mb`]): a headless box can give the
/// sweep more, a box the user is actively working on can keep more.
pub const DEFAULT_SWEEP_RAM_RESERVE_MB: usize = 1024;

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

/// Resolve a request's `ram_reserve_mb` to the effective reserve: `None` →
/// default, else clamped to `MIN..=MAX_SWEEP_RAM_RESERVE_MB`. Pure (no global) so
/// the clamp is testable without perturbing the process-global sizing state.
fn clamp_ram_reserve_mb(mb: Option<usize>) -> usize {
    mb.unwrap_or(DEFAULT_SWEEP_RAM_RESERVE_MB)
        .clamp(MIN_SWEEP_RAM_RESERVE_MB, MAX_SWEEP_RAM_RESERVE_MB)
}

/// Set the desktop reserve for the run about to start, clamped to
/// `MIN..=MAX_SWEEP_RAM_RESERVE_MB`. `None` restores the default. Call once at
/// admission, before any sizing/admission helper runs. Returns the applied value.
pub(crate) fn set_ram_reserve_mb(mb: Option<usize>) -> usize {
    let mb = clamp_ram_reserve_mb(mb);
    RAM_RESERVE_MB.store(mb, Ordering::Relaxed);
    mb
}

/// The reserve the current run is sized against (MB).
pub(crate) fn ram_reserve_mb() -> usize {
    RAM_RESERVE_MB.load(Ordering::Relaxed)
}

/// Whether the current run's per-`(combo × token)` exit scan should run on the
/// AVX-512 vector path (`resolve_exit_simd`) instead of the scalar `resolve_exit`
/// (AVX-512 exit-scan plan). Default **false** — the scalar path is the SSOT and the
/// toggle opts in.
///
/// Process-global for the same single-flight reason as [`RAM_RESERVE_MB`]: the
/// handler refuses concurrent sweeps, so exactly one run's choice is live at a time
/// and every rayon worker reads it lock-free from any fold depth. It is *how the box
/// computed*, not part of the analysis, so — like the RAM reserve — it is never
/// persisted on the run row.
static USE_SIMD: AtomicBool = AtomicBool::new(false);

/// Whether this host exposes the AVX-512 the vector exit scan needs, at runtime.
/// The CPU-feature gate the handler consults before honoring the `use_avx512`
/// toggle: a box without it is forced onto the scalar scan regardless of the
/// request. Kernel needs only `avx512f` (dead lanes are built scalar), matching
/// `generic::strategy::simd_available`.
pub(crate) fn avx512_available() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        std::is_x86_feature_detected!("avx512f")
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        false
    }
}

/// Enable/disable the AVX-512 exit scan for the run about to start. `None` clears
/// it (scalar). Call once at admission, before any fold runs — mirrors
/// [`set_ram_reserve_mb`]. Returns the applied value. The caller is responsible for
/// having already gated on [`avx512_available`]; this stores exactly what it's told.
pub(crate) fn set_use_simd(on: Option<bool>) -> bool {
    let on = on.unwrap_or(false);
    USE_SIMD.store(on, Ordering::Relaxed);
    on
}

/// Whether the current run's exit scan uses the AVX-512 path. Read lock-free by the
/// per-combo scan dispatch (`GenericSweepStrategy::resolve_exit`).
pub(crate) fn use_simd() -> bool {
    USE_SIMD.load(Ordering::Relaxed)
}

fn sweep_ram_reserve_bytes() -> u64 {
    (ram_reserve_mb() as u64).saturating_mul(1024 * 1024)
}

/// Usable RAM (bytes) for the sweep's transient peak, or `None` if host memory is
/// unreadable.
///
/// Two readings, and we take the **larger**:
/// - **Live:** `available − reserve`. What the OS says is free right now, minus the
///   desktop reserve. Honest about *external* pressure (another app grew), but it
///   also shrinks as the sweep's own allocations grow — which is the starvation bug.
/// - **Structural:** `total − reserve − resident_baseline`. Physical RAM, minus the
///   desktop reserve, minus the run's permanent resident set (the corpus, measured
///   once when headroom was real; see [`RESIDENT_BASELINE_BYTES`]). This is the
///   budget for *all* transient fold buffers combined, and it does not decay as the
///   sweep fills it — the sweep's own RSS growth is no longer subtracted from its own
///   headroom.
///
/// `max` of the two prices the sweep's own transient buffers as reusable (structural
/// wins mid-run) while still yielding to genuine external pressure when the OS has
/// even more free than the baseline predicted (live wins). It is bounded: the
/// structural term is `total − reserve − baseline`, so `baseline + used_transient ≤
/// total − reserve` at the peak — the desktop reserve stays free, upholding the
/// never-OOM-the-box contract. Before the baseline is captured (admission, pre-load)
/// it is `0`, so this is exactly the old `available − reserve` and admission sizing
/// is unchanged.
///
/// `Some(0)` only when *both* readings are under the reserve — a genuinely full box.
pub(crate) fn usable_host_bytes() -> Option<u64> {
    let (total, available) = crate::sweep::obs::host_memory_bytes()?;
    Some(usable_from(
        total,
        available,
        sweep_ram_reserve_bytes(),
        RESIDENT_BASELINE_BYTES.load(Ordering::Relaxed),
    ))
}

/// Pure core of [`usable_host_bytes`] — `max(live, structural)` (see that fn's
/// doc). Split out so the never-OOM bound can be tested without a live host read.
fn usable_from(total: u64, available: u64, reserve: u64, baseline: u64) -> u64 {
    let live = available.saturating_sub(reserve);
    let structural = if baseline == 0 {
        0 // baseline not captured yet → live reading only (old behaviour)
    } else {
        total.saturating_sub(reserve).saturating_sub(baseline)
    };
    live.max(structural)
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

/// Fold batch **sizing preference**: full [`crate::sweep::engine::HARD_MAX_COMBO_BATCH`]
/// when usable RAM remains; **8192** when already under the desktop reserve.
///
/// This is a *live* reading of free RAM, so it changes **during** a run — that is the
/// point: a run that started on a free box thins its later batches when the desktop
/// tightens. It therefore may only size *upcoming* allocations.
///
/// It must **never** be used as a validation invariant on an already-sized batch.
/// Doing so is a TOCTOU: a batch legally sized at 65536 while RAM was free gets
/// retroactively declared illegal the moment free RAM dips under the reserve, and the
/// fold bails — throwing away every group folded so far to "save" memory that is
/// already allocated and about to be freed anyway. That is exactly the mid-run
/// `fold_wave_into: n_combos 65536 > hard_max_combo_batch 8192` abort this split fixes.
/// Assertions belong against the static [`crate::sweep::engine::HARD_MAX_COMBO_BATCH`].
pub(crate) fn preferred_max_combo_batch() -> usize {
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

/// Combos one shard is always free to fall back to. The combo side is *sharded*
/// (`shard::plan_shards` + spill/merge), so however large the combo space is, its
/// resident cost floors at this many index-only entries — which is why the planner
/// prices the irreducible combo side here rather than at the planned count.
/// Matches the degraded fold-batch preference in [`preferred_max_combo_batch`].
const MIN_SHARD_COMBOS: usize = 8_192;

/// The sizing a run will execute at: how many rayon threads, how many token series
/// may be resident together, and the fold-buffer ceiling. Produced by
/// [`plan_sweep_sizing`], which *degrades* these to fit free RAM instead of
/// refusing the run.
pub(crate) struct SweepPlan {
    pub threads: usize,
    pub wave: usize,
    pub fold_budget: u64,
    /// Human-readable degradation notes (empty = ran at full preferred sizing).
    /// Surfaced to the operator so a slow run is explained, not mysterious.
    pub notes: Vec<String>,
}

/// Plan a run's sizing against free RAM by walking a **degradation ladder**
/// instead of admitting-or-refusing.
///
/// The resident peak of a sweep is a *choice*, not a constant: threads, the series
/// wave, the fold batch, the shard width and spill are all tunable downward, and
/// results are streamed per group rather than accumulated. So a box with little
/// free RAM should make the run **slower**, not impossible — refusing a 40-minute
/// job because a browser is open trades the user's time for a resource that costs
/// nothing to give back.
///
/// The ladder, in order: threads `N→1`, then fold budget `cap→floor`. The combo
/// side is priced at its shardable floor ([`MIN_SHARD_COMBOS`]), because
/// `plan_shards` can always cut the combo space down to that regardless of how
/// many combos were requested.
///
/// The **only** refusal left is a true-floor overflow: one thread, one token's
/// series, one minimum fold batch and one minimum shard still don't fit. That is a
/// genuine "this machine cannot do this", not a scheduling preference.
pub(crate) fn plan_sweep_sizing(
    preferred_threads: usize,
    max_series_bytes: usize,
    planned_combos: usize,
) -> Result<SweepPlan> {
    let mb = |b: u64| b / (1024 * 1024);
    let preferred_threads = preferred_threads.max(1);
    let fold_cap = (SWEEP_FOLD_BUDGET_CAP_MB as u64).saturating_mul(1024 * 1024);
    let fold_floor = (SWEEP_FOLD_BUDGET_FLOOR_MB as u64).saturating_mul(1024 * 1024);
    let mut notes = Vec::new();

    // Host RAM unreadable (neither Windows nor Linux): the guard is inert, so say so
    // rather than silently sizing against the flat fallback ceiling.
    let Some(usable) = usable_host_bytes() else {
        notes.push(format!(
            "host free RAM is unreadable on this platform — sizing against the flat {} MB \
             admission cap instead of live free RAM",
            DEFAULT_SWEEP_ADMISSION_BUDGET_MB
        ));
        set_fold_budget_ceiling(None);
        let wave = series_wave_size(max_series_bytes, preferred_threads);
        return Ok(SweepPlan { threads: preferred_threads, wave, fold_budget: fold_cap, notes });
    };

    let budget = usable.saturating_sub(SWEEP_ALLOC_SLACK_BYTES);
    let combo_floor = estimate_generic_combo_side_bytes(planned_combos.min(MIN_SHARD_COMBOS));
    let for_series_fold = budget.saturating_sub(combo_floor);

    // The true floor: 1 thread × 1 token's series + the smallest fold batch.
    let floor_need = (max_series_bytes as u64).saturating_add(fold_floor);
    if floor_need > for_series_fold {
        let avail = crate::sweep::obs::host_memory_bytes().map(|(_, a)| a).unwrap_or(0);
        bail!(
            "even the minimum plan (1 thread, 1 token series {} MB + {} MB fold + {} MB \
             combo shard) needs {} MB, over the {} MB usable (host free {} MB − {} MB desktop \
             reserve − {} MB slack) — this corpus cannot be swept on this machine as configured: \
             lower the desktop reserve, tighten token_cap / the date range, or free RAM",
            mb(max_series_bytes as u64),
            mb(fold_floor),
            mb(combo_floor),
            mb(floor_need.saturating_add(combo_floor).saturating_add(SWEEP_ALLOC_SLACK_BYTES)),
            mb(budget),
            mb(avail),
            ram_reserve_mb(),
            mb(SWEEP_ALLOC_SLACK_BYTES),
        );
    }

    // Rung 1 — threads. Largest count ≤ preferred whose series wave fits what's
    // left once the minimum fold batch is reserved. `unwrap_or(1)` is unreachable
    // (the floor check above proves 1 thread fits) but keeps this total.
    let threads = threads_fitting_admission(
        preferred_threads,
        max_series_bytes,
        for_series_fold.saturating_sub(fold_floor),
    )
    .unwrap_or(1);

    // Rung 2 — fold budget: whatever remains after the series wave, clamped into
    // `floor..=cap`. Installed as a *ceiling*, so the per-call live sizing in
    // `sweep_memory_budget_bytes` can still shrink below it mid-run.
    let series_peak = (threads as u64).saturating_mul(max_series_bytes as u64);
    let fold_budget = for_series_fold.saturating_sub(series_peak).clamp(fold_floor, fold_cap);
    set_fold_budget_ceiling(Some(fold_budget));

    // Wave is computed after the ceiling is installed so it sizes against the plan.
    let wave = series_wave_size(max_series_bytes, threads);

    if threads < preferred_threads {
        notes.push(format!(
            "low free RAM ({} MB usable) — running on {threads} of {preferred_threads} threads, \
             so this run will take roughly {:.1}× longer than usual",
            mb(usable),
            preferred_threads as f64 / threads as f64,
        ));
    }
    if fold_budget < fold_cap {
        notes.push(format!(
            "fold buffers capped at {} MB (of {} MB) — smaller combo batches, more series passes",
            mb(fold_budget),
            mb(fold_cap),
        ));
    }
    if host_under_ram_reserve() {
        notes.push(format!(
            "host free RAM is already under the {} MB desktop reserve — running at minimum \
             footprint; results are streamed per group, so progress is kept either way",
            ram_reserve_mb(),
        ));
    }

    Ok(SweepPlan { threads, wave, fold_budget, notes })
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

/// The redesigned generic engine's single (unprefixed) grouped-sweep table set.
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
        _ => None,
    }
}

/// Strategy ids with a grouped-sweep implementation (for error messages / UI).
pub fn strategy_ids() -> &'static [&'static str] {
    &["generic"]
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
    // Corpus-wide volume-ix patterns for flow-metric axes (`None` = non-flow).
    volume_ix_patterns: Option<Vec<Vec<String>>>,
    coarse_observer: Arc<dyn SweepObserver + Send>,
    observer: Arc<dyn SweepObserver + Send>,
    sink: Arc<dyn GroupSink + Send + Sync>,
) -> Result<GroupedSweepOutput> {
    match strategy_id {
        "generic" => {
            sweep_generic(
                axes_json, method, refine, corpus, fields, width, min_tokens, floor, max_combos,
                buy_amount_sol, volume_ix_patterns, coarse_observer, observer, sink,
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
    volume_ix_patterns: Option<Vec<Vec<String>>>,
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

    if model.references_flow() && volume_ix_patterns.is_none() {
        bail!(
            "axes reference m_flow_split/m_flow_window but volume_ix_patterns is missing — \
             supply volume_ix_patterns (string[][]) or drop the flow axes"
        );
    }
    let flow_patterns = volume_ix_patterns.as_ref().map(|p| {
        hunter_engine::metrics::flow_split::FlowPatterns::from_label_sequences(p)
    });

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
    let strategy =
        GenericSweepStrategy::new(model, buy_amount_sol, chrono::Utc::now(), flow_patterns);
    let max_series_bytes = corpus
        .tokens
        .iter()
        .map(|t| strategy.series_bytes_estimate(t))
        .max()
        .unwrap_or(0);
    let (admission_raw, host_total_mb, host_avail_mb) = effective_admission_budget_bytes();
    let mb = |b: u64| b / (1024 * 1024);

    // Plan the run's sizing down to whatever free RAM allows (threads → fold budget)
    // rather than admitting-or-refusing at the preferred sizing. Only a true-floor
    // overflow still refuses — see `plan_sweep_sizing`.
    let plan = plan_sweep_sizing(preferred_threads, max_series_bytes, planned)?;
    let SweepPlan { threads, wave, fold_budget, notes } = plan;
    // Report every degradation to the operator: a run that is 4× slower because the
    // box was busy should say so, not look like an unexplained stall.
    for note in &notes {
        tracing::warn!(note = %note, "generic sweep: degraded sizing");
        observer.notice(note);
    }
    let admission_budget = admission_raw.saturating_sub(fold_budget);
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
    // Sized, not gated: `max_combos_per_shard` reads live free RAM and the plan's fold
    // ceiling, so the combo side shrinks to fit rather than refusing. Spill+merge
    // (`shard::plan_shards`) stitches the shards back into one ranked result set.
    let shard_peak = crate::sweep::shard::max_combos_per_shard(wave, max_series_bytes).min(planned);

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
    // No re-admission against the realised sample: `plan_shards` / `combo_batch_size`
    // re-read live free RAM on every call during the fold, so the realised combo side
    // is sized down continuously rather than judged once here. `params.len() ≤ planned`
    // anyway, so the plan above is already a conservative bound.

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
// Single-combo re-simulation (for the token-results drill-in endpoint)
// ---------------------------------------------------------------------------

/// Re-simulate a single stored combo on a pre-filtered token slice.
///
/// Runs sequentially (no rayon): one combo per token is O(tokens) trivially cheap,
/// far less than the DB round-trips that preceded it. Returns one result per token,
/// always including non-fired tokens so the caller can display the full group slice.
///
/// `as_of` is the run's **own** "now" (its `created_at`), not wall-clock — see
/// [`simulate_generic_one_combo`] for why re-deriving it here corrupts the drill-in
/// (parity plan B7).
pub fn simulate_one_combo(
    strategy_id: &str,
    tokens: &[CorpusToken],
    params_json: &Value,
    buy_amount_sol: f64,
    as_of: chrono::DateTime<chrono::Utc>,
    volume_ix_patterns: Option<&[Vec<String>]>,
) -> Result<Vec<ComboTokenResult>> {
    match strategy_id {
        "generic" => simulate_generic_one_combo(
            tokens,
            params_json,
            buy_amount_sol,
            as_of,
            volume_ix_patterns,
        ),
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
/// precompute-then-scan the grouped sweep used — no axes model needed.
///
/// Deadness is judged against the **run's** `as_of` (its `created_at`), not against
/// wall-clock now. Re-deriving "now" here made a drill-in disagree with the very row
/// it drills into: a position the sweep recorded as `Open` becomes `Dead` once
/// enough real time has passed for the token to cross the quiet window, so the same
/// combo showed different exits and a different `total_pnl_sol` depending on *when
/// the user clicked it* (parity plan B7). Passing the run's own instant makes the
/// drill-in a faithful re-run of the stored result.
///
/// The scan carries each fill row's `entry_slot`/`exit_slot` (the slot of the real
/// trade the fill executes against — the fill row's own trade, or the next print when
/// the fill lands on a tick), so the handler's fill → `tx_signature` lookup resolves
/// the chart's entry/exit tx markers here the same way it does for tpsl combos.
fn simulate_generic_one_combo(
    tokens: &[CorpusToken],
    params_json: &Value,
    buy_amount_sol: f64,
    as_of: chrono::DateTime<chrono::Utc>,
    volume_ix_patterns: Option<&[Vec<String>]>,
) -> Result<Vec<ComboTokenResult>> {
    use hunter_engine::arm::CompiledRule;
    use hunter_engine::event::{LoadedRule, RuleId, TradeMode};
    use hunter_engine::fingerprint::FingerprintId;
    use hunter_engine::rule_params::RuleParams;
    use trading_core::config::constants::sol_to_lamports;
    use trading_core::strategies::kernel::CostModel;

    use crate::sweep::generic::strategy::{
        build_series_with_flow, columns_for, scan, sparse_grid_for,
    };

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
    let cost = CostModel::pumpfun_default();
    let flow_patterns = volume_ix_patterns.map(|p| {
        hunter_engine::metrics::flow_split::FlowPatterns::from_label_sequences(p)
    });

    let mut results = Vec::with_capacity(tokens.len());
    for tt in tokens {
        let series = build_series_with_flow(
            tt,
            columns.clone(),
            &grid,
            as_of,
            flow_patterns.as_ref(),
        );
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

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1024 * 1024 * 1024;

    #[test]
    fn usable_without_baseline_is_live_reading() {
        // baseline = 0 (pre-corpus-load): structural term is off, so it's exactly
        // the old `available − reserve`. Admission sizing must be unchanged.
        assert_eq!(usable_from(16 * GB, 5 * GB, GB, 0), 4 * GB);
    }

    #[test]
    fn usable_with_baseline_ignores_own_transient_growth() {
        // The starvation case: 16 GB box, 1 GB reserve, corpus baseline 5 GB. The
        // sweep's own fold buffers have driven `available` to 500 MB (< reserve →
        // live term is 0). Structural = 16 − 1 − 5 = 10 GB, so the fit test sees
        // 10 GB, not 0 — token-outer becomes reachable.
        assert_eq!(
            usable_from(16 * GB, GB / 2, GB, 5 * GB),
            10 * GB,
            "own transient RSS must not be subtracted from own headroom"
        );
    }

    #[test]
    fn usable_never_exceeds_total_minus_reserve() {
        // The never-OOM bound: at the peak, baseline (resident) + usable (transient)
        // must leave the desktop reserve free. usable ≤ total − reserve − baseline,
        // so baseline + usable ≤ total − reserve for every reading.
        for &(total, avail, reserve, baseline) in &[
            (16 * GB, 8 * GB, GB, 5 * GB),
            (16 * GB, GB / 4, GB, 6 * GB),
            (32 * GB, 20 * GB, 2 * GB, 10 * GB),
        ] {
            let usable = usable_from(total, avail, reserve, baseline);
            assert!(
                baseline + usable <= total - reserve,
                "baseline {baseline} + usable {usable} must fit under total {total} − reserve {reserve}"
            );
        }
    }

    #[test]
    fn usable_prefers_live_when_box_genuinely_freer() {
        // Another app exited: `available` (12 GB) exceeds the structural prediction
        // (16 − 1 − 5 = 10 GB). Take the live gain rather than cap at the baseline.
        assert_eq!(usable_from(16 * GB, 13 * GB, GB, 5 * GB), 12 * GB);
    }

    #[test]
    fn usable_zero_only_when_genuinely_full() {
        // Both terms under the reserve → a genuinely full box still reports 0.
        assert_eq!(usable_from(16 * GB, GB / 2, GB, 16 * GB), 0);
    }

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

    /// The whole point of the ladder: a run that does not fit at preferred sizing
    /// must come back **degraded**, never refused. Uses a series estimate large
    /// enough that the full thread pool cannot possibly fit any real box's free RAM.
    #[test]
    fn plan_degrades_instead_of_refusing_when_preferred_does_not_fit() {
        set_fold_budget_ceiling(None);
        let preferred = 16;
        // 2 GB per token series: 16 threads = 32 GB, which no test box has free.
        let huge_series = 2 * 1024 * 1024 * 1024usize;
        match plan_sweep_sizing(preferred, huge_series, 50_000) {
            Ok(plan) => {
                // Degraded, not refused — and it explained itself.
                assert!(plan.threads >= 1 && plan.threads <= preferred);
                assert!(plan.wave >= 1, "wave must keep at least one token resident");
                assert!(plan.fold_budget >= (SWEEP_FOLD_BUDGET_FLOOR_MB as u64) * 1024 * 1024);
                if plan.threads < preferred {
                    assert!(!plan.notes.is_empty(), "degradation must be reported");
                }
            }
            // The only legal refusal: one token's series alone overflows the box.
            // Then the message must name the floor, not offer a retry that can't help.
            Err(e) => {
                let msg = e.to_string();
                assert!(msg.contains("minimum plan"), "floor refusal must say so: {msg}");
            }
        }
        set_fold_budget_ceiling(None);
    }

    /// A trivial run on a readable host must plan at full preferred sizing with no
    /// notes — the ladder must not degrade a run that fits fine.
    #[test]
    fn plan_keeps_preferred_sizing_when_everything_fits() {
        set_fold_budget_ceiling(None);
        // Zero-byte series (the legacy tpsl/swing `()` token state) can always fit.
        let plan = plan_sweep_sizing(8, 0, 1_000).expect("a zero-series run always fits");
        assert_eq!(plan.threads, 8);
        assert!(plan.wave >= 1);
        set_fold_budget_ceiling(None);
    }

    /// The planned fold budget is a **ceiling**, not a pin: live sizing may go lower
    /// (RAM dropped mid-run) but never above the plan, and never below the floor.
    #[test]
    fn fold_ceiling_bounds_live_budget_without_pinning_it() {
        let floor = (SWEEP_FOLD_BUDGET_FLOOR_MB as u64) * 1024 * 1024;
        set_fold_budget_ceiling(None);
        let auto = sweep_memory_budget_bytes();

        let tight = 64 * 1024 * 1024;
        set_fold_budget_ceiling(Some(tight));
        let capped = sweep_memory_budget_bytes();
        assert!(capped <= tight.max(floor), "ceiling must bound the live budget");
        assert!(capped >= floor, "must never starve below the floor");
        assert!(capped <= auto.max(floor), "a ceiling can only lower, never raise");

        set_fold_budget_ceiling(None);
        assert_eq!(sweep_memory_budget_bytes(), auto, "clearing restores auto sizing");
    }

    #[test]
    fn ram_reserve_override_clamps_to_bounds() {
        // The per-run knob the sweep form sends: a valid MB passes through, out-of-
        // range values clamp to the floor/ceiling, and an absent value (the form
        // omits it when it still matches the default) resolves to the default.
        assert_eq!(clamp_ram_reserve_mb(Some(512)), 512);
        assert_eq!(clamp_ram_reserve_mb(Some(2048)), 2048);
        assert_eq!(clamp_ram_reserve_mb(Some(0)), MIN_SWEEP_RAM_RESERVE_MB);
        assert_eq!(clamp_ram_reserve_mb(Some(1)), MIN_SWEEP_RAM_RESERVE_MB);
        assert_eq!(clamp_ram_reserve_mb(Some(usize::MAX)), MAX_SWEEP_RAM_RESERVE_MB);
        assert_eq!(clamp_ram_reserve_mb(None), DEFAULT_SWEEP_RAM_RESERVE_MB);
    }

    #[test]
    fn larger_reserve_yields_less_usable() {
        // The selector's whole effect: at identical host readings, a bigger reserve
        // leaves strictly less RAM usable for the sweep peak (and every choice is
        // subtracted 1:1). Pure `usable_from` so it can't race the global.
        let mb = |n: u64| n * 1024 * 1024;
        let (total, avail) = (16 * GB, 8 * GB);
        let usable_256 = usable_from(total, avail, mb(256), 0);
        let usable_1g = usable_from(total, avail, mb(1024), 0);
        let usable_4g = usable_from(total, avail, mb(4096), 0);
        assert!(usable_256 > usable_1g && usable_1g > usable_4g, "bigger reserve → less usable");
        assert_eq!(usable_256 - usable_1g, mb(1024) - mb(256), "reserve subtracts 1:1");
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
