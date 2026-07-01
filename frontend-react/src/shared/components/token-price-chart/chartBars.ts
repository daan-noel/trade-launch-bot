import type { SeriesMarker, UTCTimestamp } from 'lightweight-charts';
import {
  CHART_COLORS,
  PUMP_INITIAL_VIRTUAL_SOL,
  PUMP_MIGRATION_SPOT_PRICE_SOL,
  TOKEN_TOTAL_SUPPLY,
} from './constants';
import type {
  ChartGroupMode,
  ChartMetric,
  ChartRangeSelection,
  ChartRangeStats,
  ChartTrade,
  OhlcBar,
} from './types';

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

/**
 * Spot price from the venue-neutral reserve pair: reserve_sol / reserve_token
 * (GMGN-style). Curve virtual reserves on curve rows, pool real reserves on amm
 * rows — the backend stores both in the same `reserve_*` pair.
 */
export function curveSpotPriceSol(
  trade: Pick<ChartTrade, 'reserve_sol' | 'reserve_token'>,
): number | null {
  const sol = trade.reserve_sol;
  const token = trade.reserve_token;
  if (sol == null || token == null || token <= 0) return null;
  return sol / token;
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
  const postVsol = trade.reserve_sol ?? trade.real_sol_reserves;
  const postVtoken = trade.reserve_token ?? trade.real_token_reserves;
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

/**
 * Real SOL deposited in the bonding curve at the time of a trade.
 *
 * pump.fun seeds the curve with {@link PUMP_INITIAL_VIRTUAL_SOL} virtual SOL
 * before any depositor SOL exists, so the actual SOL locked in the pool is
 * `reserve_sol − initial virtual SOL`. This is the true liquidity (and the
 * graduation-progress figure). Clamped at 0; null when the snapshot is missing.
 */
export function curveLiquiditySol(
  trade: Pick<ChartTrade, 'reserve_sol'>,
): number | null {
  const vsol = trade.reserve_sol;
  if (vsol == null || vsol <= 0) return null;
  return Math.max(0, vsol - PUMP_INITIAL_VIRTUAL_SOL);
}

/**
 * Real SOL liquidity at a trade: post-migration AMM pool reserves when present,
 * else the curve's depositor SOL. The `trades` table only carries `virtual_*`
 * (no `real_*` columns), so the curve branch is the live path.
 */
export function tradeLiquiditySol(trade: ChartTrade): number | null {
  const sol = trade.real_sol_reserves;
  if (sol != null && sol > 0) return sol;
  return curveLiquiditySol(trade);
}

/** FDV in SOL: total supply × spot price (GMGN-style MC). */
export function curveMarketCapSol(
  trade: Pick<ChartTrade, 'reserve_sol' | 'reserve_token'>,
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
  inflow: number;
  outflow: number;
  liquiditySol: number | null;
};

/**
 * The one canonical trade order, used project-wide and identical to the backend
 * (DB `ORDER BY`, the Parquet lake, and the Rust swing scan): on-chain `slot`,
 * then `tx_index` (the real intra-block position of the tx), then `leg_index`
 * (leg order within the transaction). `block_time` is second-precision and only
 * breaks ties when `tx_index` is unavailable.
 *
 * `tx_index` reproduces true on-chain execution order for ~99% of same-slot pairs
 * across the DB, so it's the authoritative intra-slot key — no reserve-chain
 * reconstruction needed. (A tiny minority of early-backfilled tokens carry a
 * mis-stamped `tx_index`; those are fixed by a re-sync, not a read-time
 * workaround.)
 */
export function compareTradesChronologically(a: ChartTrade, b: ChartTrade): number {
  const slotDiff = (a.slot ?? 0) - (b.slot ?? 0);
  if (slotDiff !== 0) return slotDiff;
  const txDiff = (a.tx_index ?? 0) - (b.tx_index ?? 0);
  if (txDiff !== 0) return txDiff;
  const legDiff = (a.leg_index ?? 0) - (b.leg_index ?? 0);
  if (legDiff !== 0) return legDiff;
  return Date.parse(a.block_time) - Date.parse(b.block_time);
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
        inflow: bucket!.inflow,
        outflow: bucket!.outflow,
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
        inflow: 0,
        outflow: 0,
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
    const inflow = trade.trade_type === 'buy' ? vol : 0;
    const outflow = trade.trade_type === 'buy' ? 0 : vol;
    const liquiditySol = tradeLiquiditySol(trade);
    const existing = buckets.get(key);

    if (existing) {
      existing.prices.push(price);
      existing.volume += vol;
      existing.inflow += inflow;
      existing.outflow += outflow;
      if (liquiditySol != null) existing.liquiditySol = liquiditySol;
    } else {
      buckets.set(key, {
        prices: [price],
        volume: vol,
        inflow,
        outflow,
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

/**
 * Drop flat placeholder bars that {@link buildContinuousBars} synthesizes for
 * intervals with no trades. Real bars always carry `volume > 0`; empty ones are
 * flat at the previous close with `volume === 0`. Trimming them collapses the
 * gaps so only intervals that actually traded remain.
 */
export function dropEmptyBars(bars: OhlcBar[]): OhlcBar[] {
  return bars.filter((b) => b.volume > 0);
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

/**
 * Aggregate trade stats over a user-drawn range, snapped to bar bounds.
 *
 * `range.lo`/`range.hi` are chart-time bounds (bucket-start seconds in time
 * mode, slot numbers in slot mode); a trade is in range when its bar key falls
 * within `[lo, hi]`, so the numbers match exactly the bars the band highlights.
 * `trades` must be in chronological order (pass the chart's sorted trades) —
 * duration and price delta are read from the first/last in-range trade.
 */
export function computeRangeStats(
  trades: ChartTrade[],
  range: ChartRangeSelection,
  groupMode: ChartGroupMode,
  intervalSec: number,
): ChartRangeStats {
  const lo = Math.min(range.lo, range.hi);
  const hi = Math.max(range.lo, range.hi);

  let inflow = 0;
  let outflow = 0;
  let buyCount = 0;
  let sellCount = 0;
  let maxBuySol = 0;
  let maxSellSol = 0;
  let firstTrade: ChartTrade | null = null;
  let lastTrade: ChartTrade | null = null;
  const wallets = new Set<string>();
  const buyers = new Set<string>();
  const sellers = new Set<string>();

  for (const trade of trades) {
    const key =
      groupMode === 'slot'
        ? tradeBarSlot(trade)
        : tradeBarTime(trade.block_time, intervalSec);
    if (key == null) continue;
    const k = key as number;
    if (k < lo || k > hi) continue;

    const sol = trade.sol_amount ?? 0;
    if (trade.wallet_address) wallets.add(trade.wallet_address);
    if (trade.trade_type === 'buy') {
      inflow += sol;
      buyCount += 1;
      if (sol > maxBuySol) maxBuySol = sol;
      if (trade.wallet_address) buyers.add(trade.wallet_address);
    } else {
      outflow += sol;
      sellCount += 1;
      if (sol > maxSellSol) maxSellSol = sol;
      if (trade.wallet_address) sellers.add(trade.wallet_address);
    }

    if (firstTrade == null) firstTrade = trade;
    lastTrade = trade;
  }

  let durationMs = 0;
  let priceDelta = 0;
  let priceDeltaPct: number | null = null;
  if (firstTrade && lastTrade) {
    const startMs = Date.parse(firstTrade.block_time);
    const endMs = Date.parse(lastTrade.block_time);
    if (!Number.isNaN(startMs) && !Number.isNaN(endMs)) {
      durationMs = Math.max(0, endMs - startMs);
    }
    const startSpot = tradeSpotPriceSol(firstTrade);
    const endSpot = tradeSpotPriceSol(lastTrade);
    if (startSpot != null && endSpot != null) {
      priceDelta = endSpot - startSpot;
      if (startSpot > 0) priceDeltaPct = (priceDelta / startSpot) * 100;
    }
  }

  return {
    inflow,
    outflow,
    netFlow: inflow - outflow,
    tradeCount: buyCount + sellCount,
    buyCount,
    sellCount,
    uniqueWallets: wallets.size,
    uniqueBuyers: buyers.size,
    uniqueSellers: sellers.size,
    maxBuySol,
    maxSellSol,
    durationMs,
    priceDelta,
    priceDeltaPct,
  };
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
