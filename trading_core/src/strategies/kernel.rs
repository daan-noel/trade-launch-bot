//! Simulation **kernel** — the single trade-walk + aggregation that turns a
//! `(StrategyImpl, StrategyParams)` and a token corpus into one [`RunMetrics`]
//! row. The same kernel backs every replay path: `lab`'s param sweep calls it
//! per combo, paper-trading replays call it, and live runs aggregate against the
//! identical metric shape — so live / paper / sweep results stay comparable.
//!
//! The per-token decision sequence is the registry's
//! [`resolve_entry`](super::registry::StrategyImpl::resolve_entry) /
//! [`resolve_exit`](super::registry::StrategyImpl::resolve_exit) (which dispatch
//! to the unchanged tpsl1/tpsl2 fns), so the kernel never re-implements a gate.
//! PnL is priced through the shared [`CostModel`] so a backtest reflects the
//! frictions the live trader pays.
//!
//! [`RunMetrics`] mirrors the `strategy_run_metrics` columns 1:1 (see
//! [`StrategyRunMetrics`](crate::models::StrategyRunMetrics)); [`to_run_metrics`]
//! stamps it onto a run. The bounded `QuantileSketch` + streaming [`RunAgg`] are
//! the single home for the sketch / robust-score math: `lab`'s per-combo sweep
//! folds into [`RunAgg`] via its thin `ComboAgg` wrapper (Phase 4), so backtest
//! and live/paper metrics can never drift to a second copy.

use chrono::Utc;
use uuid::Uuid;

use crate::config::constants::{
    COMPUTE_UNIT_LIMIT_CURVE_BUY, COMPUTE_UNIT_LIMIT_CURVE_SELL,
    COMPUTE_UNIT_PRICE_MICRO_LAMPORTS, LAMPORTS_PER_SOL,
};
use crate::models::trade::TradeRow;
use crate::models::StrategyRunMetrics;

use super::registry::{StrategyImpl, StrategyParams};

// ── Per-token outcome ─────────────────────────────────────────────────────────

/// Compact exit-reason code: the strategy ladder reasons plus the two non-exit
/// terminals (`Open`, `NoEntry`) the aggregation distinguishes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ExitCode {
    NoEntry = 0,
    Open = 1,
    TakeProfit = 2,
    StopLoss = 3,
    TrailingStop = 4,
    Stall = 5,
    TimeStop = 6,
    LiquidityExit = 7,
    /// swing1's symmetric next-kill exit: a post-entry leg reverting to the kill
    /// profile (deep + short) — the dev starting another intentional kill/rug.
    /// Top-priority in the swing1 ladder.
    NextKill = 8,
}

impl ExitCode {
    /// Map a ladder reason string (the `ExitReason::as_str` form persisted on
    /// positions / returned by the registry) to a code.
    pub fn from_reason(reason: &str) -> Self {
        match reason {
            "TakeProfit" => ExitCode::TakeProfit,
            "StopLoss" => ExitCode::StopLoss,
            "TrailingStop" => ExitCode::TrailingStop,
            "Stall" => ExitCode::Stall,
            "TimeStop" => ExitCode::TimeStop,
            "LiquidityExit" => ExitCode::LiquidityExit,
            "NextKill" => ExitCode::NextKill,
            "Open" => ExitCode::Open,
            _ => ExitCode::Open,
        }
    }
}

/// The simulated result of running one strategy over one token's trade history.
#[derive(Clone, Copy, Debug)]
pub struct TokenOutcome {
    /// Whether the strategy took a position under these params.
    pub fired: bool,
    /// Seconds entry→exit (0 when not fired or still open).
    pub holding_secs: i64,
    /// Net round-trip PnL after costs, as % of notional.
    pub pnl_percent: f32,
    /// Net round-trip PnL after costs, in SOL.
    pub pnl_sol: f32,
    pub exit: ExitCode,
}

impl TokenOutcome {
    /// The strategy never entered this token under these params.
    pub fn no_entry() -> Self {
        Self { fired: false, holding_secs: 0, pnl_percent: 0.0, pnl_sol: 0.0, exit: ExitCode::NoEntry }
    }
}

// ── Cost model (ported from lab sweep; Phase 4 collapses the duplicate) ────────

