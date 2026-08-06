import { useCallback, useMemo, useState } from 'react';

import { TokenDetailPanel } from 'components/tokens/TokenDetailPanel';
import { LazyTokenTradeChart } from 'components/tokens/LazyTokenTradeChart';
import { Accordion } from 'components/ui/Accordion';
import type { ChartEventMarker, ChartVisibleTimeRange } from 'components/token-price-chart';
import { useFlowPatternKeys } from 'hooks/useFlowPatternKeys';
import { ACCORDION_IDS } from 'lib/storage';
import type { TokenDetailRecord } from 'types';
import {
  MetricPanes,
  MetricPanesPart,
  MetricPanesProvider,
  type MetricPanesRuleOverride,
} from '@lab/components/strategy/MetricPanes';

/**
 * Lab token inspect: trade-history chart + metric panes sharing crosshair /
 * visible range. Overlay layers stay separate by `ChartEventMarker.role`:
 * `fill` = backend entry/exit result (`extraEventMarkers`); `signal` = first
 * metric-condition fire from the panes (circle / blue·amber, no price line).
 */
export function LabTokenInspect({
  detail,
  loading = false,
  error = null,
  tableId = 'lab_token_inspect_trades',
  showDetailPanel = true,
  /** Backend fill entry·exit markers; merged with pane `signal` markers. */
  extraEventMarkers = [],
  ruleOverride = null,
  positionEntry = null,
  /** Explicit pattern keys; when omitted, resolved from `ruleOverride.fingerprintId`. */
  flowPatternKeys: flowPatternKeysProp = null,
  /** `inspect` = graphs on the right, values under chart (modal). `page` = all stacked. */
  metricLayout = 'inspect',
}: {
  detail: TokenDetailRecord | null;
  loading?: boolean;
  error?: string | null;
  tableId?: string;
  /** When false, only chart + panes (caller already rendered the detail panel). */
  showDetailPanel?: boolean;
  extraEventMarkers?: ChartEventMarker[];
  /** Pin the metric panes to the inspected run's exact params (see MetricPanes). */
  ruleOverride?: MetricPanesRuleOverride | null;
  /** Inspected run's entry fill — drives the `m_position` panes (see MetricPanes). */
  positionEntry?: { time: string; price: number } | null;
  flowPatternKeys?: ReadonlySet<string> | null;
  metricLayout?: 'page' | 'inspect';
}) {
  const [crosshairTimeSec, setCrosshairTimeSec] = useState<number | null>(null);
  /** Who last drove the shared crosshair — only pane hover is pushed into the price chart. */
  const [crosshairSource, setCrosshairSource] = useState<'chart' | 'panes' | null>(null);
  const [visibleTimeRange, setVisibleTimeRange] = useState<ChartVisibleTimeRange | null>(null);
  const [paneMarkers, setPaneMarkers] = useState<ChartEventMarker[]>([]);

  const resolvedFlowKeys = useFlowPatternKeys(
    flowPatternKeysProp != null ? null : ruleOverride?.fingerprintId,
  );
  const flowPatternKeys = flowPatternKeysProp ?? resolvedFlowKeys;

  const onEventMarkersChange = useCallback((markers: ChartEventMarker[]) => {
    setPaneMarkers(markers);
  }, []);

  const eventMarkers = useMemo(
    () => [...extraEventMarkers, ...paneMarkers],
    [extraEventMarkers, paneMarkers],
  );

  const onChartCrosshair = useCallback((t: number | null) => {
    setCrosshairSource(t == null ? null : 'chart');
    setCrosshairTimeSec(t);
  }, []);

  const onPanesCrosshair = useCallback((t: number | null) => {
    setCrosshairSource(t == null ? null : 'panes');
    setCrosshairTimeSec(t);
  }, []);

  const mint = detail?.mint_address ?? '';

  const metricPanesProps = {
    mint,
    crosshairTimeSec,
    visibleTimeRange,
    onCrosshairTimeChange: onPanesCrosshair,
    onEventMarkersChange,
    ruleOverride,
    positionEntry,
  };

  const chart = (
    <LazyTokenTradeChart
      tableId={tableId}
      detail={detail}
      eventMarkers={eventMarkers}
      onCrosshairTimeChange={onChartCrosshair}
      externalCrosshairTimeSec={crosshairSource === 'panes' ? crosshairTimeSec : null}
      onVisibleTimeRangeChange={setVisibleTimeRange}
      flowPatternKeys={flowPatternKeys}
    />
  );

  return (
    <div className="flex flex-col gap-2.5">
      {/* Collapsed by default — inspect opens for the chart + metric panes; the
          token's static detail is reference, not the reason you're here. */}
      {showDetailPanel && (
        <Accordion
          title="Detail"
          padding="sm"
          bordered={false}
          storageKey={ACCORDION_IDS.inspectDetail}
          defaultOpen={false}
        >
          <TokenDetailPanel
            detail={detail}
            loading={loading}
            error={error}
            compact
          />
        </Accordion>
      )}

      {!mint ? (
        chart
      ) : metricLayout === 'page' ? (
        <>
          {chart}
          <MetricPanes {...metricPanesProps} />
        </>
      ) : (
        <MetricPanesProvider layout="inspect" {...metricPanesProps}>
          <div className="grid grid-cols-1 items-start gap-3 md:grid-cols-[minmax(0,1fr)_minmax(340px,36%)]">
            <div className="flex min-w-0 flex-col gap-2.5">
              {chart}
              <MetricPanesPart part="values" />
            </div>
            <aside className="flex min-w-0 flex-col gap-2 border-t border-white/8 pt-3 md:sticky md:top-0 md:max-h-[calc(98vh-4rem)] md:overflow-y-auto md:border-t-0 md:border-l md:pl-3 md:pt-0">
              <span className="text-[10px] font-bold uppercase tracking-widest text-text-dim">
                Metric graphs
              </span>
              <MetricPanesPart part="selector" />
              <MetricPanesPart part="graphs" />
            </aside>
          </div>
        </MetricPanesProvider>
      )}
    </div>
  );
}
