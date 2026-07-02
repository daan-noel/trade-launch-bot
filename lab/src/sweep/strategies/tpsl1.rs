//! The TPSL1 [`Strategy`] impl. Like [`tpsl2`](super::tpsl2) it wraps the **same**
//! pure entry/exit fns the live and DB-backtest paths use —
//! `find_entry_fill_in_trades` (cap 1, mirroring `run_backtest`) and
//! `find_trade_driven_exit` — so the sweep resolves byte-identical entry/exit
//! *decisions* to live trading. PnL is the frictionless `round_trip` of the
//! decision prices.
//!
//! TPSL1 differs from TPSL2 in its **param set**: it is the token-creation-filter
//! strategy, so it has *no* per-trade scalp-continuation entry gates. The
//! token-filter knobs (initial-buy/CU/max-sol-cost/…) are
//! entry *criteria* matched at token-creation against `Token` data, not per-trade
//! params `simulate` can sweep — they're the grouping fingerprint instead. What
//! remains sweepable is the exit ladder: TP/SL plus the four optional exits.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};

use crate::models::Tpsl1Rule;
use crate::strategies::tpsl_sniper_1::{entry, exit};
use crate::sweep::projection::SweepTrade;
use crate::sweep::strategy::{
    index_of, lhs_index_plan, neighbor_indices, round_trip_with_costs, CostModel, ExitCode,
    ParamSpace, Strategy, SweepMethod, TokenOutcome,
};

/// The full TPSL1 swept param set — the exit ladder. `take_profit`/`stop_loss`
/// are always-on; every other knob is `Option` and a `None` means "unbounded"
/// (the gate is disabled and the live pure fns ignore it, same as `0`). The
/// token-filter / concurrency / `buy_amount` fields stay inherited from the base
/// rule template.
#[derive(Clone, Copy, Debug)]
pub struct Tpsl1Params {
    pub take_profit: f64,
    pub stop_loss: f64,
    pub trailing_stop_pct: Option<f64>,
    pub time_stop_secs: Option<u64>,
    pub stall_secs: Option<u64>,
    pub liquidity_drop_pct: Option<f64>,
}

/// Declares the param axes and carries the base rule the swept params overlay.
pub struct Tpsl1Strategy {
    base: Tpsl1Rule,
    axes: Tpsl1Axes,
    /// Execution costs applied to every simulated round-trip (Rec 1).
    costs: CostModel,
}

/// One evaluated combo: the `Copy` scalar params paired with the `Tpsl1Rule` they
/// overlay onto the base, **resolved once at sample time** (Rec 2) — so the hot
/// loop hands the exit fn `&combo.rule` instead of cloning `base` per
/// `(combo × token)`. See `tpsl2::Tpsl2Combo` for the rationale.
#[derive(Clone)]
pub struct Tpsl1Combo {
    pub raw: Tpsl1Params,
    pub rule: Tpsl1Rule,
}

/// The resolved entry for a token: either no entry, or the fill price + time.
/// TPSL1 has no per-trade entry gate (the token-creation filter ran upstream), so
/// the entry is **param-free** — the engine resolves it once per token (see
/// `Strategy::EntryKey = ()`) and reuses it across every exit combo.
#[derive(Clone, Copy)]
pub enum Tpsl1Entry {
    None,
    Entered { price: f64, time: DateTime<Utc>, slot: u64 },
}

/// Grid axes for the coarse pass. Each `Vec` is one knob's candidate values.
/// `Serialize` so the resolved axes (after [`Tpsl1Axes::from_spec`]) are stored
/// verbatim on the sweep run for the UI to echo back / re-run.
#[derive(Clone, Serialize)]
pub struct Tpsl1Axes {
    pub take_profit: Vec<f64>,
    pub stop_loss: Vec<f64>,
    pub trailing_stop_pct: Vec<Option<f64>>,
    pub time_stop_secs: Vec<Option<u64>>,
    pub stall_secs: Vec<Option<u64>>,
    pub liquidity_drop_pct: Vec<Option<f64>>,
}