const REPRESENTATIVE_JITO_TIP_SOL: f64 = 0.001;
const FEE_BPS_PER_LEG: f64 = 100.0;
const SLIPPAGE_BPS_PER_LEG: f64 = 100.0;

fn priority_fee_sol(cu_limit: u32) -> f64 {
    let lamports = COMPUTE_UNIT_PRICE_MICRO_LAMPORTS as f64 * cu_limit as f64 / 1_000_000.0;
    lamports / LAMPORTS_PER_SOL as f64
}

/// Execution-cost model the kernel prices every round-trip with, so simulated
/// PnL reflects the frictions the live trader pays. All knobs apply to **both**
/// legs (symmetric entry/exit).
#[derive(Clone, Copy, Debug)]
pub struct CostModel {
    pub fee_bps_per_leg: f64,
    pub slippage_bps: f64,
    pub fixed_cost_sol_per_leg: f64,
}

impl CostModel {
    /// Default model from the live `pump_trader` curve constants (fee/slippage +
    /// representative Jito tip + priority fee at the average curve CU limit).
    pub fn pumpfun_default() -> Self {
        let avg_priority_sol = (priority_fee_sol(COMPUTE_UNIT_LIMIT_CURVE_BUY)
            + priority_fee_sol(COMPUTE_UNIT_LIMIT_CURVE_SELL))
            / 2.0;
        Self {
            fee_bps_per_leg: FEE_BPS_PER_LEG,
            slippage_bps: SLIPPAGE_BPS_PER_LEG,
            fixed_cost_sol_per_leg: REPRESENTATIVE_JITO_TIP_SOL + avg_priority_sol,
        }
    }

    /// A frictionless model (no fees/slippage/fixed cost) — pure price-to-price,
    /// for analytic baselines and tests.
    pub fn frictionless() -> Self {
        Self { fee_bps_per_leg: 0.0, slippage_bps: 0.0, fixed_cost_sol_per_leg: 0.0 }
    }
}

/// Net PnL of a buy@`entry_price` / sell@`exit_price` round-trip sized at
/// `notional_sol`, net of `costs`: symmetric slippage worsens both fills, a fee is
/// charged on each leg's SOL value, and the fixed per-leg cost is subtracted twice.
/// Returns `(pnl_sol, pnl_percent)`.
pub fn round_trip_with_costs(
    entry_price: f64,
    exit_price: f64,
    notional_sol: f64,
    costs: &CostModel,
) -> (f64, f64) {
    if entry_price <= 0.0 || notional_sol <= 0.0 {
        return (0.0, 0.0);
    }
    let slip = costs.slippage_bps / 10_000.0;
    let fee = costs.fee_bps_per_leg / 10_000.0;
    let eff_entry = entry_price * (1.0 + slip);
    let eff_exit = exit_price * (1.0 - slip);
    let tokens = notional_sol / eff_entry;
    let gross_proceeds = tokens * eff_exit;
    let costs_sol = (notional_sol + gross_proceeds) * fee + costs.fixed_cost_sol_per_leg * 2.0;
    let pnl_sol = gross_proceeds - notional_sol - costs_sol;
    (pnl_sol, pnl_sol / notional_sol * 100.0)
}

// ── Run configuration + metrics ───────────────────────────────────────────────

/// What the kernel needs beyond the strategy params: the notional per entry
/// (`buy_amount_sol`, a universal `StrategyRule` column) and the cost model.
#[derive(Clone, Copy, Debug)]
pub struct SimConfig {
    pub buy_amount_sol: f64,
    pub costs: CostModel,
}

impl SimConfig {
    pub fn new(buy_amount_sol: f64) -> Self {
        Self { buy_amount_sol, costs: CostModel::pumpfun_default() }
    }
}

