import { memo, useMemo } from 'react';
import {
  DOW_ROWS,
  HOURS,
  formatPct,
  heatColor,
  metricValue,
  type CreationHeatCell,
  type CreationMetric,
} from './creationStats';
import { formatWithCommas } from 'utils/format';

interface CreationHeatmapProps {
  cells: CreationHeatCell[];
  metric: CreationMetric;
  /** Window count total — used to render each cell's share-of-total. */
  total: number;
}

const EMPTY_CELL: CreationHeatCell = {
  dow: 0,
  hour: 0,
  count: 0,
  matured: 0,
  known: 0,
  migrated: 0,
  dead: 0,
};

/**
 * 7×24 (day-of-week × hour-of-day) seasonality heatmap. Pure CSS grid — no
 * charting dep (lightweight-charts can't do heatmaps). Folds all in-window
 * history onto the weekly cycle to expose cyclical creation bias.
 *
 * `count` view colors by share-of-total (so a rising volume trend doesn't drown
 * the cyclical pattern); the outcome views color by the rate itself (0..1). The
 * whole grid is memoized + recomputed only when `cells`/`metric`/`total` change,
 * so it never re-renders on a SOL/USD or live-trade tick.
 */
export const CreationHeatmap = memo(function CreationHeatmap({
  cells,
  metric,
  total,
}: CreationHeatmapProps) {
  const { lookup, maxCount } = useMemo(() => {
    const map = new Map<number, CreationHeatCell>();
    let max = 0;
    for (const c of cells) {
      map.set(c.dow * 24 + c.hour, c);
      if (c.count > max) max = c.count;
    }
    return { lookup: map, maxCount: max };
  }, [cells]);

  return (
    <div className="overflow-x-auto">
      <div
        className="grid gap-px text-[10px]"
        style={{ gridTemplateColumns: `2.4rem repeat(24, minmax(0.9rem, 1fr))` }}
      >
        {/* Header row: corner + hour labels */}
        <div />
        {HOURS.map((h) => (
          <div
            key={h}
            className="pb-1 text-center font-mono text-text-dim"
            title={`${h}:00`}
          >
            {h % 3 === 0 ? h : ''}
          </div>
        ))}

        {DOW_ROWS.map((row) => (
          <Row
            key={row.dow}
            label={row.label}
            dow={row.dow}
            lookup={lookup}
            metric={metric}
            maxCount={maxCount}
            total={total}
          />
        ))}
      </div>
    </div>
  );
});

interface RowProps {
  label: string;
  dow: number;
  lookup: Map<number, CreationHeatCell>;
  metric: CreationMetric;
  maxCount: number;
  total: number;
}

function Row({ label, dow, lookup, metric, maxCount, total }: RowProps) {
  return (
    <>
      <div className="flex items-center pr-1.5 font-semibold text-text-dim">
        {label}
      </div>
      {HOURS.map((h) => {
        const cell = lookup.get(dow * 24 + h) ?? { ...EMPTY_CELL, dow, hour: h };
        const value = metricValue(cell, metric);
        const norm =
          metric === 'count'
            ? maxCount > 0
              ? cell.count / maxCount
              : 0
            : value; // rates already 0..1
        const share = total > 0 ? cell.count / total : 0;
        const title =
          `${label} ${h}:00\n` +
          `Created: ${formatWithCommas(cell.count)} (${(share * 100).toFixed(1)}%)\n` +
          `Migrate: ${formatPct(metricValue(cell, 'migrate_rate'))}  ` +
          `Dead: ${formatPct(metricValue(cell, 'dead_rate'))}\n` +
          `Coverage: ${cell.known}/${cell.matured} matured`;
        return (
          <div
            key={h}
            title={title}
            className="aspect-square rounded-[2px] border border-white/5"
            style={{ background: heatColor(metric, norm) }}
          />
        );
      })}
    </>
  );
}
