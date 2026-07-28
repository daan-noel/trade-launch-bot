import { memo, useMemo } from 'react';
import { cn } from 'lib/cn';
import { pnlDistributionBuckets } from './walletPnlStats';
import type { TraderTokenRow } from 'types';

interface WalletPnlDistributionProps {
  rows: readonly TraderTokenRow[];
  height?: number;
}

/**
 * Win/loss size distribution — a count histogram over `wallet_realized_pnl_pct`
 * buckets, colored red/green by sign. Rows with no matched cost basis (pure
 * open bags) are excluded; see `pnlDistributionBuckets`'s doc comment. Hand-
 * rolled CSS bars (categorical x-axis, same reasoning as the ranked-PnL chart).
 */
export const WalletPnlDistribution = memo(function WalletPnlDistribution({
  rows,
  height = 160,
}: WalletPnlDistributionProps) {
  const buckets = useMemo(() => pnlDistributionBuckets(rows), [rows]);
  const maxCount = useMemo(() => buckets.reduce((m, b) => Math.max(m, b.count), 0), [buckets]);
  const total = buckets.reduce((s, b) => s + b.count, 0);

  if (total === 0) {
    return (
      <p className="text-xs text-text-dim">
        No closed round trips in this window (every row is still an open bag).
      </p>
    );
  }

  return (
    <div className="flex items-end gap-2" style={{ height }}>
      {buckets.map((b) => {
        const barHeight = maxCount > 0 ? Math.max(b.count > 0 ? 4 : 0, (b.count / maxCount) * (height - 28)) : 0;
        return (
          <div key={b.label} className="flex flex-1 flex-col items-center justify-end gap-1">
            <span className="text-[11px] font-semibold text-text-dim">{b.count > 0 ? b.count : ''}</span>
            <div
              className={cn(
                'w-full rounded-t',
                b.sign < 0 ? 'bg-red/60' : b.sign > 0 ? 'bg-green/60' : 'bg-white/20',
              )}
              style={{ height: barHeight }}
              title={`${b.label}: ${b.count} token${b.count === 1 ? '' : 's'}`}
            />
            <span className="whitespace-nowrap text-[10px] text-text-dim">{b.label}</span>
          </div>
        );
      })}
    </div>
  );
});
