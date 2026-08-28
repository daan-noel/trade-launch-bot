import { FLOW_NON_VOL_LINE_COLOR, FLOW_VOL_LINE_COLOR } from 'lib/flow/flowChartData';
import { CHART_COLORS } from './constants';
import { formatDecimalTrim } from 'utils/format';
import type { ChartCrosshairInfo } from './types';

type BarFlowFieldsProps = {
  crosshair: ChartCrosshairInfo;
  /** SOL amount formatter (e.g. "◎ 1.23") for Net/In/Out. */
  formatVol: (value: number) => string;
  /** Formatter for cumulative vol/non-vol (SOL or token, depending on basis). */
  formatFlow: (value: number) => string;
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
 * Per-bar order flow readout: net flow, inflow, outflow, price change percent,
 * and — when present — the vol-maker / non-vol overlay lines.
 *
 * The two flow fields are labelled `∑net` because that is exactly what they are:
 * the overlay is a RUNNING NET (buy − sell) since token creation, not a per-bar
 * amount and not a buys-only total. Unlabelled, they read as a rule metric — and a
 * reader who compares `NonVol` against an `m_flow_ix_window.untagged_buy` threshold
 * is comparing a lifetime net against a trailing-window buy sum. Different span,
 * different direction, no relationship.
 */
export function BarFlowFields({ crosshair, formatVol, formatFlow, layout }: BarFlowFieldsProps) {
  const { open, close, inflow, outflow, flowTagged, flowUntagged } = crosshair;
  const net = inflow - outflow;
  const deltaPct = open !== 0 ? ((close - open) / open) * 100 : null;

  // Net/In/Out are order-flow DIRECTION → buy/sell blue-red. Δ is a price move
  // → stays on the candle up/down green-red.
  const netColor = net >= 0 ? CHART_COLORS.buy : CHART_COLORS.sell;
  const deltaColor =
    deltaPct == null ? CHART_COLORS.text : deltaPct >= 0 ? CHART_COLORS.up : CHART_COLORS.down;
  const deltaValue =
    deltaPct == null
      ? '—'
      : `${deltaPct >= 0 ? '+' : ''}${formatDecimalTrim(deltaPct, 2)}%`;

  const flowFields =
    flowTagged != null || flowUntagged != null ? (
      <>
        <FlowField
          label="VolMk∑net"
          value={flowTagged != null ? formatFlow(flowTagged) : '—'}
          color={FLOW_VOL_LINE_COLOR}
          layout={layout}
        />
        {layout === 'inline' ? ' ' : null}
        <FlowField
          label="NonVol∑net"
          value={flowUntagged != null ? formatFlow(flowUntagged) : '—'}
          color={FLOW_NON_VOL_LINE_COLOR}
          layout={layout}
        />
      </>
    ) : null;

  if (layout === 'grid') {
    return (
      <div className="grid grid-cols-[auto_1fr] gap-x-2 gap-y-0.5">
        <FlowField label="Net" value={formatVol(net)} color={netColor} layout="grid" />
        <FlowField label="In" value={formatVol(inflow)} color={CHART_COLORS.buy} layout="grid" />
        <FlowField label="Out" value={formatVol(outflow)} color={CHART_COLORS.sell} layout="grid" />
        <FlowField label="Δ" value={deltaValue} color={deltaColor} layout="grid" />
        {flowFields}
      </div>
    );
  }

  return (
    <>
      <FlowField label="Net" value={formatVol(net)} color={netColor} layout="inline" />{' '}
      <FlowField label="In" value={formatVol(inflow)} color={CHART_COLORS.buy} layout="inline" />{' '}
      <FlowField label="Out" value={formatVol(outflow)} color={CHART_COLORS.sell} layout="inline" />{' '}
      <FlowField label="Δ" value={deltaValue} color={deltaColor} layout="inline" />
      {flowFields != null ? <> {flowFields}</> : null}
    </>
  );
}
