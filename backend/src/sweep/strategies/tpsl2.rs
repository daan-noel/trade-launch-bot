//! The TPSL2 [`Strategy`] impl. It wraps the **same** pure entry/exit fns the
//! live and DB-backtest paths use — `find_scalp_entry`,
//! `find_worst_case_paper_entry`, `find_trade_driven_exit` — so the sweep
//! resolves byte-identical entry/exit *decisions* to live trading. PnL is the
//! frictionless `round_trip` of the decision prices.
//!
//! This module is the TPSL2 `Strategy` impl registered in
//! [`registry`](crate::sweep::registry). A new strategy adds a sibling module
//! here with its own params/axes/`simulate` — the generic sweep layers are reused.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};

use crate::sweep::strategy::{
    index_of, lhs_index_plan, neighbor_indices, round_trip_with_costs, CostModel, ExitCode,
    ParamSpace, Strategy, SweepMethod, TokenOutcome,
};
use crate::sweep::projection::SweepTrade;
use crate::models::Tpsl2Rule;
use crate::strategies::tpsl_sniper_2::{entry, exit};

/// The full TPSL2 rule param set, every knob swept. `take_profit`/`stop_loss` are
/// always-on; every other knob is `Option` and a `None` (the default axis when
/// the page leaves it blank, or an explicit `off`) means "unbounded" — the gate
/// is disabled and the live pure fns ignore it (same as `0`). `buy_amount` and
/// the non-param rule fields stay inherited from the base rule template.
#[derive(Clone, Copy, Debug)]
pub struct Tpsl2Params {
    pub take_profit: f64,
    pub stop_loss: f64,
    pub trailing_stop_pct: Option<f64>,
    pub time_stop_secs: Option<u64>,
    pub stall_secs: Option<u64>,
    pub liquidity_drop_pct: Option<f64>,
    pub cohort_ratio: Option<f64>,
    pub entry_min_age_secs: Option<u64>,
    pub entry_min_alive_sol: Option<f64>,
    pub entry_min_organic_sol: Option<f64>,
    pub entry_pullback_pct: Option<f64>,
    pub entry_higher_low_secs: Option<u64>,
    pub entry_max_cohort_held: Option<f64>,
    pub entry_min_liquidity_sol: Option<f64>,
    pub entry_min_organic_liq: Option<f64>,
}

/// One evaluated combo: the `Copy` scalar params (for `entry_key`/`params_json`)
/// paired with the `Tpsl2Rule` they overlay onto the base, **resolved once at
/// sample time** (Rec 2). The old hot loop cloned `base` into a fresh `Tpsl2Rule`
/// on *every* `(combo × token)` `simulate` call — millions of heap allocations
/// (`rule_name`/`trade_mode` Strings + the `p_token_ix_labels` JSON array).
/// Building it per combo here (≤ `combos`, not `combos × tokens`) and handing the
/// entry/exit fns `&combo.rule` keeps their `&Tpsl2Rule` signatures — so the live
/// path is untouched and decision parity is exact.
#[derive(Clone)]
pub struct Tpsl2Combo {
    pub raw: Tpsl2Params,
    pub rule: Tpsl2Rule,
}

/// The entry-param identity of a combo: the 8 scalp-gate knobs `find_scalp_entry`
/// reads (and nothing else). Two combos with equal keys resolve the same entry on
/// any token, so the engine resolves the entry once per distinct key (Rec 1). The
/// exit knobs are deliberately absent — the entry never depends on them.
#[derive(Clone, Copy, PartialEq)]
pub struct Tpsl2EntryKey {
    min_age_secs: Option<u64>,
    min_alive_sol: Option<f64>,
    min_organic_sol: Option<f64>,
    pullback_pct: Option<f64>,
    higher_low_secs: Option<u64>,
    max_cohort_held: Option<f64>,
    min_liquidity_sol: Option<f64>,
    min_organic_liq: Option<f64>,
}

