import { useCallback, useMemo, useState } from 'react';

import { TokenDetailPanel } from 'components/tokens/TokenDetailPanel';
import { LazyTokenTradeChart } from 'components/tokens/LazyTokenTradeChart';
import { Accordion } from 'components/ui/Accordion';
import type { ChartEventMarker, ChartVisibleTimeRange } from 'components/token-price-chart';
import { patternKeysFrom } from 'lib/flow/classifyFlow';
import { volumeIxPatternsFromConfig } from 'lib/strategy/registry';
import { useGetFingerprintsQuery } from 'store/sharedEndpoints';
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
  positionEntry = null,
  /** Explicit pattern keys; when omitted, resolved from `ruleOverride.fingerprintId`. */
  flowPatternKeys: flowPatternKeysProp = null,
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
}) {
  const [crosshairTimeSec, setCrosshairTimeSec] = useState<number | null>(null);
  /** Who last drove the shared crosshair — only pane hover is pushed into the price chart. */
  const [crosshairSource, setCrosshairSource] = useState<'chart' | 'panes' | null>(null);
  const [visibleTimeRange, setVisibleTimeRange] = useState<ChartVisibleTimeRange | null>(null);
  const [paneMarkers, setPaneMarkers] = useState<ChartEventMarker[]>([]);

  const { data: fingerprints = [] } = useGetFingerprintsQuery(undefined, {
    skip: flowPatternKeysProp != null || !ruleOverride?.fingerprintId,
  });

  const flowPatternKeys = useMemo(() => {
    if (flowPatternKeysProp) return flowPatternKeysProp;
    const fpId = ruleOverride?.fingerprintId;
    if (!fpId) return null;
    const fp = fingerprints.find((f) => f.id === fpId);
    if (!fp) return null;
    return patternKeysFrom(volumeIxPatternsFromConfig(fp.metric_config));
  }, [flowPatternKeysProp, ruleOverride?.fingerprintId, fingerprints]);

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
        flowPatternKeys={flowPatternKeys}
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
            positionEntry={positionEntry}
          />
        </Accordion>
      ) : null}
    </div>
  );
}
