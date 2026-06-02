import type { SimulatedTokenResult } from '../../types';
import { formatAge } from '../../utils/format';
import type { usePriceDisplay } from '../../hooks/usePriceDisplay';
import { cn } from '../../lib/cn';

interface SimSummaryCardProps {
  ruleName: string;
  tokens: SimulatedTokenResult[];
  price: ReturnType<typeof usePriceDisplay>;
  onClose: () => void;
}

export function SimSummaryCard({ ruleName, tokens, price, onClose }: SimSummaryCardProps) {
  const tokensMatched = tokens.length;
  const winCount = tokens.filter((t) => t.exit_reason === 'TakeProfit').length;
  const lossCount = tokens.filter((t) => t.exit_reason === 'StopLoss').length;
  const openCount = tokens.filter((t) => t.exit_reason === 'Open').length;
  const closedCount = tokensMatched - openCount;
  const winRate = closedCount > 0 ? (winCount / closedCount) * 100 : 0;

  const totalEntry = tokens.reduce((s, t) => s + t.entry_amount, 0);
  const totalHolding = tokens
    .filter((t) => t.exit_reason === 'Open')
    .reduce((s, t) => s + t.entry_amount, 0);
  const totalTp = tokens
    .filter((t) => t.exit_reason === 'TakeProfit')
    .reduce((s, t) => s + (t.pnl_sol ?? 0), 0);
  const totalSl = tokens
    .filter((t) => t.exit_reason === 'StopLoss')
    .reduce((s, t) => s + Math.abs(t.pnl_sol ?? 0), 0);
  const totalPnl = totalTp - totalSl;

  const closed = tokens.filter((t) => t.exit_reason !== 'Open');
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

  const stats = [
    { label: 'Tokens', value: String(tokensMatched) },
    {
      label: 'Win Rate',
      value: `${winRate.toFixed(1)}%`,
      cls: winRate >= 50 ? 'text-primary' : 'text-red',
    },
    {
      label: 'W / L / Open',
      value: `${winCount} / ${lossCount} / ${openCount}`,
    },
    {
      label: `Total Entry (${price.unitLabel})`,
      value: price.displayAmount(totalEntry),
    },
    {
      label: `Total Holding (${price.unitLabel})`,
      value: price.displayAmount(totalHolding),
    },
    {
      label: 'Avg Entry',
      value: avgEntry != null ? price.displayAmount(avgEntry) : '—',
    },
    {
      label: `Total PnL (${price.unitLabel})`,
      value: price.displayAmount(totalPnl),
      cls: totalPnl >= 0 ? 'text-primary' : 'text-red',
    },
    {
      label: 'Avg PnL',
      value: avgPnl != null ? `${avgPnl >= 0 ? '+' : ''}${avgPnl.toFixed(1)}%` : '—',
      cls: avgPnl != null ? (avgPnl >= 0 ? 'text-primary' : 'text-red') : undefined,
    },
    {
      label: `Total TP (${price.unitLabel})`,
      value: price.displayAmount(totalTp),
      cls: 'text-green',
    },
    {
      label: `Total SL (${price.unitLabel})`,
      value: price.displayAmount(totalSl),
      cls: 'text-red',
    },
    {
      label: 'Avg Hold',
      value: avgHold != null ? formatAge(Math.round(avgHold)) : '—',
    },
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
    <div className="mb-4 overflow-hidden rounded-[10px] border border-primary/25 bg-white/2">
      <div className="flex items-center justify-between border-b border-primary/18 bg-primary/9 px-4 py-2.5">
        <span className="text-[13px] font-bold text-primary">Simulation — {ruleName}</span>
        <button type="button" onClick={onClose} className="text-text-dim hover:text-text">
          ✕
        </button>
      </div>
      <div className="grid grid-cols-[repeat(auto-fill,minmax(130px,1fr))] gap-px bg-white/4 p-px">
        {stats.map((s) => (
          <div key={s.label} className="flex flex-col gap-0.5 bg-bg-panel px-3.5 py-2.5">
            <span className="text-[9px] font-bold uppercase tracking-wider text-text-dim">
              {s.label}
            </span>
            <span className={cn('font-mono text-lg font-extrabold text-text', s.cls)}>
              {s.value}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}
