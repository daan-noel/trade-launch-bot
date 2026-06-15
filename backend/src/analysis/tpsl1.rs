//! `Strategy` impl for TPSL1 — peer to [`crate::analysis::tpsl2`]. Demonstrates
//! the abstraction: a second strategy with *different* entry mechanics (token-
//! criteria, no scalp gates) drops in as one file, touching no other layer.
//!
//! Wraps the **same** pure fns the live/backtest TPSL1 path uses:
//! `entry::find_entry_fill_in_trades` (worst-case fill = highest-priced buy in
//! the first slot + first of the second) and `exit::find_trade_driven_exit`.
//! TPSL1 has no CohortExit (E5); every reason it can emit is already in
//! [`ExitCode`]. Per the repo's clone-parity convention, a fix to the shared
//! entry/exit fns flows to both strategies automatically.

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use crate::analysis::strategy::{
    round_trip, ExitCode, ParamSpace, Strategy, SweepMethod, TokenOutcome,
};
use crate::models::trade::Trade;
use crate::models::Tpsl1Rule;
use crate::strategies::tpsl_sniper_1::{entry, exit};

/// Entry-fill window used by the live paper poll vs. backtest. The sweep mirrors
/// the backtest's deterministic `1` (highest-priced first-slot buy + first
/// second-slot buy) — the worst-case fill TPSL1 already models.
const ENTRY_SECOND_BLOCK_CAP: usize = 1;

/// The swept subset of a TPSL1 rule: the exit ladder knobs. TPSL1's entry is
/// token-criteria (matched upstream of the trade history), so there are no entry
/// gates to sweep here — the corpus is the population that already matched.
#[derive(Clone, Copy, Debug)]
pub struct Tpsl1Params {
    pub take_profit: f64,
    pub stop_loss: f64,
    pub trailing_stop_pct: Option<f64>,
    pub time_stop_secs: Option<u64>,
    pub stall_secs: Option<u64>,
    pub liquidity_drop_pct: Option<f64>,
}

pub struct Tpsl1Strategy {
    base: Tpsl1Rule,
    axes: Tpsl1Axes,
}

#[derive(Clone)]
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
        Self {
            take_profit: vec![50.0, 100.0, 200.0],
            stop_loss: vec![30.0, 50.0],
            trailing_stop_pct: vec![None, Some(20.0), Some(35.0)],
            time_stop_secs: vec![None, Some(120), Some(300)],
            stall_secs: vec![None, Some(30), Some(60)],
            liquidity_drop_pct: vec![None, Some(40.0)],
        }
    }
}

impl Tpsl1Strategy {
    pub fn new(base: Tpsl1Rule, axes: Tpsl1Axes) -> Self {
        Self { base, axes }
    }

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
                let mut out = Vec::new();
                for &tp in &a.take_profit {
                    for &sl in &a.stop_loss {
                        for &tr in &a.trailing_stop_pct {
                            for &ts in &a.time_stop_secs {
                                for &st in &a.stall_secs {
                                    for &liq in &a.liquidity_drop_pct {
                                        out.push(Tpsl1Params {
                                            take_profit: tp,
                                            stop_loss: sl,
                                            trailing_stop_pct: tr,
                                            time_stop_secs: ts,
                                            stall_secs: st,
                                            liquidity_drop_pct: liq,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
                out
            }
            SweepMethod::Random { n, seed } | SweepMethod::LatinHypercube { n, seed } => {
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

        let entry_fill = match entry::find_entry_fill_in_trades(trades, ENTRY_SECOND_BLOCK_CAP) {
            Some(e) if e.price > 0.0 => e,
            _ => return TokenOutcome::no_entry(),
        };
        let entry_price = entry_fill.price;
        let entry_time = entry_fill.block_time;
        let notional = rule.buy_amount;

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
            "take_profit": p.take_profit,
            "stop_loss": p.stop_loss,
            "trailing_stop_pct": p.trailing_stop_pct,
            "time_stop_secs": p.time_stop_secs,
            "stall_secs": p.stall_secs,
            "liquidity_drop_pct": p.liquidity_drop_pct,
        })
    }
}
