import type { UTCTimestamp } from 'lightweight-charts';
import { PUMP_MIGRATION_SPOT_PRICE_SOL, TOKEN_TOTAL_SUPPLY } from './constants';
import type { ChartMetric, ChartTrade, OhlcBar } from './types';

function tradeTimestampSec(blockTime: string): number | null {
  const ms = Date.parse(blockTime);
  if (Number.isNaN(ms)) return null;
  return Math.floor(ms / 1000);
}

export function bucketStart(sec: number, intervalSec: number): number {
  return Math.floor(sec / intervalSec) * intervalSec;
}

export function tradeBarTime(
  blockTime: string,
  intervalSec: number,
): UTCTimestamp | null {
  const sec = tradeTimestampSec(blockTime);
  if (sec == null) return null;
  return bucketStart(sec, intervalSec) as UTCTimestamp;
}

export function tradeBarSlot(trade: Pick<ChartTrade, 'slot'>): UTCTimestamp | null {
  const slot = trade.slot;
  if (slot == null) return null;
  return slot as UTCTimestamp;
}

/** Bonding-curve spot price: virtual SOL / virtual tokens (GMGN-style). */
export function curveSpotPriceSol(
  trade: Pick<ChartTrade, 'virtual_sol_reserves' | 'virtual_token_reserves'>,
): number | null {
  const vsol = trade.virtual_sol_reserves;
  const vtoken = trade.virtual_token_reserves;
  if (vsol == null || vtoken == null || vtoken <= 0) return null;
  return vsol / vtoken;
}

/** Bonding-curve liquidity in SOL (GMGN-style: 2× virtual SOL reserves). */
export function curveLiquiditySol(
  trade: Pick<ChartTrade, 'virtual_sol_reserves'>,
): number | null {
  const vsol = trade.virtual_sol_reserves;
  if (vsol == null || vsol <= 0) return null;
  return vsol * 2;
}

/** FDV in SOL: total supply × spot price (GMGN-style MC). */
export function curveMarketCapSol(
  trade: Pick<ChartTrade, 'virtual_sol_reserves' | 'virtual_token_reserves'>,
): number | null {
  const spot = curveSpotPriceSol(trade);
  if (spot == null) return null;
  return TOKEN_TOTAL_SUPPLY * spot;
}

/** Chart Y value for token ATH (spot SOL or FDV MC), after display conversion. */
export function athChartValue(
  athPriceInSol: number | null | undefined,
  metric: ChartMetric,
  toValue: (priceInSol: number) => number,
): number | null {
  if (athPriceInSol == null || athPriceInSol <= 0) return null;
  const solValue =
    metric === 'price' ? athPriceInSol : TOKEN_TOTAL_SUPPLY * athPriceInSol;
  return toValue(solValue);
}

/** Chart Y value for pump.fun migration (graduation) price, after display conversion. */
export function migrationChartValue(
  metric: ChartMetric,
  toValue: (priceInSol: number) => number,
): number {
  const solValue =
    metric === 'price'
      ? PUMP_MIGRATION_SPOT_PRICE_SOL
      : TOKEN_TOTAL_SUPPLY * PUMP_MIGRATION_SPOT_PRICE_SOL;
  return toValue(solValue);
}

export function chartValueForTrade(
  trade: ChartTrade,
  metric: ChartMetric,
): number | null {
  return metric === 'price' ? curveSpotPriceSol(trade) : curveMarketCapSol(trade);
}

/** Trades with `price_per_token` set to the chart metric (overlay / markers). */
export function tradesForChartMetric(
  trades: ChartTrade[],
  metric: ChartMetric,
): ChartTrade[] {
  const sorted = [...trades].sort(
    (a, b) => Date.parse(a.block_time) - Date.parse(b.block_time),
  );

  return sorted.flatMap((trade) => {
    const value = chartValueForTrade(trade, metric);
    if (value == null) return [];
    return [{ ...trade, price_per_token: value }];
  });
}

type TradeBucket = {
  prices: number[];
  volume: number;
  liquiditySol: number | null;
};

