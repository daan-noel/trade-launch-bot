import { memo, useMemo } from 'react';
import { cn } from 'lib/cn';
import { pctGradeBarClass, pctGradeClass } from 'lib/signedTone';
import { ToggleGroup } from 'components/ui/ToggleGroup';
import type { PnlBucket, PnlDistDensity } from './pnlSeries';

interface PnlDistributionProps {
  buckets: PnlBucket[];
  height?: number;
  emptyMessage?: string;
  /** Selected bucket edges, if any. */
  selected?: { lo: number; hi: number } | null;
  onSelectBucket?: (bucket: { lo: number; hi: number }) => void;
  /** When set with `onDensityChange`, renders Sparse / Default / Dense. */
  density?: PnlDistDensity;
  onDensityChange?: (next: PnlDistDensity) => void;
}

const DENSITY_OPTIONS: { value: PnlDistDensity; label: string; title: string }[] = [
  { value: 'sparse', label: 'Sparse', title: 'Grade-aligned buckets (few bars)' },
  { value: 'default', label: 'Default', title: 'Near-zero detail + open win tail' },
  { value: 'dense', label: 'Dense', title: '10pp steps through ±50%' },
];

/**
 * Win/loss size distribution — a count histogram over PnL% buckets, colored
 * by the shared magnitude grades (`pctGradeClass` / `pctGradeBarClass`).
 * Hand-rolled CSS bars: the x-axis is categorical, which doesn't fit
 * `lightweight-charts`' time-indexed series model, and the chart is static
 * per cohort so there is nothing to pan.
 *
 * Takes pre-computed buckets (see `pnlDistributionBuckets`) rather than rows, so
 * one renderer serves every caller's row type.
 */
export const PnlDistribution = memo(function PnlDistribution({
  buckets,
  height = 160,
  emptyMessage = 'No closed round trips in this window.',
  selected = null,
  onSelectBucket,
  density,
  onDensityChange,
}: PnlDistributionProps) {
  const maxCount = useMemo(() => buckets.reduce((m, b) => Math.max(m, b.count), 0), [buckets]);
  const total = buckets.reduce((s, b) => s + b.count, 0);
  // Dense (~15 bars) crowds under-bar labels — keep open ends + zero + occupied.
  const thinLabels = buckets.length > 10;

  const densityToggle =
    density != null && onDensityChange ? (
      <ToggleGroup
        aria-label="PnL distribution density"
        size="sm"
        tone="neutral"
        value={density}
        onChange={onDensityChange}
        options={DENSITY_OPTIONS}
        className="self-end"
      />
    ) : null;

  if (total === 0) {
    return (
      <div className="flex flex-col gap-2">
        {densityToggle}
        <p className="text-xs text-text-dim">{emptyMessage}</p>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-2">
      {densityToggle}
      <div className="flex items-end gap-1.5" style={{ height }}>
        {buckets.map((b, i) => {
          const barHeight =
            maxCount > 0 ? Math.max(b.count > 0 ? 4 : 0, (b.count / maxCount) * (height - 28)) : 0;
          const gradeText = pctGradeClass(b.repPct);
          const isSelected = selected != null && selected.lo === b.lo && selected.hi === b.hi;
          const clickable = b.count > 0 && !!onSelectBucket;
          const showLabel =
            !thinLabels ||
            b.count > 0 ||
            b.lo === -Infinity ||
            b.hi === Infinity ||
            b.lo === 0 ||
            b.hi === 0 ||
            i === 0 ||
            i === buckets.length - 1;
          return (
            <button
              key={`${b.lo}:${b.hi}`}
              type="button"
              disabled={!clickable}
              title={
                `${b.label}: ${b.count} trade${b.count === 1 ? '' : 's'}` +
                (clickable ? '\nClick to focus table' : '')
              }
              onClick={() => onSelectBucket?.({ lo: b.lo, hi: b.hi })}
              className={cn(
                'flex flex-1 flex-col items-center justify-end gap-1 rounded-sm border border-transparent bg-transparent p-0',
                clickable && 'cursor-pointer hover:border-white/20',
                !clickable && 'cursor-default',
                isSelected && 'ring-2 ring-primary ring-offset-1 ring-offset-bg-panel',
              )}
            >
              <span className={cn('text-[11px] font-semibold', b.count > 0 ? gradeText : 'text-text-dim')}>
                {b.count > 0 ? b.count : ''}
              </span>
              <div
                className={cn('w-full rounded-t', pctGradeBarClass(b.repPct))}
                style={{ height: barHeight }}
              />
              <span className={cn('whitespace-nowrap text-[10px]', gradeText)}>
                {showLabel ? b.label : '\u00a0'}
              </span>
            </button>
          );
        })}
      </div>
    </div>
  );
});
