//! The TPSL1 [`Strategy`] impl. Like [`tpsl2`](super::tpsl2) it wraps the **same**
//! pure entry/exit fns the live and DB-backtest paths use —
//! `find_entry_fill_in_trades` (cap 1, mirroring `run_backtest`) and
//! `find_trade_driven_exit` — so the sweep resolves byte-identical entry/exit
//! *decisions* to live trading. PnL is the frictionless `round_trip` of the
//! decision prices.
//!
//! TPSL1 differs from TPSL2 in its **param set**: it is the token-creation-filter
//! strategy, so it has *no* per-trade scalp-continuation entry gates and *no*
//! cohort-dump exit. The token-filter knobs (initial-buy/CU/max-sol-cost/…) are
//! entry *criteria* matched at token-creation against `Token` data, not per-trade
//! params `simulate` can sweep — they're the grouping fingerprint instead. What
//! remains sweepable is the exit ladder: TP/SL plus the four optional exits.

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};

use crate::models::trade::Trade;
use crate::models::Tpsl1Rule;
use crate::strategies::tpsl_sniper_1::{entry, exit};
use crate::sweep::strategy::{
    round_trip, ExitCode, ParamSpace, Strategy, SweepMethod, TokenOutcome,
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
}

impl Tpsl1Strategy {
    pub fn new(base: Tpsl1Rule, axes: Tpsl1Axes) -> Self {
        Self { base, axes }
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
}

impl ParamSpace for Tpsl1Strategy {
    type Params = Tpsl1Params;

    fn sample(&self, method: SweepMethod) -> Vec<Tpsl1Params> {
        let a = &self.axes;
        match method {
            SweepMethod::Grid => {
                // Full grid = Cartesian product of all 6 axes. Mixed-radix decode
                // (one index per combo) keeps this flat and adding an axis a
                // one-line change, vs. a 6-deep loop nest.
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
                    out.push(Tpsl1Params {
                        take_profit: take!(a.take_profit),
                        stop_loss: take!(a.stop_loss),
                        trailing_stop_pct: take!(a.trailing_stop_pct),
                        time_stop_secs: take!(a.time_stop_secs),
                        stall_secs: take!(a.stall_secs),
                        liquidity_drop_pct: take!(a.liquidity_drop_pct),
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
                    .map(|_| Tpsl1Params {
                        take_profit: pick(&mut rng, &a.take_profit),
                        stop_loss: pick(&mut rng, &a.stop_loss),
                        trailing_stop_pct: pick(&mut rng, &a.trailing_stop_pct),
                        time_stop_secs: pick(&mut rng, &a.time_stop_secs),
                        stall_secs: pick(&mut rng, &a.stall_secs),
                        liquidity_drop_pct: pick(&mut rng, &a.liquidity_drop_pct),
                    })
                    .collect()
            }
        }
    }
}

fn pick<T: Copy>(rng: &mut StdRng, xs: &[T]) -> T {
    xs[rng.gen_range(0..xs.len())]
}

impl Strategy for Tpsl1Strategy {
    fn id(&self) -> &'static str {
        "tpsl1"
    }

    fn simulate(&self, trades: &[Trade], params: &Tpsl1Params) -> TokenOutcome {
        let rule = self.rule_from(params);

        // (1) Entry fill — the live/backtest fill resolution (cap 1, matching
        // `run_backtest`). TPSL1 has no per-trade entry gate; the token-creation
        // filter ran upstream when the corpus was selected.
        let entry_fill = match entry::find_entry_fill_in_trades(trades, 1) {
            Some(e) => e,
            None => return TokenOutcome::no_entry(),
        };
        if entry_fill.price <= 0.0 {
            return TokenOutcome::no_entry();
        }
        let entry_price = entry_fill.price;
        let entry_time = entry_fill.block_time;
        let notional = rule.buy_amount;

        // (2) Exit decision via the shared ladder.
        match exit::find_trade_driven_exit(trades, entry_time, entry_price, &rule) {
            Some(f) => {
                let econ = round_trip(entry_price, f.price, notional);
                TokenOutcome {
                    fired: true,
                    holding_secs: (f.block_time - entry_time).num_seconds(),
                    pnl_percent: econ.pnl_percent as f32,
                    pnl_sol: econ.pnl_sol as f32,
                    exit: ExitCode::from_reason(&f.reason.to_string()),
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

    fn params_json(&self, p: &Tpsl1Params) -> serde_json::Value {
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
    fn params_json_emits_exactly_the_frontend_contract_keys() {
        let s = strategy();
        let p = s.sample(SweepMethod::Grid)[0];
        let json = s.params_json(&p);
        let obj = json.as_object().expect("params_json is an object");
        let mut keys: Vec<&str> = obj.keys().map(String::as_str).collect();
        keys.sort_unstable();
        let mut expected: Vec<&str> = EXPECTED_KEYS.to_vec();
        expected.sort_unstable();
        assert_eq!(keys, expected);
    }
}
