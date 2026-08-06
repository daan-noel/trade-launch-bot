import { memo, useMemo } from 'react';
import { cn } from 'lib/cn';
import { AmountCell } from 'components/tokens/priceCells';
import { formatDecimalTrim } from 'utils/format';
import { rankByValue, type RankedBarRow } from './pnlSeries';

interface RankedPnlBarsProps {
  rows: readonly RankedBarRow[];
  /**
   * Show the best/worst `maxEachSide` rows on either end rather than every row.
   * `0` (or omitted) shows **every** row — which is what a rule scoreboard
   * wants; a several-hundred-row wallet ranking is what needs the cap.
   */
  maxEachSide?: number;
  emptyMessage?: string;
}

/**
 * Values ranked best → worst as horizontal CSS bars (no charting dep — a
 * bar-per-category chart doesn't fit `lightweight-charts`' time-indexed series
 * model). Ranked on realized money, not win rate: see
 * `docs/plans/strategies/wallet-analysis.md` on why a hit-rate ranking can
 * surface the worst-expectancy cohort at the best hit rate.
 */
export const RankedPnlBars = memo(function RankedPnlBars({
  rows,
  maxEachSide = 0,
  emptyMessage = 'Nothing to rank.',
}: RankedPnlBarsProps) {
  const { shown, maxAbs, hiddenCount, splitAt } = useMemo(() => {
    const ranked = rankByValue(rows);
    const max = ranked.reduce((m, r) => Math.max(m, Math.abs(r.value)), 0);
    if (maxEachSide <= 0 || ranked.length <= maxEachSide * 2) {
      return { shown: ranked, maxAbs: max, hiddenCount: 0, splitAt: -1 };
    }
    const top = ranked.slice(0, maxEachSide);
    const bottom = ranked.slice(-maxEachSide);
    return {
      shown: [...top, ...bottom],
      maxAbs: max,
      hiddenCount: ranked.length - top.length - bottom.length,
      splitAt: top.length - 1,
    };
  }, [rows, maxEachSide]);

  if (shown.length === 0) {
    return <p className="text-xs text-text-dim">{emptyMessage}</p>;
  }

  return (
    <div className="flex flex-col gap-1">
      {shown.map((r, i) => {
        const isLoss = r.value < 0;
        const widthPct = maxAbs > 0 ? Math.max(2, (Math.abs(r.value) / maxAbs) * 100) : 0;
        return (
          <div key={r.key}>
            <div className="flex items-center gap-2 text-[11px]">
              <span
                className="w-28 shrink-0 truncate font-mono text-text-dim"
                title={r.title ?? r.label}
              >
                {r.label}
              </span>
              <div className="relative h-4 flex-1 overflow-hidden rounded bg-white/4">
                <div
                  className={cn('h-full rounded', isLoss ? 'bg-red/60' : 'bg-green/60')}
                  style={{ width: `${widthPct}%`, marginLeft: isLoss ? 'auto' : undefined }}
                />
              </div>
              <span
                className={cn(
                  'w-20 shrink-0 text-right font-mono font-semibold',
                  isLoss ? 'text-red' : 'text-green',
                )}
              >
                <AmountCell sol={r.value} />
              </span>
              {r.tag && (
                <span className="shrink-0 text-[9px] font-bold uppercase text-info">{r.tag}</span>
              )}
            </div>
            {/* A divider where the truncated middle would have been. */}
            {hiddenCount > 0 && i === splitAt && (
              <div className="my-1 border-t border-dashed border-white/10 py-0.5 text-center text-[10px] text-text-dim/60">
                {formatDecimalTrim(hiddenCount, 0)} row{hiddenCount === 1 ? '' : 's'} hidden —
                ranked between these two
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
});
