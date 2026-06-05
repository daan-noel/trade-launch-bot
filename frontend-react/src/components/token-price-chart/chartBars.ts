import type { SeriesMarker, UTCTimestamp } from 'lightweight-charts';
import { CHART_COLORS, PUMP_MIGRATION_SPOT_PRICE_SOL, TOKEN_TOTAL_SUPPLY } from './constants';
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

/** Matches backend `MIN_TRADE_SOL` (10k lamports); safety net for pre-filter data. */
const MIN_CHART_SOL = 1e-5;

/** Bonding-curve spot price: virtual SOL / virtual tokens (GMGN-style). */
export function curveSpotPriceSol(
  trade: Pick<ChartTrade, 'virtual_sol_reserves' | 'virtual_token_reserves'>,
): number | null {
  const vsol = trade.virtual_sol_reserves;
  const vtoken = trade.virtual_token_reserves;
  if (vsol == null || vtoken == null || vtoken <= 0) return null;
  return vsol / vtoken;
}

/** PumpSwap pool spot: quote SOL / base tokens (post-migration). */
export function poolSpotPriceSol(
  trade: Pick<ChartTrade, 'real_sol_reserves' | 'real_token_reserves'>,
): number | null {
  const sol = trade.real_sol_reserves;
  const token = trade.real_token_reserves;
  if (sol == null || token == null || token <= 0) return null;
  return sol / token;
}

/**
 * GMGN-style spot for charting: curve virtual reserves, then pool reserves
 * (AMM), then execution price as last resort.
 */
export function tradeSpotPriceSol(trade: ChartTrade): number | null {
  const spot = curveSpotPriceSol(trade) ?? poolSpotPriceSol(trade);
  if (spot != null && spot > 0 && Number.isFinite(spot)) return spot;
  if (trade.price_per_token > 0 && Number.isFinite(trade.price_per_token)) {
    return trade.price_per_token;
  }
  return null;
}

/**
 * Curve/pool spot price (SOL per raw token) *before* this trade executed.
 *
 * The on-chain pump.fun `TradeEvent` only snapshots reserves *after* the swap,
 * so without this the very first bar would have to open at the post-first-trade
 * price — collapsing (and often inverting) the genesis candle. We recover the
 * pre-trade price from the post-trade reserves and the token delta via the
 * constant-product invariant `k = vsol·vtoken` (fee-independent, since pump.fun
 * fees never enter the virtual reserves). Returns null when the inputs are
 * missing or non-finite, so callers fall back to the post-trade price.
 */
export function preTradeSpotPriceSol(trade: ChartTrade): number | null {
  const postVsol = trade.virtual_sol_reserves ?? trade.real_sol_reserves;
  const postVtoken = trade.virtual_token_reserves ?? trade.real_token_reserves;
  const tokenAmount = trade.token_amount;
  if (
    postVsol == null ||
    postVtoken == null ||
    tokenAmount == null ||
    postVsol <= 0 ||
    postVtoken <= 0 ||
    tokenAmount <= 0
  ) {
    return null;
  }
  // Buys remove tokens from the curve; sells add them back.
  const preVtoken =
    trade.trade_type === 'buy'
      ? postVtoken + tokenAmount
      : postVtoken - tokenAmount;
  if (preVtoken <= 0) return null;
  const k = postVsol * postVtoken;
  const preSpot = k / (preVtoken * preVtoken);
  return preSpot > 0 && Number.isFinite(preSpot) ? preSpot : null;
}

/** Bonding-curve liquidity in SOL (GMGN-style: 2× virtual SOL reserves). */
export function curveLiquiditySol(
  trade: Pick<ChartTrade, 'virtual_sol_reserves'>,
): number | null {
  const vsol = trade.virtual_sol_reserves;
  if (vsol == null || vsol <= 0) return null;
  return vsol * 2;
}

/** Pool or curve liquidity in SOL (2× quote/virtual SOL reserves). */
export function tradeLiquiditySol(trade: ChartTrade): number | null {
  const sol = trade.virtual_sol_reserves ?? trade.real_sol_reserves;
  if (sol == null || sol <= 0) return null;
  return sol * 2;
}

/** FDV in SOL: total supply × spot price (GMGN-style MC). */
export function curveMarketCapSol(
  trade: Pick<ChartTrade, 'virtual_sol_reserves' | 'virtual_token_reserves'>,
): number | null {
  const spot = curveSpotPriceSol(trade);
  if (spot == null) return null;
  return TOKEN_TOTAL_SUPPLY * spot;
}

/** FDV in SOL from chart spot (curve, pool, or execution fallback). */
export function tradeMarketCapSol(trade: ChartTrade): number | null {
  const spot = tradeSpotPriceSol(trade);
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
  return metric === 'price' ? tradeSpotPriceSol(trade) : tradeMarketCapSol(trade);
}

/** Pre-trade chart value (price or FDV MC) — mirrors {@link chartValueForTrade}. */
function preTradeChartValue(trade: ChartTrade, metric: ChartMetric): number | null {
  const spot = preTradeSpotPriceSol(trade);
  if (spot == null) return null;
  return metric === 'price' ? spot : TOKEN_TOTAL_SUPPLY * spot;
}

