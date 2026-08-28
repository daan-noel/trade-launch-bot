import { useCallback, useMemo, useState } from 'react';

import { TokenDetailPanel } from 'components/tokens/TokenDetailPanel';
import { LazyTokenTradeChart } from 'components/tokens/LazyTokenTradeChart';
import { Accordion } from 'components/ui/Accordion';
import type { ChartEventMarker, ChartVisibleTimeRange } from 'components/token-price-chart';
import { useFlowPatternSource } from 'hooks/useFlowPatternKeys';
import { ACCORDION_IDS } from 'lib/storage';
import type { MetricConditionLanes } from 'lib/strategy/metricPanes';
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
  exitReason = null,
  /** Explicit pattern keys; when omitted, resolved from `ruleOverride.fingerprintId`. */
  flowPatternKeys: flowPatternKeysProp = null,
  flowFingerprintId: flowFingerprintIdProp = null,
  flowReadOnly = false,
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
  /** The run's exit reason — picks the condition the timeline draws as a value line. */
  exitReason?: string | null;
  flowPatternKeys?: ReadonlySet<string> | null;
  /** Fingerprint the keys came from — the Tagged-badge write target. Like
   *  {@link flowPatternKeys}, falls back to `ruleOverride.fingerprintId`. */
  flowFingerprintId?: string | null;
  /** A stored run's frozen patterns — display only (see `BarTradesPanel`). */
  flowReadOnly?: boolean;
  metricLayout?: 'page' | 'inspect';
}) {
  const [crosshairTimeSec, setCrosshairTimeSec] = useState<number | null>(null);
  /** Who last drove the shared crosshair — only pane hover is pushed into the price chart. */
  const [crosshairSource, setCrosshairSource] = useState<'chart' | 'panes' | null>(null);
  const [visibleTimeRange, setVisibleTimeRange] = useState<ChartVisibleTimeRange | null>(null);
  const [paneMarkers, setPaneMarkers] = useState<ChartEventMarker[]>([]);
  // The panes publish the rule's condition lanes; the chart draws them in its
  // bottom pane. Same shape the live position modal's timeline uses.
  const [conditionBands, setConditionBands] = useState<MetricConditionLanes | null>(null);

  // Resolve the SOURCE, not just the keys: the inspected run's fingerprint is the
  // row a Tagged-badge edit writes to, and every inspect host (sim, sweep promote,
  // dry-run, rule analyze) already knows it — so no reader has to re-pick it.
  // Resolved even when the caller passed explicit keys: "classify with what" and
  // "edit which row" are different questions, and the run this inspect belongs to
  // answers the second one whether or not it overrode the first.
  const resolvedFlowSource = useFlowPatternSource(ruleOverride?.fingerprintId);
  const flowPatternKeys = flowPatternKeysProp ?? resolvedFlowSource.keys;
  const flowFingerprintId = flowFingerprintIdProp ?? resolvedFlowSource.fingerprintId;

  const onEventMarkersChange = useCallback((markers: ChartEventMarker[]) => {
    setPaneMarkers(markers);
  }, []);

  const onConditionBandsChange = useCallback((bands: MetricConditionLanes | null) => {
    setConditionBands(bands);
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
    onConditionBandsChange,
    exitReason,
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
      flowFingerprintId={flowFingerprintId}
      flowReadOnly={flowReadOnly}
      timeBands={conditionBands?.lanes ?? null}
      timeBandCoverage={conditionBands?.coverage ?? null}
      valueLane={conditionBands?.valueLane ?? null}
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
