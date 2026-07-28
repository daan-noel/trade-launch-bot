import { memo, useMemo } from 'react';
import {
  DOW_ROWS,
  HOURS,
  METRIC_KIND,
  formatPct,
  heatColor,
  metricValue,
  type CreationHeatCell,
  type CreationMetric,
} from './creationStats';
import { formatCompact, formatWithCommas } from 'utils/format';
import { cn } from 'lib/cn';

interface CreationHeatmapProps {
  cells: CreationHeatCell[];
  metric: CreationMetric;
  /** Window count total — used to render each cell's share-of-total. */
  total: number;
  /** Opt-in: makes every tile clickable (a drill-down into that recurring
   *  day×hour slot) — the tile gets a pointer cursor + hover ring. Omit for a
   *  read-only heatmap. */
  onCellClick?: (dow: number, hour: number) => void;
  /** Highlights one tile (the currently drilled-into slot). */
  selectedCell?: { dow: number; hour: number } | null;
}

const EMPTY_CELL: CreationHeatCell = {
  dow: 0,
  hour: 0,
  count: 0,
  matured: 0,
  known: 0,
  migrated: 0,
  dead: 0,
  trades: 0,
  trades_avg: null,
  trades_per_day: 0,
};

/**
 * 7×24 (day-of-week × hour-of-day) seasonality heatmap. Pure CSS grid — no
 * charting dep (lightweight-charts can't do heatmaps). Folds all in-window
 * history onto the weekly cycle to expose cyclical creation bias.
 *
 * Normalization/label are driven by the active metric's `MetricKind` (see
 * `creationStats.ts`): `magnitude` (`count`/`trades`/`trades_per_day`) colors
 * by share-of-max (so a rising volume trend doesn't drown the cyclical
 * pattern); `rate` (migrate %/dead %) colors by the rate itself, contrast-
 * stretched across cells; `ratio` (trades/token) does the same but on a
 * log1p scale (outlier-robustness without a server-side median). The whole
 * grid is memoized + recomputed only when `cells`/`metric`/`total` change, so
 * it never re-renders on a SOL/USD or live-trade tick.
 */
export const CreationHeatmap = memo(function CreationHeatmap({
  cells,
  metric,
  total,
  onCellClick,
  selectedCell,
}: CreationHeatmapProps) {
  // `magnitude` metrics normalize against the max value observed for the
  // ACTIVE metric (share-of-max) — not always `count`, so a metric switch
  // re-maxes too. `rate`/`ratio` metrics shade by each cell's position within
  // the observed min→max spread (contrast stretch), not the absolute value.
  // Nearly every token dies, so dead-rate clusters ~0.9–1.0 and migrate-rate
  // ~0.0–0.1; absolute shading would paint the whole grid one flat tone and
  // hide the cyclical pattern. `ratio` (trades_per_token) stretches on a
  // log1p scale so one huge-trade-count token can't wash out the rest.
  const kind = METRIC_KIND[metric];
  const { lookup, maxValue, rateMin, rateSpan } = useMemo(() => {
    const map = new Map<number, CreationHeatCell>();
    let max = 0;
    let rMin = Infinity;
    let rMax = -Infinity;
    for (const c of cells) {
      map.set(c.dow * 24 + c.hour, c);
      if (kind === 'magnitude') {
        const v = metricValue(c, metric) ?? 0;
        if (v > max) max = v;
      } else {
        const raw = metricValue(c, metric);
        if (raw != null) {
          const v = kind === 'ratio' ? Math.log1p(raw) : raw;
          if (v < rMin) rMin = v;
          if (v > rMax) rMax = v;
        }
      }
    }
    return { lookup: map, maxValue: max, rateMin: rMin, rateSpan: rMax > rMin ? rMax - rMin : 0 };
  }, [cells, metric, kind]);

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
            kind={kind}
            maxValue={maxValue}
            rateMin={rateMin}
            rateSpan={rateSpan}
            total={total}
            onCellClick={onCellClick}
            selectedCell={selectedCell}
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
  kind: 'magnitude' | 'rate' | 'ratio';
  maxValue: number;
  rateMin: number;
  rateSpan: number;
  total: number;
  onCellClick?: (dow: number, hour: number) => void;
  selectedCell?: { dow: number; hour: number } | null;
}

function Row({
  label,
  dow,
  lookup,
  metric,
  kind,
  maxValue,
  rateMin,
  rateSpan,
  total,
  onCellClick,
  selectedCell,
}: RowProps) {
  return (
    <>
      <div className="flex items-center pr-1.5 font-semibold text-text-dim">
        {label}
      </div>
      {HOURS.map((h) => {
        const cell = lookup.get(dow * 24 + h) ?? { ...EMPTY_CELL, dow, hour: h };
        const value = metricValue(cell, metric);
        const norm =
          kind === 'magnitude'
            ? maxValue > 0
              ? (value ?? 0) / maxValue
              : 0
            : value == null
              ? null // no matured outcome / no trade coverage → "no data" wash
              : rateSpan > 0
                ? ((kind === 'ratio' ? Math.log1p(value) : value) - rateMin) / rateSpan // contrast-stretch across cells
                : 1; // every cell equal → flat full tone
        const share = total > 0 ? cell.count / total : 0;
        // In-cell readout: compact number for magnitude views, integer percent
        // for rate views, compact number (not a percent) for the ratio view.
        const cellLabel =
          kind === 'magnitude'
            ? (value ?? 0) > 0
              ? formatCompact(value ?? 0, 1)
              : ''
            : value == null
              ? ''
              : kind === 'rate'
                ? `${Math.round(value * 100)}%`
                : formatCompact(value, 1);
        const clickable = onCellClick != null;
        const selected = selectedCell?.dow === dow && selectedCell?.hour === h;
        const title =
          `${label} ${h}:00\n` +
          `Created: ${formatWithCommas(cell.count)} (${(share * 100).toFixed(1)}%)\n` +
          `Migrate: ${formatPct(metricValue(cell, 'migrate_rate'))}  ` +
          `Dead: ${formatPct(metricValue(cell, 'dead_rate'))}\n` +
          `Trades: ${formatWithCommas(cell.trades)} (avg ${cell.trades_avg != null ? cell.trades_avg.toFixed(1) : '—'}/token) · ${cell.trades_per_day.toFixed(1)}/day, lifetime-to-last-sync\n` +
          `Coverage: ${cell.known}/${cell.matured} matured` +
          (clickable ? '\nClick to view tokens' : '');
        return (
          <button
            key={h}
            type="button"
            title={title}
            disabled={!clickable}
            onClick={clickable ? () => onCellClick(dow, h) : undefined}
            className={cn(
              'flex aspect-square items-center justify-center overflow-hidden rounded-[2px] border border-white/5 text-center font-mono leading-none tabular-nums text-white/85',
              clickable && 'cursor-pointer transition hover:ring-1 hover:ring-primary/70',
              selected && 'ring-2 ring-primary',
            )}
            style={{ background: heatColor(metric, norm) }}
          >
            {cellLabel}
          </button>
        );
      })}
    </>
  );
}