impl Default for Tpsl1Axes {
    fn default() -> Self {
        // TP/SL plus the trailing/time/stall exits keep a real candidate grid;
        // the liquidity-drop exit defaults to a single `[None]` = "unbounded" so
        // it doesn't expand the grid until the page supplies values for it.
        Self {
            take_profit: vec![50.0, 100.0, 200.0],
            stop_loss: vec![30.0, 50.0],
            trailing_stop_pct: vec![None, Some(20.0), Some(35.0)],
            time_stop_secs: vec![None, Some(120), Some(300)],
            stall_secs: vec![None, Some(30), Some(60)],
            liquidity_drop_pct: vec![None],
        }
    }
}

/// The page-editable grid: one optional candidate list per swept knob. An
/// omitted **or empty** axis falls back to that axis's hardcoded
/// [`Tpsl1Axes::default`] value, so a partial grid is valid. `null` inside a
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
}

impl Tpsl1Axes {
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
        let d = Tpsl1Axes::default();
        Self {
            take_profit: axis(&spec.take_profit, d.take_profit),
            stop_loss: axis(&spec.stop_loss, d.stop_loss),
            trailing_stop_pct: axis(&spec.trailing_stop_pct, d.trailing_stop_pct),
            time_stop_secs: axis(&spec.time_stop_secs, d.time_stop_secs),
            stall_secs: axis(&spec.stall_secs, d.stall_secs),
            liquidity_drop_pct: axis(&spec.liquidity_drop_pct, d.liquidity_drop_pct),
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
        ]
        .iter()
        .map(|&l| l as u128)
        .product()
    }
}

impl Tpsl1Strategy {
    pub fn new(base: Tpsl1Rule, axes: Tpsl1Axes) -> Self {
        Self { base, axes, costs: CostModel::pumpfun_default() }
    }

    /// Pair a scalar param set with its resolved `Tpsl1Rule` (built once here, not
    /// per token in the hot loop — see [`Tpsl1Combo`]).
    fn combo(&self, raw: Tpsl1Params) -> Tpsl1Combo {
        Tpsl1Combo { rule: self.rule_from(&raw), raw }
    }

    /// Decode a flat grid index into its combo by mixed-radix over the axis
    /// lengths (the low-order digit is `take_profit`, matching the LHS plan's
    /// column order). Shared by the full-grid and the random (without-replacement)
    /// samplers so the two index the **same** grid identically.
    // The `take` macro advances `rem` after every axis, including the last whose
    // result is never re-read — an intentional dead store, not a bug.
    #[allow(unused_assignments)]
    fn combo_at(&self, index: u128) -> Tpsl1Combo {
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
        self.combo(Tpsl1Params {
            take_profit: take!(a.take_profit),
            stop_loss: take!(a.stop_loss),
            trailing_stop_pct: take!(a.trailing_stop_pct),
            time_stop_secs: take!(a.time_stop_secs),
            stall_secs: take!(a.stall_secs),
            liquidity_drop_pct: take!(a.liquidity_drop_pct),
        })
    }

    /// Overlay one param set onto the base rule, producing the exact `Tpsl1Rule`
    /// the live pure fns expect.
    fn rule_from(&self, p: &Tpsl1Params) -> Tpsl1Rule {
        let mut r = self.base.clone();
        r.p_exit_take_profit = p.take_profit;
        r.p_exit_stop_loss = p.stop_loss;
        r.p_exit_trailing_stop_pct = p.trailing_stop_pct;
        r.p_exit_time_stop_secs = p.time_stop_secs;
        r.p_exit_stall_secs = p.stall_secs;
        r.p_exit_liquidity_drop_pct = p.liquidity_drop_pct;
        r
    }

    /// Minimal strategy for re-simulating a single stored combo.
    pub fn for_replay(base: Tpsl1Rule) -> Self {
        Self { base, axes: Tpsl1Axes::default(), costs: CostModel::pumpfun_default() }
    }

