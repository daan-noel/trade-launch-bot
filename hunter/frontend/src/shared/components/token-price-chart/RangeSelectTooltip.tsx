import { CHART_COLORS } from './constants';
import { tooltipHorizontalStyle } from './tooltipPlacement';
import type { ChartRangeTooltipState } from './types';
import { formatDecimalTrim } from 'utils/format';

/** Box width cap — also what the edge flip is measured against (see the twin
 *  constant in {@link ./BarCrosshairTooltip}). */
const MAX_W = 260;

/** Compact duration label (shared with the band's chip). */
export function formatRangeDuration(ms: number): string {
  const sec = ms / 1000;
  if (sec < 60) return `${formatDecimalTrim(sec, 1)}s`;
  if (sec < 3600) return `${formatDecimalTrim(sec / 60, 1)}m`;
  return `${formatDecimalTrim(sec / 3600, 1)}h`;
}

/** Totals tooltip shown when the crosshair hovers the range-selection label chip. */
export function RangeSelectTooltip({
  tooltip,
  formatAmount,
  formatPrice,
  containerWidth,
}: {
  tooltip: ChartRangeTooltipState;
  /** Format a SOL flow amount in the chart's display unit. */
  formatAmount: (sol: number) => string;
  /** Format a price magnitude (priceInSol) in the chart's display unit. */
  formatPrice: (priceInSol: number) => string;
  /** Width of the positioned chart panel; omit when unknown (no edge flip). */
  containerWidth?: number;
}) {
  const { stats, point } = tooltip;
  const {
    inflow,
    outflow,
    netFlow,
    durationMs,
    priceDelta,
    priceDeltaPct,
    tradeCount,
    buyCount,
    sellCount,
    uniqueWallets,
    uniqueBuyers,
    uniqueSellers,
    maxBuySol,
    maxSellSol,
  } = stats;
  const accent = CHART_COLORS.rangeBandBorder;
  const deltaUp = priceDelta >= 0;

  return (
    <div
      className="pointer-events-none absolute z-20 rounded-md border px-2.5 py-2 font-mono text-[10px] leading-snug shadow-lg"
      style={{
        ...tooltipHorizontalStyle(point.x, MAX_W, containerWidth),
        maxWidth: MAX_W,
        top: point.y - 10,
        borderColor: `${accent}aa`,
        backgroundColor: '#0d0d0df0',
        color: CHART_COLORS.panelText,
      }}
    >
      <div className="mb-1.5 flex items-center gap-2">
        <span
          className="rounded px-1.5 py-px text-[9px] font-bold tracking-wide"
          style={{
            border: `1px solid ${accent}`,
            backgroundColor: `${accent}22`,
            color: accent,
          }}
        >
          SELECTION
        </span>
        <span style={{ color: CHART_COLORS.panelTextDim }}>
          {formatRangeDuration(durationMs)}
        </span>
      </div>
      <div className="grid grid-cols-[auto_1fr] gap-x-2 gap-y-0.5">
        <span style={{ color: CHART_COLORS.panelTextDim }}>In</span>
        <span>{formatAmount(inflow)}</span>
        <span style={{ color: CHART_COLORS.panelTextDim }}>Out</span>
        <span>{formatAmount(outflow)}</span>
        <span style={{ color: CHART_COLORS.panelTextDim }}>Net</span>
        <span className={netFlow >= 0 ? 'text-primary' : 'text-red'}>
          {formatAmount(netFlow)}
        </span>
        <span style={{ color: CHART_COLORS.panelTextDim }}>Δ</span>
        <span className={deltaUp ? 'text-primary' : 'text-red'}>
          {deltaUp ? '+' : '−'}
          {formatPrice(Math.abs(priceDelta))}
          {priceDeltaPct != null && (
            <span>
              {' '}({deltaUp ? '+' : ''}
              {formatDecimalTrim(priceDeltaPct, 2)}%)
            </span>
          )}
        </span>
        <span style={{ color: CHART_COLORS.panelTextDim }}>Trades</span>
        <span>
          {tradeCount}{' '}
          <span className="text-buy">↑{buyCount}</span>
          <span style={{ color: CHART_COLORS.panelTextDim }}> / </span>
          <span className="text-sell">↓{sellCount}</span>
        </span>
        <span style={{ color: CHART_COLORS.panelTextDim }}>Wallets</span>
        <span>
          {uniqueWallets}{' '}
          <span style={{ color: CHART_COLORS.panelTextDim }}>
            (<span className="text-buy">{uniqueBuyers}b</span>
            {' / '}
            <span className="text-sell">{uniqueSellers}s</span>)
          </span>
        </span>
        <span style={{ color: CHART_COLORS.panelTextDim }}>Max buy</span>
        <span className="text-buy">{formatAmount(maxBuySol)}</span>
        <span style={{ color: CHART_COLORS.panelTextDim }}>Max sell</span>
        <span className="text-sell">{formatAmount(maxSellSol)}</span>
      </div>
    </div>
  );
}
