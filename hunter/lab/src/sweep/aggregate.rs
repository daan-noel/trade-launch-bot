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

use crate::sweep::strategy::{TokenOutcome, N_EXIT_METRIC_SLOTS};
use trading_core::models::grouped_sweep::ComboTokenResult;
use trading_core::strategies::kernel::{
    checklist_score, exact_run_metrics, ExitCode, RunAgg, RunMetrics, TokenOutcome as CoreOutcome,
};

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
/// exact; only the two interior quantiles are approximate. `metrics_by_slot` is
/// the one sweep-only addition over `RunAgg`: a fixed `N_EXIT_METRIC_SLOTS`-size
/// counter array (not one entry per distinct metric label), so it keeps the same
/// O(1)-per-combo guarantee instead of growing with how many conditions a rule
/// authors.
#[derive(Clone, Default)]
pub struct ComboAgg {
    run: RunAgg,
    metrics_by_slot: [u32; N_EXIT_METRIC_SLOTS],
}

impl ComboAgg {
    /// Fold one token's outcome under this combo. The sweep's [`TokenOutcome`]
    /// carries extra drill-in fields (entry/exit time + price) the aggregate never
    /// reads, so the fold narrows it to the core [`CoreOutcome`] the accumulator
    /// consumes — a register-level copy of five scalars, no allocation. No-entry
    /// rows are ignored by the accumulator. `exit_metric_slot` (also `Copy`, also
    /// resolved bind-time) additionally bumps the bounded per-metric breakdown —
    /// still O(1) per token, no allocation.
    pub fn record(&mut self, o: &TokenOutcome) {
        self.run.record(&CoreOutcome {
            fired: o.fired,
            holding_secs: o.holding_secs,
            pnl_percent: o.pnl_percent,
            pnl_sol: o.pnl_sol,
            exit: o.exit,
        });
        if o.exit == ExitCode::Metrics {
            if let Some(slot) = o.exit_metric_slot {
                let idx = (slot as usize).min(N_EXIT_METRIC_SLOTS - 1);
                self.metrics_by_slot[idx] += 1;
            }
        }
    }

    /// Collapse to the final ranked row, stamping the combo's id.
    pub fn finalize(self, combo_id: u32) -> ComboMetrics {
        let mut m = ComboMetrics::from_run(combo_id, self.run.finalize());
        m.n_exit_metrics_by_slot = self.metrics_by_slot;
        m
    }
}