/**
 * Display-space `open` to seed the very first bar: the pre-trade value of the
 * first trade that actually contributes to a bucket (same dust/validity filter
 * as {@link collectTradeBuckets}). Null when it can't be reconstructed, in which
 * case the genesis bar falls back to opening at the first trade's post-trade
 * price (the previous behaviour).
 */
function seedOpenValue(
  sortedTrades: ChartTrade[],
  metric: ChartMetric,
  toValue: (priceInSol: number) => number,
): number | null {
  for (const trade of sortedTrades) {
    if (trade.sol_amount != null && trade.sol_amount < MIN_CHART_SOL) continue;
    if (chartValueForTrade(trade, metric) == null) continue;
    const pre = preTradeChartValue(trade, metric);
    return pre == null ? null : toValue(pre);
  }
  return null;
}

/** Trades with `price_per_token` set to the chart metric (overlay / markers). */
export function tradesForChartMetric(
  trades: ChartTrade[],
  metric: ChartMetric,
): ChartTrade[] {
  const sorted = [...trades].sort(compareTradesChronologically);

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

/**
 * Deterministic chronological order for trades: on-chain `slot`, then block
 * time, then transaction signature (keeps a transaction's legs contiguous),
 * then `leg_index` (leg order within the transaction).
 *
 * Same-slot trades from *different* transactions share an identical
 * second-precision block time and carry no recoverable intra-block index, so
 * they fall back to signature order — deterministic, though not guaranteed to
 * match true block order. This still fixes the common case (multi-leg/bundled
 * transactions) where open/close were previously order-dependent.
 */
export function compareTradesChronologically(a: ChartTrade, b: ChartTrade): number {
  const slotDiff = (a.slot ?? 0) - (b.slot ?? 0);
  if (slotDiff !== 0) return slotDiff;
  const timeDiff = Date.parse(a.block_time) - Date.parse(b.block_time);
  if (timeDiff !== 0) return timeDiff;
  const sigA = a.tx_signature ?? '';
  const sigB = b.tx_signature ?? '';
  if (sigA !== sigB) return sigA < sigB ? -1 : 1;
  return (a.leg_index ?? 0) - (b.leg_index ?? 0);
}

function sortTradesChronologically(trades: ChartTrade[]): ChartTrade[] {
  return [...trades].sort(compareTradesChronologically);
}

/** GMGN-style: open = prev bar close (continuous spot); empty intervals flat at prev close. */
function buildContinuousBars(
  buckets: Map<number, TradeBucket>,
  startKey: number,
  endKey: number,
  step: number,
  seedOpen: number | null = null,
): OhlcBar[] {
  const bars: OhlcBar[] = [];
  // Seed the first bar's open with the pre-trade price (GMGN-style); every later
  // bar opens at the previous bar's close.
  let prevClose: number | null = seedOpen;
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
    if (trade.sol_amount != null && trade.sol_amount < MIN_CHART_SOL) continue;

    const key = bucketKey(trade);
    if (key == null) continue;

    const value = chartValueForTrade(trade, metric);
    if (value == null) continue;

    const price = toValue(value);
    const vol = trade.sol_amount ?? 1;
    const liquiditySol = tradeLiquiditySol(trade);
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

  const sorted = sortTradesChronologically(trades);

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
  const seedOpen = seedOpenValue(sorted, metric, toValue);
  return buildContinuousBars(
    buckets,
    keys[0],
    keys[keys.length - 1],
    intervalSec,
    seedOpen,
  );
}

export function aggregateTradesToBarsBySlot(
  trades: ChartTrade[],
  toValue: (priceInSol: number) => number,
  metric: ChartMetric,
): OhlcBar[] {
  if (trades.length === 0) return [];

  const sorted = sortTradesChronologically(trades);

  const buckets = collectTradeBuckets(
    sorted,
    (trade) => trade.slot ?? null,
    toValue,
    metric,
  );

  if (buckets.size === 0) return [];

  const keys = [...buckets.keys()].sort((a, b) => a - b);
  const seedOpen = seedOpenValue(sorted, metric, toValue);
  return buildContinuousBars(buckets, keys[0], keys[keys.length - 1], 1, seedOpen);
}

export function barsToLineData(bars: OhlcBar[]) {
  return bars.map((b) => ({ time: b.time, value: b.close }));
}

export function barsToCandleData(
  bars: OhlcBar[],
  highlightBarTimes?: ReadonlySet<number>,
) {
  const border = CHART_COLORS.barSelected;
  return bars.map((b) => {
    const candle = {
      time: b.time,
      open: b.open,
      high: b.high,
      low: b.low,
      close: b.close,
    };
    if (highlightBarTimes?.has(b.time as number)) {
      return { ...candle, borderColor: border };
    }
    return candle;
  });
}

/** Arrow marker for a selected bar (line + candle charts). */
export function barSelectionMarker(
  bar: OhlcBar,
  color: string = CHART_COLORS.barSelected,
): SeriesMarker<UTCTimestamp> {
  const bullish = bar.close >= bar.open;
  return {
    time: bar.time,
    position: bullish ? 'belowBar' : 'aboveBar',
    color,
    shape: bullish ? 'arrowUp' : 'arrowDown',
    size: 2,
  };
}
