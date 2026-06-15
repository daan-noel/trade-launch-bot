//! Aggregate per-(combo, token) [`TokenOutcome`]s into one ranked metrics row
//! per combo — the param-pair table the UI shows. The sweep produces combos ×
//! tokens outcomes; this collapses them to `combos` rows (bounded — hundreds to
//! low thousands), small enough to hold in RAM, persist whole, and serve whole.
//!
//! Metrics cover the criteria a param search ranks on: profitability
//! (`total_pnl_sol`, `expectancy_sol`, `profit_factor`), success (`win_rate`),
//! central return (`mean`/`median_pnl_pct`), tails (`best`/`worst`/`p90`),
//! holding time, and the exit-reason mix.

use crate::analysis::strategy::{ExitCode, TokenOutcome};

/// Streaming accumulator for one combo across every token it fired on. PnL stats
/// mark open positions to their last price (so a still-running token still
/// counts); holding-time stats use closed positions only.
#[derive(Default, Clone)]
pub struct ComboAgg {
    fired: u64,
    open: u64,
    wins: u64,
    pnl_sol_sum: f64,
    gross_win_sol: f64,
    gross_loss_sol: f64,
    /// pnl% of every fired token (mark-to-market for open) — for quantiles.
    pnl_pct: Vec<f32>,
    /// holding seconds of closed positions only.
    holding: Vec<i32>,
    /// counts indexed by [`exit_index`].
    exit_counts: [u32; 8],
}

impl ComboAgg {
    /// Fold one token's outcome under this combo. No-entry rows are ignored.
    pub fn record(&mut self, o: &TokenOutcome) {
        if !o.fired {
            return;
        }
        self.fired += 1;
        self.pnl_sol_sum += o.pnl_sol as f64;
        self.pnl_pct.push(o.pnl_percent);
        if o.pnl_sol > 0.0 {
            self.wins += 1;
            self.gross_win_sol += o.pnl_sol as f64;
        } else if o.pnl_sol < 0.0 {
            self.gross_loss_sol += -(o.pnl_sol as f64);
        }
        if o.exit == ExitCode::Open {
            self.open += 1;
        } else {
            self.holding.push(o.holding_secs as i32);
        }
        self.exit_counts[exit_index(o.exit)] += 1;
    }

    /// Collapse to the final ranked row. Sorts the internal vectors.
    pub fn finalize(mut self, combo_id: u32) -> ComboMetrics {
        let n = self.fired as f64;
        let (median_pnl_pct, p90_pnl_pct, best_pnl_pct, worst_pnl_pct) = pct_stats(&mut self.pnl_pct);
        let mean_pnl_pct = if self.fired == 0 {
            0.0
        } else {
            self.pnl_pct.iter().map(|v| *v as f64).sum::<f64>() / n
        };
        let (avg_holding_secs, median_holding_secs) = holding_stats(&mut self.holding);
        let profit_factor = if self.gross_loss_sol > 0.0 {
            Some(self.gross_win_sol / self.gross_loss_sol)
        } else {
            None // no losing trades → undefined (shown as ∞)
        };
        ComboMetrics {
            combo_id,
            n_fired: self.fired,
            n_open: self.open,
            n_closed: self.fired - self.open,
            win_rate: if self.fired == 0 { 0.0 } else { self.wins as f64 / n },
            total_pnl_sol: self.pnl_sol_sum,
            mean_pnl_pct,
            median_pnl_pct,
            p90_pnl_pct,
            best_pnl_pct,
            worst_pnl_pct,
            profit_factor,
            expectancy_sol: if self.fired == 0 { 0.0 } else { self.pnl_sol_sum / n },
            avg_holding_secs,
            median_holding_secs,
            exit_take_profit: self.exit_counts[0],
            exit_stop_loss: self.exit_counts[1],
            exit_trailing: self.exit_counts[2],
            exit_stall: self.exit_counts[3],
            exit_time: self.exit_counts[4],
            exit_liquidity: self.exit_counts[5],
            exit_cohort: self.exit_counts[6],
            exit_open: self.exit_counts[7],
        }
    }
}

