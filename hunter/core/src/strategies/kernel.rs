//! Simulation **kernel** — the shared metric-aggregation primitives that turn a
//! stream of per-token [`TokenOutcome`]s into one rolled-up [`RunMetrics`] row.
//! The same primitives back every replay path (`lab`'s param sweep, live/paper
//! run rollups), so live / paper / sweep results stay comparable.
//!
//! PnL is priced through the shared [`CostModel`] ([`round_trip_with_costs`]) so
//! a backtest reflects the frictions the live trader pays. The bounded
//! `QuantileSketch` + streaming [`RunAgg`] are the single home for the sketch /
//! robust-score math: `lab`'s per-combo sweep folds into [`RunAgg`] via its thin
//! `ComboAgg` wrapper, so backtest and live/paper metrics can never drift to a
//! second copy.

use crate::config::constants::{
    COMPUTE_UNIT_LIMIT_CURVE_BUY, COMPUTE_UNIT_LIMIT_CURVE_SELL,
    COMPUTE_UNIT_PRICE_MICRO_LAMPORTS, LAMPORTS_PER_SOL,
};

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
    /// Analysis-only death-close: the ladder never fired but the token is provably
    /// dead (liquidity gone + gone silent), so the sim closes the bag at the last
    /// meaningful trade instead of leaving it `Open` at a stale price. Live never
    /// produces this (it closes silent tokens via its clock sweep). Counts as a
    /// **closed** loss in the rollup. See [`crate::strategies::death`].
    Dead = 9,
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
            "Dead" => ExitCode::Dead,
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

/// Round a PnL figure through `f32` precision and back. The sweep's
/// [`TokenOutcome`] stores `pnl_sol`/`pnl_percent` as `f32` (register-friendly,
/// no per-outcome allocation across millions of `(combo × token)` rows); a
/// single-rule simulate keeps `f64` end-to-end. Left unrounded, the two paths'
/// headline numbers drift by float noise even when every decision and cost input
/// is identical. Simulate calls this on both `round_trip_with_costs` outputs
/// before display/summation so it quantizes exactly like the sweep does.
pub fn quantize_f32(x: f64) -> f64 {
    x as f32 as f64
}

/// **Canonical "return %"** — the single definition of realized return shared by
/// the live rules table, the lab rules table, the positions-summary panel, and the
/// sweep. Capital-weighted: net PnL as a percent of the total SOL *deployed* across
/// the closed positions, i.e. `Σ pnl_sol / Σ entry_sol × 100`.
///
/// Because the denominator is total capital (always ≥ 0), the sign of this figure
/// **can never disagree** with the sign of the summed SOL PnL — the two headline
/// columns move together by construction. This replaces the old
/// `mean(per-trade price %)`, which mixed an equal-weighted mean of size-independent
/// price ratios with a size-weighted SOL sum and so could show `+%`/`−◎` (or the
/// reverse) on the same rule. Under a fixed per-trade notional (the sweep) it
/// reduces exactly to the mean of per-trade percents, so backtest numbers are
/// unchanged. Returns `0.0` when no capital was deployed.
pub fn weighted_return_pct(sum_pnl_sol: f64, sum_capital_sol: f64) -> f64 {
    if sum_capital_sol > 0.0 {
        sum_pnl_sol / sum_capital_sol * 100.0
    } else {
        0.0
    }
}

// ── Run metrics ────────────────────────────────────────────────────────────────

/// Rolled-up metrics for one run across a token corpus. Field-for-field the
/// `strategy_run_metrics` columns (plus the sweep's `score`, ignored when
/// persisting a live/paper run).
#[derive(Clone, Debug, PartialEq)]
pub struct RunMetrics {
    pub n_fired: u64,
    pub n_open: u64,
    pub n_closed: u64,
    /// Realized-only (`wins / n_closed`) — a still-`Open` mark is not a win/loss yet.
    pub win_rate: f64,
    /// Realized-only: the sum of closed positions' PnL, never a still-`Open` mark.
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
    /// Analysis-only death-closes (`ExitCode::Dead`): positions closed at the last
    /// meaningful trade because the token died silent. 0 in live rollups. Counts as
    /// closed (loss), so it lifts `n_closed` and lowers `n_open`.
    pub n_exit_dead: u32,
    pub n_exit_open: u32,
}

// ── Streaming aggregate (ported from lab sweep::aggregate) ─────────────────────

