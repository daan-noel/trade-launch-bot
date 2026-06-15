//! The TPSL2 [`Strategy`] impl. It wraps the **same** pure entry/exit fns the
//! live and DB-backtest paths use — `find_scalp_entry`,
//! `find_worst_case_paper_entry`, `find_trade_driven_exit` — so the sweep
//! resolves byte-identical entry/exit *decisions* to live trading. PnL is the
//! frictionless `round_trip` of the decision prices.
//!
//! This module is the TPSL2 `Strategy` impl registered in
//! [`registry`](crate::sweep::registry). A new strategy adds a sibling module
//! here with its own params/axes/`simulate` — the generic sweep layers are reused.

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};

use crate::sweep::strategy::{
    round_trip, ExitCode, ParamSpace, Strategy, SweepMethod, TokenOutcome,
};
use crate::models::trade::Trade;
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

/// Declares the param axes and carries the base rule the swept params overlay.
pub struct Tpsl2Strategy {
    base: Tpsl2Rule,
    axes: Tpsl2Axes,
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
}

impl Tpsl2Strategy {
    pub fn new(base: Tpsl2Rule, axes: Tpsl2Axes) -> Self {
        Self { base, axes }
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
    type Params = Tpsl2Params;

    fn sample(&self, method: SweepMethod) -> Vec<Tpsl2Params> {
        let a = &self.axes;
        match method {
            SweepMethod::Grid => {
                // Full grid = Cartesian product of all 15 axes. Mixed-radix decode
                // (one index per combo) keeps this flat and adding an axis a
                // one-line change, vs. a 15-deep loop nest.
                let total = a.combo_count();
                let mut out = Vec::with_capacity(total);
                for idx in 0..total {
                    let mut rem = idx;
                    // `take` pulls this axis's value for `idx` and advances `rem`.
                    macro_rules! take {
                        ($axis:expr) => {{
                            let xs = &$axis;
                            let v = xs[rem % xs.len()];
                            rem /= xs.len();
                            v
                        }};
                    }
                    out.push(Tpsl2Params {
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
                    });
                }
                out
            }
            SweepMethod::Random { n, seed } | SweepMethod::LatinHypercube { n, seed } => {
                // Random draw from each axis's candidate set. (LHS degrades to a
                // seeded random draw here; the axes are discrete so the coverage
                // gain is marginal — kept as a distinct tag for the analysis layer.)
                let mut rng = StdRng::seed_from_u64(seed);
                (0..n)
                    .map(|_| Tpsl2Params {
                        take_profit: pick(&mut rng, &a.take_profit),
                        stop_loss: pick(&mut rng, &a.stop_loss),
                        trailing_stop_pct: pick(&mut rng, &a.trailing_stop_pct),
                        time_stop_secs: pick(&mut rng, &a.time_stop_secs),
                        stall_secs: pick(&mut rng, &a.stall_secs),
                        liquidity_drop_pct: pick(&mut rng, &a.liquidity_drop_pct),
                        cohort_ratio: pick(&mut rng, &a.cohort_ratio),
                        entry_min_age_secs: pick(&mut rng, &a.entry_min_age_secs),
                        entry_min_alive_sol: pick(&mut rng, &a.entry_min_alive_sol),
                        entry_min_organic_sol: pick(&mut rng, &a.entry_min_organic_sol),
                        entry_pullback_pct: pick(&mut rng, &a.entry_pullback_pct),
                        entry_higher_low_secs: pick(&mut rng, &a.entry_higher_low_secs),
                        entry_max_cohort_held: pick(&mut rng, &a.entry_max_cohort_held),
                        entry_min_liquidity_sol: pick(&mut rng, &a.entry_min_liquidity_sol),
                        entry_min_organic_liq: pick(&mut rng, &a.entry_min_organic_liq),
                    })
                    .collect()
            }
        }
    }
}

fn pick<T: Copy>(rng: &mut StdRng, xs: &[T]) -> T {
    xs[rng.gen_range(0..xs.len())]
}

impl Strategy for Tpsl2Strategy {
    fn id(&self) -> &'static str {
        "tpsl2"
    }

    fn simulate(&self, trades: &[Trade], params: &Tpsl2Params) -> TokenOutcome {
        let rule = self.rule_from(params);

        // (1) Decision: the scalp-entry trigger — the live gate logic, unchanged.
        let target = match entry::find_scalp_entry(trades, &rule) {
            Some(t) => t,
            None => return TokenOutcome::no_entry(),
        };
        // (2) Worst-case entry fill (adverse same-/next-slot tick).
        let entry_fill = entry::find_worst_case_paper_entry(trades, &target.tx_signature);
        if entry_fill.price <= 0.0 {
            return TokenOutcome::no_entry();
        }
        let entry_price = entry_fill.price;
        let entry_time = entry_fill.block_time;
        let notional = rule.buy_amount;

        // (3) Exit decision via the shared ladder.
        match exit::find_trade_driven_exit(trades, entry_time, entry_price, &rule) {
            Some(f) => {
                let econ = round_trip(entry_price, f.price, notional);
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
                let econ = round_trip(entry_price, last_price, notional);
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

    fn params_json(&self, p: &Tpsl2Params) -> serde_json::Value {
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
