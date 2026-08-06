import { memo, useMemo } from 'react';
import { cn } from 'lib/cn';
import { formatDecimalTrim } from 'utils/format';
import { CHART_COLORS } from 'components/token-price-chart/constants';
import { DOW_ROWS, HOURS, type PnlHeatCell } from './pnlSeries';

interface PnlHeatmapProps {
  cells: PnlHeatCell[];
  /** What one cell's `count` counts, for the tooltip ("trade", "token"). */
  unitLabel?: string;
  emptyMessage?: string;
}

/** `#rrggbb` → `rgba(r,g,b,a)`. Keeps the heat wash on the SSOT candle palette
 *  instead of re-typing green/red hexes that then drift from `CHART_COLORS`. */
function withAlpha(hex: string, alpha: number): string {
  const r = parseInt(hex.slice(1, 3), 16);
  const g = parseInt(hex.slice(3, 5), 16);
  const b = parseInt(hex.slice(5, 7), 16);
  return `rgba(${r},${g},${b},${alpha.toFixed(3)})`;
}

/** Diverging green/red wash by PnL sign, opacity by magnitude relative to the
 *  largest |pnl| across all 168 cells (so one blowout hour doesn't flatten
 *  everything else at max intensity — the same contrast-stretch idea
 *  `CreationHeatmap` uses for rate metrics, signed here). */
function heatColor(pnlSol: number, count: number, maxAbs: number): string {
  if (count === 0) return 'rgba(255,255,255,0.02)';
  if (pnlSol === 0) return 'rgba(255,255,255,0.05)';
  const norm = maxAbs > 0 ? Math.min(1, Math.abs(pnlSol) / maxAbs) : 0;
  const alpha = 0.08 + 0.78 * norm;
  return withAlpha(pnlSol > 0 ? CHART_COLORS.up : CHART_COLORS.down, alpha);
}

/**
 * Day-of-week × hour-of-day PnL heatmap — "when do these trades make or lose
 * money". Pure CSS grid (mirrors `CreationHeatmap`), on a signed green/red scale
 * rather than a magnitude-only one. Bucketed in the caller's timezone.
 */
export const PnlHeatmap = memo(function PnlHeatmap({
  cells,
  unitLabel = 'trade',
  emptyMessage = 'No trades in this window to plot.',
}: PnlHeatmapProps) {
  const { lookup, maxAbs, totalCount } = useMemo(() => {
    const map = new Map<number, PnlHeatCell>();
    let max = 0;
    let total = 0;
    for (const c of cells) {
      map.set(c.dow * 24 + c.hour, c);
      if (Math.abs(c.pnl_sol) > max) max = Math.abs(c.pnl_sol);
      total += c.count;
    }
    return { lookup: map, maxAbs: max, totalCount: total };
  }, [cells]);

  if (totalCount === 0) {
    return <p className="text-xs text-text-dim">{emptyMessage}</p>;
  }

  return (
    <div className="overflow-x-auto">
      <div
        className="grid gap-px text-[10px]"
        style={{ gridTemplateColumns: `2.4rem repeat(24, minmax(0.9rem, 1fr))` }}
      >
        <div />
        {HOURS.map((h) => (
          <div key={h} className="pb-1 text-center font-mono text-text-dim" title={`${h}:00`}>
            {h % 3 === 0 ? h : ''}
          </div>
        ))}

        {DOW_ROWS.map((rowDef) => (
          <Row
            key={rowDef.dow}
            label={rowDef.label}
            dow={rowDef.dow}
            lookup={lookup}
            maxAbs={maxAbs}
            unitLabel={unitLabel}
          />
        ))}
      </div>
    </div>
  );
});

function Row({
  label,
  dow,
  lookup,
  maxAbs,
  unitLabel,
}: {
  label: string;
  dow: number;
  lookup: Map<number, PnlHeatCell>;
  maxAbs: number;
  unitLabel: string;
}) {
  return (
    <>
      <div className="flex items-center pr-1.5 font-semibold text-text-dim">{label}</div>
      {HOURS.map((h) => {
        const cell = lookup.get(dow * 24 + h) ?? { dow, hour: h, pnl_sol: 0, count: 0 };
        const cellLabel = cell.count > 0 ? formatDecimalTrim(cell.pnl_sol, 1) : '';
        const title =
          `${label} ${h}:00\n` +
          (cell.count > 0
            ? `PnL: ${cell.pnl_sol >= 0 ? '+' : ''}${formatDecimalTrim(cell.pnl_sol, 3)} SOL over ${cell.count} ${unitLabel}${cell.count === 1 ? '' : 's'}`
            : 'No trades');
        return (
          <div
            key={h}
            title={title}
            className={cn(
              'flex aspect-square items-center justify-center overflow-hidden rounded-[2px] border border-white/5 text-center font-mono leading-none tabular-nums text-white/85',
            )}
            style={{ background: heatColor(cell.pnl_sol, cell.count, maxAbs) }}
          >
            {cellLabel}
          </div>
        );
      })}
    </>
  );
}