/// The resolved entry for a token under one entry-param tuple: either no entry, or
/// the worst-case paper fill price + time. Cached by the engine and reused across
/// the tuple's whole exit sub-grid.
#[derive(Clone, Copy)]
pub enum Tpsl2Entry {
    None,
    Entered { price: f64, time: DateTime<Utc> },
}

/// Declares the param axes and carries the base rule the swept params overlay.
pub struct Tpsl2Strategy {
    base: Tpsl2Rule,
    axes: Tpsl2Axes,
    /// Execution costs applied to every simulated round-trip (Rec 1).
    costs: CostModel,
}

/// Grid axes for the coarse pass. Each `Vec` is one knob's candidate values.
/// `Serialize` so the resolved axes (after [`Tpsl2Axes::from_spec`]) are stored
/// verbatim on the sweep run for the UI to echo back / re-run.
#[derive(Clone, Serialize)]
pub struct Tpsl2Axes {
    pub take_profit: Vec<f64>,
    pub stop_loss: Vec<f64>,
    pub trailing_stop_pct: Vec<Option<f64>>,
    pub time_stop_secs: Vec<Option<u64>>,
    pub stall_secs: Vec<Option<u64>>,
    pub liquidity_drop_pct: Vec<Option<f64>>,
    pub cohort_ratio: Vec<Option<f64>>,
    pub entry_min_age_secs: Vec<Option<u64>>,
    pub entry_min_alive_sol: Vec<Option<f64>>,
    pub entry_min_organic_sol: Vec<Option<f64>>,
    pub entry_pullback_pct: Vec<Option<f64>>,
    pub entry_higher_low_secs: Vec<Option<u64>>,
    pub entry_max_cohort_held: Vec<Option<f64>>,
    pub entry_min_liquidity_sol: Vec<Option<f64>>,
    pub entry_min_organic_liq: Vec<Option<f64>>,
}

impl Default for Tpsl2Axes {
    fn default() -> Self {
        // The five high-leverage knobs keep a real candidate grid; every other
        // knob defaults to a single `[None]` = "unbounded" so it doesn't expand
        // the grid until the page supplies values for it.
        Self {
            take_profit: vec![50.0, 100.0, 200.0],
            stop_loss: vec![30.0, 50.0],
            trailing_stop_pct: vec![None, Some(20.0), Some(35.0)],
            time_stop_secs: vec![None, Some(120), Some(300)],
            stall_secs: vec![None, Some(30), Some(60)],
            liquidity_drop_pct: vec![None],
            cohort_ratio: vec![None],
            entry_min_age_secs: vec![Some(10), Some(30)],
            entry_min_alive_sol: vec![None],
            entry_min_organic_sol: vec![None],
            entry_pullback_pct: vec![None, Some(10.0)],
            entry_higher_low_secs: vec![None],
            entry_max_cohort_held: vec![None],
            entry_min_liquidity_sol: vec![None, Some(5.0)],
            entry_min_organic_liq: vec![None],
        }
    }
}

/// The page-editable grid: one optional candidate list per swept knob. An
/// omitted **or empty** axis falls back to that axis's hardcoded
/// [`Tpsl2Axes::default`] value, so a partial grid is valid. `null` inside a
/// nullable axis is the "disabled" option for that knob.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct AxesSpec {
    #[serde(default)]
    pub take_profit: Option<Vec<f64>>,
    #[serde(default)]
    pub stop_loss: Option<Vec<f64>>,
    #[serde(default)]
    pub trailing_stop_pct: Option<Vec<Option<f64>>>,
    #[serde(default)]
    pub time_stop_secs: Option<Vec<Option<u64>>>,
    #[serde(default)]
    pub stall_secs: Option<Vec<Option<u64>>>,
    #[serde(default)]
    pub liquidity_drop_pct: Option<Vec<Option<f64>>>,
    #[serde(default)]
    pub cohort_ratio: Option<Vec<Option<f64>>>,
    #[serde(default)]
    pub entry_min_age_secs: Option<Vec<Option<u64>>>,
    #[serde(default)]
    pub entry_min_alive_sol: Option<Vec<Option<f64>>>,
    #[serde(default)]
    pub entry_min_organic_sol: Option<Vec<Option<f64>>>,
    #[serde(default)]
    pub entry_pullback_pct: Option<Vec<Option<f64>>>,
    #[serde(default)]
    pub entry_higher_low_secs: Option<Vec<Option<u64>>>,
    #[serde(default)]
    pub entry_max_cohort_held: Option<Vec<Option<f64>>>,
    #[serde(default)]
    pub entry_min_liquidity_sol: Option<Vec<Option<f64>>>,
    #[serde(default)]
    pub entry_min_organic_liq: Option<Vec<Option<f64>>>,
}

