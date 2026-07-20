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
    /// The generic engine's single metric-condition exit (`ExitReason::Metrics`):
    /// any of a rule's exit metric conditions became true. Collapses the legacy
    /// ladder's granular metric exits (trailing / stall / time / liquidity /
    /// next-kill) into one bucket — the redesigned engine has no per-metric exit
    /// codes, only "an exit condition group fired". Only the generic sweep/replay
    /// emits it; the legacy strategies never do.
    Metrics = 10,
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
            "Metrics" => ExitCode::Metrics,
            "Open" => ExitCode::Open,
            // Matched fingerprint / armed but never filled — distinct from still-Open.
            "NoEntry" => ExitCode::NoEntry,
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
///
/// **Also the wire shape.** Serialized straight to the frontend by every surface
/// that reports a run's outcome — single-rule simulate, grouped sweep, and a
/// live/paper run — so all three send the same field names and the UI can render
/// them through one component instead of three ad-hoc shapes (parity plan B4).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RunMetrics {
    pub n_fired: u64,
    pub n_open: u64,
    pub n_closed: u64,
    /// Realized-only (`wins / n_closed`) — a still-`Open` mark is not a win/loss yet.
    pub win_rate: f64,
    /// Realized-only: the sum of closed positions' PnL, never a still-`Open` mark.
    pub total_pnl_sol: f64,
    /// **Unrealized** counterpart to `total_pnl_sol`: the sum of still-`Open`
    /// positions' mark-to-last-price PnL. Reported alongside the realized total
    /// (never folded into it) so a run whose losers are all still open can't read
    /// as profitable — `total_pnl_sol + open_pnl_sol` is the mark-to-market total.
    /// Every other field on this struct stays realized-only (parity plan C2).
    pub open_pnl_sol: f64,
    pub expectancy_sol: f64,
    pub mean_pnl_pct: f64,
    pub median_pnl_pct: f64,
    pub p90_pnl_pct: f64,
    pub best_pnl_pct: f64,
    pub worst_pnl_pct: f64,
    pub std_pnl_pct: f64,
    pub profit_factor: Option<f64>,
    /// Mean per-trade pnl% over **all fired** positions (still-open marks
    /// included). The profitability term in [`checklist_score`].
    pub mtm_pnl_pct: f64,
    /// Checklist rank (see [`checklist_score`]): MTM% × fire-rate × open-drag
    /// × win-rate. `None` when nothing fired. Grouped sweep rewrites this with
    /// the group's matched-token count after finalize.
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
    /// Generic-engine metric-condition exits (`ExitCode::Metrics`). 0 for the
    /// legacy strategies (which use the granular ladder codes above).
    /// Equals `n_exit_metrics_win + n_exit_metrics_loss`.
    pub n_exit_metrics: u32,
    /// Metric exits with positive realized SOL. `#[serde(default)]` so older
    /// sweep rows that only stored the total still deserialize.
    #[serde(default)]
    pub n_exit_metrics_win: u32,
    /// Metric exits that are not wins (loss or break-even).
    #[serde(default)]
    pub n_exit_metrics_loss: u32,
    pub n_exit_open: u32,
}

// ── Streaming aggregate (ported from lab sweep::aggregate) ─────────────────────

/// Floor under win-rate in [`checklist_score`] so an all-open book (WR = 0)
/// still gets a tiny multiplier instead of zeroing the whole rank.
const SCORE_WIN_RATE_FLOOR: f64 = 0.01;
/// Weight on open-share in [`checklist_score`]: `× (1 − w · n_open/n_fired)`.
const SCORE_OPEN_DRAG: f64 = 0.5;

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
/// second copy of the sketch / score math to drift.
#[derive(Clone)]
pub struct RunAgg {
    fired: u64,
    open: u64,
    wins: u64,
    pnl_sol_sum: f64,
    /// Unrealized mark-to-last-price sum over the still-`Open` positions. Kept
    /// strictly apart from `pnl_sol_sum` so no realized figure can absorb it.
    open_pnl_sol_sum: f64,
    gross_win_sol: f64,
    gross_loss_sol: f64,
    pnl_min: f32,
    pnl_max: f32,
    pnl_sketch: QuantileSketch,
    closed_pct_sum: f64,
    closed_pct_sum_sq: f64,
    /// Σ pnl% over **all fired** (open marks included) — feeds `mtm_pnl_pct`.
    fired_pct_sum: f64,
    holding_sum: i64,
    holding_sketch: QuantileSketch,
    exit_counts: [u32; 10],
    /// `ExitCode::Metrics` with `pnl_sol > 0`.
    metrics_win: u32,
    /// `ExitCode::Metrics` that are not wins (`pnl_sol <= 0`).
    metrics_loss: u32,
}

