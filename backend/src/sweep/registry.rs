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
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::{Tpsl1Rule, Tpsl2Rule};
use crate::storage::repositories::grouped_sweep_repo::GroupedSweepTables;
use crate::storage::repositories::tpsl1_strategy_rule_repo::Tpsl1StrategyRuleRepo;
use crate::storage::repositories::tpsl2_strategy_rule_repo::Tpsl2StrategyRuleRepo;
use crate::sweep::corpus::Corpus;
use crate::sweep::grouped_engine::{run_grouped_sweep, GroupResult};
use crate::sweep::grouping::GroupField;
use crate::sweep::progress::SweepObserver;
use crate::sweep::strategies::tpsl1::{
    AxesSpec as Tpsl1AxesSpec, Tpsl1Axes, Tpsl1Strategy,
};
use crate::sweep::strategies::tpsl2::{AxesSpec, Tpsl2Axes, Tpsl2Strategy};
use crate::sweep::strategy::{ParamSpace, Strategy, SweepMethod};

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
/// fingerprinted `corpus`. Async because resolving the base rule hits the DB; the
/// CPU sweep itself runs on a bounded blocking pool.
// One thin dispatch fn called once from the handler — bundling these into a
// struct would only add an indirection for a single call site.
#[allow(clippy::too_many_arguments)]
pub async fn run_grouped(
    pool: PgPool,
    strategy_id: &str,
    rule_id: Option<Uuid>,
    axes_json: Value,
    method: SweepMethod,
    corpus: Corpus,
    fields: Vec<GroupField>,
    min_tokens: usize,
    max_combos: Option<usize>,
    observer: Arc<dyn SweepObserver + Send>,
) -> Result<GroupedSweepOutput> {
    match strategy_id {
        "tpsl1" => {
            sweep_tpsl1(
                pool, rule_id, axes_json, method, corpus, fields, min_tokens, max_combos, observer,
            )
            .await
        }
        "tpsl2" => {
            sweep_tpsl2(
                pool, rule_id, axes_json, method, corpus, fields, min_tokens, max_combos, observer,
            )
            .await
        }
        other => bail!(
            "strategy '{other}' has no grouped-sweep implementation yet (supported: {:?})",
            strategy_ids()
        ),
    }
}

/// Rayon threads for the in-process sweep: leave ≥2 cores for the live trading
/// hot path (ingest / sell-confirm), at least 1.
fn bounded_threads() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get().saturating_sub(2).max(1))
        .unwrap_or(2)
}

// ---------------------------------------------------------------------------
// TPSL2 strategy entry point
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
async fn sweep_tpsl2(
    pool: PgPool,
    rule_id: Option<Uuid>,
    axes_json: Value,
    method: SweepMethod,
    corpus: Corpus,
    fields: Vec<GroupField>,
    min_tokens: usize,
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

    let base = resolve_tpsl2_rule(&pool, rule_id).await?;
    let strategy = Tpsl2Strategy::new(base, axes);
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

    // Capture the param JSON before `strategy` moves into the blocking task.
    let combo_params: Vec<Value> = params.iter().map(|p| strategy.params_json(p)).collect();
    let combo_count = params.len();
    let threads = bounded_threads();

    let groups = tokio::task::spawn_blocking(move || -> Result<Vec<GroupResult>> {
        // Bound the pool so the sweep can't saturate every core in the live
        // process; `install` makes the inner `par_iter` use this pool.
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .thread_name(|i| format!("grouped-sweep-{i}"))
            .build()
            .map_err(|e| anyhow!("rayon pool build failed: {e}"))?;
        pool.install(|| {
            run_grouped_sweep(&strategy, &params, &corpus, &fields, min_tokens, observer.as_ref())
        })
    })
    .await??;

    Ok(GroupedSweepOutput {
        combo_count,
        combo_params,
        axes_json,
        groups,
    })
}

/// Resolve the base TPSL2 rule the swept params overlay: the given id, else the
/// first rule in the DB (a template to overlay).
async fn resolve_tpsl2_rule(pool: &PgPool, rule_id: Option<Uuid>) -> Result<Tpsl2Rule> {
    let repo = Tpsl2StrategyRuleRepo::new(pool.clone());
    match rule_id {
        Some(id) => repo
            .find_by_id(id)
            .await?
            .ok_or_else(|| anyhow!("no tpsl2 rule with id {id}")),
        None => repo
            .find_all()
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("no tpsl2 rules in DB — create one to use as the base template")),
    }
}

// ---------------------------------------------------------------------------
// TPSL1 strategy entry point
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
async fn sweep_tpsl1(
    pool: PgPool,
    rule_id: Option<Uuid>,
    axes_json: Value,
    method: SweepMethod,
    corpus: Corpus,
    fields: Vec<GroupField>,
    min_tokens: usize,
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

    let base = resolve_tpsl1_rule(&pool, rule_id).await?;
    let strategy = Tpsl1Strategy::new(base, axes);
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

    // Capture the param JSON before `strategy` moves into the blocking task.
    let combo_params: Vec<Value> = params.iter().map(|p| strategy.params_json(p)).collect();
    let combo_count = params.len();
    let threads = bounded_threads();

    let groups = tokio::task::spawn_blocking(move || -> Result<Vec<GroupResult>> {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .thread_name(|i| format!("grouped-sweep-{i}"))
            .build()
            .map_err(|e| anyhow!("rayon pool build failed: {e}"))?;
        pool.install(|| {
            run_grouped_sweep(&strategy, &params, &corpus, &fields, min_tokens, observer.as_ref())
        })
    })
    .await??;

    Ok(GroupedSweepOutput {
        combo_count,
        combo_params,
        axes_json,
        groups,
    })
}

/// Resolve the base TPSL1 rule the swept params overlay: the given id, else the
/// first rule in the DB (a template to overlay).
async fn resolve_tpsl1_rule(pool: &PgPool, rule_id: Option<Uuid>) -> Result<Tpsl1Rule> {
    let repo = Tpsl1StrategyRuleRepo::new(pool.clone());
    match rule_id {
        Some(id) => repo
            .find_by_id(id)
            .await?
            .ok_or_else(|| anyhow!("no tpsl1 rule with id {id}")),
        None => repo
            .find_all()
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("no tpsl1 rules in DB — create one to use as the base template")),
    }
}