impl Tpsl2Axes {
    /// Build axes from a page-supplied [`AxesSpec`], falling back to the default
    /// for any omitted/empty axis. Dedups each axis (the grid product shouldn't
    /// double-count a repeated value the UI may submit).
    pub fn from_spec(spec: &AxesSpec) -> Self {
        fn axis<T: Clone + PartialEq>(supplied: &Option<Vec<T>>, default: Vec<T>) -> Vec<T> {
            match supplied {
                Some(v) if !v.is_empty() => {
                    let mut out: Vec<T> = Vec::with_capacity(v.len());
                    for x in v {
                        if !out.contains(x) {
                            out.push(x.clone());
                        }
                    }
                    out
                }
                _ => default,
            }
        }
        let d = Tpsl2Axes::default();
        Self {
            take_profit: axis(&spec.take_profit, d.take_profit),
            stop_loss: axis(&spec.stop_loss, d.stop_loss),
            trailing_stop_pct: axis(&spec.trailing_stop_pct, d.trailing_stop_pct),
            time_stop_secs: axis(&spec.time_stop_secs, d.time_stop_secs),
            stall_secs: axis(&spec.stall_secs, d.stall_secs),
            liquidity_drop_pct: axis(&spec.liquidity_drop_pct, d.liquidity_drop_pct),
            cohort_ratio: axis(&spec.cohort_ratio, d.cohort_ratio),
            entry_min_age_secs: axis(&spec.entry_min_age_secs, d.entry_min_age_secs),
            entry_min_alive_sol: axis(&spec.entry_min_alive_sol, d.entry_min_alive_sol),
            entry_min_organic_sol: axis(&spec.entry_min_organic_sol, d.entry_min_organic_sol),
            entry_pullback_pct: axis(&spec.entry_pullback_pct, d.entry_pullback_pct),
            entry_higher_low_secs: axis(&spec.entry_higher_low_secs, d.entry_higher_low_secs),
            entry_max_cohort_held: axis(&spec.entry_max_cohort_held, d.entry_max_cohort_held),
            entry_min_liquidity_sol: axis(&spec.entry_min_liquidity_sol, d.entry_min_liquidity_sol),
            entry_min_organic_liq: axis(&spec.entry_min_organic_liq, d.entry_min_organic_liq),
        }
    }

    /// Number of combos a full grid over these axes yields (product of lengths).
    /// The handler rejects a spec whose product exceeds the combo cap.
    pub fn combo_count(&self) -> usize {
        self.take_profit.len()
            * self.stop_loss.len()
            * self.trailing_stop_pct.len()
            * self.time_stop_secs.len()
            * self.stall_secs.len()
            * self.liquidity_drop_pct.len()
            * self.cohort_ratio.len()
            * self.entry_min_age_secs.len()
            * self.entry_min_alive_sol.len()
            * self.entry_min_organic_sol.len()
            * self.entry_pullback_pct.len()
            * self.entry_higher_low_secs.len()
            * self.entry_max_cohort_held.len()
            * self.entry_min_liquidity_sol.len()
            * self.entry_min_organic_liq.len()
    }

