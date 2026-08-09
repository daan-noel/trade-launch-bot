//! Run rollup — the **pure** half of finalizing a live/paper run: turn the run's
//! persisted [`StrategyPosition`] rows into the one [`StrategyRunMetrics`] row
//! `strategy_run_metrics` holds.
//!
//! The arithmetic is not re-derived here. Every figure comes out of the shared
//! [`kernel`](crate::strategies::kernel) aggregator via [`exact_run_metrics`], the
//! same code the grouped sweep and single-rule simulate fold through — so a
//! finished run's PnL / win-rate / holding stats are directly comparable to a
//! backtest of the same rule instead of being a second, drifting copy. This module
//! only maps a PG row onto the kernel's [`TokenOutcome`] vocabulary.
//!
//! Rollup is bounded (one run's positions) and always runs off the decision loop —
//! see `live`'s engine sink, which finalizes a run when its rule stops being
//! active. The DB side lives on
//! [`StrategyRepo`](crate::storage::repositories::StrategyRepo).

use chrono::Utc;
use uuid::Uuid;

use crate::models::{StrategyPosition, StrategyRunMetrics};
use crate::strategies::kernel::{exact_run_metrics, ExitCode, RunMetrics, TokenOutcome};

/// A run's rolled-up metrics plus how many of its positions are still unsettled.
#[derive(Debug, Clone)]
pub struct RunRollup {
    /// The `strategy_run_metrics` row to persist.
    pub row: StrategyRunMetrics,
    /// The full kernel shape (superset of the DB row — carries `open_pnl_sol`,
    /// `score`, and the metric-exit win/loss split the table has no columns for).
    pub metrics: RunMetrics,
    /// Positions that have not reached a terminal state yet. Non-zero means this
    /// rollup is provisional: the run was closed while positions were still
    /// draining, so it must be re-rolled as they settle.
    pub unsettled: u64,
}

/// One position → the kernel's outcome vocabulary.
///
/// * **Not entered** (no entry fill ever landed — `BuySubmitted` that failed, an
///   `EntryFailed` row) ⇒ `no_entry`: the accumulator ignores it entirely, so a
///   rule that armed 500 times and filled 3 is not scored on 500 trades.
/// * **Entered but not terminally closed** (`Holding`, `ExitPending`, and both
///   no-fill exits — a stuck/unconfirmed exit may still hold the bag) ⇒ `Open`,
///   carrying no PnL: a mark-to-market value would need the live token price,
///   which is not in the position row. It counts toward `n_fired`/`n_open` only.
/// * **Closed** (`End`) ⇒ bucketed by [`ExitCode::from_closed_reason`], with
///   realized SOL / % from the position's own [`StrategyPosition::realized_pnl_sol`]
///   / [`StrategyPosition::pnl_pct`] — the same accessors the Console and the
///   positions summary report, so a run's total can't disagree with its rows.
///
/// Since 0006 `pnl_pct` is a **capital** return (`pnl_sol / entry_sol`), not a
/// price ratio, so each outcome's `pnl_percent` and `pnl_sol` now agree in sign —
/// the same basis the sweep and simulate already fed the kernel (their percent
/// comes out of `round_trip_with_costs`). One residual asymmetry remains and is
/// deliberate: [`RunMetrics::mean_pnl_pct`](crate::strategies::kernel::RunMetrics)
/// is the equal-weighted mean of those per-trade percents. Under the fixed
/// notional a backtest uses that IS the capital-weighted return; a live rule
/// whose `buy_amount_lamports` changed mid-run has varying notionals, so its
/// stored mean can differ from `PositionsSummary::return_pct`, which is the
/// capital-weighted figure every live surface shows. Closing that gap needs a
/// per-outcome notional on [`TokenOutcome`] (and a `strategy_run_metrics`
/// column), which is why the run-metrics row is not the headline anywhere.
pub fn position_outcome(p: &StrategyPosition) -> TokenOutcome {
    if !p.is_entered() {
        return TokenOutcome::no_entry();
    }
    if p.status != "End" {
        return TokenOutcome {
            fired: true,
            holding_secs: 0,
            pnl_percent: 0.0,
            pnl_sol: 0.0,
            exit: ExitCode::Open,
        };
    }
    let holding_secs = match (p.entry_time, p.exit_time) {
        (Some(entry), Some(exit)) => (exit - entry).num_seconds().max(0),
        _ => 0,
    };
    TokenOutcome {
        fired: true,
        holding_secs,
        pnl_percent: p.pnl_pct().unwrap_or(0.0) as f32,
        pnl_sol: p.realized_pnl_sol().unwrap_or(0.0) as f32,
        exit: ExitCode::from_closed_reason(p.exit_reason.as_deref()),
    }
}

/// Fold a run's positions into its persistable metrics row. Pure — no clock
/// dependency beyond the `rolled_up_at` stamp, no DB.
///
/// `rolled_up_at` is stamped from the caller's read of the positions, and the
/// upsert only advances a row whose stamp is older — so two concurrent re-rolls
/// (two positions of a draining run settling at once) can't let the staler read
/// overwrite the fresher one.
pub fn roll_up(run_id: Uuid, positions: &[StrategyPosition]) -> RunRollup {
    let outcomes: Vec<TokenOutcome> = positions.iter().map(position_outcome).collect();
    let metrics = exact_run_metrics(outcomes.iter());
    let unsettled = positions.iter().filter(|p| !p.is_closed()).count() as u64;
    RunRollup {
        row: run_metrics_row(run_id, &metrics),
        metrics,
        unsettled,
    }
}