    /// Reconstruct a combo from the `params_json` stored in the results table.
    /// JSON keys match [`Strategy::params_json`]'s `"exit_*"` output.
    pub fn combo_from_params_json(&self, v: &serde_json::Value) -> anyhow::Result<Tpsl1Combo> {
        fn opt_f(v: &serde_json::Value, k: &str) -> Option<f64> {
            v.get(k).and_then(|x| x.as_f64())
        }
        fn opt_u(v: &serde_json::Value, k: &str) -> Option<u64> {
            v.get(k).and_then(|x| x.as_u64())
        }
        let take_profit = v
            .get("exit_take_profit")
            .and_then(|x| x.as_f64())
            .ok_or_else(|| anyhow::anyhow!("params_json missing 'exit_take_profit'"))?;
        let stop_loss = v
            .get("exit_stop_loss")
            .and_then(|x| x.as_f64())
            .ok_or_else(|| anyhow::anyhow!("params_json missing 'exit_stop_loss'"))?;
        let raw = Tpsl1Params {
            take_profit,
            stop_loss,
            trailing_stop_pct: opt_f(v, "exit_trailing_stop_pct"),
            time_stop_secs: opt_u(v, "exit_time_stop_secs"),
            stall_secs: opt_u(v, "exit_stall_secs"),
            liquidity_drop_pct: opt_f(v, "exit_liquidity_drop_pct"),
        };
        Ok(self.combo(raw))
    }
}

impl ParamSpace for Tpsl1Strategy {
    type Params = Tpsl1Combo;

    // The LHS `take` macro advances `col` after every axis, including the last
    // whose result is never re-read — an intentional dead store, not a bug.
    #[allow(unused_assignments)]
    fn sample(&self, method: SweepMethod) -> Vec<Tpsl1Combo> {
        let a = &self.axes;
        match method {
            SweepMethod::Grid => {
                // Full grid = Cartesian product of all 6 axes, one combo per flat
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
                // (every candidate value sampled ⌊n/len⌋–⌈n/len⌉ times, columns
                // decorrelated). The `lens` array MUST stay in the same axis order
                // as the `take!` calls below (plan column index == axis position).
                let mut rng = StdRng::seed_from_u64(seed);
                let lens = [
                    a.take_profit.len(),
                    a.stop_loss.len(),
                    a.trailing_stop_pct.len(),
                    a.time_stop_secs.len(),
                    a.stall_secs.len(),
                    a.liquidity_drop_pct.len(),
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
                        self.combo(Tpsl1Params {
                            take_profit: take!(a.take_profit),
                            stop_loss: take!(a.stop_loss),
                            trailing_stop_pct: take!(a.trailing_stop_pct),
                            time_stop_secs: take!(a.time_stop_secs),
                            stall_secs: take!(a.stall_secs),
                            liquidity_drop_pct: take!(a.liquidity_drop_pct),
                        })
                    })
                    .collect()
            }
        }
    }

    /// Coordinate-move neighborhood: for each survivor, vary one axis at a time to
    /// its adjacent candidate value(s), holding the others fixed. `Tpsl1Params` is
    /// `Copy`, so each neighbor is a cheap field overwrite. Duplicates are fine —
    /// the grouped driver dedups by `params_json`.
    fn refine(&self, survivors: &[Tpsl1Combo]) -> Vec<Tpsl1Combo> {
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
        }
        out
    }
}

impl Strategy for Tpsl1Strategy {
    type Entry = Tpsl1Entry;
    // Entry is param-free (no per-trade gate), so every combo shares one key and
    // the engine resolves the entry once per token.
    type EntryKey = ();
    // No param-independent per-token state to hoist.
    type TokenState = ();

    fn entry_key(&self, _params: &Tpsl1Combo) {}

    fn prepare_token(&self, _trades: &[SweepTrade]) {}