/// One ranked param-pair row: the combo's aggregated outcome across all tokens.
/// Field-for-field the core [`RunMetrics`] columns plus a `combo_id`, so a sweep
/// row and a live/paper run's `strategy_run_metrics` stay directly comparable.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ComboMetrics {
    pub combo_id: u32,
    pub n_fired: u64,
    pub n_open: u64,
    pub n_closed: u64,
    pub win_rate: f64,
    pub total_pnl_sol: f64,
    /// Unrealized mark-to-last-price sum over the combo's still-`Open` positions.
    /// Never folded into `total_pnl_sol` (which stays realized-only) — carried so
    /// the group/combo summary can show the mark-to-market total beside it and a
    /// combo that simply leaves its losers open can't read as profitable.
    pub open_pnl_sol: f64,
    pub mean_pnl_pct: f64,
    pub median_pnl_pct: f64,
    pub p90_pnl_pct: f64,
    pub best_pnl_pct: f64,
    pub worst_pnl_pct: f64,
    /// Stddev of realized (closed) per-trade pnl% — display only.
    pub std_pnl_pct: f64,
    /// gross wins ÷ gross losses; `None` = no losing trades (infinite).
    pub profit_factor: Option<f64>,
    /// Mean pnl% over all fired (still-open marks included).
    pub mtm_pnl_pct: f64,
    /// Checklist rank — the table's default sort. Grouped sweep rewrites with
    /// the group's matched-token count via [`Self::rescore_for_group`].
    /// `None` when nothing fired.
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
    /// Analysis-only death-closes: positions closed at the last meaningful trade
    /// because the token died silent (see `trading_core::strategies::death`).
    pub n_exit_dead: u32,
    /// Generic-engine metric-condition exits (`ExitCode::Metrics`). 0 for the
    /// legacy tpsl/swing sweeps (their ladder uses the granular codes above).
    pub n_exit_metrics: u32,
    /// `n_exit_metrics` broken down by WHICH authored exit condition fired —
    /// slot `i` is this combo's rule's `i`-th authored exit req (0-based,
    /// overflow folded into the last slot). `sum() == n_exit_metrics`. See
    /// [`crate::sweep::strategy::N_EXIT_METRIC_SLOTS`] / `BoundCombo::exit_metric_label`
    /// for how a slot is assigned, and the run/group API response's
    /// `exit_metric_legend` header for what each slot names.
    pub n_exit_metrics_by_slot: [u32; N_EXIT_METRIC_SLOTS],
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
            open_pnl_sol: m.open_pnl_sol,
            mean_pnl_pct: m.mean_pnl_pct,
            median_pnl_pct: m.median_pnl_pct,
            p90_pnl_pct: m.p90_pnl_pct,
            best_pnl_pct: m.best_pnl_pct,
            worst_pnl_pct: m.worst_pnl_pct,
            std_pnl_pct: m.std_pnl_pct,
            profit_factor: m.profit_factor,
            mtm_pnl_pct: m.mtm_pnl_pct,
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
            n_exit_dead: m.n_exit_dead,
            n_exit_metrics: m.n_exit_metrics,
            // Populated by `ComboAgg::finalize` (streaming) or `exact_from_rows`
            // (per-token rows) — never known from the core `RunMetrics` alone.
            n_exit_metrics_by_slot: [0; N_EXIT_METRIC_SLOTS],
            n_exit_open: m.n_exit_open,
        }
    }

    /// Exact-quantile metrics for a single re-simulated combo's per-token rows —
    /// the drill-in view's counterpart to a sweep's (sketch-approximated)
    /// persisted row. `rows` is bounded (one combo × one group's tokens), so
    /// holding every value in memory for an exact percentile is cheap — unlike
    /// the full combos × tokens sweep [`ComboAgg`] streams through
    /// [`RunAgg`](trading_core::strategies::kernel::RunAgg) for. See
    /// [`trading_core::strategies::kernel::exact_run_metrics`] (parity plan D1).
    pub fn exact_from_rows(combo_id: u32, rows: &[ComboTokenResult]) -> Self {
        let outcomes: Vec<CoreOutcome> = rows
            .iter()
            .filter(|r| r.fired)
            .map(|r| CoreOutcome {
                fired: r.fired,
                holding_secs: r.holding_secs,
                pnl_percent: r.pnl_pct,
                pnl_sol: r.pnl_sol,
                exit: ExitCode::from_reason(&r.exit),
            })
            .collect();
        let mut m = Self::from_run(combo_id, exact_run_metrics(outcomes.iter()));
        // `exit_metric_slot` rides on the drill-in row itself (resolved the same
        // bind-time way `ComboAgg::record` reads it), so the exact per-token path
        // gets the identical breakdown without re-parsing `exit`.
        for r in rows.iter().filter(|r| r.fired) {
            if ExitCode::from_reason(&r.exit) == ExitCode::Metrics {
                if let Some(slot) = r.exit_metric_slot {
                    let idx = (slot as usize).min(N_EXIT_METRIC_SLOTS - 1);
                    m.n_exit_metrics_by_slot[idx] += 1;
                }
            }
        }
        m
    }

    /// Rewrite [`Self::score`] with the group's matched-token count so fire-rate
    /// is `n_fired / matched` (not the provisional `= 1` from finalize).
    pub fn rescore_for_group(&mut self, matched: usize) {
        self.score = checklist_score(
            self.n_fired,
            self.n_open,
            matched as u64,
            self.mtm_pnl_pct,
            self.win_rate,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sweep::strategy::ExitCode;

    fn combo_row(pnl_sol: f32, pnl_pct: f32, exit: &str, holding: i64, fired: bool) -> ComboTokenResult {
        ComboTokenResult {
            mint_address: "mint".into(),
            symbol: "SYM".into(),
            fired,
            pnl_sol,
            pnl_pct,
            holding_secs: holding,
            exit: exit.into(),
            exit_metric_slot: None,
            entry_time: None,
            entry_price: None,
            entry_tx: None,
            entry_slot: None,
            exit_time: None,
            exit_price: None,
            exit_tx: None,
            exit_slot: None,
            created_at: None,
            ath_price: None,
            token: Default::default(),
        }
    }

    #[test]
    fn exact_from_rows_matches_exact_run_metrics_and_excludes_unfired() {
        let rows = vec![
            combo_row(2.0, 100.0, "TakeProfit", 10, true),
            combo_row(-1.0, -50.0, "StopLoss", 20, true),
            combo_row(5.0, 999.0, "Open", 0, true),
            combo_row(0.0, 0.0, "NoEntry", 0, false), // must be ignored
        ];
        let m = ComboMetrics::exact_from_rows(3, &rows);
        assert_eq!(m.combo_id, 3);
        assert_eq!(m.n_fired, 3, "the unfired NoEntry row must not count");
        assert_eq!(m.n_open, 1);
        assert_eq!(m.n_closed, 2);
        assert!((m.total_pnl_sol - 1.0).abs() < 1e-9, "open PnL excluded from the total");
        assert!((m.open_pnl_sol - 5.0).abs() < 1e-9, "open mark surfaced separately");
        assert_eq!(m.best_pnl_pct, 100.0);
        assert_eq!(m.worst_pnl_pct, -50.0);
    }

    fn outcome(pnl_sol: f32, pnl_pct: f32, exit: ExitCode, holding: i64) -> TokenOutcome {
        outcome_with_slot(pnl_sol, pnl_pct, exit, holding, None)
    }

    /// Like [`outcome`] but for tests exercising the per-metric slot breakdown.
    fn outcome_with_slot(
        pnl_sol: f32,
        pnl_pct: f32,
        exit: ExitCode,
        holding: i64,
        exit_metric_slot: Option<u8>,
    ) -> TokenOutcome {
        TokenOutcome {
            fired: true,
            holding_secs: holding,
            pnl_percent: pnl_pct,
            pnl_sol,
            exit,
            exit_metric: None,
            exit_operator: None,
            exit_metric_value: None,
            exit_metric_slot,
            entry_time: None,
            entry_price: None,
            entry_slot: None,
            exit_time: None,
            exit_price: None,
            exit_slot: None,
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
    fn metrics_exits_bucket_by_slot_and_sum_to_the_total() {
        let mut a = ComboAgg::default();
        a.record(&outcome_with_slot(0.5, 25.0, ExitCode::Metrics, 5, Some(0)));
        a.record(&outcome_with_slot(0.5, 25.0, ExitCode::Metrics, 5, Some(0)));
        a.record(&outcome_with_slot(-0.2, -10.0, ExitCode::Metrics, 5, Some(1)));
        // A slot past the fixed bound folds into the last one, never dropped.
        a.record(&outcome_with_slot(0.1, 5.0, ExitCode::Metrics, 5, Some(200)));
        // No slot info (legacy path) still counts toward the total, just not a slot.
        a.record(&outcome_with_slot(0.1, 5.0, ExitCode::Metrics, 5, None));
        let m = a.finalize(0);
        assert_eq!(m.n_exit_metrics, 5);
        assert_eq!(m.n_exit_metrics_by_slot[0], 2);
        assert_eq!(m.n_exit_metrics_by_slot[1], 1);
        assert_eq!(m.n_exit_metrics_by_slot[N_EXIT_METRIC_SLOTS - 1], 1);
        assert_eq!(m.n_exit_metrics_by_slot.iter().sum::<u32>(), 4, "the slotless row is excluded from the breakdown, not fabricated into a slot");
    }

    #[test]
    fn no_losses_gives_infinite_factor() {
        let mut a = ComboAgg::default();
        a.record(&outcome(1.0, 10.0, ExitCode::TakeProfit, 5));
        assert_eq!(a.finalize(0).profit_factor, None);
    }

    #[test]
    fn score_none_when_nothing_fired() {
        let m = ComboAgg::default().finalize(0);
        assert_eq!(m.n_fired, 0);
        assert_eq!(m.score, None);
    }

    #[test]
    fn score_is_checklist_product_and_includes_open_in_mtm() {
        let mut a = ComboAgg::default();
        a.record(&outcome(0.1, 10.0, ExitCode::TakeProfit, 5));
        a.record(&outcome(0.1, 10.0, ExitCode::TakeProfit, 5));
        a.record(&outcome(5.0, 90.0, ExitCode::Open, 0));
        let mut m = a.finalize(0);
        assert!((m.mtm_pnl_pct - 110.0 / 3.0).abs() < 1e-6);
        // finalize uses matched=n_fired → fire_rate=1; open_drag=1/3; win_rate=1.
        let expected = m.mtm_pnl_pct * (1.0 - 0.5 / 3.0);
        assert!((m.score.unwrap() - expected).abs() < 1e-6);
        // Group rescore with matched=6 → fire_rate=0.5.
        m.rescore_for_group(6);
        assert!((m.score.unwrap() - expected * 0.5).abs() < 1e-6);
    }

    #[test]
    fn open_positions_excluded_from_headline_pnl_and_win_rate() {
        // Parity plan C2: a still-open mark-to-last-price must not move the
        // headline total_pnl_sol / win_rate / mean / best / worst — those are
        // realized-only. Both the sweep drill-in and the single-rule simulate now
        // *display* an open row's mark-to-last PnL per token, but the aggregate
        // excludes it here. `n_fired` still counts the open position.
        let mut a = ComboAgg::default();
        a.record(&outcome(1.0, 50.0, ExitCode::TakeProfit, 10));
        a.record(&outcome(-1.0, -50.0, ExitCode::StopLoss, 10));
        // A huge unrealized open winner — must not leak into any realized figure.
        a.record(&outcome(1_000.0, 5_000.0, ExitCode::Open, 0));
        let m = a.finalize(0);
        assert_eq!(m.n_fired, 3);
        assert_eq!(m.n_open, 1);
        assert_eq!(m.n_closed, 2);
        assert!((m.total_pnl_sol - 0.0).abs() < 1e-9, "open PnL excluded from total");
        assert!(
            (m.open_pnl_sol - 1_000.0).abs() < 1e-9,
            "the open mark is reported separately, not silently dropped"
        );
        assert!((m.win_rate - 0.5).abs() < 1e-9, "win rate over closed trades only");
        assert!((m.mean_pnl_pct - 0.0).abs() < 1e-9);
        assert_eq!(m.best_pnl_pct, 50.0, "open mark must not become the best");
        assert_eq!(m.worst_pnl_pct, -50.0);
        assert!((m.expectancy_sol - 0.0).abs() < 1e-9);
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