/// One ranked param-pair row: the combo's aggregated outcome across all tokens.
#[derive(Clone, Debug)]
pub struct ComboMetrics {
    pub combo_id: u32,
    pub n_fired: u64,
    pub n_open: u64,
    pub n_closed: u64,
    pub win_rate: f64,
    pub total_pnl_sol: f64,
    pub mean_pnl_pct: f64,
    pub median_pnl_pct: f64,
    pub p90_pnl_pct: f64,
    pub best_pnl_pct: f64,
    pub worst_pnl_pct: f64,
    /// gross wins ÷ gross losses; `None` = no losing trades (infinite).
    pub profit_factor: Option<f64>,
    pub expectancy_sol: f64,
    pub avg_holding_secs: f64,
    pub median_holding_secs: f64,
    pub exit_take_profit: u32,
    pub exit_stop_loss: u32,
    pub exit_trailing: u32,
    pub exit_stall: u32,
    pub exit_time: u32,
    pub exit_liquidity: u32,
    pub exit_cohort: u32,
    pub exit_open: u32,
}

fn exit_index(e: ExitCode) -> usize {
    match e {
        ExitCode::TakeProfit => 0,
        ExitCode::StopLoss => 1,
        ExitCode::TrailingStop => 2,
        ExitCode::Stall => 3,
        ExitCode::TimeStop => 4,
        ExitCode::LiquidityExit => 5,
        ExitCode::CohortExit => 6,
        // Open + the two non-fired terminals (never recorded) bucket here.
        ExitCode::Open | ExitCode::NoEntry => 7,
    }
}

/// (median, p90, best=max, worst=min) of a pnl% sample. Sorts in place.
fn pct_stats(xs: &mut [f32]) -> (f64, f64, f64, f64) {
    if xs.is_empty() {
        return (0.0, 0.0, 0.0, 0.0);
    }
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = xs[xs.len() / 2] as f64;
    let p90 = xs[((xs.len() as f64 * 0.9) as usize).min(xs.len() - 1)] as f64;
    let worst = xs[0] as f64;
    let best = xs[xs.len() - 1] as f64;
    (median, p90, best, worst)
}

/// (mean, median) holding seconds. Sorts in place.
fn holding_stats(xs: &mut [i32]) -> (f64, f64) {
    if xs.is_empty() {
        return (0.0, 0.0);
    }
    let mean = xs.iter().map(|v| *v as f64).sum::<f64>() / xs.len() as f64;
    xs.sort_unstable();
    let median = xs[xs.len() / 2] as f64;
    (mean, median)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(pnl_sol: f32, pnl_pct: f32, exit: ExitCode, holding: i64) -> TokenOutcome {
        TokenOutcome {
            fired: true,
            holding_secs: holding,
            pnl_percent: pnl_pct,
            pnl_sol,
            exit,
        }
    }

    #[test]
    fn aggregates_wins_losses_and_factor() {
        let mut a = ComboAgg::default();
        a.record(&outcome(2.0, 100.0, ExitCode::TakeProfit, 10));
        a.record(&outcome(-1.0, -50.0, ExitCode::StopLoss, 20));
        a.record(&TokenOutcome::no_entry()); // ignored
        let m = a.finalize(7);
        assert_eq!(m.combo_id, 7);
        assert_eq!(m.n_fired, 2);
        assert_eq!(m.n_closed, 2);
        assert!((m.win_rate - 0.5).abs() < 1e-9);
        assert!((m.total_pnl_sol - 1.0).abs() < 1e-9);
        assert_eq!(m.profit_factor, Some(2.0)); // 2.0 win / 1.0 loss
        assert_eq!(m.exit_take_profit, 1);
        assert_eq!(m.exit_stop_loss, 1);
    }

    #[test]
    fn no_losses_gives_infinite_factor() {
        let mut a = ComboAgg::default();
        a.record(&outcome(1.0, 10.0, ExitCode::TakeProfit, 5));
        assert_eq!(a.finalize(0).profit_factor, None);
    }

    #[test]
    fn open_position_excluded_from_holding() {
        let mut a = ComboAgg::default();
        a.record(&outcome(0.5, 5.0, ExitCode::Open, 0));
        let m = a.finalize(0);
        assert_eq!(m.n_open, 1);
        assert_eq!(m.n_closed, 0);
        assert_eq!(m.avg_holding_secs, 0.0);
        assert_eq!(m.exit_open, 1);
    }
}