    fn resolve_entry(&self, trades: &[SweepTrade], _state: &(), _params: &Tpsl1Combo) -> Tpsl1Entry {
        // (1) Entry fill — the live/backtest fill resolution (cap 1, matching
        // `run_backtest`). TPSL1 has no per-trade entry gate; the token-creation
        // filter ran upstream when the corpus was selected.
        let Some(entry_fill) = entry::find_entry_fill_in_trades(trades, 1) else {
            return Tpsl1Entry::None;
        };
        if entry_fill.price <= 0.0 {
            return Tpsl1Entry::None;
        }
        Tpsl1Entry::Entered {
            price: entry_fill.price,
            time: entry_fill.block_time,
            slot: entry_fill.slot,
        }
    }

    fn resolve_exit(
        &self,
        trades: &[SweepTrade],
        _state: &(),
        entry: &Tpsl1Entry,
        params: &Tpsl1Combo,
    ) -> TokenOutcome {
        let Tpsl1Entry::Entered { price: entry_price, time: entry_time, slot: entry_slot } = *entry
        else {
            return TokenOutcome::no_entry();
        };
        let rule = &params.rule;
        let notional = rule.buy_amount;

        // (2) Exit decision via the shared ladder.
        match exit::find_trade_driven_exit(trades, entry_time, entry_price, rule) {
            Some(f) => {
                let (pnl_sol, pnl_percent) =
                    round_trip_with_costs(entry_price, f.price, notional, &self.costs);
                TokenOutcome {
                    fired: true,
                    holding_secs: (f.block_time - entry_time).num_seconds(),
                    pnl_percent: pnl_percent as f32,
                    pnl_sol: pnl_sol as f32,
                    exit: ExitCode::from_reason(&f.reason.to_string()),
                    entry_time: Some(entry_time),
                    entry_price: Some(entry_price),
                    entry_slot: Some(entry_slot),
                    exit_time: Some(f.block_time),
                    exit_price: Some(f.price),
                    exit_slot: Some(f.slot),
                }
            }
            None => {
                // Still open at end of history — mark unrealized PnL at last price,
                // so the scoring layer can separate open from closed outcomes.
                let last_price = trades.last().map(|t| t.price_per_token).unwrap_or(entry_price);
                let (pnl_sol, pnl_percent) =
                    round_trip_with_costs(entry_price, last_price, notional, &self.costs);
                TokenOutcome {
                    fired: true,
                    holding_secs: 0,
                    pnl_percent: pnl_percent as f32,
                    pnl_sol: pnl_sol as f32,
                    exit: ExitCode::Open,
                    entry_time: Some(entry_time),
                    entry_price: Some(entry_price),
                    entry_slot: Some(entry_slot),
                    exit_time: None,
                    exit_price: None,
                    exit_slot: None,
                }
            }
        }
    }

