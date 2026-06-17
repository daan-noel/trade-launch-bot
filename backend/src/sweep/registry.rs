//! Strategy registry — the **one** place a strategy is wired into the grouped
//! sweep. It maps a `strategy_id` to (a) its per-strategy DB table triple and
//! (b) its concrete sweep entry point. The handler and repo are otherwise fully
//! generic (table-name- and data-driven), so adding "swing" later means: write a
//! `strategies/swing.rs` (`Strategy` + `ParamSpace` + `AxesSpec`), add its tables
//! + a match arm here, and a `0002_*` migration — nothing else changes.
//!
//! The CPU-heavy sweep runs in a bounded rayon pool inside `spawn_blocking` so it
//! can never starve the live trading hot path (ingest / sell-confirm).

use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;

use crate::models::{Tpsl1Rule, Tpsl2Rule};
use crate::storage::repositories::grouped_sweep_repo::GroupedSweepTables;
use crate::sweep::aggregate::ComboAgg;
use crate::sweep::corpus::Corpus;
use crate::sweep::grouped_engine::{run_grouped_with_refine, CoverageFloor, GroupResult};
use crate::sweep::grouping::GroupField;
use crate::sweep::progress::SweepObserver;
use crate::sweep::strategies::tpsl1::{
    AxesSpec as Tpsl1AxesSpec, Tpsl1Axes, Tpsl1Strategy,
};
use crate::sweep::strategies::tpsl2::{AxesSpec, Tpsl2Axes, Tpsl2Strategy};
use crate::sweep::strategy::{ParamSpace, RefineSpec, Strategy, SweepMethod};

/// Default cap on the param combos a single grouped sweep evaluates **per group**.
/// Bounds the `groups × combos × tokens` work; the handler rejects a full grid
/// whose product exceeds this before any sweep runs, and random/LHS draws are
/// clamped to it. A run may raise this per-request (up to [`HARD_MAX_COMBOS`]).
pub const MAX_COMBOS: usize = 5_000;

/// Absolute backstop on the per-request combo cap. A run can opt into more than
/// [`MAX_COMBOS`] (sweeps are infrequent and the caller accepts the wait), but
/// never past this — so a fat-fingered override still can't run away with the
/// `groups × combos × tokens` work or monopolise the bounded rayon pool.
pub const HARD_MAX_COMBOS: usize = 500_000;

/// Resolve the effective per-group combo cap for a run: the request override if
/// given, else the default, clamped to the hard backstop (and ≥ 1).
fn effective_cap(max_combos: Option<usize>) -> usize {
    max_combos.unwrap_or(MAX_COMBOS).clamp(1, HARD_MAX_COMBOS)
}

/// Default ceiling (MB) on the per-combo fold accumulators a single run may
/// allocate. The sweep holds one contiguous `vec![ComboAgg; combos]` per active
/// fold; the small-group phase runs up to `pool_threads` of those concurrently, so
/// peak ≈ `threads × combos × sizeof(ComboAgg)`. `HARD_MAX_COMBOS` alone doesn't
/// bound this (a 500k-combo run is hundreds of MB × threads), so a too-large run
/// used to OOM-**abort the whole process** (a failed Rust alloc calls `abort()`).
/// This budget converts that into a clean rejected request. Override with
/// `SWEEP_MEMORY_BUDGET_MB`.
const DEFAULT_SWEEP_MEMORY_BUDGET_MB: usize = 1_024;

/// The fold-accumulator memory budget in bytes (`SWEEP_MEMORY_BUDGET_MB` or the
/// default), saturating so a fat-fingered override can't wrap.
fn sweep_memory_budget_bytes() -> u64 {
    std::env::var("SWEEP_MEMORY_BUDGET_MB")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|&m| m >= 1)
        .unwrap_or(DEFAULT_SWEEP_MEMORY_BUDGET_MB)
        .saturating_mul(1024 * 1024) as u64
}

