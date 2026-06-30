//! Aggregate per-(combo, token) [`TokenOutcome`]s into one ranked metrics row
//! per combo — the param-pair table the UI shows. The sweep produces combos ×
//! tokens outcomes; this collapses them to `combos` rows (bounded — hundreds to
//! low thousands), small enough to hold in RAM, persist whole, and serve whole.
//!
//! The streaming accumulator, the bounded quantile sketch, and the robust score
//! are **not** defined here — they live once in the core simulation kernel
//! ([`trading_core::strategies::kernel::RunAgg`]) so backtest, live, and paper runs
//! fold into byte-identical metrics. [`ComboAgg`] is a thin per-combo wrapper over
//! that accumulator; [`ComboMetrics`] is the sweep's persisted row shape (the core
//! [`RunMetrics`] columns plus a `combo_id`), built by [`ComboMetrics::from_run`].
//!
//! Metrics cover the criteria a param search ranks on: profitability
//! (`total_pnl_sol`, `expectancy_sol`, `profit_factor`), success (`win_rate`),
//! central return (`mean`/`median_pnl_pct`), tails (`best`/`worst`/`p90`),
//! holding time, and the exit-reason mix.

use crate::sweep::strategy::TokenOutcome;
use trading_core::strategies::kernel::{RunAgg, RunMetrics, TokenOutcome as CoreOutcome};

/// Streaming accumulator for one combo across every token it fired on. A thin
/// wrapper over the core kernel's [`RunAgg`]: it owns no statistics of its own, so
/// the sketch / mean / robust-score math has exactly one implementation shared
/// with live and paper runs. Stays `Default` + `Clone` so the engine can hold one
/// contiguous `vec![ComboAgg; n_combos]` in the fold thread.
///
/// **Bounded memory (O(1) per combo).** Median/p90 come from the [`RunAgg`]'s
/// fixed-size DDSketch (~0.6 KB, ~15% relative error), not a per-token `Vec`, so a
/// combo's footprint never grows with the number of tokens it fires on — the cap
/// that lets high-`max_combos` sweeps run without OOM. Best/worst/mean/total stay
/// exact; only the two interior quantiles are approximate.
#[derive(Clone, Default)]
pub struct ComboAgg(RunAgg);

impl ComboAgg {
    /// Fold one token's outcome under this combo. The sweep's [`TokenOutcome`]
    /// carries extra drill-in fields (entry/exit time + price) the aggregate never
    /// reads, so the fold narrows it to the core [`CoreOutcome`] the accumulator
    /// consumes — a register-level copy of five scalars, no allocation. No-entry
    /// rows are ignored by the accumulator.
    pub fn record(&mut self, o: &TokenOutcome) {
        self.0.record(&CoreOutcome {
            fired: o.fired,
            holding_secs: o.holding_secs,
            pnl_percent: o.pnl_percent,
            pnl_sol: o.pnl_sol,
            exit: o.exit,
        });
    }

    /// Collapse to the final ranked row, stamping the combo's id.
    pub fn finalize(self, combo_id: u32) -> ComboMetrics {
        ComboMetrics::from_run(combo_id, self.0.finalize())
    }
}

/// One ranked param-pair row: the combo's aggregated outcome across all tokens.
/// Field-for-field the core [`RunMetrics`] columns plus a `combo_id`, so a sweep
/// row and a live/paper run's `strategy_run_metrics` stay directly comparable.
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
    /// Stddev of realized (closed) per-trade pnl% — the dispersion/risk term the
    /// `score` penalises. `0` when fewer than 2 closed trades.
    pub std_pnl_pct: f64,
    /// gross wins ÷ gross losses; `None` = no losing trades (infinite).
    pub profit_factor: Option<f64>,
    /// Robust rank: `μ − Z·σ/√n` over closed trades — the table's default sort.
    /// `None` when n_closed < 2 (no evidence of a repeatable edge → sorts last).
    pub score: Option<f64>,
    pub expectancy_sol: f64,
    pub avg_holding_secs: f64,
    pub median_holding_secs: f64,
    /// Per-exit-reason trade counts (how each combo's closed trades terminated),
    /// not param values. `n_` marks them as counts like `n_fired`/`n_closed`.
    pub n_exit_take_profit: u32,
    pub n_exit_stop_loss: u32,
    pub n_exit_trailing: u32,
    pub n_exit_stall: u32,
    pub n_exit_time: u32,
    pub n_exit_liquidity: u32,
    pub n_exit_cohort: u32,
    /// swing1's symmetric next-kill flee count. 0 for tpsl1/tpsl2 (they never
    /// produce `ExitCode::NextKill`).
    pub n_exit_next_kill: u32,
    pub n_exit_open: u32,
}

