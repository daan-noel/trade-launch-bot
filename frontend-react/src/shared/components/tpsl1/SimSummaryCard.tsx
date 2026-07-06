import { useMemo, type ReactNode } from 'react';
import type { PositionsSummary } from 'types';
import { formatAge } from 'utils/format';
import type { usePriceDisplay } from 'hooks/usePriceDisplay';
import { cn } from 'lib/cn';

interface SimSummaryCardProps {
  ruleName: string;
  price: ReturnType<typeof usePriceDisplay>;
  /** Dismiss handler. Omit to render the card without a ✕ (e.g. when it's an
   *  intrinsic part of a section, like the live Positions summary). */
  onClose?: () => void;
  /** Card heading; defaults to "Simulation Results". */
  title?: string;
  /** Server-computed run/rule-wide aggregates the card renders — correct over the
   *  whole run (not just the current page) and using the backend win rule. All SOL
   *  fields are already human SOL. */
  summary: PositionsSummary;
}

export function SimSummaryCard({
  ruleName,
  price,
  onClose,
  title = 'Simulation Results',
  summary,
}: SimSummaryCardProps) {
  // The server `summary` is the source of truth — correct over the whole run
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
  const heroStats = [
    {
      label: `Total PnL (${price.unitLabel})`,
      value: price.displayAmount(totalPnl),
      cls: totalPnl >= 0 ? 'text-primary' : 'text-red',
    },
    {
      label: 'Win Rate',
      value: `${winRate.toFixed(1)}%`,
      cls: winRate >= 50 ? 'text-primary' : 'text-red',
    },
    {
      label: 'Return %',
      value: avgPnl != null ? `${avgPnl >= 0 ? '+' : ''}${avgPnl.toFixed(1)}%` : '—',
      cls: avgPnl != null ? (avgPnl >= 0 ? 'text-primary' : 'text-red') : undefined,
    },
    { label: 'Tokens', value: String(tokensMatched) },
  ];

  const detailStats: { label: string; value?: string; node?: ReactNode; cls?: string }[] = [
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
    <div className="mb-5">
      <div className="mb-4 flex items-center gap-2.5">
        <span className="h-4 w-1 rounded-full bg-primary" />
        <h3 className="text-sm font-bold text-text">{title}</h3>
        <span className="truncate font-mono text-[11px] text-text-dim">{ruleName}</span>
        <span className="flex-1" />
        {onClose && (
          <button
            type="button"
            onClick={onClose}
            className="text-text-dim transition hover:text-text"
          >
            ✕
          </button>
        )}
      </div>

      <div className="flex flex-wrap gap-x-10 gap-y-4">
        {heroStats.map((s) => (
          <div key={s.label} className="flex flex-col gap-1">
            <span className="text-[10px] font-semibold uppercase tracking-wider text-text-dim">
              {s.label}
            </span>
            <span
              className={cn(
                'font-mono text-3xl font-extrabold leading-none tracking-tight text-text',
                s.cls,
              )}
            >
              {s.value}
            </span>
          </div>
        ))}
      </div>

      <div className="mt-5 flex flex-wrap gap-x-8 gap-y-3 border-t border-white/6 pt-4">
        {detailStats.map((s) => (
          <div key={s.label} className="flex min-w-[84px] flex-col gap-0.5">
            <span className="text-[9px] font-semibold uppercase tracking-wider text-text-dim">
              {s.label}
            </span>
            <span className={cn('font-mono text-sm font-bold text-text', s.cls)}>
              {s.node ?? s.value}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}
