import {
  MIN_CHART_SOL,
  chartValueForTrade,
  tradeBarSlot,
  tradeBarTime,
} from './chartBars';
import type { ChartGroupMode, ChartLensItem, ChartMetric, ChartTrade, OhlcBar } from './types';

/**
 * One bar a highlight lens hits — "the thing you picked traded in this candle".
 *
 * Every hit draws at one strength, so a lens reads as one flat color and several
 * armed lenses stay apart by hue alone. How much it moved is the lane mark's height
 * and the size labels, never the wash.
 */
export interface LensBarTint {
  /** Bar key — bucket-start seconds in time mode, slot number in slot mode. */
  barTime: number;
  /** The matched trades in this bar, in chart order — what the size labels under
   *  the wash and the hover list print, so neither can disagree with the wash. */
  trades: readonly ChartTrade[];
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
 * The dust/validity guards here MIRROR `collectTradeBuckets` on purpose: a trade
 * the bar itself dropped would paint a hit, a size label and a SOL total on a
 * candle that never held it. A bar key the chart has no bar for is skipped rather
 * than drawn at a coordinate the reader can't check against a candle.
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

  const barTimes = new Set<number>();
  for (const bar of bars) barTimes.add(bar.time as number);

  const matchedTrades = new Map<number, ChartTrade[]>();
  let buys = 0;
  let sells = 0;
  let buySol = 0;
  let sellSol = 0;

  for (const trade of trades) {
    if (trade.amount_sol != null && trade.amount_sol < MIN_CHART_SOL) continue;
    if (chartValueForTrade(trade, metric) == null) continue;
    if (!matches(trade)) continue;

    const key = lensBarKey(trade, groupMode, intervalSec);
    if (key == null || !barTimes.has(key)) continue;

    // `?? 1` mirrors the bucket math: a row with no recorded SOL still counts as
    // one unit of volume there, so the SOL totals stay in the same currency.
    const sol = trade.amount_sol ?? 1;
    const inBar = matchedTrades.get(key);
    if (inBar) inBar.push(trade);
    else matchedTrades.set(key, [trade]);
    if (trade.trade_type === 'buy') {
      buys += 1;
      buySol += sol;
    } else {
      sells += 1;
      sellSol += sol;
    }
  }

  const tint: LensBarTint[] = [];
  for (const [barTime, inBar] of matchedTrades) tint.push({ barTime, trades: inBar });
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

/** A lens item's identity — the key of its entry in `ChartLensMatches`. */
export function lensItemId(item: Pick<ChartLensItem, 'kind' | 'key'>): string {
  return `${item.kind}:${item.key}`;
}

/** Does `trade` belong to this lens item? Structure identity is ordered, exact and
 *  whole-sequence — the key `volumePatterns.patternKey` builds, inlined so this
 *  folder stays portable. A set/subset match here would silently mean something
 *  else than it does everywhere else in the app. */
export function lensItemMatches(
  item: Pick<ChartLensItem, 'kind' | 'key'>,
  trade: ChartTrade,
): boolean {
  if (item.kind === 'wallet') return trade.wallet_address === item.key;
  const labels = trade.instruction_labels;
  return !!labels && labels.length > 0 && JSON.stringify(labels) === item.key;
}

/** A matched trade's SOL size as the chart prints it under a wash: 3 decimals
 *  below 1 SOL, 2 up to 100, whole SOL above. */
export function formatLensSol(sol: number): string {
  const a = Math.abs(sol);
  return a >= 100 ? a.toFixed(0) : a >= 1 ? a.toFixed(2) : a.toFixed(3);
}
