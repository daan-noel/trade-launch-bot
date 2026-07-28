import { memo, useMemo } from 'react';
import { cn } from 'lib/cn';
import { AmountCell } from 'components/tokens/priceCells';
import { formatDecimalTrim } from 'utils/format';
import { rankByTotalPnl } from './walletPnlStats';
import type { TraderTokenRow } from 'types';

interface WalletRankedPnlBarsProps {
  rows: readonly TraderTokenRow[];
  /** Show the best/worst `maxEachSide` rows on either end (default 15) rather
   *  than every row — a 300-token wallet would otherwise render an unreadable
   *  wall of bars. */
  maxEachSide?: number;
}

/**
 * Per-token PnL ranked best → worst, as horizontal CSS bars (no charting dep,
 * same "hand-roll it" convention as the day×hour heatmap — a bar-per-category
 * chart doesn't fit `lightweight-charts`' time-indexed series model). Ranked on
 * `wallet_total_pnl_sol` (mark-to-market), not win rate — see the wallet-
 * analysis doc's own finding that ranking by hit rate alone can surface the
 * worst-expectancy cohort.
 */
export const WalletRankedPnlBars = memo(function WalletRankedPnlBars({
  rows,
  maxEachSide = 15,
}: WalletRankedPnlBarsProps) {
  const { shown, maxAbs, hiddenCount } = useMemo(() => {
    const ranked = rankByTotalPnl(rows);
    const max = ranked.reduce((m, r) => Math.max(m, Math.abs(r.wallet_total_pnl_sol)), 0);
    if (ranked.length <= maxEachSide * 2) {
      return { shown: ranked, maxAbs: max, hiddenCount: 0 };
    }
    const top = ranked.slice(0, maxEachSide);
    const bottom = ranked.slice(-maxEachSide);
    return { shown: [...top, ...bottom], maxAbs: max, hiddenCount: ranked.length - top.length - bottom.length };
  }, [rows, maxEachSide]);

  if (shown.length === 0) {
    return <p className="text-xs text-text-dim">No tokens to rank.</p>;
  }

  return (
    <div className="flex flex-col gap-1">
      {shown.map((r, i) => {
        const pnl = r.wallet_total_pnl_sol;
        const isLoss = pnl < 0;
        const widthPct = maxAbs > 0 ? Math.max(2, (Math.abs(pnl) / maxAbs) * 100) : 0;
        // A divider where the truncated middle would have been (once, right
        // after the last winner shown).
        const showGapDivider = hiddenCount > 0 && i === Math.min(maxEachSide, shown.length) - 1;
        return (
          <div key={r.mint_address}>
            <div className="flex items-center gap-2 text-[11px]">
              <span className="w-28 shrink-0 truncate font-mono text-text-dim" title={r.mint_address}>
                {r.symbol || r.name || r.mint_address.slice(0, 8)}
              </span>
              <div className="relative h-4 flex-1 overflow-hidden rounded bg-white/4">
                <div
                  className={cn('h-full rounded', isLoss ? 'bg-red/60' : 'bg-green/60')}
                  style={{ width: `${widthPct}%`, marginLeft: isLoss ? 'auto' : undefined }}
                />
              </div>
              <span
                className={cn('w-20 shrink-0 text-right font-mono font-semibold', isLoss ? 'text-red' : 'text-green')}
              >
                <AmountCell sol={pnl} />
              </span>
              {r.wallet_is_open && (
                <span className="shrink-0 text-[9px] font-bold uppercase text-info" title="Still holding a bag">
                  open
                </span>
              )}
            </div>
            {showGapDivider && (
              <div className="my-1 border-t border-dashed border-white/10 py-0.5 text-center text-[10px] text-text-dim/60">
                {formatDecimalTrim(hiddenCount, 0)} token{hiddenCount === 1 ? '' : 's'} hidden — ranked between these two
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
});
