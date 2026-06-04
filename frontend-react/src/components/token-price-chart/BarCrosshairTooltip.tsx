import { CHART_COLORS } from './constants';
import { BarCrosshairFields } from './BarCrosshairFields';
import type { ChartBarTooltipState } from './types';

export function BarCrosshairTooltip({
  tooltip,
  formatPrice,
  formatVol,
  formatTime,
}: {
  tooltip: ChartBarTooltipState;
  formatPrice: (value: number) => string;
  formatVol: (value: number) => string;
  formatTime: (barTime: ChartBarTooltipState['barTime']) => string;
}) {
  const { point, style, barTime, open, high, low, close, volume, liquiditySol } = tooltip;

  return (
    <div
      className="pointer-events-none absolute z-20 max-w-[240px] rounded-md border px-2.5 py-2 font-mono text-[10px] leading-snug shadow-lg"
      style={{
        left: point.x + 14,
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
      </div>
      <BarCrosshairFields
        style={style}
        crosshair={{ open, high, low, close, volume, liquiditySol }}
        formatPrice={formatPrice}
        formatVol={formatVol}
        layout="grid"
      />
    </div>
  );
}