    /// Grid size as `u128` — the index space the random sampler draws **without
    /// replacement** from. `u128` (not `combo_count`'s `usize`) so a wide page-
    /// supplied grid can't overflow the product before the draw clamps to it.
    fn grid_total_u128(&self) -> u128 {
        [
            self.take_profit.len(),
            self.stop_loss.len(),
            self.trailing_stop_pct.len(),
            self.time_stop_secs.len(),
            self.stall_secs.len(),
            self.liquidity_drop_pct.len(),
            self.cohort_ratio.len(),
            self.entry_min_age_secs.len(),
            self.entry_min_alive_sol.len(),
            self.entry_min_organic_sol.len(),
            self.entry_pullback_pct.len(),
            self.entry_higher_low_secs.len(),
            self.entry_max_cohort_held.len(),
            self.entry_min_liquidity_sol.len(),
            self.entry_min_organic_liq.len(),
        ]
        .iter()
        .map(|&l| l as u128)
        .product()
    }
}

impl Tpsl2Strategy {
    pub fn new(base: Tpsl2Rule, axes: Tpsl2Axes) -> Self {
        Self { base, axes, costs: CostModel::pumpfun_default() }
    }

    /// Pair a scalar param set with its resolved `Tpsl2Rule` (built once here, not
    /// per token in the hot loop — see [`Tpsl2Combo`]).
    fn combo(&self, raw: Tpsl2Params) -> Tpsl2Combo {
        Tpsl2Combo { rule: self.rule_from(&raw), raw }
    }

    /// Decode a flat grid index into its combo by mixed-radix over the axis
    /// lengths (the low-order digit is `take_profit`, matching the LHS plan's
    /// column order). Shared by the full-grid and the random (without-replacement)
    /// samplers so the two index the **same** grid identically.
    fn combo_at(&self, index: u128) -> Tpsl2Combo {
        let a = &self.axes;
        let mut rem = index;
        // `take` pulls this axis's value for `index` and advances `rem`.
        macro_rules! take {
            ($axis:expr) => {{
                let xs = &$axis;
                let v = xs[(rem % xs.len() as u128) as usize];
                rem /= xs.len() as u128;
                v
            }};
        }
        self.combo(Tpsl2Params {
            take_profit: take!(a.take_profit),
            stop_loss: take!(a.stop_loss),
            trailing_stop_pct: take!(a.trailing_stop_pct),
            time_stop_secs: take!(a.time_stop_secs),
            stall_secs: take!(a.stall_secs),
            liquidity_drop_pct: take!(a.liquidity_drop_pct),
            cohort_ratio: take!(a.cohort_ratio),
            entry_min_age_secs: take!(a.entry_min_age_secs),
            entry_min_alive_sol: take!(a.entry_min_alive_sol),
            entry_min_organic_sol: take!(a.entry_min_organic_sol),
            entry_pullback_pct: take!(a.entry_pullback_pct),
            entry_higher_low_secs: take!(a.entry_higher_low_secs),
            entry_max_cohort_held: take!(a.entry_max_cohort_held),
            entry_min_liquidity_sol: take!(a.entry_min_liquidity_sol),
            entry_min_organic_liq: take!(a.entry_min_organic_liq),
        })
    }

    /// Overlay one param set onto the base rule, producing the exact `Tpsl2Rule`
    /// the live pure fns expect.
    fn rule_from(&self, p: &Tpsl2Params) -> Tpsl2Rule {
        let mut r = self.base.clone();
        r.p_exit_take_profit = p.take_profit;
        r.p_exit_stop_loss = p.stop_loss;
        r.p_exit_trailing_stop_pct = p.trailing_stop_pct;
        r.p_exit_time_stop_secs = p.time_stop_secs;
        r.p_exit_stall_secs = p.stall_secs;
        r.p_exit_liquidity_drop_pct = p.liquidity_drop_pct;
        r.p_exit_cohort_ratio = p.cohort_ratio;
        r.p_entry_min_age_secs = p.entry_min_age_secs;
        r.p_entry_min_alive_sol = p.entry_min_alive_sol;
        r.p_entry_min_organic_sol = p.entry_min_organic_sol;
        r.p_entry_pullback_pct = p.entry_pullback_pct;
        r.p_entry_higher_low_secs = p.entry_higher_low_secs;
        r.p_entry_max_cohort_held = p.entry_max_cohort_held;
        r.p_entry_min_liquidity_sol = p.entry_min_liquidity_sol;
        r.p_entry_min_organic_liq = p.entry_min_organic_liq;
        r
    }
}

