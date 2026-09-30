import { CHART_COLORS } from './constants';
import { formatAgePrecise, formatDecimalTrim } from 'utils/format';
import { BarFlowFields } from './BarFlowFields';
import { formatLensSol } from './lensTint';
import { tooltipHorizontalStyle } from './tooltipPlacement';
import type { ChartBarTooltipState, ChartTrade } from './types';

/** One highlighted trade in the hovered bar, with the color of every armed lens
 *  item it matches (a wallet AND a structure → two dots). */
export interface LensTooltipRow {
  trade: ChartTrade;
  colors: string[];
}

/** Rows listed before the rest collapse into "+k more" — the box has to stay a
 *  tooltip, not become a second trades table. */
const MAX_LENS_ROWS = 8;

/** Box width cap — also what the edge flip is measured against, so it's set as a
 *  style (one source) rather than a `max-w-[…]` class that could drift from it. */
const MAX_W = 240;

export function BarCrosshairTooltip({
  tooltip,
  formatVol,
  formatFlow,
  formatTime,
  containerWidth,
  lensRows = null,
}: {
  tooltip: ChartBarTooltipState;
  formatVol: (value: number) => string;
  formatFlow: (value: number) => string;
  formatTime: (barTime: ChartBarTooltipState['barTime']) => string;
  /** Width of the positioned chart panel; omit when unknown (no edge flip). */
  containerWidth?: number;
  /** The armed highlight lenses' trades in this bar — size, fee and wallet, so a
   *  washed candle reads without clicking it. */
  lensRows?: readonly LensTooltipRow[] | null;
}) {
  const { point, barTime, ageSec } = tooltip;

  return (
    <div
      className="pointer-events-none absolute z-20 rounded-md border px-2.5 py-2 font-mono text-[10px] leading-snug shadow-lg"
      style={{
        ...tooltipHorizontalStyle(point.x, MAX_W, containerWidth),
        maxWidth: MAX_W,
        top: point.y - 10,
        borderColor: `${CHART_COLORS.crosshair}`,
        backgroundColor: '#0d0d0df0',
        color: CHART_COLORS.panelText,
      }}
    >
      <div
        className="mb-1.5 text-[9px] font-bold tracking-wide"
        style={{ color: CHART_COLORS.panelTextDim }}
      >
        {formatTime(barTime)}
        {ageSec != null && (
          <span style={{ color: CHART_COLORS.panelText }}> · +{formatAgePrecise(ageSec)}</span>
        )}
      </div>
      <BarFlowFields
        crosshair={tooltip}
        formatVol={formatVol}
        formatFlow={formatFlow}
        layout="grid"
      />
      {lensRows && lensRows.length > 0 && <LensRows rows={lensRows} />}
    </div>
  );
}

function shortAddr(addr: string | undefined): string {
  if (!addr) return '—';
  return addr.length > 12 ? `${addr.slice(0, 4)}…${addr.slice(-4)}` : addr;
}

/** The highlighted trades of the hovered bar: side and SOL size, network fee,
 *  wallet — largest first, the same order the size labels stack in. */
function LensRows({ rows }: { rows: readonly LensTooltipRow[] }) {
  const sorted = [...rows].sort((a, b) => (b.trade.amount_sol ?? 0) - (a.trade.amount_sol ?? 0));
  const shown = sorted.slice(0, MAX_LENS_ROWS);
  return (
    <div className="mt-1.5 border-t pt-1.5" style={{ borderColor: CHART_COLORS.crosshair }}>
      {shown.map(({ trade, colors }, i) => {
        const isBuy = trade.trade_type === 'buy';
        return (
          <div key={i} className="flex items-center gap-1 whitespace-nowrap">
            <span className="flex gap-px">
              {colors.map((c) => (
                <span key={c} className="inline-block size-1.5 rounded-full" style={{ backgroundColor: c }} />
              ))}
            </span>
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