impl Default for RunAgg {
    fn default() -> Self {
        Self {
            fired: 0,
            open: 0,
            wins: 0,
            pnl_sol_sum: 0.0,
            open_pnl_sol_sum: 0.0,
            gross_win_sol: 0.0,
            gross_loss_sol: 0.0,
            pnl_min: f32::INFINITY,
            pnl_max: f32::NEG_INFINITY,
            pnl_sketch: QuantileSketch::default(),
            closed_pct_sum: 0.0,
            closed_pct_sum_sq: 0.0,
            fired_pct_sum: 0.0,
            holding_sum: 0,
            holding_sketch: QuantileSketch::default(),
            exit_counts: [0; 10],
            metrics_win: 0,
            metrics_loss: 0,
        }
    }
}

impl RunAgg {
    /// Fold one token's outcome into the accumulator. No-entry rows are ignored.
    /// A still-`Open` outcome counts toward `n_fired`/`n_open`/its exit-count
    /// slot, and its mark-to-last-price PnL accumulates into the separate
    /// `open_pnl_sol_sum` — because it is unrealized it never touches the
    /// realized PnL sum, win/loss counters, quantile sketch, or holding-time
    /// stats (parity plan C2).
    pub fn record(&mut self, o: &TokenOutcome) {
        if !o.fired {
            return;
        }
        self.fired += 1;
        let p = o.pnl_percent as f64;
        self.fired_pct_sum += p;
        if o.exit == ExitCode::Open {
            self.open += 1;
            self.open_pnl_sol_sum += o.pnl_sol as f64;
        } else {
            self.pnl_sol_sum += o.pnl_sol as f64;
            self.pnl_min = self.pnl_min.min(o.pnl_percent);
            self.pnl_max = self.pnl_max.max(o.pnl_percent);
            self.pnl_sketch.record(p);
            if o.pnl_sol > 0.0 {
                self.wins += 1;
                self.gross_win_sol += o.pnl_sol as f64;
            } else if o.pnl_sol < 0.0 {
                self.gross_loss_sol += -(o.pnl_sol as f64);
            }
            self.holding_sum += o.holding_secs;
            self.holding_sketch.record(o.holding_secs as f64);
            self.closed_pct_sum += p;
            self.closed_pct_sum_sq += p * p;
            if o.exit == ExitCode::Metrics {
                if o.pnl_sol > 0.0 {
                    self.metrics_win += 1;
                } else {
                    self.metrics_loss += 1;
                }
            }
        }
        self.exit_counts[exit_index(o.exit)] += 1;
    }