/// Narrow the kernel's [`RunMetrics`] onto the `strategy_run_metrics` columns.
/// The table has no home for `open_pnl_sol` / `mtm_pnl_pct` / `score` / the
/// metric win-loss split — those stay on the kernel shape (sweep + simulate
/// report them) rather than being duplicated into a live-run column that only
/// ever holds a partial answer.
pub fn run_metrics_row(run_id: Uuid, m: &RunMetrics) -> StrategyRunMetrics {
    StrategyRunMetrics {
        run_id,
        rolled_up_at: Utc::now(),
        n_fired: m.n_fired as i32,
        n_open: m.n_open as i32,
        n_closed: m.n_closed as i32,
        win_rate: m.win_rate as f32,
        total_pnl_sol: m.total_pnl_sol as f32,
        expectancy_sol: m.expectancy_sol as f32,
        mean_pnl_pct: m.mean_pnl_pct as f32,
        median_pnl_pct: m.median_pnl_pct as f32,
        p90_pnl_pct: m.p90_pnl_pct as f32,
        best_pnl_pct: m.best_pnl_pct as f32,
        worst_pnl_pct: m.worst_pnl_pct as f32,
        std_pnl_pct: m.std_pnl_pct as f32,
        profit_factor: m.profit_factor.map(|v| v as f32),
        avg_holding_secs: m.avg_holding_secs as f32,
        median_holding_secs: m.median_holding_secs as f32,
        n_exit_take_profit: m.n_exit_take_profit as i32,
        n_exit_stop_loss: m.n_exit_stop_loss as i32,
        n_exit_trailing: m.n_exit_trailing as i32,
        n_exit_stall: m.n_exit_stall as i32,
        n_exit_time: m.n_exit_time as i32,
        n_exit_liquidity: m.n_exit_liquidity as i32,
        n_exit_dead: m.n_exit_dead as i32,
        n_exit_metrics: m.n_exit_metrics as i32,
        n_exit_manual: m.n_exit_manual as i32,
        n_exit_migrated: m.n_exit_migrated as i32,
        n_exit_open: m.n_exit_open as i32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn position(status: &str, reason: Option<&str>, entry: Option<f64>, exit: Option<f64>) -> StrategyPosition {
        let mut p = StrategyPosition::new(
            Uuid::new_v4(),
            "generic".into(),
            Uuid::new_v4(),
            "paper".into(),
            "mint".into(),
            "paper".into(),
        );
        let now = Utc::now();
        p.status = status.to_string();
        p.exit_reason = reason.map(str::to_string);
        p.entry_price = entry.map(|_| 1.0);
        p.entry_sol = entry;
        p.entry_time = entry.map(|_| now - Duration::seconds(30));
        p.exit_price = exit.map(|_| 2.0);
        p.exit_sol = exit;
        p.exit_time = exit.map(|_| now);
        p
    }

    #[test]
    fn a_manual_close_is_realized_not_left_open() {
        // "Manual" has no ladder bucket; before `from_closed_reason` it fell through
        // to `Open`, which parked a settled trade's PnL outside every realized figure.
        let rows = vec![position("End", Some("Manual"), Some(1.0), Some(1.5))];
        let r = roll_up(Uuid::new_v4(), &rows);
        assert_eq!(r.metrics.n_closed, 1);
        assert_eq!(r.metrics.n_open, 0);
        assert_eq!(r.metrics.n_exit_manual, 1);
        assert!((r.metrics.total_pnl_sol - 0.5).abs() < 1e-6);
        assert_eq!(r.unsettled, 0);
    }

    #[test]
    fn an_unknown_label_on_a_closed_row_still_counts_as_closed() {
        assert_eq!(ExitCode::from_closed_reason(None), ExitCode::Manual);
        assert_eq!(ExitCode::from_closed_reason(Some("wat")), ExitCode::Manual);
        assert_eq!(ExitCode::from_closed_reason(Some("Open")), ExitCode::Manual);
        assert_eq!(
            ExitCode::from_closed_reason(Some("stall > 3")),
            ExitCode::Metrics
        );
    }

    #[test]
    fn unentered_rows_never_score_and_open_rows_stay_unsettled() {
        let rows = vec![
            position("EntryFailed", None, None, None),
            position("Holding", None, Some(1.0), None),
            position("ExitStuck", None, Some(1.0), None),
            position("End", Some("TakeProfit"), Some(1.0), Some(2.0)),
        ];
        let r = roll_up(Uuid::new_v4(), &rows);
        // EntryFailed never entered ⇒ not fired; the other three did.
        assert_eq!(r.metrics.n_fired, 3);
        assert_eq!(r.metrics.n_open, 2);
        assert_eq!(r.metrics.n_closed, 1);
        assert_eq!(r.metrics.n_exit_take_profit, 1);
        // Three rows are not terminal (EntryFailed is), so the run is still draining.
        assert_eq!(r.unsettled, 2);
        assert_eq!(r.row.n_closed, 1);
    }
}
