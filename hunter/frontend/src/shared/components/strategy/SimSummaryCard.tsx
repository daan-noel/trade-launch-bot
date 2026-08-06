import { useMemo } from 'react';
import type { PositionsSummary } from 'types';
import { runSummarySections, zeroExitCounts, type RunSummary } from 'lib/strategy/runSummary';
import type { usePriceDisplay } from 'hooks/usePriceDisplay';
import { SummaryStatsPanel, type SummaryStat } from './SummaryStatsPanel';

interface SimSummaryCardProps {
  ruleName: string;
  price: ReturnType<typeof usePriceDisplay>;
  /** Dismiss handler. Omit to render the card without a ✕ (e.g. when it's an
   *  intrinsic part of a section, like the live Positions summary). */
  onClose?: () => void;
  /** Card heading; defaults to "Simulation Results". */
  title?: string;
  /** Server-computed aggregates over the table's filtered cohort — correct over the
   *  whole matching population (not just the current page). All SOL fields are
   *  already human SOL. */
  summary: PositionsSummary;
}

/**
 * Map the live/paper SQL aggregate onto the shared two-band run summary, so a
 * running rule reports the same metrics under the same labels as its simulate and
 * its sweep (parity plan B11/F3).
 *
 * `median`/`p90`/`std`/`score` stay `null`: the live summary is a Postgres
 * aggregate with no per-position distribution to draw a quantile from, and
 * emitting `0` would assert a measurement that was never taken. Everything else
 * is derivable — `win_rate` is a 0-100 percent here and 0..1 in the shared shape.
 */
function toRunSummary(s: PositionsSummary): RunSummary {
  const closed = s.closed;
  // Older live binaries may omit `exits`; treat as zeros so the card still
  // renders instead of blanking the Analyze panel on a TypeError.
  const exits = s.exits ?? {
    take_profit: 0,
    stop_loss: 0,
    metrics: 0,
    metrics_win: 0,
    metrics_loss: 0,
    dead: 0,
    manual: 0,
    trailing: 0,
    stall: 0,
    time: 0,
    liquidity: 0,
  };
  const realized = {
    n_fired: s.tokens,
    n_open: s.open,
    n_closed: closed,
    win_rate: closed > 0 ? s.win_rate / 100 : 0,
    total_pnl_sol: s.total_pnl_sol,
    open_pnl_sol: s.open_pnl_sol,
    expectancy_sol: closed > 0 ? s.total_pnl_sol / closed : 0,
    mean_pnl_pct: s.avg_pnl_pct,
    median_pnl_pct: null,
    p90_pnl_pct: null,
    best_pnl_pct: s.best_pct,
    worst_pnl_pct: s.worst_pct,
    std_pnl_pct: null,
    profit_factor: s.total_losses_sol > 0 ? s.total_gains_sol / s.total_losses_sol : null,
    score: null,
    avg_holding_secs: s.avg_hold_secs,
    median_holding_secs: 0,
    // Exit reasons now come off the same SQL aggregate (counted per reason in
    // the one pass), so a live rule's breakdown is real rather than the zeros
    // this used to emit. Anything the backend doesn't model — an `ExitFailed`
    // position has no reason at all — is reconciled into `Other` by
    // `exitBreakdown` against `n_closed`, never silently dropped.
    n_exit_take_profit: exits.take_profit,
    n_exit_stop_loss: exits.stop_loss,
    n_exit_trailing: exits.trailing,
    n_exit_stall: exits.stall,
    n_exit_time: exits.time,
    n_exit_liquidity: exits.liquidity,
    n_exit_dead: exits.dead,
    n_exit_metrics: exits.metrics,
    n_exit_metrics_win: exits.metrics_win ?? 0,
    n_exit_metrics_loss: exits.metrics_loss ?? 0,
    n_exit_manual: exits.manual,
    n_exit_open: s.open,
  };
  // MTM differs from realized only by folding the open mark into the total — the
  // per-trade distribution of an unsettled position isn't known here. Exit
  // counters are zeroed on this band to match the Rust `kernel::run_summary`
  // (see its `mtm_band_reports_no_exit_reasons` test): the band reclassifies the
  // open positions as settled, and they have no exit reason to attribute.
  const mtm = {
    ...realized,
    ...zeroExitCounts(),
    total_pnl_sol: s.total_pnl_sol + s.open_pnl_sol,
  };
  return { realized, mtm };
}

export function SimSummaryCard({
  ruleName,
  price,
  onClose,
  title = 'Simulation Results',
  summary,
}: SimSummaryCardProps) {
  // The server `summary` is the source of truth — correct over the filtered cohort
  // (not just the current page) and already in human SOL. Derive once and memoize
  // so these don't recompute on price-unit ticks.
  const { hero, sections } = useMemo(
    () => runSummarySections(toRunSummary(summary), { migrated: summary.migrated }),
    [summary],
  );

  // Capital deployment is live-only — a backtest has no treasury — so it stays a
  // separate strip rather than being forced into the shared bands. These are the
  // figures the price-unit toggle applies to.
  const capitalStats: SummaryStat[] = useMemo(() => {
    const avgEntry = summary.tokens > 0 ? summary.total_entry_sol / summary.tokens : null;
    return [
      { label: `Total Entry (${price.unitLabel})`, value: price.displayAmount(summary.total_entry_sol) },
      { label: `Total Holding (${price.unitLabel})`, value: price.displayAmount(summary.total_holding_sol) },
      { label: 'Avg Entry', value: avgEntry != null ? price.displayAmount(avgEntry) : '—' },
      {
        label: `Total Gains (${price.unitLabel})`,
        value: price.displayAmount(summary.total_gains_sol),
        cls: 'text-green',
      },
      {
        label: `Total Losses (${price.unitLabel})`,
        value: price.displayAmount(summary.total_losses_sol),
        cls: 'text-red',
      },
    ];
  }, [summary, price]);

  return (
    <SummaryStatsPanel
      title={title}
      subtitle={ruleName}
      onClose={onClose}
      heroStats={hero}
      sections={[...sections, { title: 'Capital', hint: 'SOL deployed by this rule', stats: capitalStats }]}
    />
  );
}