    /// Collapse the accumulator to the final rolled-up [`RunMetrics`]. Every
    /// PnL/win-rate/holding figure is realized-only (denominator `n_closed`,
    /// never `n_fired`) — see [`RunAgg`]'s doc. Score uses MTM% (opens included)
    /// with `matched = n_fired` (fire-rate = 1); grouped sweep rewrites score
    /// with the group's token count via [`checklist_score`].
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
        let mtm_pnl_pct = if self.fired == 0 {
            0.0
        } else {
            self.fired_pct_sum / self.fired as f64
        };
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
        let expectancy_sol = if n_closed == 0 { 0.0 } else { self.pnl_sol_sum / n };
        let std_pnl_pct = sample_std_pct(n_closed, self.closed_pct_sum, self.closed_pct_sum_sq);
        let win_rate = if n_closed == 0 { 0.0 } else { self.wins as f64 / n };
        let score = checklist_score(self.fired, self.open, self.fired, mtm_pnl_pct, win_rate);
        RunMetrics {
            n_fired: self.fired,
            n_open: self.open,
            n_closed,
            win_rate,
            total_pnl_sol: self.pnl_sol_sum,
            open_pnl_sol: self.open_pnl_sol_sum,
            expectancy_sol,
            mean_pnl_pct,
            median_pnl_pct,
            p90_pnl_pct,
            best_pnl_pct,
            worst_pnl_pct,
            std_pnl_pct,
            profit_factor,
            mtm_pnl_pct,
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
            n_exit_metrics: self.exit_counts[9],
            n_exit_metrics_win: self.metrics_win,
            n_exit_metrics_loss: self.metrics_loss,
        }
    }
}

/// Exact-quantile counterpart to [`RunAgg`] for a **bounded** set of outcomes —
/// e.g. one sweep combo's per-token rows when re-simulated standalone (the
/// grouped-sweep drill-in), never the full combos × tokens sweep (unbounded;
/// that's exactly why [`RunAgg`] streams through a fixed-size sketch instead of
/// holding every value). Same realized-only semantics as `RunAgg::record`/
/// `finalize` (a still-`Open` mark contributes to `n_fired`/`n_open` only, never
/// to a PnL/win-rate/holding figure), but `median_pnl_pct`/`p90_pnl_pct`/
/// `median_holding_secs` are exact nearest-rank percentiles over the collected
/// values instead of the sketch's ~15% relative error — so a drill-in's summary
/// can be compared directly against a single-rule simulate's own small-N exact
/// aggregate (parity plan D1).
pub fn exact_run_metrics<'a>(outcomes: impl Iterator<Item = &'a TokenOutcome>) -> RunMetrics {
    let mut fired = 0u64;
    let mut open = 0u64;
    let mut wins = 0u64;
    let mut pnl_sol_sum = 0.0f64;
    let mut open_pnl_sol_sum = 0.0f64;
    let mut gross_win_sol = 0.0f64;
    let mut gross_loss_sol = 0.0f64;
    let mut closed_pct: Vec<f64> = Vec::new();
    let mut closed_holding: Vec<i64> = Vec::new();
    let mut fired_pct_sum = 0.0f64;
    let mut exit_counts = [0u32; 10];
    let mut metrics_win = 0u32;
    let mut metrics_loss = 0u32;

    for o in outcomes {
        if !o.fired {
            continue;
        }
        fired += 1;
        let pnl_pct = o.pnl_percent as f64;
        fired_pct_sum += pnl_pct;
        if o.exit == ExitCode::Open {
            open += 1;
            open_pnl_sol_sum += o.pnl_sol as f64;
        } else {
            let pnl_sol = o.pnl_sol as f64;
            pnl_sol_sum += pnl_sol;
            if pnl_sol > 0.0 {
                wins += 1;
                gross_win_sol += pnl_sol;
            } else if pnl_sol < 0.0 {
                gross_loss_sol += -pnl_sol;
            }
            closed_pct.push(pnl_pct);
            closed_holding.push(o.holding_secs);
            if o.exit == ExitCode::Metrics {
                if o.pnl_sol > 0.0 {
                    metrics_win += 1;
                } else {
                    metrics_loss += 1;
                }
            }
        }
        exit_counts[exit_index(o.exit)] += 1;
    }

    let n_closed = closed_pct.len() as u64;
    let n = n_closed as f64;
    closed_pct.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let (median_pnl_pct, p90_pnl_pct, best_pnl_pct, worst_pnl_pct) = if closed_pct.is_empty() {
        (0.0, 0.0, 0.0, 0.0)
    } else {
        (
            exact_quantile_f64(&closed_pct, 0.5),
            exact_quantile_f64(&closed_pct, 0.9),
            *closed_pct.last().expect("non-empty"),
            closed_pct[0],
        )
    };
    let closed_pct_sum: f64 = closed_pct.iter().sum();
    let closed_pct_sum_sq: f64 = closed_pct.iter().map(|p| p * p).sum();
    let mean_pnl_pct = if n_closed == 0 { 0.0 } else { closed_pct_sum / n };
    let mtm_pnl_pct = if fired == 0 { 0.0 } else { fired_pct_sum / fired as f64 };
    closed_holding.sort_unstable();
    let (avg_holding_secs, median_holding_secs) = if closed_holding.is_empty() {
        (0.0, 0.0)
    } else {
        (closed_holding.iter().sum::<i64>() as f64 / n, exact_quantile_i64(&closed_holding, 0.5))
    };
    let profit_factor =
        if gross_loss_sol > 0.0 { Some(gross_win_sol / gross_loss_sol) } else { None };
    let expectancy_sol = if n_closed == 0 { 0.0 } else { pnl_sol_sum / n };
    let std_pnl_pct = sample_std_pct(n_closed, closed_pct_sum, closed_pct_sum_sq);
    let win_rate = if n_closed == 0 { 0.0 } else { wins as f64 / n };
    let score = checklist_score(fired, open, fired, mtm_pnl_pct, win_rate);

    RunMetrics {
        n_fired: fired,
        n_open: open,
        n_closed,
        win_rate,
        total_pnl_sol: pnl_sol_sum,
        open_pnl_sol: open_pnl_sol_sum,
        expectancy_sol,
        mean_pnl_pct,
        median_pnl_pct,
        p90_pnl_pct,
        best_pnl_pct,
        worst_pnl_pct,
        std_pnl_pct,
        profit_factor,
        mtm_pnl_pct,
        score,
        avg_holding_secs,
        median_holding_secs,
        n_exit_take_profit: exit_counts[0],
        n_exit_stop_loss: exit_counts[1],
        n_exit_trailing: exit_counts[2],
        n_exit_stall: exit_counts[3],
        n_exit_time: exit_counts[4],
        n_exit_liquidity: exit_counts[5],
        n_exit_open: exit_counts[6],
        n_exit_next_kill: exit_counts[7],
        n_exit_dead: exit_counts[8],
        n_exit_metrics: exit_counts[9],
        n_exit_metrics_win: metrics_win,
        n_exit_metrics_loss: metrics_loss,
    }
}