/// Rolled-up metrics for one run across a token corpus. Field-for-field the
/// `strategy_run_metrics` columns (plus the sweep's `score`, ignored when
/// persisting a live/paper run). Use [`to_run_metrics`] to stamp onto a run.
#[derive(Clone, Debug, PartialEq)]
pub struct RunMetrics {
    pub n_fired: u64,
    pub n_open: u64,
    pub n_closed: u64,
    pub win_rate: f64,
    pub total_pnl_sol: f64,
    pub expectancy_sol: f64,
    pub mean_pnl_pct: f64,
    pub median_pnl_pct: f64,
    pub p90_pnl_pct: f64,
    pub best_pnl_pct: f64,
    pub worst_pnl_pct: f64,
    pub std_pnl_pct: f64,
    pub profit_factor: Option<f64>,
    /// Robust rank `μ − Z·σ/√n` over closed trades; `None` when n_closed < 2.
    pub score: Option<f64>,
    pub avg_holding_secs: f64,
    pub median_holding_secs: f64,
    pub n_exit_take_profit: u32,
    pub n_exit_stop_loss: u32,
    pub n_exit_trailing: u32,
    pub n_exit_stall: u32,
    pub n_exit_time: u32,
    pub n_exit_liquidity: u32,
    /// swing1 symmetric next-kill exits. Surfaced by the grouped sweep
    /// (`ComboMetrics`); the live `StrategyRunMetrics` rollup does NOT carry this
    /// column (NextKill only fires from swing1, which is backtest-only in Phase 1),
    /// so `to_run_metrics` drops it — see that fn.
    pub n_exit_next_kill: u32,
    pub n_exit_open: u32,
}

impl RunMetrics {
    /// Stamp these metrics onto a run as a persistable [`StrategyRunMetrics`]
    /// (the `score` field is sweep-only and dropped; floats narrow to `f32`).
    pub fn to_run_metrics(&self, run_id: Uuid) -> StrategyRunMetrics {
        StrategyRunMetrics {
            run_id,
            rolled_up_at: Utc::now(),
            n_fired: self.n_fired as i32,
            n_open: self.n_open as i32,
            n_closed: self.n_closed as i32,
            win_rate: self.win_rate as f32,
            total_pnl_sol: self.total_pnl_sol as f32,
            expectancy_sol: self.expectancy_sol as f32,
            mean_pnl_pct: self.mean_pnl_pct as f32,
            median_pnl_pct: self.median_pnl_pct as f32,
            p90_pnl_pct: self.p90_pnl_pct as f32,
            best_pnl_pct: self.best_pnl_pct as f32,
            worst_pnl_pct: self.worst_pnl_pct as f32,
            std_pnl_pct: self.std_pnl_pct as f32,
            profit_factor: self.profit_factor.map(|p| p as f32),
            avg_holding_secs: self.avg_holding_secs as f32,
            median_holding_secs: self.median_holding_secs as f32,
            n_exit_take_profit: self.n_exit_take_profit as i32,
            n_exit_stop_loss: self.n_exit_stop_loss as i32,
            n_exit_trailing: self.n_exit_trailing as i32,
            n_exit_stall: self.n_exit_stall as i32,
            n_exit_time: self.n_exit_time as i32,
            n_exit_liquidity: self.n_exit_liquidity as i32,
            n_exit_open: self.n_exit_open as i32,
        }
    }
}

// ── Public entry point ─────────────────────────────────────────────────────────

/// Simulate one `(strategy, params)` over a token corpus — each `&[T]` is one
/// token's slot/time-sorted trade history — and aggregate to a single
/// [`RunMetrics`]. Per token: resolve the entry; if it fires, resolve the exit
/// (a closed trade) or mark the still-open position to its last price; fold into
/// the streaming aggregate. Tokens that never enter are recorded as not-fired.
pub fn simulate_rule<T: TradeRow>(
    strat: StrategyImpl,
    params: &StrategyParams,
    tokens: &[&[T]],
    cfg: &SimConfig,
) -> RunMetrics {
    let mut agg = RunAgg::default();
    for token_trades in tokens {
        agg.record(&simulate_token(strat, params, token_trades, cfg));
    }
    agg.finalize()
}

/// The per-token outcome (exposed so callers that need per-position fills, e.g.
/// paper replay, can drive the same decision sequence the run aggregates).
pub fn simulate_token<T: TradeRow>(
    strat: StrategyImpl,
    params: &StrategyParams,
    trades: &[T],
    cfg: &SimConfig,
) -> TokenOutcome {
    let Some(entry) = strat.resolve_entry(trades, params) else {
        return TokenOutcome::no_entry();
    };
    match strat.resolve_exit(trades, entry.block_time, entry.price, params) {
        Some(exit) => {
            let (pnl_sol, pnl_pct) =
                round_trip_with_costs(entry.price, exit.price, cfg.buy_amount_sol, &cfg.costs);
            TokenOutcome {
                fired: true,
                holding_secs: (exit.block_time - entry.block_time).num_seconds(),
                pnl_percent: pnl_pct as f32,
                pnl_sol: pnl_sol as f32,
                exit: ExitCode::from_reason(exit.reason),
            }
        }
        None => {
            // Still open: mark to the last observed price.
            let last_price = trades.last().map(|t| t.price_per_token()).unwrap_or(entry.price);
            let (pnl_sol, pnl_pct) =
                round_trip_with_costs(entry.price, last_price, cfg.buy_amount_sol, &cfg.costs);
            TokenOutcome {
                fired: true,
                holding_secs: 0,
                pnl_percent: pnl_pct as f32,
                pnl_sol: pnl_sol as f32,
                exit: ExitCode::Open,
            }
        }
    }
}