impl ParamSpace for Tpsl2Strategy {
    type Params = Tpsl2Combo;

    fn sample(&self, method: SweepMethod) -> Vec<Tpsl2Combo> {
        let a = &self.axes;
        match method {
            SweepMethod::Grid => {
                // Full grid = Cartesian product of all 15 axes, one combo per flat
                // index (mixed-radix decode in `combo_at`). The grid is pre-checked
                // ≤ cap, so `combo_count` (usize) can't overflow here.
                (0..a.combo_count() as u128).map(|idx| self.combo_at(idx)).collect()
            }
            SweepMethod::Random { n, seed } => {
                // Draw `n` combos **without replacement** from the grid index space:
                // a plain per-axis `pick` silently collapses to < n distinct combos
                // when the axes are small (the old behaviour). Drawing distinct grid
                // indices guarantees `min(n, grid_size)` distinct combos and logs the
                // shrinkage instead of hiding it. (#9 hygiene.)
                let total = a.grid_total_u128();
                let target = (n as u128).min(total) as usize;
                let mut rng = StdRng::seed_from_u64(seed);
                let mut seen: HashSet<u128> = HashSet::with_capacity(target);
                let mut out = Vec::with_capacity(target);
                while out.len() < target {
                    let idx = rng.gen_range(0..total);
                    if seen.insert(idx) {
                        out.push(self.combo_at(idx));
                    }
                }
                if target < n {
                    tracing::info!(
                        requested = n,
                        distinct = target,
                        "random sweep: grid smaller than N — sampled all distinct combos"
                    );
                }
                out
            }
            SweepMethod::LatinHypercube { n, seed } => {
                // Real LHS over the discrete axes: per-axis balanced+permuted strata
                // (every candidate value on every axis sampled ⌊n/len⌋–⌈n/len⌉ times,
                // columns decorrelated) — even coverage `Random` only reaches in
                // expectation. The `lens` array MUST stay in the same axis order as
                // the `take!` calls below (plan column index == axis position).
                let mut rng = StdRng::seed_from_u64(seed);
                let lens = [
                    a.take_profit.len(),
                    a.stop_loss.len(),
                    a.trailing_stop_pct.len(),
                    a.time_stop_secs.len(),
                    a.stall_secs.len(),
                    a.liquidity_drop_pct.len(),
                    a.cohort_ratio.len(),
                    a.entry_min_age_secs.len(),
                    a.entry_min_alive_sol.len(),
                    a.entry_min_organic_sol.len(),
                    a.entry_pullback_pct.len(),
                    a.entry_higher_low_secs.len(),
                    a.entry_max_cohort_held.len(),
                    a.entry_min_liquidity_sol.len(),
                    a.entry_min_organic_liq.len(),
                ];
                let plan = lhs_index_plan(&mut rng, n, &lens);
                (0..n)
                    .map(|i| {
                        let mut col = 0usize;
                        // `take` reads axis `col`'s LHS-planned value for draw `i`.
                        macro_rules! take {
                            ($axis:expr) => {{
                                let v = $axis[plan[col][i]];
                                col += 1;
                                v
                            }};
                        }
                        self.combo(Tpsl2Params {
                            take_profit: take!(a.take_profit),
                            stop_loss: take!(a.stop_loss),
                            trailing_stop_pct: take!(a.trailing_stop_pct),
                            time_stop_secs: take!(a.time_stop_secs),
                            stall_secs: take!(a.stall_secs),
                            liquidity_drop_pct: take!(a.liquidity_drop_pct),
                            cohort_ratio: take!(a.cohort_ratio),
                            entry_min_age_secs: take!(a.entry_min_age_secs),
                            entry_min_alive_sol: take!(a.entry_min_alive_sol),
                            entry_min_organic_sol: take!(a.entry_min_organic_sol),
                            entry_pullback_pct: take!(a.entry_pullback_pct),
                            entry_higher_low_secs: take!(a.entry_higher_low_secs),
                            entry_max_cohort_held: take!(a.entry_max_cohort_held),
                            entry_min_liquidity_sol: take!(a.entry_min_liquidity_sol),
                            entry_min_organic_liq: take!(a.entry_min_organic_liq),
                        })
                    })
                    .collect()
            }
        }
    }

