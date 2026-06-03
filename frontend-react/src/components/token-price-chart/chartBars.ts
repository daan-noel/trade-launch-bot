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

export function tradesForChartMetric(
  trades: ChartTrade[],
  metric: ChartMetric,
): ChartTrade[] {
  const sorted = [...trades].sort(
    (a, b) => Date.parse(a.block_time) - Date.parse(b.block_time),
  );

  return sorted.flatMap((trade) => {
    const value =
      metric === 'price' ? curveSpotPriceSol(trade) : curveMarketCapSol(trade);
    if (value == null) return [];
    return [{ ...trade, price_per_token: value }];
  });
}

export function aggregateTradesToBars(
  trades: ChartTrade[],
  intervalSec: number,
  toValue: (priceInSol: number) => number,
): OhlcBar[] {
  if (trades.length === 0) return [];

  const buckets = new Map<
    number,
    { open: number; high: number; low: number; close: number; volume: number }
  >();

  const sorted = [...trades].sort(
    (a, b) => Date.parse(a.block_time) - Date.parse(b.block_time),
  );

  for (const trade of sorted) {
    const sec = tradeTimestampSec(trade.block_time);
    if (sec == null) continue;

    const time = bucketStart(sec, intervalSec);
    const price = toValue(trade.price_per_token);
    const vol = trade.sol_amount ?? 1;

    const existing = buckets.get(time);
    if (existing) {
      existing.high = Math.max(existing.high, price);
      existing.low = Math.min(existing.low, price);
      existing.close = price;
      existing.volume += vol;
    } else {
      buckets.set(time, {
        open: price,
        high: price,
        low: price,
        close: price,
        volume: vol,
      });
    }
  }

  return Array.from(buckets.entries())
    .sort(([a], [b]) => a - b)
    .map(([time, bar]) => ({
      time: time as UTCTimestamp,
      ...bar,
    }));
}

export function aggregateTradesToBarsBySlot(
  trades: ChartTrade[],
  toValue: (priceInSol: number) => number,
): OhlcBar[] {
  if (trades.length === 0) return [];

  const buckets = new Map<
    number,
    { open: number; high: number; low: number; close: number; volume: number }
  >();

  const sorted = [...trades].sort((a, b) => {
    const slotDiff = (a.slot ?? 0) - (b.slot ?? 0);
    if (slotDiff !== 0) return slotDiff;
    return Date.parse(a.block_time) - Date.parse(b.block_time);
  });

  for (const trade of sorted) {
    const slot = trade.slot;
    if (slot == null) continue;

    const price = toValue(trade.price_per_token);
    const vol = trade.sol_amount ?? 1;

    const existing = buckets.get(slot);
    if (existing) {
      existing.high = Math.max(existing.high, price);
      existing.low = Math.min(existing.low, price);
      existing.close = price;
      existing.volume += vol;
    } else {
      buckets.set(slot, {
        open: price,
        high: price,
        low: price,
        close: price,
        volume: vol,
      });
    }
  }

  return Array.from(buckets.entries())
    .sort(([a], [b]) => a - b)
    .map(([slot, bar]) => ({
      time: slot as UTCTimestamp,
      ...bar,
    }));
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
