import { useCallback, useMemo, useState } from 'react';

import { TokenDetailPanel } from 'components/tokens/TokenDetailPanel';
import { LazyTokenTradeChart } from 'components/tokens/LazyTokenTradeChart';
import { Accordion } from 'components/ui/Accordion';
import type { ChartEventMarker, ChartVisibleTimeRange } from 'components/token-price-chart';
import type { TokenDetailRecord } from 'types';
import { MetricPanes, type MetricPanesRuleOverride } from '@lab/components/strategy/MetricPanes';

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
}) {
  const [crosshairTimeSec, setCrosshairTimeSec] = useState<number | null>(null);
  /** Who last drove the shared crosshair — only pane hover is pushed into the price chart. */
  const [crosshairSource, setCrosshairSource] = useState<'chart' | 'panes' | null>(null);
  const [visibleTimeRange, setVisibleTimeRange] = useState<ChartVisibleTimeRange | null>(null);
  const [paneMarkers, setPaneMarkers] = useState<ChartEventMarker[]>([]);

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

  return (
    <div className="flex flex-col gap-2.5">
      {showDetailPanel && (
        <Accordion title="Detail" padding="sm" bordered={false} storageKey="mt:inspect-detail-open">
          <TokenDetailPanel detail={detail} loading={loading} error={error} />
        </Accordion>
      )}
      <LazyTokenTradeChart
        tableId={tableId}
        detail={detail}
        eventMarkers={eventMarkers}
        onCrosshairTimeChange={onChartCrosshair}
        externalCrosshairTimeSec={crosshairSource === 'panes' ? crosshairTimeSec : null}
        onVisibleTimeRangeChange={setVisibleTimeRange}
      />
      {mint ? (
        <Accordion title="Metric panes" bordered={false} padding="sm" storageKey="mt:inspect-panes-open">
          <MetricPanes
            mint={mint}
            crosshairTimeSec={crosshairTimeSec}
            visibleTimeRange={visibleTimeRange}
            onCrosshairTimeChange={onPanesCrosshair}
            onEventMarkersChange={onEventMarkersChange}
            ruleOverride={ruleOverride}
          />
        </Accordion>
      ) : null}
    </div>
  );
}