    /// Coordinate-move neighborhood: for each survivor, vary one axis at a time to
    /// its adjacent candidate value(s), holding the others fixed. `Tpsl2Params` is
    /// `Copy`, so each neighbor is a cheap field overwrite. Duplicates are fine —
    /// the grouped driver dedups by `params_json`.
    fn refine(&self, survivors: &[Tpsl2Combo]) -> Vec<Tpsl2Combo> {
        let a = &self.axes;
        let mut out = Vec::new();
        for s in survivors {
            let p = s.raw;
            // For one axis, push the survivor with that field moved to each
            // adjacent candidate (skips an axis whose value isn't on its list).
            macro_rules! walk {
                ($axis:expr, $field:ident) => {{
                    let xs = &$axis;
                    if let Some(i) = index_of(xs, &p.$field) {
                        for ni in neighbor_indices(i, xs.len()) {
                            let mut np = p;
                            np.$field = xs[ni];
                            out.push(self.combo(np));
                        }
                    }
                }};
            }
            walk!(a.take_profit, take_profit);
            walk!(a.stop_loss, stop_loss);
            walk!(a.trailing_stop_pct, trailing_stop_pct);
            walk!(a.time_stop_secs, time_stop_secs);
            walk!(a.stall_secs, stall_secs);
            walk!(a.liquidity_drop_pct, liquidity_drop_pct);
            walk!(a.cohort_ratio, cohort_ratio);
            walk!(a.entry_min_age_secs, entry_min_age_secs);
            walk!(a.entry_min_alive_sol, entry_min_alive_sol);
            walk!(a.entry_min_organic_sol, entry_min_organic_sol);
            walk!(a.entry_pullback_pct, entry_pullback_pct);
            walk!(a.entry_higher_low_secs, entry_higher_low_secs);
            walk!(a.entry_max_cohort_held, entry_max_cohort_held);
            walk!(a.entry_min_liquidity_sol, entry_min_liquidity_sol);
            walk!(a.entry_min_organic_liq, entry_min_organic_liq);
        }
        out
    }
}

impl Strategy for Tpsl2Strategy {
    type Entry = Tpsl2Entry;
    type EntryKey = Tpsl2EntryKey;
    /// The token's launch cohort (interned `u32` wallet ids) — built once per token
    /// and reused across every entry-param tuple, instead of rebuilding the
    /// `HashSet` inside each `find_scalp_entry` resolve (the #7 cohort hoist).
    type TokenState = HashSet<u32>;

