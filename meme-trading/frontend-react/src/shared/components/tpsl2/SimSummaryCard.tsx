import { useMemo, type ReactNode } from 'react';
import type { PositionsSummary, SimulatedTokenResult } from 'types';
import { formatAge } from 'utils/format';
import type { usePriceDisplay } from 'hooks/usePriceDisplay';
import { cn } from 'lib/cn';

const EXIT_REASON_META: { key: string; label: string; cls: string }[] = [
  { key: 'TakeProfit', label: 'TP', cls: 'text-green' },
  { key: 'StopLoss', label: 'SL', cls: 'text-red' },
  { key: 'TrailingStop', label: 'Trail', cls: 'text-text-mid' },
  { key: 'Stall', label: 'Stall', cls: 'text-text-mid' },
  { key: 'TimeStop', label: 'Time', cls: 'text-text-mid' },
  { key: 'LiquidityExit', label: 'Liq', cls: 'text-text-mid' },
  { key: 'Dead', label: 'Dead', cls: 'text-red' },
  { key: 'Open', label: 'Open', cls: 'text-text-dim' },
];

interface SimSummaryCardProps {
  ruleName: string;
  tokens: SimulatedTokenResult[];
  price: ReturnType<typeof usePriceDisplay>;
  /** Dismiss handler. Omit to render the card without a ✕ (e.g. when it's an
   *  intrinsic part of a section, like the live Positions summary). */
  onClose?: () => void;
  /** Card heading; defaults to "Simulation Results". */
  title?: string;
  /** Server-computed run/rule-wide aggregates. When provided, the headline +
   *  detail stats render THESE (correct over the whole run, backend win rule)
   *  instead of deriving from `tokens` — which, under server-side pagination, is
   *  only the current page. The per-exit-reason strip stays token-derived (the
   *  server summary doesn't carry exit-reason breakdowns), so it simply doesn't
   *  render when the card is fed a `summary` with an empty `tokens` list. Sim/
   *  backtest callers omit `summary` and keep the full client-side derivation. */
  summary?: PositionsSummary | null;
}

export function SimSummaryCard({
  ruleName,
  tokens,
  price,
  onClose,
  title = 'Simulation Results',
  summary,
}: SimSummaryCardProps) {
  // All aggregates are pure functions of the inputs, so derive them once and
  // memoize. When the server `summary` is supplied it wins for the headline +
  // detail stats — the client-side token derivation is only the current page under
  // pagination. `exitCounts` stays token-derived (server carries no breakdown).
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
    exitCounts,
  } = useMemo(() => {
    const exitCountsAgg = tokens.reduce<Record<string, { total: number; wins: number; losses: number }>>(
      (acc, t) => {
        if (!acc[t.exit_reason]) acc[t.exit_reason] = { total: 0, wins: 0, losses: 0 };
        acc[t.exit_reason].total += 1;
        if (t.exit_reason !== 'Open') {
          if ((t.pnl_sol ?? 0) >= 0) acc[t.exit_reason].wins += 1;
          else acc[t.exit_reason].losses += 1;
        }
        return acc;
      },
      {},
    );
    if (summary) {
      return {
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
        exitCounts: exitCountsAgg,
      };
    }
    const tokensMatched = tokens.length;
    const openCount = tokens.filter((t) => t.exit_reason === 'Open').length;
    const closed = tokens.filter((t) => t.exit_reason !== 'Open');
    const closedCount = closed.length;
    // Win/loss by realized-PnL sign — a TrailingStop (or any future exit reason)
    // can resolve to either, so classify on PnL, not the exit reason.
    const winCount = closed.filter((t) => (t.pnl_sol ?? 0) >= 0).length;
    const lossCount = closedCount - winCount;
    const winRate = closedCount > 0 ? (winCount / closedCount) * 100 : 0;

    // entry_token_amount is a TOKEN count; SOL invested = entry_price × tokens.
    const entrySol = (t: SimulatedTokenResult) => t.entry_price * t.entry_token_amount;
    const totalEntry = tokens.reduce((s, t) => s + entrySol(t), 0);
    const totalHolding = tokens
      .filter((t) => t.exit_reason === 'Open')
      .reduce((s, t) => s + entrySol(t), 0);
    const totalGains = closed
      .filter((t) => (t.pnl_sol ?? 0) >= 0)
      .reduce((s, t) => s + (t.pnl_sol ?? 0), 0);
    const totalLosses = closed
      .filter((t) => (t.pnl_sol ?? 0) < 0)
      .reduce((s, t) => s + Math.abs(t.pnl_sol ?? 0), 0);
    const totalPnl = totalGains - totalLosses;
    // Canonical capital-weighted return: total PnL ÷ SOL deployed on the closed
    // positions (× 100), so its sign always matches `totalPnl` — never the
    // mean-of-per-trade-% that could flip against the SOL total under uneven sizing.
    const closedEntry = closed.reduce((s, t) => s + entrySol(t), 0);
    const avgPnl = closedEntry > 0 ? (totalPnl / closedEntry) * 100 : null;
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
      exitCounts: exitCountsAgg,
    };
  }, [tokens, summary]);

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

      {EXIT_REASON_META.some(({ key }) => (exitCounts[key]?.total ?? 0) > 0) && (
        <div className="mt-4 flex flex-wrap gap-x-6 gap-y-2 border-t border-white/6 pt-4">
          <span className="w-full text-[9px] font-semibold uppercase tracking-wider text-text-dim">
            Exit Reasons
          </span>
          {EXIT_REASON_META.filter(({ key }) => (exitCounts[key]?.total ?? 0) > 0).map(({ key, label, cls }) => {
            const c = exitCounts[key];
            return (
              <div key={key} className="flex flex-col gap-0.5">
                <span className="text-[9px] font-semibold uppercase tracking-wider text-text-dim">
                  {label}
                </span>
                <span className={cn('font-mono text-sm font-bold', cls)}>
                  {c.total}
                </span>
                {key !== 'Open' && (
                  <span className="font-mono text-[10px] font-semibold">
                    <span className="text-green">{c.wins}</span>
                    <span className="text-text-dim">/</span>
                    <span className="text-red">{c.losses}</span>
                  </span>
                )}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
