import { useMemo } from 'react';
import {
  TokenPriceChart,
  type ChartEventMarker,
} from 'components/token-price-chart';
import { usePriceUnit } from 'context/PriceUnitContext';
import { useWatchTokenTradesLive } from 'hooks/useTokenTradesLive';
import { apiErrorMessage, useGetTokenDetailQuery, useGetTokenTradesQuery } from 'store/apiSlice';
import type { TradeRecord } from 'types';

const EMPTY: TradeRecord[] = [];

/**
 * Compact mint price chart for Floor / Portfolio row detail — entry/exit
 * markers only (no trades table under the chart).
 */
export function FloorMintChart({
  mint,
  markers,
  height = 220,
  tableId = 'floor-mint-chart',
  /** Fingerprint `volume_ix_patterns` keys — enables the vol/non-vol overlay. */
  flowPatternKeys = null,
}: {
  mint: string;
  markers?: ChartEventMarker[] | null;
  height?: number;
  tableId?: string;
  flowPatternKeys?: ReadonlySet<string> | null;
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
      <div className="mb-1 flex items-baseline gap-2">
        <span className="text-xs font-semibold text-text">{symbol}</span>
        <span className="font-mono text-[10px] text-text-dim">{mint.slice(0, 8)}…</span>
      </div>
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
        eventMarkers={markers ?? null}
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
