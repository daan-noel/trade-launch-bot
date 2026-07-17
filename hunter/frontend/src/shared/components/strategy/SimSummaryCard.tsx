import { useMemo } from 'react';
import type { PositionsSummary } from 'types';
import { formatAge } from 'utils/format';
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

export function SimSummaryCard({
  ruleName,
  price,
  onClose,
  title = 'Simulation Results',
  summary,
}: SimSummaryCardProps) {
  // The server `summary` is the source of truth — correct over the filtered cohort
  // (not just the current page) and already in human SOL. Derive the display
  // aggregates once and memoize so these don't recompute on price-unit ticks.
  const {
    tokensMatched,
    openCount,
    winCount,
    lossCount,
    winRate,
    totalEntry,
    totalHolding,
    totalGains,
    totalLosses,
    totalPnl,
    avgPnl,
    avgEntry,
    avgHold,
    best,
    worst,
  } = useMemo(
    () => ({
      tokensMatched: summary.tokens,
      openCount: summary.open,
      winCount: summary.win,
      lossCount: summary.loss,
      winRate: summary.win_rate,
      totalEntry: summary.total_entry_sol,
      totalHolding: summary.total_holding_sol,
      totalGains: summary.total_gains_sol,
      totalLosses: summary.total_losses_sol,
      totalPnl: summary.total_pnl_sol,
      avgPnl: summary.closed > 0 ? summary.avg_pnl_pct : null,
      avgEntry: summary.tokens > 0 ? summary.total_entry_sol / summary.tokens : null,
      avgHold: summary.closed > 0 ? summary.avg_hold_secs : null,
      best: summary.best_pct,
      worst: summary.worst_pct,
    }),
    [summary],
  );

  // Headline KPIs, shown large; the rest read as a lighter secondary strip.
  const heroStats: SummaryStat[] = [
    {
      label: `Total PnL (${price.unitLabel})`,
      value: price.displayAmount(totalPnl),
      cls: totalPnl >= 0 ? 'text-primary' : 'text-red',
    },
    {
      label: 'Win Rate',
      value: winRate != null && Number.isFinite(winRate) ? `${winRate.toFixed(1)}%` : '—',
      cls:
        winRate != null && Number.isFinite(winRate)
          ? winRate >= 50
            ? 'text-primary'
            : 'text-red'
          : undefined,
    },
    {
      label: 'Return %',
      value: avgPnl != null ? `${avgPnl >= 0 ? '+' : ''}${avgPnl.toFixed(1)}%` : '—',
      cls: avgPnl != null ? (avgPnl >= 0 ? 'text-primary' : 'text-red') : undefined,
    },
    { label: 'Tokens', value: String(tokensMatched) },
  ];

  const detailStats: SummaryStat[] = [
    {
      label: 'W / L / Open',
      node: (
        <>
          <span className="text-green">{winCount}</span>
          <span className="text-text-dim"> / </span>
          <span className="text-red">{lossCount}</span>
          <span className="text-text-dim"> / </span>
          <span className="text-text-mid">{openCount}</span>
        </>
      ),
    },
    { label: `Total Entry (${price.unitLabel})`, value: price.displayAmount(totalEntry) },
    { label: `Total Holding (${price.unitLabel})`, value: price.displayAmount(totalHolding) },
    { label: 'Avg Entry', value: avgEntry != null ? price.displayAmount(avgEntry) : '—' },
    {
      label: `Total Gains (${price.unitLabel})`,
      value: price.displayAmount(totalGains),
      cls: 'text-green',
    },
    {
      label: `Total Losses (${price.unitLabel})`,
      value: price.displayAmount(totalLosses),
      cls: 'text-red',
    },
    { label: 'Avg Hold', value: avgHold != null ? formatAge(Math.round(avgHold)) : '—' },
    {
      label: 'Best',
      value: best != null ? `${best >= 0 ? '+' : ''}${best.toFixed(1)}%` : '—',
      cls: 'text-green',
    },
    {
      label: 'Worst',
      value: worst != null ? `${worst >= 0 ? '+' : ''}${worst.toFixed(1)}%` : '—',
      cls: 'text-red',
    },
  ];

  return (
    <SummaryStatsPanel
      title={title}
      subtitle={ruleName}
      onClose={onClose}
      heroStats={heroStats}
      detailStats={detailStats}
    />
  );
}
