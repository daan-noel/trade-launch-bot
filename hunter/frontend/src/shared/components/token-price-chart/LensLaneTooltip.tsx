import { CHART_COLORS } from './constants';
import { formatDecimalTrim } from 'utils/format';
import { formatLensSol } from './lensTint';
import { tooltipHorizontalStyle } from './tooltipPlacement';
import type { ChartLensLaneTooltipState } from './types';

/** Rows listed before the rest collapse into "+k more" — the box has to stay a
 *  tooltip, not become a second trades table. */
const MAX_ROWS = 8;

/** Box width cap — also what the edge flip is measured against (see the twin
 *  constant in {@link ./BarCrosshairTooltip}). */
const MAX_W = 240;

function shortAddr(addr: string | undefined): string {
  if (!addr) return '—';
  return addr.length > 12 ? `${addr.slice(0, 4)}…${addr.slice(-4)}` : addr;
}

/**
 * Hovering one highlight-lane mark: that row's trades in that bar — side and SOL
 * size, network fee, wallet — largest first. Only this row: another row's trades
 * in the same bar show when its own mark is hovered, and the candle's own numbers
 * show over the candles.
 */
export function LensLaneTooltip({
  tooltip,
  formatTime,
  containerWidth,
}: {
  tooltip: ChartLensLaneTooltipState;
  formatTime: (barTime: ChartLensLaneTooltipState['barTime']) => string;
  /** Width of the positioned chart panel; omit when unknown (no edge flip). */
  containerWidth?: number;
}) {
  const { point, barTime, label, color, trades } = tooltip;
  const sorted = [...trades].sort((a, b) => (b.amount_sol ?? 0) - (a.amount_sol ?? 0));
  const shown = sorted.slice(0, MAX_ROWS);

  return (
    <div
      className="pointer-events-none absolute z-30 rounded-md border px-2.5 py-2 font-mono text-[10px] leading-snug shadow-lg"
      style={{
        ...tooltipHorizontalStyle(point.x, MAX_W, containerWidth),
        maxWidth: MAX_W,
        bottom: `calc(100% - ${point.y - 10}px)`,
        borderColor: color,
        backgroundColor: '#0d0d0df5',
        color: CHART_COLORS.panelText,
      }}
    >
      <div className="mb-1 flex items-center gap-1.5 text-[9px] font-bold tracking-wide">
        <span className="inline-block size-2 shrink-0 rounded-full" style={{ backgroundColor: color }} />
        <span className="truncate" style={{ color }}>
          {label}
        </span>
        <span className="ml-auto shrink-0" style={{ color: CHART_COLORS.panelTextDim }}>
          {formatTime(barTime)}
        </span>
      </div>
      {shown.map((trade, i) => {
        const isBuy = trade.trade_type === 'buy';
        return (
          <div key={i} className="flex items-center gap-1.5 whitespace-nowrap">
            <span style={{ color: isBuy ? CHART_COLORS.buy : CHART_COLORS.sell }}>
              {isBuy ? 'B' : 'S'} {trade.amount_sol != null ? formatLensSol(trade.amount_sol) : '—'}
            </span>
            <span style={{ color: CHART_COLORS.panelTextDim }}>
              fee {trade.fee_sol != null ? formatDecimalTrim(trade.fee_sol, 9) : '—'}
            </span>
            <span style={{ color: CHART_COLORS.panelTextDim }}>{shortAddr(trade.wallet_address)}</span>
          </div>
        );
      })}
      {sorted.length > shown.length && (
        <div style={{ color: CHART_COLORS.panelTextDim }}>+{sorted.length - shown.length} more</div>
      )}
    </div>
  );
}