/// Reject — **before allocating** — a run whose worst-case fold-accumulator memory
/// (`threads × combos × sizeof(ComboAgg)`) would blow the budget, so an oversized
/// `max_combos` fails as a clear error instead of OOM-aborting the backend. `cap`
/// is the worst-case combo count (the refine union is also capped at it). Returns
/// `Ok` with nothing; on success the caller logs the projection.
fn check_sweep_memory(cap: usize, threads: usize) -> Result<()> {
    let per = std::mem::size_of::<ComboAgg>() as u64;
    let budget = sweep_memory_budget_bytes();
    let projected = fits_sweep_memory(cap, threads, per, budget)?;
    tracing::info!(
        cap,
        threads,
        per_combo_bytes = per,
        projected_mb = projected / (1024 * 1024),
        budget_mb = budget / (1024 * 1024),
        "grouped sweep: fold-accumulator memory within budget"
    );
    Ok(())
}

/// Pure budget check: the worst-case fold-accumulator bytes
/// (`threads × combos × per`, saturating) against `budget`. Returns the projected
/// bytes when it fits, else a caller-facing error. Split out so the arithmetic is
/// unit-testable without touching env / `ComboAgg` layout.
fn fits_sweep_memory(cap: usize, threads: usize, per: u64, budget: u64) -> Result<u64> {
    let projected = (cap as u64)
        .saturating_mul(threads as u64)
        .saturating_mul(per);
    if projected > budget {
        let mb = |b: u64| b / (1024 * 1024);
        bail!(
            "sweep would allocate ~{} MB of fold accumulators ({cap} combos × {threads} threads × {per} B), \
             over the {} MB budget — lower max_combos, narrow the axes, or raise SWEEP_MEMORY_BUDGET_MB",
            mb(projected),
            mb(budget),
        );
    }
    Ok(projected)
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
};

/// TPSL1's own grouped-sweep tables (same shape as TPSL2's, separate per the
/// per-strategy-tables design). Mirror these names in the `0004` migration.
const TPSL1_TABLES: GroupedSweepTables = GroupedSweepTables {
    runs: "tpsl1_grouped_sweep_runs",
    groups: "tpsl1_grouped_sweep_groups",
    results: "tpsl1_grouped_sweep_results",
};

/// The DB table triple for a strategy's grouped sweeps, or `None` for an unknown
/// / not-yet-wired strategy.
pub fn tables_for(strategy_id: &str) -> Option<GroupedSweepTables> {
    match strategy_id {
        "tpsl1" => Some(TPSL1_TABLES),
        "tpsl2" => Some(TPSL2_TABLES),
        _ => None,
    }
}

/// Strategy ids with a grouped-sweep implementation (for error messages / UI).
pub fn strategy_ids() -> &'static [&'static str] {
    &["tpsl1", "tpsl2"]
}

// ---------------------------------------------------------------------------
// Sweep output (generic — strategy-blind from here on)
// ---------------------------------------------------------------------------

/// The strategy-agnostic result of a grouped sweep, ready for the generic repo:
/// the per-group sweep results plus the combo→param-JSON map (indexed by
/// `combo_id`) and the resolved axes JSON to store on the run.
pub struct GroupedSweepOutput {
    /// Number of param combos evaluated per group (== `combo_params.len()`).
    pub combo_count: usize,
    /// `params_json` for each combo, indexed by `combo_id`.
    pub combo_params: Vec<Value>,
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
    min_tokens: usize,
    floor: CoverageFloor,
    max_combos: Option<usize>,
    observer: Arc<dyn SweepObserver + Send>,
) -> Result<GroupedSweepOutput> {
    match strategy_id {
        "tpsl1" => {
            sweep_tpsl1(
                axes_json, method, refine, corpus, fields, min_tokens, floor, max_combos,
                observer,
            )
            .await
        }
        "tpsl2" => {
            sweep_tpsl2(
                axes_json, method, refine, corpus, fields, min_tokens, floor, max_combos,
                observer,
            )
            .await
        }
        other => bail!(
            "strategy '{other}' has no grouped-sweep implementation yet (supported: {:?})",
            strategy_ids()
        ),
    }
}

/// Tokio runtime worker threads — must stay in sync with `worker_threads` in
/// `main.rs`'s `#[tokio::main]`. The sweep's rayon pool reserves these (plus the
/// actix HTTP workers) so an in-process sweep can't starve the live trading hot
/// path (ingest / sell-confirm) the data-scale guardrails protect.
const TOKIO_WORKER_THREADS: usize = 4;

