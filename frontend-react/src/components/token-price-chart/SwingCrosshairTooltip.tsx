import { CHART_COLORS } from './constants';
import { cn } from './cn';
import type { ChartSwingTooltipState } from './types';
import { formatDecimalTrim } from '../../utils/format';
import { useTimezone } from '../../context/TimezoneContext';
import { formatTimestampMsCompact } from '../../utils/date';

function formatDuration(ms: number | undefined): string {
  if (ms == null) return '—';
  const sec = ms / 1000;
  if (sec < 60) return `${formatDecimalTrim(sec, 1)}s`;
  if (sec < 3600) return `${formatDecimalTrim(sec / 60, 1)}m`;
  return `${formatDecimalTrim(sec / 3600, 1)}h`;
}

export function SwingCrosshairTooltip({
  tooltip,
  formatPrice,
  formatAmount,
}: {
  tooltip: ChartSwingTooltipState;
  formatPrice: (priceInSol: number) => string;
  formatAmount: (sol: number) => string;
}) {
  const { timezone } = useTimezone();
  const { leg, point } = tooltip;
  const isHigh = leg.type === 'swing_high';
  const accent = isHigh ? CHART_COLORS.swingHigh : CHART_COLORS.swingLow;
  const changePct =
    leg.start_price > 0
      ? ((leg.end_price - leg.start_price) / leg.start_price) * 100
      : null;

  return (
    <div
      className="pointer-events-none absolute z-20 max-w-[240px] rounded-md border px-2.5 py-2 font-mono text-[10px] leading-snug shadow-lg"
      style={{
        left: point.x + 14,
        top: point.y - 10,
        borderColor: `${accent}66`,
        backgroundColor: '#0d0d0df0',
        color: CHART_COLORS.panelText,
      }}
    >
      <div className="mb-1.5 flex items-center gap-2">
        <span
          className={cn(
            'rounded px-1.5 py-px text-[9px] font-bold tracking-wide',
            isHigh ? 'text-primary' : 'text-red',
          )}
          style={{
            border: `1px solid ${accent}`,
            backgroundColor: `${accent}22`,
            color: accent,
          }}
        >
          {isHigh ? 'SWING HIGH' : 'SWING LOW'}
        </span>
        <span style={{ color: CHART_COLORS.panelTextDim }}>
          {formatDuration(leg.duration_ms)}
        </span>
      </div>
      <div className="grid grid-cols-[auto_1fr] gap-x-2 gap-y-0.5">
        <span style={{ color: CHART_COLORS.panelTextDim }}>Start</span>
        <span>
          {formatPrice(leg.start_price)} · {formatTimestampMsCompact(leg.start_at, timezone)}
        </span>
        <span style={{ color: CHART_COLORS.panelTextDim }}>End</span>
        <span>
          {formatPrice(leg.end_price)} · {formatTimestampMsCompact(leg.end_at, timezone)}
        </span>
        {changePct != null && (
          <>
            <span style={{ color: CHART_COLORS.panelTextDim }}>Δ%</span>
            <span className={changePct >= 0 ? 'text-primary' : 'text-red'}>
              {changePct >= 0 ? '+' : ''}
              {formatDecimalTrim(changePct, 2)}%
            </span>
          </>
        )}
        <span style={{ color: CHART_COLORS.panelTextDim }}>In</span>
        <span>{leg.inflow != null ? formatAmount(leg.inflow) : '—'}</span>
        <span style={{ color: CHART_COLORS.panelTextDim }}>Out</span>
        <span>{leg.outflow != null ? formatAmount(leg.outflow) : '—'}</span>
        <span style={{ color: CHART_COLORS.panelTextDim }}>Net</span>
        <span
          className={
            leg.net_flow != null ? (leg.net_flow >= 0 ? 'text-primary' : 'text-red') : undefined
          }
        >
          {leg.net_flow != null ? formatAmount(leg.net_flow) : '—'}
        </span>
        <span style={{ color: CHART_COLORS.panelTextDim }}>Trades</span>
        <span>{leg.trade_count ?? '—'}</span>
      </div>
    </div>
  );
}
