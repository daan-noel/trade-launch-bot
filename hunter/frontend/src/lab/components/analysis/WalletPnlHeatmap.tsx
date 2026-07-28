import { memo, useMemo } from 'react';
import { cn } from 'lib/cn';
import { formatDecimalTrim } from 'utils/format';
import { DOW_ROWS, HOURS, type PnlHeatCell } from './walletPnlStats';

interface WalletPnlHeatmapProps {
  cells: PnlHeatCell[];
}

/** Diverging green/red wash by PnL sign, opacity by magnitude relative to the
 *  largest |pnl| observed across all 168 cells (so one blowout hour doesn't
 *  wash out everything else at a flat max intensity — same contrast-stretch
 *  idea `CreationHeatmap` uses for rate metrics, just signed here). */
function heatColor(pnlSol: number, count: number, maxAbs: number): string {
  if (count === 0) return 'rgba(255,255,255,0.02)';
  if (pnlSol === 0) return 'rgba(255,255,255,0.05)';
  const norm = maxAbs > 0 ? Math.min(1, Math.abs(pnlSol) / maxAbs) : 0;
  const alpha = 0.08 + 0.78 * norm;
  // Green (buy/up) for a net-positive slot, red (sell/down) for net-negative —
  // matches `--color-green`/`--color-red` (the candle up/down pair) rather than
  // introducing a third palette.
  return pnlSol > 0 ? `rgba(16,185,129,${alpha.toFixed(3)})` : `rgba(239,68,68,${alpha.toFixed(3)})`;
}

/**
 * Day-of-week × hour-of-day heatmap of this wallet's PnL — pure CSS grid (no
 * charting dep, mirrors `CreationHeatmap`), colored on a signed green/red scale
 * instead of a magnitude-only one. Bucketed by each mint's most-recent trade in
 * the window (see `buildPnlHeatCells`'s doc comment for the per-mint-grain
 * caveat), in the caller's selected timezone.
 */
export const WalletPnlHeatmap = memo(function WalletPnlHeatmap({ cells }: WalletPnlHeatmapProps) {
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
    return <p className="text-xs text-text-dim">No trades in this window to plot.</p>;
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
}: {
  label: string;
  dow: number;
  lookup: Map<number, PnlHeatCell>;
  maxAbs: number;
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
            ? `PnL: ${cell.pnl_sol >= 0 ? '+' : ''}${formatDecimalTrim(cell.pnl_sol, 3)} SOL over ${cell.count} token${cell.count === 1 ? '' : 's'}`
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