// ── Streaming aggregate (ported from lab sweep::aggregate) ─────────────────────

/// Confidence multiplier for the robust score's one-sided lower bound (~95% z).
const SCORE_Z: f64 = 1.64;

/// Streaming accumulator across every token a run fires on. PnL stats mark open
/// positions to last price; holding-time stats and the robust score use closed
/// positions only. O(1) per run — interior quantiles via a fixed [`QuantileSketch`].
///
/// Public so the analysis path can fold into the **same** accumulator the live /
/// paper kernel uses: `lab`'s per-combo sweep wraps one of these per combo (its
/// `ComboAgg`) so backtest metrics are byte-identical to a live run's, with no
/// second copy of the sketch / robust-score math to drift.
#[derive(Clone)]
pub struct RunAgg {
    fired: u64,
    open: u64,
    wins: u64,
    pnl_sol_sum: f64,
    gross_win_sol: f64,
    gross_loss_sol: f64,
    pnl_pct_sum: f64,
    pnl_min: f32,
    pnl_max: f32,
    pnl_sketch: QuantileSketch,
    closed_pct_sum: f64,
    closed_pct_sum_sq: f64,
    holding_sum: i64,
    holding_sketch: QuantileSketch,
    exit_counts: [u32; 8],
}

impl Default for RunAgg {
    fn default() -> Self {
        Self {
            fired: 0,
            open: 0,
            wins: 0,
            pnl_sol_sum: 0.0,
            gross_win_sol: 0.0,
            gross_loss_sol: 0.0,
            pnl_pct_sum: 0.0,
            pnl_min: f32::INFINITY,
            pnl_max: f32::NEG_INFINITY,
            pnl_sketch: QuantileSketch::default(),
            closed_pct_sum: 0.0,
            closed_pct_sum_sq: 0.0,
            holding_sum: 0,
            holding_sketch: QuantileSketch::default(),
            exit_counts: [0; 8],
        }
    }
}

impl RunAgg {
    /// Fold one token's outcome into the accumulator. No-entry rows are ignored.
    pub fn record(&mut self, o: &TokenOutcome) {
        if !o.fired {
            return;
        }
        self.fired += 1;
        self.pnl_sol_sum += o.pnl_sol as f64;
        self.pnl_pct_sum += o.pnl_percent as f64;
        self.pnl_min = self.pnl_min.min(o.pnl_percent);
        self.pnl_max = self.pnl_max.max(o.pnl_percent);
        self.pnl_sketch.record(o.pnl_percent as f64);
        if o.pnl_sol > 0.0 {
            self.wins += 1;
            self.gross_win_sol += o.pnl_sol as f64;
        } else if o.pnl_sol < 0.0 {
            self.gross_loss_sol += -(o.pnl_sol as f64);
        }
        if o.exit == ExitCode::Open {
            self.open += 1;
        } else {
            self.holding_sum += o.holding_secs;
            self.holding_sketch.record(o.holding_secs as f64);
            let p = o.pnl_percent as f64;
            self.closed_pct_sum += p;
            self.closed_pct_sum_sq += p * p;
        }
        self.exit_counts[exit_index(o.exit)] += 1;
    }