/// Rayon threads for the in-process sweep. Sized against the *whole* thread
/// budget, not just `cores`: reserve the tokio runtime ([`TOKIO_WORKER_THREADS`])
/// and the actix HTTP workers (`HTTP_WORKERS`, default 2) so on a small box the
/// sweep can't pin the cores ingest / sell-confirm run on. Override explicitly
/// with `SWEEP_RAYON_THREADS`. Always ≥1.
fn bounded_threads() -> usize {
    if let Some(n) = std::env::var("SWEEP_RAYON_THREADS")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|&n| n >= 1)
    {
        return n;
    }
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(TOKIO_WORKER_THREADS);
    let http_workers = std::env::var("HTTP_WORKERS")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(2);
    cores
        .saturating_sub(TOKIO_WORKER_THREADS + http_workers)
        .max(1)
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
    min_tokens: usize,
    floor: CoverageFloor,
    max_combos: Option<usize>,
    observer: Arc<dyn SweepObserver + Send>,
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

    let strategy = Tpsl2Strategy::new(sweep_base_rule_tpsl2(), axes);
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
    // Reject a run whose fold accumulators would exhaust memory before allocating
    // (turns a process-killing OOM abort into a clean error). `cap` bounds the
    // combo count across both the coarse and refine passes.
    check_sweep_memory(cap, threads)?;

    // The coarse→refine driver may grow the combo set (a neighborhood around each
    // group's survivors), so the final param list — and thus its `params_json` — is
    // only known after the sweep. Capture it inside the blocking task.
    let (combo_params, groups) = tokio::task::spawn_blocking(
        move || -> Result<(Vec<Value>, Vec<GroupResult>)> {
            // Bound the pool so the sweep can't saturate every core in the live
            // process; `install` makes the inner `par_iter` use this pool.
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .thread_name(|i| format!("grouped-sweep-{i}"))
                .build()
                .map_err(|e| anyhow!("rayon pool build failed: {e}"))?;
            pool.install(|| {
                let (final_params, groups) = run_grouped_with_refine(
                    &strategy,
                    params,
                    refine,
                    &corpus,
                    &fields,
                    min_tokens,
                    floor,
                    cap,
                    observer.as_ref(),
                )?;
                let combo_params: Vec<Value> =
                    final_params.iter().map(|p| strategy.params_json(p)).collect();
                Ok((combo_params, groups))
            })
        },
    )
    .await??;

    Ok(GroupedSweepOutput {
        combo_count: combo_params.len(),
        combo_params,
        axes_json,
        groups,
    })
}

/// Notional (in SOL) every simulated round-trip is priced at — the **only**
/// base-rule field either sweep actually consumes. Every entry/exit knob is
/// overwritten by the swept axes, and the token-creation filters/limits are never
/// read in the grouped sweep, so the sweep needs no DB rule at all — the base rule
/// is synthesized in-process by `sweep_base_rule_tpsl{1,2}`.
///
/// Set this to the per-trade size you actually intend to trade live: the
/// `CostModel`'s fixed costs (Jito tip + priority fee) are a *fixed* SOL amount
/// per leg, so a larger notional makes friction look smaller (and a smaller one
/// larger) — which shifts the per-combo expectancy ranking. `1.0` is a neutral
/// placeholder, not a recommendation.
const SWEEP_BASE_BUY_AMOUNT_SOL: f64 = 1.0;

