import { CHART_COLORS } from './constants';
import type { ChartChainTooltipState } from './types';
import { formatDecimalTrim } from 'utils/format';

function formatDuration(ms: number): string {
  const sec = ms / 1000;
  if (sec < 60) return `${formatDecimalTrim(sec, 1)}s`;
  if (sec < 3600) return `${formatDecimalTrim(sec / 60, 1)}m`;
  return `${formatDecimalTrim(sec / 3600, 1)}h`;
}

/** Buy/sell split of the trades inside the chain window. */
export interface ChainTradeCounts {
  total: number;
  buy: number;
  sell: number;
}

/** Totals tooltip shown when the crosshair hovers the chain label chip. */
export function ChainHighlightTooltip({
  tooltip,
  tradeCounts,
  formatAmount,
  formatPrice,
}: {
  tooltip: ChartChainTooltipState;
  /** Total/buy/sell trade counts inside the chain window. */
  tradeCounts: ChainTradeCounts;
  /** Format a SOL flow amount in the chart's display unit. */
  formatAmount: (sol: number) => string;
  /** Format a price magnitude (priceInSol) in the chart's display unit. */
  formatPrice: (priceInSol: number) => string;
}) {
  const { highlight, point } = tooltip;
  const { inflow, outflow, netFlow, durationMs, priceDelta, priceDeltaPct, pairCount } = highlight;
  const accent = CHART_COLORS.chainBandBorder;
  const deltaUp = priceDelta >= 0;

  return (
    <div
      className="pointer-events-none absolute z-20 max-w-[240px] rounded-md border px-2.5 py-2 font-mono text-[10px] leading-snug shadow-lg"
      style={{
        left: point.x + 14,
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
          LONGEST CHAIN
        </span>
        <span style={{ color: CHART_COLORS.panelTextDim }}>{formatDuration(durationMs)}</span>
      </div>
      <div className="grid grid-cols-[auto_1fr] gap-x-2 gap-y-0.5">
        <span style={{ color: CHART_COLORS.panelTextDim }}>Pairs</span>
        <span>{pairCount}</span>
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
          {tradeCounts.total}{' '}
          <span className="text-primary">↑{tradeCounts.buy}</span>
          <span style={{ color: CHART_COLORS.panelTextDim }}> / </span>
          <span className="text-red">↓{tradeCounts.sell}</span>
        </span>
      </div>
    </div>
  );
}