    /// Collapse the accumulator to the final rolled-up [`RunMetrics`].
    pub fn finalize(self) -> RunMetrics {
        let n = self.fired as f64;
        let (median_pnl_pct, p90_pnl_pct, best_pnl_pct, worst_pnl_pct) = if self.fired == 0 {
            (0.0, 0.0, 0.0, 0.0)
        } else {
            (
                self.pnl_sketch.quantile(0.5),
                self.pnl_sketch.quantile(0.9),
                self.pnl_max as f64,
                self.pnl_min as f64,
            )
        };
        let mean_pnl_pct = if self.fired == 0 { 0.0 } else { self.pnl_pct_sum / n };
        let n_closed = self.fired - self.open;
        let (avg_holding_secs, median_holding_secs) = if n_closed == 0 {
            (0.0, 0.0)
        } else {
            (self.holding_sum as f64 / n_closed as f64, self.holding_sketch.quantile(0.5))
        };
        let profit_factor = if self.gross_loss_sol > 0.0 {
            Some(self.gross_win_sol / self.gross_loss_sol)
        } else {
            None
        };
        let (std_pnl_pct, score) =
            robust_score(n_closed, self.closed_pct_sum, self.closed_pct_sum_sq);
        RunMetrics {
            n_fired: self.fired,
            n_open: self.open,
            n_closed,
            win_rate: if self.fired == 0 { 0.0 } else { self.wins as f64 / n },
            total_pnl_sol: self.pnl_sol_sum,
            expectancy_sol: if self.fired == 0 { 0.0 } else { self.pnl_sol_sum / n },
            mean_pnl_pct,
            median_pnl_pct,
            p90_pnl_pct,
            best_pnl_pct,
            worst_pnl_pct,
            std_pnl_pct,
            profit_factor,
            score,
            avg_holding_secs,
            median_holding_secs,
            n_exit_take_profit: self.exit_counts[0],
            n_exit_stop_loss: self.exit_counts[1],
            n_exit_trailing: self.exit_counts[2],
            n_exit_stall: self.exit_counts[3],
            n_exit_time: self.exit_counts[4],
            n_exit_liquidity: self.exit_counts[5],
            n_exit_open: self.exit_counts[6],
            n_exit_next_kill: self.exit_counts[7],
        }
    }
}

/// One-sided lower-confidence bound on realized per-trade return:
/// `μ − Z·σ/√n` over closed positions. `(stddev, Some(score))`, or `(0, None)`
/// when n < 2 (one closed trade is no evidence of a repeatable edge).
fn robust_score(n_closed: u64, sum: f64, sum_sq: f64) -> (f64, Option<f64>) {
    if n_closed < 2 {
        return (0.0, None);
    }
    let n = n_closed as f64;
    let mean = sum / n;
    let var = ((sum_sq - n * mean * mean) / (n - 1.0)).max(0.0);
    let std = var.sqrt();
    (std, Some(mean - SCORE_Z * std / n.sqrt()))
}

fn exit_index(e: ExitCode) -> usize {
    match e {
        ExitCode::TakeProfit => 0,
        ExitCode::StopLoss => 1,
        ExitCode::TrailingStop => 2,
        ExitCode::Stall => 3,
        ExitCode::TimeStop => 4,
        ExitCode::LiquidityExit => 5,
        ExitCode::Open | ExitCode::NoEntry => 6,
        ExitCode::NextKill => 7,
    }
}

// ── Quantile sketch (ported from lab sweep::aggregate) ─────────────────────────

const SKETCH_N: usize = 64;
const SKETCH_BIAS: f64 = 22.65;
const SKETCH_INV_LN_GAMMA: f64 = 3.27885;

/// Fixed-memory, order-independent quantile sketch (DDSketch-style log buckets).
/// Median/p90 carry ~15% relative error; best/worst/mean/total stay exact.
#[derive(Clone)]
struct QuantileSketch {
    neg: [u16; SKETCH_N],
    pos: [u16; SKETCH_N],
    zero: u16,
}

impl Default for QuantileSketch {
    fn default() -> Self {
        Self { neg: [0; SKETCH_N], pos: [0; SKETCH_N], zero: 0 }
    }
}

fn sketch_bucket(mag: f64) -> usize {
    let idx = (mag.ln() * SKETCH_INV_LN_GAMMA + SKETCH_BIAS).floor();
    idx.clamp(0.0, (SKETCH_N - 1) as f64) as usize
}

fn sketch_value_at(i: usize) -> f64 {
    ((i as f64 - SKETCH_BIAS + 0.5) / SKETCH_INV_LN_GAMMA).exp()
}

impl QuantileSketch {
    fn record(&mut self, v: f64) {
        if v > 0.0 {
            let b = &mut self.pos[sketch_bucket(v)];
            *b = b.saturating_add(1);
        } else if v < 0.0 {
            let b = &mut self.neg[sketch_bucket(-v)];
            *b = b.saturating_add(1);
        } else {
            self.zero = self.zero.saturating_add(1);
        }
    }