impl ComboMetrics {
    /// Stamp a combo id onto a finalized core [`RunMetrics`] (the sweep's only
    /// addition over the shared run-metric shape).
    pub fn from_run(combo_id: u32, m: RunMetrics) -> Self {
        Self {
            combo_id,
            n_fired: m.n_fired,
            n_open: m.n_open,
            n_closed: m.n_closed,
            win_rate: m.win_rate,
            total_pnl_sol: m.total_pnl_sol,
            mean_pnl_pct: m.mean_pnl_pct,
            median_pnl_pct: m.median_pnl_pct,
            p90_pnl_pct: m.p90_pnl_pct,
            best_pnl_pct: m.best_pnl_pct,
            worst_pnl_pct: m.worst_pnl_pct,
            std_pnl_pct: m.std_pnl_pct,
            profit_factor: m.profit_factor,
            score: m.score,
            expectancy_sol: m.expectancy_sol,
            avg_holding_secs: m.avg_holding_secs,
            median_holding_secs: m.median_holding_secs,
            n_exit_take_profit: m.n_exit_take_profit,
            n_exit_stop_loss: m.n_exit_stop_loss,
            n_exit_trailing: m.n_exit_trailing,
            n_exit_stall: m.n_exit_stall,
            n_exit_time: m.n_exit_time,
            n_exit_liquidity: m.n_exit_liquidity,
            n_exit_cohort: m.n_exit_cohort,
            n_exit_next_kill: m.n_exit_next_kill,
            n_exit_open: m.n_exit_open,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sweep::strategy::ExitCode;

    fn outcome(pnl_sol: f32, pnl_pct: f32, exit: ExitCode, holding: i64) -> TokenOutcome {
        TokenOutcome {
            fired: true,
            holding_secs: holding,
            pnl_percent: pnl_pct,
            pnl_sol,
            exit,
            entry_time: None,
            entry_price: None,
            exit_time: None,
            exit_price: None,
        }
    }

    // These exercise the `ComboAgg` wrapper end-to-end (sweep `TokenOutcome` →
    // core `RunAgg` fold → `ComboMetrics`). The sketch/score math itself is
    // unit-tested in `trading_core::strategies::kernel`.

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
        assert_eq!(m.n_exit_take_profit, 1);
        assert_eq!(m.n_exit_stop_loss, 1);
    }

    #[test]
    fn no_losses_gives_infinite_factor() {
        let mut a = ComboAgg::default();
        a.record(&outcome(1.0, 10.0, ExitCode::TakeProfit, 5));
        assert_eq!(a.finalize(0).profit_factor, None);
    }

    #[test]
    fn score_none_under_two_closed_trades() {
        // One closed trade: a great return but no evidence it repeats → no score.
        let mut a = ComboAgg::default();
        a.record(&outcome(2.0, 200.0, ExitCode::TakeProfit, 10));
        let m = a.finalize(0);
        assert_eq!(m.n_closed, 1);
        assert_eq!(m.score, None);
        assert_eq!(m.std_pnl_pct, 0.0);
    }

    #[test]
    fn score_penalises_dispersion_below_the_mean() {
        // Same realized mean (0%), but a wide spread → σ>0 → score < mean.
        let mut a = ComboAgg::default();
        a.record(&outcome(1.0, 100.0, ExitCode::TakeProfit, 5));
        a.record(&outcome(-1.0, -100.0, ExitCode::StopLoss, 5));
        let m = a.finalize(0);
        assert!(m.std_pnl_pct > 0.0);
        assert!(m.score.unwrap() < 0.0, "dispersion pulls the lower bound under the mean");
    }

    #[test]
    fn open_positions_excluded_from_score_and_holding() {
        // An open (unrealized) winner must not feed the realized mean/stddev or
        // the holding-time stats.
        let mut a = ComboAgg::default();
        a.record(&outcome(0.1, 10.0, ExitCode::TakeProfit, 5));
        a.record(&outcome(0.1, 10.0, ExitCode::TakeProfit, 5));
        a.record(&outcome(5.0, 999.0, ExitCode::Open, 0)); // ignored by the score
        let m = a.finalize(0);
        assert_eq!(m.n_closed, 2);
        assert_eq!(m.n_open, 1);
        assert!(m.std_pnl_pct.abs() < 1e-9, "two identical closed returns → σ=0");
        assert_eq!(m.score, Some(10.0));
        assert_eq!(m.n_exit_open, 1);
    }

    #[test]
    fn best_worst_and_mean_are_exact() {
        // best/worst (running min/max) and mean (running sum) stay precise even
        // though median/p90 go through the approximate sketch.
        let mut a = ComboAgg::default();
        for v in [10.0, -30.0, 250.0, 5.0, -100.0] {
            a.record(&outcome(0.1, v, ExitCode::TakeProfit, 7));
        }
        let m = a.finalize(0);
        assert_eq!(m.best_pnl_pct, 250.0);
        assert_eq!(m.worst_pnl_pct, -100.0);
        // mean = (10 − 30 + 250 + 5 − 100) / 5 = 27
        assert!((m.mean_pnl_pct - 27.0).abs() < 1e-9);
    }

    #[test]
    fn sketch_median_p90_within_relative_error() {
        // 1..=1000 → exact median 500/501, p90 ~900. The core 64-bucket log sketch
        // must land within its ~15% relative-error band on both.
        let mut a = ComboAgg::default();
        for v in 1..=1000 {
            a.record(&outcome(0.1, v as f32, ExitCode::TakeProfit, v));
        }
        let m = a.finalize(0);
        assert!((m.median_pnl_pct - 500.0).abs() / 500.0 < 0.16, "median {}", m.median_pnl_pct);
        assert!((m.p90_pnl_pct - 900.0).abs() / 900.0 < 0.16, "p90 {}", m.p90_pnl_pct);
        assert!(
            (m.median_holding_secs - 500.0).abs() / 500.0 < 0.16,
            "holding median {}",
            m.median_holding_secs
        );
    }
}
