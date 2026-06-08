import { useCallback, useState } from 'react';
import {
  TokenPriceChart,
  type ChartBarSelection,
  type ChartMetric,
} from 'components/token-price-chart';
import { usePriceUnit } from 'context/PriceUnitContext';
import { apiErrorMessage, useGetTokenTradesQuery } from 'store/apiSlice';
import type { TokenDetailRecord, TradeRecord } from 'types';

/** Stable empty reference so the chart doesn't re-aggregate on every render. */
const EMPTY_TRADES: TradeRecord[] = [];

interface TokenTradeChartProps {
  detail: TokenDetailRecord | null;
}

/**
 * Trade-history price chart for the selected token's detail panel. Pulls the
 * per-mint trades from the shared RTK Query cache (same key as the Swing
 * detection page, so a token already viewed there renders instantly) and feeds
 * them to the reusable {@link TokenPriceChart}.
 */
export function TokenTradeChart({ detail }: TokenTradeChartProps) {
  const { unit, usdRate } = usePriceUnit();
  const [metric, setMetric] = useState<ChartMetric>('price');
  const [selectedBar, setSelectedBar] = useState<ChartBarSelection | null>(null);

  const mint = detail?.mint_address ?? '';
  const {
    data: tradesData,
    isFetching: tradesLoading,
    error: tradesErrorRaw,
  } = useGetTokenTradesQuery(mint, { skip: !mint });

  const toChartValue = useCallback(
    (sol: number) => (unit === 'USD' && usdRate != null ? sol * usdRate : sol),
    [unit, usdRate],
  );

  if (!detail) return null;

  const trades = tradesData ?? EMPTY_TRADES;
  const tradesError = apiErrorMessage(tradesErrorRaw, 'Failed to load trades');
  const symbol = detail.symbol || detail.name || mint;
  const priceLabel = metric === 'mc' ? `MC (${unit})` : unit;

  return (
    <div className="border-t border-white/7 pt-2">
      <div className="mb-1.5 text-[9px] font-bold uppercase tracking-widest text-text-dim">
        Trade History
      </div>
      <TokenPriceChart
        symbol={symbol}
        id={mint}
        trades={trades}
        loading={tradesLoading}
        error={tradesError}
        toValue={toChartValue}
        priceLabel={priceLabel}
        priceUnit={unit}
        metric={metric}
        onMetricChange={setMetric}
        selectedBar={selectedBar}
        onBarClick={setSelectedBar}
        athPriceInSol={detail.ath_price ?? null}
        isMigrated={detail.is_migrated}
        isMayhemMode={detail.is_mayhem_mode}
        isCashbackEnabled={detail.is_cashback_enabled}
        tokenCreatedAt={detail.created_at}
      />
    </div>
  );
}
