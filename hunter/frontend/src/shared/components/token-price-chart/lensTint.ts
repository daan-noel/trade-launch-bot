import {
  MIN_CHART_SOL,
  chartValueForTrade,
  tradeBarSlot,
  tradeBarTime,
} from './chartBars';
import type { ChartGroupMode, ChartMetric, ChartTrade, OhlcBar } from './types';

/**
 * Per-bar strength of a highlight lens — "how much of this candle is the thing
 * you picked".
 *
 * A candle is almost never 100% one wallet or one ix structure, so a binary tint
 * overstates every bar it paints. The share is what makes the overlay honest:
 * one dust leg in a busy slot renders faint, a slot the target owns renders solid.
 */
export interface LensBarTint {
  /** Bar key — bucket-start seconds in time mode, slot number in slot mode. */
  barTime: number;
  /** Matched SOL / bar SOL, clamped to [0, 1]. */
  share: number;
}

/** What one lens found across the token's whole history. */
export interface LensMatch {
  tint: LensBarTint[];
  buys: number;
  sells: number;
  buySol: number;
  sellSol: number;
  /** Bar keys carrying a match, ascending — first/last are the lens' extent. */
  firstBarTime: number | null;
  lastBarTime: number | null;
}

export const EMPTY_LENS_MATCH: LensMatch = {
  tint: [],
  buys: 0,
  sells: 0,
  buySol: 0,
  sellSol: 0,
  firstBarTime: null,
  lastBarTime: null,
};

/** The bar a trade lands in, keyed exactly the way the chart buckets it. */
function lensBarKey(
  trade: ChartTrade,
  groupMode: ChartGroupMode,
  intervalSec: number,
): number | null {
  const key =
    groupMode === 'slot'
      ? tradeBarSlot(trade)
      : tradeBarTime(trade.block_time, intervalSec);
  return key == null ? null : (key as number);
}

/**
 * Bucket the trades a lens matches against the bars already on the chart.
 *
 * The dust/validity guards here MIRROR `collectTradeBuckets` on purpose: the
 * denominator is `OhlcBar.volume`, so counting a trade the bar itself dropped
 * would paint a share above 1 on a candle that never held it. A bar key the chart
 * has no bar for is skipped rather than drawn at a coordinate the reader can't
 * check against a candle.
 */
export function buildLensMatch(
  trades: readonly ChartTrade[],
  bars: readonly OhlcBar[],
  groupMode: ChartGroupMode,
  intervalSec: number,
  metric: ChartMetric,
  matches: (trade: ChartTrade) => boolean,
): LensMatch {
  if (bars.length === 0) return EMPTY_LENS_MATCH;

  const barVolume = new Map<number, number>();
  for (const bar of bars) barVolume.set(bar.time as number, bar.volume);

  const matchedSol = new Map<number, number>();
  let buys = 0;
  let sells = 0;
  let buySol = 0;
  let sellSol = 0;

  for (const trade of trades) {
    if (trade.amount_sol != null && trade.amount_sol < MIN_CHART_SOL) continue;
    if (chartValueForTrade(trade, metric) == null) continue;
    if (!matches(trade)) continue;

    const key = lensBarKey(trade, groupMode, intervalSec);
    if (key == null || !barVolume.has(key)) continue;

    // `?? 1` mirrors the bucket math: a row with no recorded SOL still counts as
    // one unit of volume there, so the ratio stays in the same currency.
    const sol = trade.amount_sol ?? 1;
    matchedSol.set(key, (matchedSol.get(key) ?? 0) + sol);
    if (trade.trade_type === 'buy') {
      buys += 1;
      buySol += sol;
    } else {
      sells += 1;
      sellSol += sol;
    }
  }

  const tint: LensBarTint[] = [];
  for (const [barTime, sol] of matchedSol) {
    const denom = barVolume.get(barTime) ?? 0;
    tint.push({ barTime, share: denom > 0 ? Math.min(1, sol / denom) : 1 });
  }
  tint.sort((a, b) => a.barTime - b.barTime);

  return {
    tint,
    buys,
    sells,
    buySol,
    sellSol,
    firstBarTime: tint.length > 0 ? tint[0].barTime : null,
    lastBarTime: tint.length > 0 ? tint[tint.length - 1].barTime : null,
  };
}
