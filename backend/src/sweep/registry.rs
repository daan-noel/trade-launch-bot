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

use crate::models::grouped_sweep::ComboTokenResult;
use crate::models::{Tpsl1Rule, Tpsl2Rule};
use crate::storage::repositories::grouped_sweep_repo::GroupedSweepTables;
use crate::sweep::corpus::{Corpus, TokenTrades};
use crate::sweep::engine::fill_outcomes;
use crate::sweep::grouped_engine::{run_grouped_with_refine, CoverageFloor, GroupResult, GroupSink};
use crate::sweep::grouping::GroupField;
use crate::sweep::progress::SweepObserver;
use crate::sweep::strategies::tpsl1::{
    AxesSpec as Tpsl1AxesSpec, Tpsl1Axes, Tpsl1Strategy,
};
use crate::sweep::strategies::tpsl2::{AxesSpec, Tpsl2Axes, Tpsl2Strategy};
use crate::sweep::strategy::{ExitCode, ParamSpace, RefineSpec, SweepMethod};

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

/// Default ceiling (MB) on the per-combo fold accumulators a single run may hold at
/// once. The sweep folds combos in batches of `combo_batch_size` so peak ≈
/// `threads × BATCH × sizeof(ComboAgg)`; this budget is what sizes `BATCH` (Phase
/// 2.5). A combo set too big to hold at once is swept in sequential batches rather
/// than rejected (the prior behaviour) — `HARD_MAX_COMBOS` still bounds the work.
/// Override with `SWEEP_MEMORY_BUDGET_MB`.
///
/// 256 MB is generous: at MAX_COMBOS=5_000 combos × 2 threads × ~600 B/ComboAgg
/// actual peak is ~6 MB. Even at HARD_MAX_COMBOS=500_000 it's ~600 MB, so a
/// 256 MB default batches those in ~2 passes with zero quality impact. The old
/// 1024 MB default appeared verbatim in the holistic admission guard (rss + corpus +
/// this), causing false OOM rejections on 4 GB boxes running a live server process.
const DEFAULT_SWEEP_MEMORY_BUDGET_MB: usize = 256;

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
    min_tokens: usize,
    floor: CoverageFloor,
    max_combos: Option<usize>,
    coarse_observer: Arc<dyn SweepObserver + Send>,
    observer: Arc<dyn SweepObserver + Send>,
    sink: Arc<dyn GroupSink + Send + Sync>,
) -> Result<GroupedSweepOutput> {
    match strategy_id {
        "tpsl1" => {
            sweep_tpsl1(
                axes_json, method, refine, corpus, fields, min_tokens, floor, max_combos,
                coarse_observer, observer, sink,
            )
            .await
        }
        "tpsl2" => {
            sweep_tpsl2(
                axes_json, method, refine, corpus, fields, min_tokens, floor, max_combos,
                coarse_observer, observer, sink,
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
                // fill_outcomes → resolve_entry/exit → exit ladder + cohort fns
                // stack up enough frames to overflow on large corpora. 8 MB matches
                // the Linux thread default and gives comfortable headroom.
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
    tokens: &[TokenTrades],
    params_json: &Value,
) -> Result<Vec<ComboTokenResult>> {
    match strategy_id {
        "tpsl2" => simulate_tpsl2_one_combo(tokens, params_json),
        "tpsl1" => simulate_tpsl1_one_combo(tokens, params_json),
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
        ExitCode::CohortExit => "CohortExit",
    }
}

fn simulate_tpsl2_one_combo(
    tokens: &[TokenTrades],
    params_json: &Value,
) -> Result<Vec<ComboTokenResult>> {
    let has_cohort = params_json.get("exit_cohort_ratio").and_then(|v| v.as_f64()).is_some();
    let strategy = Tpsl2Strategy::for_replay(sweep_base_rule_tpsl2(), has_cohort);
    let combo = strategy.combo_from_params_json(params_json)?;
    let params = std::slice::from_ref(&combo);
    let noop = Noop;
    let mut results = Vec::with_capacity(tokens.len());
    for tt in tokens {
        let mut outs = Vec::with_capacity(1);
        let _ = fill_outcomes(&strategy, params, &tt.trades, &noop, &mut outs);
        let o = outs
            .into_iter()
            .next()
            .unwrap_or_else(crate::sweep::strategy::TokenOutcome::no_entry);
        results.push(ComboTokenResult {
            mint: tt.mint.clone(),
            symbol: tt.symbol.clone(),
            fired: o.fired,
            pnl_sol: o.pnl_sol,
            pnl_pct: o.pnl_percent,
            holding_secs: o.holding_secs,
            exit: exit_label(o.exit).to_string(),
            entry_time: o.entry_time.map(|t| t.to_rfc3339()),
            entry_price: o.entry_price,
            exit_time: o.exit_time.map(|t| t.to_rfc3339()),
            exit_price: o.exit_price,
            created_at: None,
            creator_wallet: None,
            ath_price: None,
            ath_timestamp: None,
            current_price: None,
            market_cap: None,
            volume_sol: None,
            trade_count: None,
            is_migrated: None,
            is_dead: None,
        });
    }
    Ok(results)
}

fn simulate_tpsl1_one_combo(
    tokens: &[TokenTrades],
    params_json: &Value,
) -> Result<Vec<ComboTokenResult>> {
    let strategy = Tpsl1Strategy::for_replay(sweep_base_rule_tpsl1());
    let combo = strategy.combo_from_params_json(params_json)?;
    let params = std::slice::from_ref(&combo);
    let noop = Noop;
    let mut results = Vec::with_capacity(tokens.len());
    for tt in tokens {
        let mut outs = Vec::with_capacity(1);
        let _ = fill_outcomes(&strategy, params, &tt.trades, &noop, &mut outs);
        let o = outs
            .into_iter()
            .next()
            .unwrap_or_else(crate::sweep::strategy::TokenOutcome::no_entry);
        results.push(ComboTokenResult {
            mint: tt.mint.clone(),
            symbol: tt.symbol.clone(),
            fired: o.fired,
            pnl_sol: o.pnl_sol,
            pnl_pct: o.pnl_percent,
            holding_secs: o.holding_secs,
            exit: exit_label(o.exit).to_string(),
            entry_time: o.entry_time.map(|t| t.to_rfc3339()),
            entry_price: o.entry_price,
            exit_time: o.exit_time.map(|t| t.to_rfc3339()),
            exit_price: o.exit_price,
            created_at: None,
            creator_wallet: None,
            ath_price: None,
            ath_timestamp: None,
            current_price: None,
            market_cap: None,
            volume_sol: None,
            trade_count: None,
            is_migrated: None,
            is_dead: None,
        });
    }
    Ok(results)
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