/// Confidence multiplier for the robust score's one-sided lower bound (~95% z).
const SCORE_Z: f64 = 1.64;

/// Streaming accumulator across every token a run fires on. Every PnL/holding/
/// win-rate stat is **realized-only** (closed positions — includes the
/// analysis-only death-close, excludes a still-`Open` mark-to-last-price): an
/// unrealized mark isn't a trade outcome yet, so folding it into "win rate" or
/// "total PnL" mixed marks-to-market with realized returns and made a sweep's
/// headline numbers depend on exactly when the corpus window happened to end
/// (parity plan C2). `n_fired`/`n_open` still count every position taken,
/// `Open` still included, so the UI can show "X open" alongside the realized
/// figures. O(1) per run — interior quantiles via a fixed [`QuantileSketch`].
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
    pnl_min: f32,
    pnl_max: f32,
    pnl_sketch: QuantileSketch,
    closed_pct_sum: f64,
    closed_pct_sum_sq: f64,
    holding_sum: i64,
    holding_sketch: QuantileSketch,
    exit_counts: [u32; 9],
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
            pnl_min: f32::INFINITY,
            pnl_max: f32::NEG_INFINITY,
            pnl_sketch: QuantileSketch::default(),
            closed_pct_sum: 0.0,
            closed_pct_sum_sq: 0.0,
            holding_sum: 0,
            holding_sketch: QuantileSketch::default(),
            exit_counts: [0; 9],
        }
    }
}

impl RunAgg {
    /// Fold one token's outcome into the accumulator. No-entry rows are ignored.
    /// A still-`Open` outcome counts toward `n_fired`/`n_open`/its exit-count
    /// slot only — its mark-to-last-price PnL is unrealized, so it never touches
    /// the PnL sums, win/loss counters, quantile sketch, or holding-time stats
    /// (parity plan C2).
    pub fn record(&mut self, o: &TokenOutcome) {
        if !o.fired {
            return;
        }
        self.fired += 1;
        if o.exit == ExitCode::Open {
            self.open += 1;
        } else {
            self.pnl_sol_sum += o.pnl_sol as f64;
            self.pnl_min = self.pnl_min.min(o.pnl_percent);
            self.pnl_max = self.pnl_max.max(o.pnl_percent);
            self.pnl_sketch.record(o.pnl_percent as f64);
            if o.pnl_sol > 0.0 {
                self.wins += 1;
                self.gross_win_sol += o.pnl_sol as f64;
            } else if o.pnl_sol < 0.0 {
                self.gross_loss_sol += -(o.pnl_sol as f64);
            }
            self.holding_sum += o.holding_secs;
            self.holding_sketch.record(o.holding_secs as f64);
            let p = o.pnl_percent as f64;
            self.closed_pct_sum += p;
            self.closed_pct_sum_sq += p * p;
        }
        self.exit_counts[exit_index(o.exit)] += 1;
    }

    /// Collapse the accumulator to the final rolled-up [`RunMetrics`]. Every
    /// PnL/win-rate/holding figure is realized-only (denominator `n_closed`,
    /// never `n_fired`) — see [`RunAgg`]'s doc.
    pub fn finalize(self) -> RunMetrics {
        let n_closed = self.fired - self.open;
        let n = n_closed as f64;
        let (median_pnl_pct, p90_pnl_pct, best_pnl_pct, worst_pnl_pct) = if n_closed == 0 {
            (0.0, 0.0, 0.0, 0.0)
        } else {
            (
                self.pnl_sketch.quantile(0.5),
                self.pnl_sketch.quantile(0.9),
                self.pnl_max as f64,
                self.pnl_min as f64,
            )
        };
        let mean_pnl_pct = if n_closed == 0 { 0.0 } else { self.closed_pct_sum / n };
        let (avg_holding_secs, median_holding_secs) = if n_closed == 0 {
            (0.0, 0.0)
        } else {
            (self.holding_sum as f64 / n, self.holding_sketch.quantile(0.5))
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
            win_rate: if n_closed == 0 { 0.0 } else { self.wins as f64 / n },
            total_pnl_sol: self.pnl_sol_sum,
            expectancy_sol: if n_closed == 0 { 0.0 } else { self.pnl_sol_sum / n },
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
            n_exit_dead: self.exit_counts[8],
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
        ExitCode::Dead => 8,
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
}
