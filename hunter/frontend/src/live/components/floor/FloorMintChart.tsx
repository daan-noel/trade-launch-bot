import { useMemo } from 'react';
import {
  TokenPriceChart,
  type ChartBarSelection,
  type ChartChrome,
  type ChartEventMarker,
  type ChartRangeSelectionDetail,
  type ChartTimeBand,
  type ChartTimeSpan,
  type ChartValueLane,
} from 'components/token-price-chart';
import { usePriceUnit } from 'context/PriceUnitContext';
import { useWatchTokenTradesLive } from 'hooks/useTokenTradesLive';
import { apiErrorMessage, useGetTokenDetailQuery, useGetTokenTradesQuery } from 'store/apiSlice';
import type { TradeRecord } from 'types';

const EMPTY: TradeRecord[] = [];

/**
 * Compact mint price chart for Floor / Portfolio / Console — entry/exit markers
 * plus the candle-click / range-drag selection.
 *
 * The selection is **controlled**: a host that wants the trades table wires
 * `useBarTradesSelection` and renders `MintBarTradesPanel` wherever it has the
 * width for it (`FloorPositionDetail` puts it full-width under the chart ∥ fills
 * grid). Omitting these props leaves clicks inert, which is what a narrow host
 * such as the Console manual-trade column wants.
 *
 * Pass `chrome="compact"` in narrow hosts so the toolbar collapses behind a
 * Tools toggle and the plot keeps the height.
 */
export function FloorMintChart({
  mint,
  markers,
  height = 220,
  tableId = 'floor-mint-chart',
  chrome = 'full',
  /** Fingerprint `volume_ix_patterns` keys — enables the vol/non-vol overlay. */
  flowPatternKeys = null,
  selectedBar = null,
  onBarClick,
  onRangeChange,
  onCrosshairTimeChange,
  externalCrosshairTimeSec = null,
  timeBands = null,
  timeBandCoverage = null,
  valueLane = null,
}: {
  mint: string;
  markers?: ChartEventMarker[] | null;
  height?: number;
  tableId?: string;
  chrome?: ChartChrome;
  flowPatternKeys?: ReadonlySet<string> | null;
  selectedBar?: ChartBarSelection | null;
  onBarClick?: (selection: ChartBarSelection | null) => void;
  onRangeChange?: (range: ChartRangeSelectionDetail | null) => void;
  /** Wall-clock unix seconds under the crosshair; null on pointer-out. Feeds the
   *  position modal's rule-condition readout. */
  onCrosshairTimeChange?: (timeSec: number | null) => void;
  /** Drive the crosshair from a sibling surface. */
  externalCrosshairTimeSec?: number | null;
  /** Bottom-pane on/off lanes — the position modal's rule-condition timeline. */
  timeBands?: ChartTimeBand[] | null;
  /** The stretch those lanes speak for. */
  timeBandCoverage?: ChartTimeSpan | null;
  valueLane?: ChartValueLane | null;
}) {
  const { unit, usdRate } = usePriceUnit();
  useWatchTokenTradesLive(mint || null);

  const { data: detail, isFetching: detailLoading } = useGetTokenDetailQuery(mint, {
    skip: !mint,
  });
  const {
    data: tradesData,
    isFetching: tradesLoading,
    error: tradesError,
  } = useGetTokenTradesQuery(mint, { skip: !mint });

  const trades = tradesData ?? EMPTY;
  const symbol = detail?.symbol || detail?.name || mint.slice(0, 8);
  const loading = detailLoading || tradesLoading;
  const err = apiErrorMessage(tradesError as never);

  const toValue = useMemo(
    () => (sol: number) => (unit === 'USD' && usdRate != null ? sol * usdRate : sol),
    [unit, usdRate],
  );

  if (!mint) return null;

  return (
    <div className="min-w-0">
      {/* Full chrome already titles the chart; compact's thin bar does too —
          skip the duplicate symbol line so narrow hosts don't lose plot height. */}
      {chrome === 'full' ? (
        <div className="mb-1 flex items-baseline gap-2">
          <span className="text-xs font-semibold text-text">{symbol}</span>
          <span className="font-mono text-[10px] text-text-dim">{mint.slice(0, 8)}…</span>
        </div>
      ) : null}
      <TokenPriceChart
        symbol={symbol}
        id={`${tableId}:${mint}`}
        trades={trades}
        loading={loading}
        error={err}
        toValue={toValue}
        priceLabel={unit}
        priceUnit={unit}
        height={height}
        chrome={chrome}
        eventMarkers={markers ?? null}
        selectedBar={selectedBar}
        onBarClick={onBarClick}
        onRangeChange={onRangeChange}
        onCrosshairTimeChange={onCrosshairTimeChange}
        externalCrosshairTimeSec={externalCrosshairTimeSec}
        timeBands={timeBands}
        timeBandCoverage={timeBandCoverage}
        valueLane={valueLane}
        tokenCreatedAt={detail?.created_at ?? undefined}
        athPriceInSol={detail?.ath_price ?? null}
        isMigrated={detail?.is_migrated}
        isCashbackEnabled={detail?.is_cashback_enabled}
        creatorWallet={detail?.creator_wallet ?? null}
        flowPatternKeys={flowPatternKeys}
      />
    </div>
  );
}
