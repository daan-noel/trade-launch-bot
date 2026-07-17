import { useCallback, useState } from 'react';

import { TokenDetailPanel } from 'components/tokens/TokenDetailPanel';
import { TokenTradeChart } from 'components/tokens/TokenTradeChart';
import type { ChartEventMarker, ChartVisibleTimeRange } from 'components/token-price-chart';
import type { TokenDetailRecord } from 'types';
import { MetricPanes } from '@lab/components/strategy/MetricPanes';

/**
 * Lab token inspect: trade-history chart + metric panes sharing crosshair /
 * visible range, with rule metric entry/exit markers on the price chart.
 */
export function LabTokenInspect({
  detail,
  loading = false,
  error = null,
  tableId = 'lab_token_inspect_trades',
  showDetailPanel = true,
}: {
  detail: TokenDetailRecord | null;
  loading?: boolean;
  error?: string | null;
  tableId?: string;
  /** When false, only chart + panes (caller already rendered the detail panel). */
  showDetailPanel?: boolean;
}) {
  const [crosshairTimeSec, setCrosshairTimeSec] = useState<number | null>(null);
  const [visibleTimeRange, setVisibleTimeRange] = useState<ChartVisibleTimeRange | null>(null);
  const [eventMarkers, setEventMarkers] = useState<ChartEventMarker[]>([]);

  const onEventMarkersChange = useCallback((markers: ChartEventMarker[]) => {
    setEventMarkers(markers);
  }, []);

  const mint = detail?.mint_address ?? '';

  return (
    <div className="flex flex-col gap-2.5">
      {showDetailPanel && (
        <TokenDetailPanel detail={detail} loading={loading} error={error} />
      )}
      <TokenTradeChart
        tableId={tableId}
        detail={detail}
        eventMarkers={eventMarkers}
        onCrosshairTimeChange={setCrosshairTimeSec}
        onVisibleTimeRangeChange={setVisibleTimeRange}
      />
      {mint ? (
        <div className="border-t border-white/7 pt-2">
          <h2 className="mb-2 text-[11px] font-semibold uppercase tracking-widest text-text-dim">
            Metric panes
          </h2>
          <MetricPanes
            mint={mint}
            crosshairTimeSec={crosshairTimeSec}
            visibleTimeRange={visibleTimeRange}
            onEventMarkersChange={onEventMarkersChange}
          />
        </div>
      ) : null}
    </div>
  );
}
