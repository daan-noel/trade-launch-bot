import { memo, useMemo } from 'react';
import { cn } from 'lib/cn';
import type { PnlBucket } from './pnlSeries';

interface PnlDistributionProps {
  buckets: PnlBucket[];
  height?: number;
  emptyMessage?: string;
}

/**
 * Win/loss size distribution — a count histogram over PnL% buckets, colored
 * red/green by sign (the buy/sell convention). Hand-rolled CSS bars: the x-axis
 * is categorical, which doesn't fit `lightweight-charts`' time-indexed series
 * model, and the chart is static per cohort so there is nothing to pan.
 *
 * Takes pre-computed buckets (see `pnlDistributionBuckets`) rather than rows, so
 * one renderer serves every caller's row type.
 */
export const PnlDistribution = memo(function PnlDistribution({
  buckets,
  height = 160,
  emptyMessage = 'No closed round trips in this window.',
}: PnlDistributionProps) {
  const maxCount = useMemo(() => buckets.reduce((m, b) => Math.max(m, b.count), 0), [buckets]);
  const total = buckets.reduce((s, b) => s + b.count, 0);

  if (total === 0) {
    return <p className="text-xs text-text-dim">{emptyMessage}</p>;
  }

  return (
    <div className="flex items-end gap-2" style={{ height }}>
      {buckets.map((b) => {
        const barHeight =
          maxCount > 0 ? Math.max(b.count > 0 ? 4 : 0, (b.count / maxCount) * (height - 28)) : 0;
        return (
          <div key={b.label} className="flex flex-1 flex-col items-center justify-end gap-1">
            <span className="text-[11px] font-semibold text-text-dim">
              {b.count > 0 ? b.count : ''}
            </span>
            <div
              className={cn(
                'w-full rounded-t',
                b.sign < 0 ? 'bg-red/60' : b.sign > 0 ? 'bg-green/60' : 'bg-white/20',
              )}
              style={{ height: barHeight }}
              title={`${b.label}: ${b.count} trade${b.count === 1 ? '' : 's'}`}
            />
            <span className="whitespace-nowrap text-[10px] text-text-dim">{b.label}</span>
          </div>
        );
      })}
    </div>
  );
});