    fn id(&self) -> &'static str {
        "tpsl2"
    }

    fn entry_key(&self, params: &Tpsl2Combo) -> Tpsl2EntryKey {
        let p = &params.raw;
        Tpsl2EntryKey {
            min_age_secs: p.entry_min_age_secs,
            min_alive_sol: p.entry_min_alive_sol,
            min_organic_sol: p.entry_min_organic_sol,
            pullback_pct: p.entry_pullback_pct,
            higher_low_secs: p.entry_higher_low_secs,
            max_cohort_held: p.entry_max_cohort_held,
            min_liquidity_sol: p.entry_min_liquidity_sol,
            min_organic_liq: p.entry_min_organic_liq,
        }
    }

    fn prepare_token(&self, trades: &[SweepTrade]) -> HashSet<u32> {
        // Cohort depends only on the trade slice (first slot + window), never on a
        // rule's params — so hoist it out of the per-entry-key resolve.
        entry::scalp_cohort(trades)
    }

    fn resolve_entry(
        &self,
        trades: &[SweepTrade],
        cohort: &HashSet<u32>,
        params: &Tpsl2Combo,
    ) -> Tpsl2Entry {
        let rule = &params.rule;
        // (1) Decision: the scalp-entry trigger — the live gate logic, unchanged,
        // but fed the per-token cohort so the `HashSet` isn't rebuilt per resolve.
        // The indexed variant hands back the trigger's position so the worst-case
        // fill is resolved by index, never by `tx_signature` — letting `SweepTrade`
        // carry no signature string at all (Phase 1.2).
        let Some((trigger_idx, _)) = entry::find_scalp_entry_with_cohort_indexed(trades, rule, cohort)
        else {
            return Tpsl2Entry::None;
        };
        // (2) Worst-case entry fill (adverse same-/next-slot tick).
        let entry_fill = entry::find_worst_case_paper_entry_at(trades, trigger_idx);
        if entry_fill.price <= 0.0 {
            return Tpsl2Entry::None;
        }
        Tpsl2Entry::Entered { price: entry_fill.price, time: entry_fill.block_time }
    }

    fn resolve_exit(
        &self,
        trades: &[SweepTrade],
        entry: &Tpsl2Entry,
        params: &Tpsl2Combo,
    ) -> TokenOutcome {
        let Tpsl2Entry::Entered { price: entry_price, time: entry_time } = *entry else {
            return TokenOutcome::no_entry();
        };
        let rule = &params.rule;
        let notional = rule.buy_amount;

        // (3) Exit decision via the shared ladder.
        match exit::find_trade_driven_exit(trades, entry_time, entry_price, rule) {
            Some(f) => {
                let econ = round_trip_with_costs(entry_price, f.price, notional, &self.costs);
                TokenOutcome {
                    fired: true,
                    holding_secs: (f.block_time - entry_time).num_seconds(),
                    pnl_percent: econ.pnl_percent as f32,
                    pnl_sol: econ.pnl_sol as f32,
                    exit: ExitCode::from_reason(f.reason.as_str()),
                }
            }
            None => {
                // Still open at end of history — mark unrealized PnL at last price,
                // so the scoring layer can separate open from closed outcomes.
                let last_price = trades.last().map(|t| t.price_per_token).unwrap_or(entry_price);
                let econ = round_trip_with_costs(entry_price, last_price, notional, &self.costs);
                TokenOutcome {
                    fired: true,
                    holding_secs: 0,
                    pnl_percent: econ.pnl_percent as f32,
                    pnl_sol: econ.pnl_sol as f32,
                    exit: ExitCode::Open,
                }
            }
        }
    }

    fn params_json(&self, params: &Tpsl2Combo) -> serde_json::Value {
        let p = &params.raw;
        serde_json::json!({
            "exit_take_profit": p.take_profit,
            "exit_stop_loss": p.stop_loss,
            "exit_trailing_stop_pct": p.trailing_stop_pct,
            "exit_time_stop_secs": p.time_stop_secs,
            "exit_stall_secs": p.stall_secs,
            "exit_liquidity_drop_pct": p.liquidity_drop_pct,
            "exit_cohort_ratio": p.cohort_ratio,
            "entry_min_age_secs": p.entry_min_age_secs,
            "entry_min_alive_sol": p.entry_min_alive_sol,
            "entry_min_organic_sol": p.entry_min_organic_sol,
            "entry_pullback_pct": p.entry_pullback_pct,
            "entry_higher_low_secs": p.entry_higher_low_secs,
            "entry_max_cohort_held": p.entry_max_cohort_held,
            "entry_min_liquidity_sol": p.entry_min_liquidity_sol,
            "entry_min_organic_liq": p.entry_min_organic_liq,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strategy() -> Tpsl2Strategy {
        // A minimal base rule; the swept params overlay the entry/exit fields.
        let base = Tpsl2Rule::new(
            "test".into(),
            Some(1.0),
            None,
            None,
            serde_json::json!([]),
            "paper".into(),
            1.0,
            50.0,
            30.0,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );
        Tpsl2Strategy::new(base, Tpsl2Axes::default())
    }

    #[test]
    #[allow(clippy::float_cmp)] // candidate values are exact, compared for identity
    fn refine_walks_adjacent_axis_values_one_axis_at_a_time() {
        let s = strategy();
        let a = Tpsl2Axes::default();
        // A survivor at index 0 on every axis: each multi-valued axis (len ≥ 2)
        // contributes its single forward neighbor, every single-valued axis none.
        // Default tpsl2 has 8 multi-valued axes ⇒ 8 neighbors.
        let survivor = s.combo(Tpsl2Params {
            take_profit: a.take_profit[0],
            stop_loss: a.stop_loss[0],
            trailing_stop_pct: a.trailing_stop_pct[0],
            time_stop_secs: a.time_stop_secs[0],
            stall_secs: a.stall_secs[0],
            liquidity_drop_pct: a.liquidity_drop_pct[0],
            cohort_ratio: a.cohort_ratio[0],
            entry_min_age_secs: a.entry_min_age_secs[0],
            entry_min_alive_sol: a.entry_min_alive_sol[0],
            entry_min_organic_sol: a.entry_min_organic_sol[0],
            entry_pullback_pct: a.entry_pullback_pct[0],
            entry_higher_low_secs: a.entry_higher_low_secs[0],
            entry_max_cohort_held: a.entry_max_cohort_held[0],
            entry_min_liquidity_sol: a.entry_min_liquidity_sol[0],
            entry_min_organic_liq: a.entry_min_organic_liq[0],
        });
        let neighbors = s.refine(std::slice::from_ref(&survivor));
        assert_eq!(neighbors.len(), 8, "one forward coordinate move per multi-valued axis");

        // Every neighbor differs from the survivor in exactly one field, and the
        // changed value is on that axis (a valid candidate).
        let p = survivor.raw;
        for n in &neighbors {
            let q = n.raw;
            let diffs = (q.take_profit != p.take_profit) as u8
                + (q.stop_loss != p.stop_loss) as u8
                + (q.trailing_stop_pct != p.trailing_stop_pct) as u8
                + (q.time_stop_secs != p.time_stop_secs) as u8
                + (q.stall_secs != p.stall_secs) as u8
                + (q.liquidity_drop_pct != p.liquidity_drop_pct) as u8
                + (q.cohort_ratio != p.cohort_ratio) as u8
                + (q.entry_min_age_secs != p.entry_min_age_secs) as u8
                + (q.entry_min_alive_sol != p.entry_min_alive_sol) as u8
                + (q.entry_min_organic_sol != p.entry_min_organic_sol) as u8
                + (q.entry_pullback_pct != p.entry_pullback_pct) as u8
                + (q.entry_higher_low_secs != p.entry_higher_low_secs) as u8
                + (q.entry_max_cohort_held != p.entry_max_cohort_held) as u8
                + (q.entry_min_liquidity_sol != p.entry_min_liquidity_sol) as u8
                + (q.entry_min_organic_liq != p.entry_min_organic_liq) as u8;
            assert_eq!(diffs, 1, "a coordinate move changes exactly one axis");
        }
    }

    #[test]
    fn no_survivors_yields_no_neighbors() {
        assert!(strategy().refine(&[]).is_empty());
    }

    #[test]
    fn random_samples_without_replacement() {
        let s = strategy();
        let grid = s.axes.combo_count();
        let distinct = |combos: &[Tpsl2Combo]| -> usize {
            combos
                .iter()
                .map(|c| s.params_json(c).to_string())
                .collect::<std::collections::HashSet<_>>()
                .len()
        };

        // N over the grid clamps to the distinct grid size — no silent duplicates.
        let over = s.sample(SweepMethod::Random { n: grid * 4, seed: 7 });
        assert_eq!(over.len(), grid, "clamped to grid size");
        assert_eq!(distinct(&over), over.len(), "all distinct");

        // N under the grid returns exactly N distinct combos.
        let under = s.sample(SweepMethod::Random { n: 200, seed: 7 });
        assert_eq!(under.len(), 200);
        assert_eq!(distinct(&under), 200, "all distinct");
    }
}