/// Synthetic TPSL2 base rule the swept params overlay. Only `buy_amount` is
/// meaningful (see [`SWEEP_BASE_BUY_AMOUNT_SOL`]); every other field is either
/// overwritten by the swept axes or unused in the grouped sweep, so we build it
/// in-memory instead of requiring a DB template rule.
fn sweep_base_rule_tpsl2() -> Tpsl2Rule {
    Tpsl2Rule::new(
        "sweep-synthetic-base".into(),
        None,                  // p_token_initial_buy_sol — unused in sweep
        None,                  // p_token_cu_limit        — unused in sweep
        None,                  // p_token_cu_price        — unused in sweep
        serde_json::json!([]), // p_token_ix_labels       — unused in sweep
        "paper".into(),        // trade_mode              — unused in sweep
        SWEEP_BASE_BUY_AMOUNT_SOL,
        0.0,                   // p_exit_take_profit      — overlaid per combo
        0.0,                   // p_exit_stop_loss        — overlaid per combo
        None,                  // p_token_max_sol_cost    — unused in sweep
        None,                  // p_token_spendable_sol_in — unused in sweep
        None,                  // p_max_concurrent_tokens — unused in grouped sweep
        None,                  // p_max_total_tokens      — unused in grouped sweep
        None,                  // tolerance_pct           — unused in sweep
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
    min_tokens: usize,
    floor: CoverageFloor,
    max_combos: Option<usize>,
    observer: Arc<dyn SweepObserver + Send>,
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

    let strategy = Tpsl1Strategy::new(sweep_base_rule_tpsl1(), axes);
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
    // Reject a run whose fold accumulators would exhaust memory before allocating
    // (turns a process-killing OOM abort into a clean error).
    check_sweep_memory(cap, threads)?;

    // The coarse→refine driver may grow the combo set, so the final param list is
    // only known after the sweep — capture its `params_json` inside the task.
    let (combo_params, groups) = tokio::task::spawn_blocking(
        move || -> Result<(Vec<Value>, Vec<GroupResult>)> {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .thread_name(|i| format!("grouped-sweep-{i}"))
                .build()
                .map_err(|e| anyhow!("rayon pool build failed: {e}"))?;
            pool.install(|| {
                let (final_params, groups) = run_grouped_with_refine(
                    &strategy,
                    params,
                    refine,
                    &corpus,
                    &fields,
                    min_tokens,
                    floor,
                    cap,
                    observer.as_ref(),
                )?;
                let combo_params: Vec<Value> =
                    final_params.iter().map(|p| strategy.params_json(p)).collect();
                Ok((combo_params, groups))
            })
        },
    )
    .await??;

    Ok(GroupedSweepOutput {
        combo_count: combo_params.len(),
        combo_params,
        axes_json,
        groups,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MB: u64 = 1024 * 1024;

    #[test]
    fn within_budget_returns_projected_bytes() {
        // 5k combos × 4 threads × 600 B ≈ 11 MB — comfortably under a 1 GB budget.
        let projected = fits_sweep_memory(5_000, 4, 600, 1_024 * MB).unwrap();
        assert_eq!(projected, 5_000 * 4 * 600);
    }

    #[test]
    fn over_budget_is_rejected_not_aborted() {
        // The reported OOM: ~315k combos × ~600 B × several threads is GBs — must
        // surface as an Err the handler maps to a 4xx, never an allocation.
        let err = fits_sweep_memory(315_000, 6, 600, 1_024 * MB).unwrap_err();
        assert!(err.to_string().contains("over the"), "got: {err}");
    }

    #[test]
    fn projection_saturates_instead_of_wrapping() {
        // A fat-fingered cap can't wrap u64 into a small product that sneaks past
        // the budget — saturation keeps it at the ceiling (rejected).
        assert!(fits_sweep_memory(usize::MAX, usize::MAX, u64::MAX, 1_024 * MB).is_err());
    }
}

/// Synthetic TPSL1 base rule the swept params overlay. As with TPSL2, only
/// `buy_amount` is meaningful (see [`SWEEP_BASE_BUY_AMOUNT_SOL`]) — every other
/// field is overwritten by the swept exit ladder or unused in the grouped sweep.
fn sweep_base_rule_tpsl1() -> Tpsl1Rule {
    Tpsl1Rule::new(
        "sweep-synthetic-base".into(),
        None,                  // p_token_initial_buy_sol — unused in sweep
        None,                  // p_token_cu_limit        — unused in sweep
        None,                  // p_token_cu_price        — unused in sweep
        serde_json::json!([]), // p_token_ix_labels       — unused in sweep
        "paper".into(),        // trade_mode              — unused in sweep
        SWEEP_BASE_BUY_AMOUNT_SOL,
        0.0,                   // p_exit_take_profit      — overlaid per combo
        0.0,                   // p_exit_stop_loss        — overlaid per combo
        None,                  // p_token_max_sol_cost    — unused in sweep
        None,                  // p_token_spendable_sol_in — unused in sweep
        None,                  // p_max_concurrent_tokens — unused in grouped sweep
        None,                  // p_max_total_tokens      — unused in grouped sweep
        None,                  // tolerance_pct           — unused in sweep
        None,                  // p_exit_trailing_stop_pct — overlaid per combo
        None,                  // p_exit_time_stop_secs    — overlaid per combo
        None,                  // p_exit_stall_secs        — overlaid per combo
        None,                  // p_exit_liquidity_drop_pct — overlaid per combo
    )
}