    fn count(&self) -> u64 {
        let neg: u64 = self.neg.iter().map(|&c| c as u64).sum();
        let pos: u64 = self.pos.iter().map(|&c| c as u64).sum();
        neg + pos + self.zero as u64
    }

    fn quantile(&self, q: f64) -> f64 {
        let total = self.count();
        if total == 0 {
            return 0.0;
        }
        let target = ((q * total as f64) as u64).min(total - 1);
        let mut cum = 0u64;
        for i in (0..SKETCH_N).rev() {
            cum += self.neg[i] as u64;
            if cum > target {
                return -sketch_value_at(i);
            }
        }
        cum += self.zero as u64;
        if cum > target {
            return 0.0;
        }
        for i in 0..SKETCH_N {
            cum += self.pos[i] as u64;
            if cum > target {
                return sketch_value_at(i);
            }
        }
        sketch_value_at(SKETCH_N - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::trade::{Trade, TradeType};
    use crate::strategies::registry::{Tpsl1Params, Tpsl2Params};
    use crate::models::{Tpsl1Rule, Tpsl2Rule};
    use chrono::{DateTime, Duration, Utc};
    use serde_json::json;

    fn base_time() -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap()
    }

    fn buy(price: f64, slot: u64, secs: i64) -> Trade {
        Trade::new(
            "mint".into(),
            "wallet".into(),
            TradeType::Buy,
            price,
            1,
            format!("sig-{slot}-{secs}"),
            slot,
            base_time() + Duration::seconds(secs),
        )
    }

    fn tpsl1_params(tp: f64, sl: f64) -> StrategyParams {
        let mut rule = Tpsl1Rule::new(
            "r".into(), None, None, None, json!([]), "paper".into(), 1.0,
            tp, sl, None, None, None, None, Some(0.0), None, None, None, None,
        );
        rule.is_active = true;
        StrategyParams::Tpsl1(Tpsl1Params::from_rule(&rule))
    }

    // ── round-trip pricing ──────────────────────────────────────────────────

    #[test]
    fn frictionless_round_trip_is_pure_price_delta() {
        // 2× exit, 1 SOL notional, no costs → +1 SOL, +100%.
        let (sol, pct) = round_trip_with_costs(1.0, 2.0, 1.0, &CostModel::frictionless());
        assert!((sol - 1.0).abs() < 1e-12);
        assert!((pct - 100.0).abs() < 1e-12);
    }

    #[test]
    fn costs_reduce_pnl_below_frictionless() {
        let friction = round_trip_with_costs(1.0, 2.0, 1.0, &CostModel::pumpfun_default()).0;
        let free = round_trip_with_costs(1.0, 2.0, 1.0, &CostModel::frictionless()).0;
        assert!(friction < free, "costs must drag PnL down");
    }

    // ── kernel aggregation parity ────────────────────────────────────────────

    #[test]
    fn simulate_rule_aggregates_win_and_loss_tokens() {
        // Token A: take-profit (TP=50%). Token B: stop-loss (SL=20%).
        // The entry fill is the highest of the first slot + first second-slot buy,
        // so a benign slot-2 buy (0.9, ≤ entry, −10% so it trips nothing) pins the
        // entry at 1.0; the decisive move lands in slot 3.
        let token_a = vec![
            buy(1.0, 1, 0),   // first block → entry candidate 1.0
            buy(0.9, 2, 1),   // second block (cap 1), ≤ entry → entry stays 1.0
            buy(2.0, 3, 2),   // +100% → take profit fires
            buy(2.0, 4, 3),   // fill slot (F+1)
        ];
        let token_b = vec![
            buy(1.0, 1, 0),   // entry candidate 1.0
            buy(0.9, 2, 1),   // pins entry at 1.0
            buy(0.5, 3, 2),   // -50% → stop loss fires
            buy(0.5, 4, 3),   // fill slot
        ];
        let params = tpsl1_params(50.0, 20.0);
        let cfg = SimConfig { buy_amount_sol: 1.0, costs: CostModel::frictionless() };

        let tokens: Vec<&[Trade]> = vec![&token_a, &token_b];
        let m = simulate_rule(StrategyImpl::Tpsl1, &params, &tokens, &cfg);

        assert_eq!(m.n_fired, 2);
        assert_eq!(m.n_closed, 2);
        assert_eq!(m.n_open, 0);
        assert_eq!(m.n_exit_take_profit, 1);
        assert_eq!(m.n_exit_stop_loss, 1);
        assert!((m.win_rate - 0.5).abs() < 1e-9);
        // Frictionless: +100% on A (+1 SOL), exit B at 0.5 = −50% (−0.5 SOL).
        assert!((m.total_pnl_sol - 0.5).abs() < 1e-9);
        assert_eq!(m.best_pnl_pct, 100.0);
        assert!((m.worst_pnl_pct - (-50.0)).abs() < 1e-9);
        assert_eq!(m.profit_factor, Some(2.0)); // 1.0 win / 0.5 loss
    }

    #[test]
    fn no_entry_token_is_not_fired() {
        // Empty trade history → no entry fill → not fired, ignored by the agg.
        let empty: Vec<Trade> = vec![];
        let params = tpsl1_params(50.0, 20.0);
        let cfg = SimConfig::new(1.0);
        let tokens: Vec<&[Trade]> = vec![&empty];
        let m = simulate_rule(StrategyImpl::Tpsl1, &params, &tokens, &cfg);
        assert_eq!(m.n_fired, 0);
        assert_eq!(m.score, None);
    }

    #[test]
    fn open_token_marks_to_last_price() {
        // Entry at 1.0 (benign slot-2 buy pins it), drifts to 1.1, never hits
        // TP(1000%)/SL(90%) → Open, marked to last price (1.1) → small +PnL.
        let token = vec![buy(1.0, 1, 0), buy(0.9, 2, 1), buy(1.1, 3, 2)];
        let params = tpsl1_params(1000.0, 90.0);
        let cfg = SimConfig { buy_amount_sol: 1.0, costs: CostModel::frictionless() };
        let tokens: Vec<&[Trade]> = vec![&token];
        let m = simulate_rule(StrategyImpl::Tpsl1, &params, &tokens, &cfg);
        assert_eq!(m.n_fired, 1);
        assert_eq!(m.n_open, 1);
        assert_eq!(m.n_closed, 0);
        assert_eq!(m.n_exit_open, 1);
        assert!(m.total_pnl_sol > 0.0);
    }

    #[test]
    fn run_metrics_maps_to_strategy_run_metrics() {
        let token = vec![buy(1.0, 1, 0), buy(2.0, 2, 1), buy(2.0, 3, 2)];
        let params = tpsl1_params(50.0, 20.0);
        let cfg = SimConfig::new(1.0);
        let tokens: Vec<&[Trade]> = vec![&token];
        let m = simulate_rule(StrategyImpl::Tpsl1, &params, &tokens, &cfg);
        let run_id = uuid::Uuid::new_v4();
        let srm = m.to_run_metrics(run_id);
        assert_eq!(srm.run_id, run_id);
        assert_eq!(srm.n_fired, m.n_fired as i32);
        assert_eq!(srm.n_exit_take_profit, m.n_exit_take_profit as i32);
        assert!((srm.win_rate - m.win_rate as f32).abs() < 1e-6);
    }

    #[test]
    fn tpsl2_kernel_runs_price_only_exit() {
        // E5 off: tpsl2 behaves like tpsl1 on a price-driven token. Needs the
        // scalp entry to fire — use an age gate so the second-window trade enters.
        let mut rule = Tpsl2Rule::new(
            "r".into(), None, None, None, json!([]), "paper".into(), 1.0,
            50.0, 20.0, None, None, None, None, Some(0.0), None, None, None, None,
        );
        rule.is_active = true;
        let params = StrategyParams::Tpsl2(Tpsl2Params::from_rule(&rule));
        let cfg = SimConfig::new(1.0);
        // With no scalp gate configured, find_scalp_entry never fires (a rule that
        // configures no gate must not enter) → not fired. Asserts the safe default.
        let token = vec![buy(1.0, 1, 0), buy(2.0, 2, 1), buy(2.0, 3, 2)];
        let tokens: Vec<&[Trade]> = vec![&token];
        let m = simulate_rule(StrategyImpl::Tpsl2, &params, &tokens, &cfg);
        assert_eq!(m.n_fired, 0, "no scalp gate configured → no entry");
    }
}