/// A run reported **twice over the same outcomes** — the shape every surface that
/// summarizes a run (single-rule simulate, grouped sweep, live/paper) sends to the
/// frontend, so one component renders all three (parity plan B4/F1-F3).
///
/// Reporting both is the point. A still-`Open` position has a mark-to-last-price
/// PnL but no realized outcome, so [`realized`](Self::realized) measures closed
/// trades only — which, read alone, flatters a rule that simply never closed its
/// losers: they never entered the sum. [`mtm`](Self::mtm) values every fired
/// position, open bags included. Neither is "the" answer — realized is what
/// actually happened, MTM is what the run is currently worth, and the **gap
/// between them is the signal**: it says how much of the headline is still
/// unsettled.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RunSummary {
    /// Closed positions only. Identical to [`exact_run_metrics`]'s output.
    pub realized: RunMetrics,
    /// Every fired position, open ones valued at their last price.
    ///
    /// Only the PnL / win-rate / central-tendency fields are meaningful here; the
    /// `n_exit_*` counts are forced to zero because an open position has no exit
    /// reason to bucket — read them off [`realized`](Self::realized).
    pub mtm: RunMetrics,
}

/// Build the two-band [`RunSummary`] from one pass' worth of outcomes.
///
/// The MTM band is produced by re-running the **same** [`exact_run_metrics`] with
/// the open positions reclassified as closed, rather than by a second hand-rolled
/// copy of the arithmetic — so the two bands can never drift apart and compare
/// tile-for-tile down the column.
pub fn run_summary<'a>(outcomes: impl Iterator<Item = &'a TokenOutcome>) -> RunSummary {
    let all: Vec<TokenOutcome> = outcomes.copied().collect();
    let realized = exact_run_metrics(all.iter());

    // Reclassify Open → a closed bucket so the same aggregator counts its mark as a
    // settled outcome. `TakeProfit` is an arbitrary stand-in purely to get past the
    // `== Open` test; the resulting exit counts are meaningless and zeroed below.
    let marked: Vec<TokenOutcome> = all
        .iter()
        .map(|o| TokenOutcome {
            exit: if o.exit == ExitCode::Open { ExitCode::TakeProfit } else { o.exit },
            ..*o
        })
        .collect();
    let mut mtm = exact_run_metrics(marked.iter());

    // An open position contributes no exit reason — don't let the stand-in above
    // masquerade as a real take-profit.
    mtm.n_exit_take_profit = 0;
    mtm.n_exit_stop_loss = 0;
    mtm.n_exit_trailing = 0;
    mtm.n_exit_stall = 0;
    mtm.n_exit_time = 0;
    mtm.n_exit_liquidity = 0;
    mtm.n_exit_next_kill = 0;
    mtm.n_exit_dead = 0;
    mtm.n_exit_metrics = 0;
    mtm.n_exit_metrics_win = 0;
    mtm.n_exit_metrics_loss = 0;
    mtm.n_exit_open = 0;
    // The open cohort is what MTM folded in; keep the counts describing the run.
    mtm.n_open = realized.n_open;
    mtm.open_pnl_sol = realized.open_pnl_sol;

    RunSummary { realized, mtm }
}