    fn params_json(&self, params: &Tpsl1Combo) -> serde_json::Value {
        let p = &params.raw;
        serde_json::json!({
            "exit_take_profit": p.take_profit,
            "exit_stop_loss": p.stop_loss,
            "exit_trailing_stop_pct": p.trailing_stop_pct,
            "exit_time_stop_secs": p.time_stop_secs,
            "exit_stall_secs": p.stall_secs,
            "exit_liquidity_drop_pct": p.liquidity_drop_pct,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The frontend `TPSL1_PARAM_KEYS` / `TPSL1_AXES` are this exact key set —
    /// guard the contract so a backend rename can't silently desync the columns.
    const EXPECTED_KEYS: &[&str] = &[
        "exit_take_profit",
        "exit_stop_loss",
        "exit_trailing_stop_pct",
        "exit_time_stop_secs",
        "exit_stall_secs",
        "exit_liquidity_drop_pct",
    ];

    fn strategy() -> Tpsl1Strategy {
        // A minimal base rule; the swept params overlay the exit fields.
        let base = Tpsl1Rule::new(
            "test".into(),
            Some(1.0),
            None,
            None,
            serde_json::json!([]),
            "paper".into(),
            1.0,
            50.0,
            20.0,
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
        Tpsl1Strategy::new(base, Tpsl1Axes::default())
    }

    #[test]
    fn default_grid_combo_count_is_product_of_axis_lengths() {
        let a = Tpsl1Axes::default();
        // 3 * 2 * 3 * 3 * 3 * 1 = 162.
        assert_eq!(a.combo_count(), 162);
    }

    #[test]
    fn empty_or_omitted_axes_fall_back_to_defaults() {
        let spec = AxesSpec::default();
        let axes = Tpsl1Axes::from_spec(&spec);
        assert_eq!(axes.combo_count(), Tpsl1Axes::default().combo_count());
    }

    #[test]
    fn grid_sample_len_matches_combo_count() {
        let s = strategy();
        let combos = s.sample(SweepMethod::Grid);
        assert_eq!(combos.len(), s.axes.combo_count());
    }

    #[test]
    fn random_samples_without_replacement() {
        let s = strategy();
        let grid = s.axes.combo_count(); // 162
        let distinct = |combos: &[Tpsl1Combo]| -> usize {
            combos
                .iter()
                .map(|c| s.params_json(c).to_string())
                .collect::<std::collections::HashSet<_>>()
                .len()
        };

        // N over the grid clamps to the distinct grid size — no silent duplicates.
        let over = s.sample(SweepMethod::Random { n: grid * 8, seed: 7 });
        assert_eq!(over.len(), grid, "clamped to grid size");
        assert_eq!(distinct(&over), over.len(), "all distinct");

        // N under the grid returns exactly N distinct combos.
        let under = s.sample(SweepMethod::Random { n: 40, seed: 7 });
        assert_eq!(under.len(), 40);
        assert_eq!(distinct(&under), 40, "all distinct");
    }

    #[test]
    #[allow(clippy::float_cmp)] // candidate values are exact, compared for identity
    fn refine_walks_adjacent_axis_values_one_axis_at_a_time() {
        let s = strategy();
        let a = Tpsl1Axes::default();
        // A survivor at an interior index on every multi-valued axis, so each
        // contributes both neighbors (len-3 axes) or its one neighbor (len-2):
        // tp[1] → 2, sl[1] → 1, trailing[1] → 2, time[1] → 2, stall[1] → 2,
        // liquidity (len 1) → 0  ⇒ 9 neighbors total.
        let survivor = s.combo(Tpsl1Params {
            take_profit: a.take_profit[1],
            stop_loss: a.stop_loss[1],
            trailing_stop_pct: a.trailing_stop_pct[1],
            time_stop_secs: a.time_stop_secs[1],
            stall_secs: a.stall_secs[1],
            liquidity_drop_pct: a.liquidity_drop_pct[0],
        });
        let neighbors = s.refine(std::slice::from_ref(&survivor));
        assert_eq!(neighbors.len(), 9, "one coordinate move per adjacent candidate");

        // Every neighbor differs from the survivor in exactly one field.
        let p = survivor.raw;
        for n in &neighbors {
            let q = n.raw;
            let diffs = (q.take_profit != p.take_profit) as u8
                + (q.stop_loss != p.stop_loss) as u8
                + (q.trailing_stop_pct != p.trailing_stop_pct) as u8
                + (q.time_stop_secs != p.time_stop_secs) as u8
                + (q.stall_secs != p.stall_secs) as u8
                + (q.liquidity_drop_pct != p.liquidity_drop_pct) as u8;
            assert_eq!(diffs, 1, "a coordinate move changes exactly one axis");
        }
    }

    #[test]
    fn params_json_emits_exactly_the_frontend_contract_keys() {
        let s = strategy();
        let combos = s.sample(SweepMethod::Grid);
        let json = s.params_json(&combos[0]);
        let obj = json.as_object().expect("params_json is an object");
        let mut keys: Vec<&str> = obj.keys().map(String::as_str).collect();
        keys.sort_unstable();
        let mut expected: Vec<&str> = EXPECTED_KEYS.to_vec();
        expected.sort_unstable();
        assert_eq!(keys, expected);
    }
}
