import { CHART_COLORS } from './constants';
import { formatDecimalTrim } from '../../utils/format';
import type { ChartCrosshairInfo } from './types';

type BarFlowFieldsProps = {
  crosshair: ChartCrosshairInfo;
  /** SOL amount formatter (e.g. "◎ 1.23"). */
  formatVol: (value: number) => string;
  layout: 'grid' | 'inline';
};

function FlowField({
  label,
  value,
  color,
  layout,
}: {
  label: string;
  value: string;
  color: string;
  layout: 'grid' | 'inline';
}) {
  if (layout === 'grid') {
    return (
      <>
        <span style={{ color: CHART_COLORS.panelTextDim }}>{label}</span>
        <span style={{ color }}>{value}</span>
      </>
    );
  }
  return (
    <span style={{ color }}>
      <span className="font-semibold">{label}</span> {value}
    </span>
  );
}

/**
 * Per-bar order flow readout: net flow, inflow, outflow, and the bar's price
 * change percent — shown on crosshair hover in place of OHLC/Vol/Liq.
 */
export function BarFlowFields({ crosshair, formatVol, layout }: BarFlowFieldsProps) {
  const { open, close, inflow, outflow } = crosshair;
  const net = inflow - outflow;
  const deltaPct = open !== 0 ? ((close - open) / open) * 100 : null;

  const netColor = net >= 0 ? CHART_COLORS.up : CHART_COLORS.down;
  const deltaColor =
    deltaPct == null ? CHART_COLORS.text : deltaPct >= 0 ? CHART_COLORS.up : CHART_COLORS.down;
  const deltaValue =
    deltaPct == null
      ? '—'
      : `${deltaPct >= 0 ? '+' : ''}${formatDecimalTrim(deltaPct, 2)}%`;

  if (layout === 'grid') {
    return (
      <div className="grid grid-cols-[auto_1fr] gap-x-2 gap-y-0.5">
        <FlowField label="Net" value={formatVol(net)} color={netColor} layout="grid" />
        <FlowField label="In" value={formatVol(inflow)} color={CHART_COLORS.up} layout="grid" />
        <FlowField label="Out" value={formatVol(outflow)} color={CHART_COLORS.down} layout="grid" />
        <FlowField label="Δ" value={deltaValue} color={deltaColor} layout="grid" />
      </div>
    );
  }

  return (
    <>
      <FlowField label="Net" value={formatVol(net)} color={netColor} layout="inline" />{' '}
      <FlowField label="In" value={formatVol(inflow)} color={CHART_COLORS.up} layout="inline" />{' '}
      <FlowField label="Out" value={formatVol(outflow)} color={CHART_COLORS.down} layout="inline" />{' '}
      <FlowField label="Δ" value={deltaValue} color={deltaColor} layout="inline" />
    </>
  );
}