/** GMGN-style: fill every interval, open[i] = close[i-1], flat bars when no trades. */
function buildContinuousBars(
  buckets: Map<number, TradeBucket>,
  startKey: number,
  endKey: number,
  step: number,
): OhlcBar[] {
  const bars: OhlcBar[] = [];
  let prevClose: number | null = null;
  let lastLiquidity: number | null = null;

  for (let key = startKey; key <= endKey; key += step) {
    const bucket = buckets.get(key);
    const prices = bucket?.prices ?? [];

    if (prices.length > 0) {
      const open = prevClose ?? prices[0];
      const close = prices[prices.length - 1];
      const high = Math.max(open, ...prices);
      const low = Math.min(open, ...prices);
      if (bucket!.liquiditySol != null) lastLiquidity = bucket!.liquiditySol;

      bars.push({
        time: key as UTCTimestamp,
        open,
        high,
        low,
        close,
        volume: bucket!.volume,
        liquiditySol: lastLiquidity,
      });
      prevClose = close;
    } else if (prevClose != null) {
      bars.push({
        time: key as UTCTimestamp,
        open: prevClose,
        high: prevClose,
        low: prevClose,
        close: prevClose,
        volume: 0,
        liquiditySol: lastLiquidity,
      });
    }
  }

  return bars;
}

function collectTradeBuckets(
  trades: ChartTrade[],
  bucketKey: (trade: ChartTrade) => number | null,
  toValue: (priceInSol: number) => number,
  metric: ChartMetric,
): Map<number, TradeBucket> {
  const buckets = new Map<number, TradeBucket>();

  for (const trade of trades) {
    const key = bucketKey(trade);
    if (key == null) continue;

    const value = chartValueForTrade(trade, metric);
    if (value == null) continue;

    const price = toValue(value);
    const vol = trade.sol_amount ?? 1;
    const liquiditySol = curveLiquiditySol(trade);
    const existing = buckets.get(key);

    if (existing) {
      existing.prices.push(price);
      existing.volume += vol;
      if (liquiditySol != null) existing.liquiditySol = liquiditySol;
    } else {
      buckets.set(key, {
        prices: [price],
        volume: vol,
        liquiditySol,
      });
    }
  }

  return buckets;
}

export function aggregateTradesToBars(
  trades: ChartTrade[],
  intervalSec: number,
  toValue: (priceInSol: number) => number,
  metric: ChartMetric,
): OhlcBar[] {
  if (trades.length === 0) return [];

  const sorted = [...trades].sort(
    (a, b) => Date.parse(a.block_time) - Date.parse(b.block_time),
  );

  const buckets = collectTradeBuckets(
    sorted,
    (trade) => {
      const sec = tradeTimestampSec(trade.block_time);
      return sec == null ? null : bucketStart(sec, intervalSec);
    },
    toValue,
    metric,
  );

  if (buckets.size === 0) return [];

  const keys = [...buckets.keys()].sort((a, b) => a - b);
  return buildContinuousBars(buckets, keys[0], keys[keys.length - 1], intervalSec);
}

export function aggregateTradesToBarsBySlot(
  trades: ChartTrade[],
  toValue: (priceInSol: number) => number,
  metric: ChartMetric,
): OhlcBar[] {
  if (trades.length === 0) return [];

  const sorted = [...trades].sort((a, b) => {
    const slotDiff = (a.slot ?? 0) - (b.slot ?? 0);
    if (slotDiff !== 0) return slotDiff;
    return Date.parse(a.block_time) - Date.parse(b.block_time);
  });

  const buckets = collectTradeBuckets(
    sorted,
    (trade) => trade.slot ?? null,
    toValue,
    metric,
  );

  if (buckets.size === 0) return [];

  const keys = [...buckets.keys()].sort((a, b) => a - b);
  return buildContinuousBars(buckets, keys[0], keys[keys.length - 1], 1);
}

export function barsToLineData(bars: OhlcBar[]) {
  return bars.map((b) => ({ time: b.time, value: b.close }));
}

export function barsToCandleData(bars: OhlcBar[]) {
  return bars.map((b) => ({
    time: b.time,
    open: b.open,
    high: b.high,
    low: b.low,
    close: b.close,
  }));
}