/// Nearest-rank percentile `q` (`0.0..=1.0`) over an ascending-sorted, non-empty
/// slice. `q=0.5`/`q=0.9` are the median/p90 [`exact_run_metrics`] needs.
fn exact_quantile_f64(sorted: &[f64], q: f64) -> f64 {
    let idx = (((sorted.len() - 1) as f64) * q).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

/// [`exact_quantile_f64`]'s `i64` counterpart (holding-time seconds).
fn exact_quantile_i64(sorted: &[i64], q: f64) -> f64 {
    let idx = (((sorted.len() - 1) as f64) * q).round() as usize;
    sorted[idx.min(sorted.len() - 1)] as f64
}

/// Sample stddev of closed per-trade pnl% — display column only.
fn sample_std_pct(n_closed: u64, sum: f64, sum_sq: f64) -> f64 {
    if n_closed < 2 {
        return 0.0;
    }
    let n = n_closed as f64;
    let mean = sum / n;
    let var = ((sum_sq - n * mean * mean) / (n - 1.0)).max(0.0);
    var.sqrt()
}

/// Manual-checklist rank used by the grouped sweep:
/// `mtm_pct × (n_fired/matched) × (1 − 0.5·n_open/n_fired) × max(win_rate, ε)`.
///
/// - `mtm_pct` — mean pnl% over all fired (still-open marks included)
/// - fire-rate — coverage of the matched group (capped at 1)
/// - open-drag — soft penalty for unsettled bags
/// - win-rate — closed-only; floored so all-open books don't zero the score
///
/// `None` when nothing fired or `matched == 0`. Public so the sweep can rewrite
/// a combo's score with the group's true matched-token count after finalize.
pub fn checklist_score(
    n_fired: u64,
    n_open: u64,
    matched: u64,
    mtm_pnl_pct: f64,
    win_rate: f64,
) -> Option<f64> {
    if n_fired == 0 || matched == 0 {
        return None;
    }
    let fire_rate = (n_fired as f64 / matched as f64).min(1.0);
    let open_drag = (n_open as f64 / n_fired as f64).min(1.0);
    let wr = win_rate.max(SCORE_WIN_RATE_FLOOR);
    Some(mtm_pnl_pct * fire_rate * (1.0 - SCORE_OPEN_DRAG * open_drag) * wr)
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
        ExitCode::Metrics => 9,
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

    // ── exact_run_metrics (parity plan D1) ──────────────────────────────────

    fn outcome(pnl_sol: f32, pnl_pct: f32, exit: ExitCode, holding: i64) -> TokenOutcome {
        TokenOutcome { fired: true, holding_secs: holding, pnl_percent: pnl_pct, pnl_sol, exit }
    }

    #[test]
    fn exact_metrics_matches_streaming_agg_on_the_same_outcomes() {
        // exact_run_metrics must agree with RunAgg (the streaming/sketch path) on
        // every field RunAgg computes exactly already — the only thing that should
        // ever differ is that median/p90/median_holding_secs stop being approximate.
        let rows = vec![
            outcome(2.0, 100.0, ExitCode::TakeProfit, 10),
            outcome(-1.0, -50.0, ExitCode::StopLoss, 20),
            outcome(5.0, 999.0, ExitCode::Open, 0),
            TokenOutcome::no_entry(),
        ];
        let mut agg = RunAgg::default();
        for o in &rows {
            agg.record(o);
        }
        let streaming = agg.finalize();
        let exact = exact_run_metrics(rows.iter());
        assert_eq!(exact.n_fired, streaming.n_fired);
        assert_eq!(exact.n_open, streaming.n_open);
        assert_eq!(exact.n_closed, streaming.n_closed);
        assert!((exact.win_rate - streaming.win_rate).abs() < 1e-9);
        assert!((exact.total_pnl_sol - streaming.total_pnl_sol).abs() < 1e-9);
        assert!((exact.mean_pnl_pct - streaming.mean_pnl_pct).abs() < 1e-9);
        assert_eq!(exact.profit_factor, streaming.profit_factor);
        assert!((exact.score.unwrap() - streaming.score.unwrap()).abs() < 1e-9);
    }

    #[test]
    fn exact_median_and_p90_have_no_sketch_error() {
        // 1..=1000 → exact median is 500 or 501 (nearest-rank on 1000 values picks
        // one deterministically), exact p90 is exactly 900 — no ~15% band needed.
        let rows: Vec<TokenOutcome> =
            (1..=1000).map(|v| outcome(0.1, v as f32, ExitCode::TakeProfit, v)).collect();
        let m = exact_run_metrics(rows.iter());
        assert!((500.0..=501.0).contains(&m.median_pnl_pct), "median {}", m.median_pnl_pct);
        assert_eq!(m.p90_pnl_pct, 900.0);
    }

    #[test]
    fn exact_metrics_excludes_open_from_headline_figures() {
        let rows = vec![
            outcome(1.0, 50.0, ExitCode::TakeProfit, 10),
            outcome(-1.0, -50.0, ExitCode::StopLoss, 10),
            outcome(1_000.0, 5_000.0, ExitCode::Open, 0),
        ];
        let m = exact_run_metrics(rows.iter());
        assert_eq!(m.n_fired, 3);
        assert_eq!(m.n_open, 1);
        assert!((m.total_pnl_sol - 0.0).abs() < 1e-9);
        assert_eq!(m.best_pnl_pct, 50.0);
        assert_eq!(m.worst_pnl_pct, -50.0);
    }

    // ── two-band run summary (parity plan B4) ───────────────────────────────

    #[test]
    fn run_summary_bands_split_realized_from_mark_to_market() {
        let rows = vec![
            outcome(1.0, 50.0, ExitCode::TakeProfit, 10),
            outcome(-1.0, -50.0, ExitCode::StopLoss, 10),
            outcome(-4.0, -80.0, ExitCode::Open, 0), // a big unrealized LOSER
        ];
        let s = run_summary(rows.iter());

        // Realized reads flat — the loser never closed.
        assert!((s.realized.total_pnl_sol - 0.0).abs() < 1e-9);
        assert_eq!(s.realized.n_closed, 2);
        // MTM tells the truth about what the run is currently worth.
        assert!((s.mtm.total_pnl_sol - -4.0).abs() < 1e-9);
        assert_eq!(s.mtm.n_closed, 3, "MTM settles every fired position");
        assert!((s.mtm.worst_pnl_pct - -80.0).abs() < 1e-9, "the open loser is the MTM worst");
        // Both bands agree on how much is unsettled.
        assert_eq!(s.realized.n_open, 1);
        assert_eq!(s.mtm.n_open, 1);
        assert!((s.mtm.open_pnl_sol - -4.0).abs() < 1e-9);
    }

    #[test]
    fn run_summary_bands_are_identical_when_nothing_is_open() {
        let rows = vec![
            outcome(1.0, 50.0, ExitCode::TakeProfit, 10),
            outcome(-1.0, -50.0, ExitCode::StopLoss, 10),
        ];
        let s = run_summary(rows.iter());
        assert!((s.realized.total_pnl_sol - s.mtm.total_pnl_sol).abs() < 1e-9);
        assert!((s.realized.win_rate - s.mtm.win_rate).abs() < 1e-9);
        assert!((s.realized.median_pnl_pct - s.mtm.median_pnl_pct).abs() < 1e-9);
    }

    #[test]
    fn mtm_band_reports_no_exit_reasons() {
        // The Open→TakeProfit reclassification must never surface as a real exit.
        let rows = vec![
            outcome(1.0, 50.0, ExitCode::TakeProfit, 10),
            outcome(2.0, 90.0, ExitCode::Open, 0),
        ];
        let s = run_summary(rows.iter());
        assert_eq!(s.realized.n_exit_take_profit, 1);
        assert_eq!(s.mtm.n_exit_take_profit, 0, "stand-in must not read as a take-profit");
    }

    #[test]
    fn exact_metrics_over_no_outcomes_is_all_zero() {
        let m = exact_run_metrics(std::iter::empty());
        assert_eq!(m.n_fired, 0);
        assert_eq!(m.score, None);
        assert_eq!(m.profit_factor, None);
    }

    // ── checklist_score ─────────────────────────────────────────────────────

    #[test]
    fn score_is_mtm_pct_when_fully_closed_and_all_wins() {
        // fire_rate=1, open_drag=0, win_rate=1 → score == mtm_pnl_pct.
        let rows = vec![
            outcome(0.5, 50.0, ExitCode::TakeProfit, 5),
            outcome(0.5, 50.0, ExitCode::TakeProfit, 5),
        ];
        let m = exact_run_metrics(rows.iter());
        assert!((m.mtm_pnl_pct - 50.0).abs() < 1e-9);
        assert_eq!(m.score, Some(50.0));
    }

    #[test]
    fn score_includes_open_marks_in_mtm_and_penalises_open_share() {
        let rows = vec![
            outcome(0.1, 10.0, ExitCode::TakeProfit, 5),
            outcome(0.1, 10.0, ExitCode::TakeProfit, 5),
            outcome(5.0, 90.0, ExitCode::Open, 0),
        ];
        let m = exact_run_metrics(rows.iter());
        // MTM mean = (10+10+90)/3 = 36.666…
        assert!((m.mtm_pnl_pct - 110.0 / 3.0).abs() < 1e-9);
        // × 1 × (1 − 0.5·1/3) × 1.0 = × (5/6)
        let expected = m.mtm_pnl_pct * (1.0 - 0.5 / 3.0);
        assert!((m.score.unwrap() - expected).abs() < 1e-9);
    }

    #[test]
    fn score_none_when_nothing_fired() {
        assert_eq!(exact_run_metrics(std::iter::empty()).score, None);
        assert_eq!(
            checklist_score(0, 0, 10, 50.0, 1.0),
            None,
            "unfired combo has no score"
        );
    }

    #[test]
    fn checklist_score_scales_with_fire_rate() {
        // Same book, half coverage → half score.
        let full = checklist_score(10, 0, 10, 40.0, 1.0).unwrap();
        let half = checklist_score(5, 0, 10, 40.0, 1.0).unwrap();
        assert!((full - 40.0).abs() < 1e-9);
        assert!((half - 20.0).abs() < 1e-9);
    }
}
