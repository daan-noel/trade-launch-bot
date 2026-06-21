import { useMemo, type ReactNode } from 'react';
import type { SimulatedTokenResult } from 'types';
import { formatAge } from 'utils/format';
import type { usePriceDisplay } from 'hooks/usePriceDisplay';
import { cn } from 'lib/cn';

interface SimSummaryCardProps {
  ruleName: string;
  tokens: SimulatedTokenResult[];
  price: ReturnType<typeof usePriceDisplay>;
  /** Dismiss handler. Omit to render the card without a ✕ (e.g. when it's an
   *  intrinsic part of a section, like the live Positions summary). */
  onClose?: () => void;
  /** Card heading; defaults to "Simulation Results". */
  title?: string;
}

export function SimSummaryCard({
  ruleName,
  tokens,
  price,
  onClose,
  title = 'Simulation Results',
}: SimSummaryCardProps) {
  // All aggregates are pure functions of `tokens`, so derive them once and
  // memoize: otherwise these ~10 filter/reduce passes re-run on every render
  // (price-unit ticks, parent re-renders) for a list that hasn't changed.
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
  } = useMemo(() => {
    const tokensMatched = tokens.length;
    const openCount = tokens.filter((t) => t.exit_reason === 'Open').length;
    const closed = tokens.filter((t) => t.exit_reason !== 'Open');
    const closedCount = closed.length;
    // Win/loss by realized-PnL sign — a TrailingStop (or any future exit reason)
    // can resolve to either, so classify on PnL, not the exit reason.
    const winCount = closed.filter((t) => (t.pnl_sol ?? 0) >= 0).length;
    const lossCount = closedCount - winCount;
    const winRate = closedCount > 0 ? (winCount / closedCount) * 100 : 0;

    const totalEntry = tokens.reduce((s, t) => s + t.entry_token_amount, 0);
    const totalHolding = tokens
      .filter((t) => t.exit_reason === 'Open')
      .reduce((s, t) => s + t.entry_token_amount, 0);
    const totalGains = closed
      .filter((t) => (t.pnl_sol ?? 0) >= 0)
      .reduce((s, t) => s + (t.pnl_sol ?? 0), 0);
    const totalLosses = closed
      .filter((t) => (t.pnl_sol ?? 0) < 0)
      .reduce((s, t) => s + Math.abs(t.pnl_sol ?? 0), 0);
    const totalPnl = totalGains - totalLosses;
    const avgPnl =
      closed.length > 0
        ? closed.reduce((s, t) => s + (t.pnl_percent ?? 0), 0) / closed.length
        : null;
    const avgEntry = tokensMatched > 0 ? totalEntry / tokensMatched : null;
    const avgHold =
      closed.length > 0
        ? closed.reduce((s, t) => s + (t.holding_secs ?? 0), 0) / closed.length
        : null;
    const best = closed.reduce<number | null>((m, t) => {
      if (t.pnl_percent == null) return m;
      return m == null ? t.pnl_percent : Math.max(m, t.pnl_percent);
    }, null);
    const worst = closed.reduce<number | null>((m, t) => {
      if (t.pnl_percent == null) return m;
      return m == null ? t.pnl_percent : Math.min(m, t.pnl_percent);
    }, null);

    return {
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
    };
  }, [tokens]);

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
      label: 'Avg PnL',
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
