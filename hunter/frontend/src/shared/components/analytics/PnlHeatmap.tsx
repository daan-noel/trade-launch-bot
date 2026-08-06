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
  selected?: { dow: number; hour: number } | null;
  onSelectCell?: (cell: { dow: number; hour: number }) => void;
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

/** Compact in-cell label — full precision stays on the tooltip. */
function shortPnl(pnl: number): string {
  const abs = Math.abs(pnl);
  if (abs >= 10) return `${pnl > 0 ? '+' : ''}${formatDecimalTrim(pnl, 0)}`;
  if (abs >= 1) return `${pnl > 0 ? '+' : ''}${formatDecimalTrim(pnl, 1)}`;
  return `${pnl > 0 ? '+' : ''}${formatDecimalTrim(pnl, 2)}`;
}

/**
 * Day-of-week × hour-of-day PnL heatmap. Cells stretch across the card
 * (`1fr` columns, min height) so a full-width History row stays clickable
 * and readable instead of a 0.9rem postage stamp.
 */
export const PnlHeatmap = memo(function PnlHeatmap({
  cells,
  unitLabel = 'trade',
  emptyMessage = 'No trades in this window to plot.',
  selected = null,
  onSelectCell,
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
    <div className="w-full">
      <div
        className="grid w-full gap-1 text-[11px]"
        style={{ gridTemplateColumns: `2.75rem repeat(24, minmax(0, 1fr))` }}
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
            selected={selected}
            onSelectCell={onSelectCell}
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
  selected,
  onSelectCell,
}: {
  label: string;
  dow: number;
  lookup: Map<number, PnlHeatCell>;
  maxAbs: number;
  unitLabel: string;
  selected: { dow: number; hour: number } | null;
  onSelectCell?: (cell: { dow: number; hour: number }) => void;
}) {
  return (
    <>
      <div className="flex items-center pr-1.5 text-[11px] font-semibold text-text-dim">{label}</div>
      {HOURS.map((h) => {
        const cell = lookup.get(dow * 24 + h) ?? { dow, hour: h, pnl_sol: 0, count: 0 };
        const cellLabel = cell.count > 0 ? shortPnl(cell.pnl_sol) : '';
        const isSelected = selected?.dow === dow && selected?.hour === h;
        const clickable = cell.count > 0 && !!onSelectCell;
        const title =
          `${label} ${h}:00\n` +
          (cell.count > 0
            ? `PnL: ${cell.pnl_sol >= 0 ? '+' : ''}${formatDecimalTrim(cell.pnl_sol, 3)} SOL over ${cell.count} ${unitLabel}${cell.count === 1 ? '' : 's'}`
            : 'No trades') +
          (clickable ? '\nClick to focus table' : '');
        return (
          <button
            key={h}
            type="button"
            title={title}
            disabled={!clickable}
            onClick={() => onSelectCell?.({ dow, hour: h })}
            className={cn(
              'flex min-h-[2.25rem] items-center justify-center overflow-hidden rounded-sm border border-white/5 px-0.5 text-center font-mono text-[10px] leading-none tabular-nums text-white/90',
              clickable && 'cursor-pointer hover:border-white/40',
              !clickable && 'cursor-default',
              isSelected && 'ring-2 ring-primary ring-offset-1 ring-offset-bg-panel',
            )}
            style={{ background: heatColor(cell.pnl_sol, cell.count, maxAbs) }}
          >
            <span className="max-w-full truncate">{cellLabel}</span>
          </button>
        );
      })}
    </>
  );
}
