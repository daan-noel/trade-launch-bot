import type { SwingLegRecord } from '../../types';

/**
 * "Chain of Swings" stats for a single token.
 *
 * The detector emits strictly-alternating legs (swing_high, swing_low, …). A
 * *swing pair* is a swing_high immediately followed by a swing_low — one
 * complete up-then-down cycle. A leading lone-low or trailing lone-high leg is
 * not part of any pair and is skipped.
 *
 * Pairs are atomic and walked in time order. Two consecutive pairs are *linked*
 * when the idle gap between them (`next.startAt - current.endAt`, i.e. the next
 * pair's high start minus the current pair's low end) is within the
 * chain-latency budget. A *chain* is a maximal run of ≥ 2 linked pairs; an
 * isolated pair is not a chain.
 */
export interface SwingChainStats {
  /** Number of swing legs detected for the token. */
  swingCount: number;
  /** Number of high→low swing pairs (complete up-then-down cycles). */
  totalPairCount: number;
  /** Pair count of the largest chain (≥ 2 linked pairs); 0 if none link. */
  maxSequentialPairCount: number;
  /** Number of chains (runs of ≥ 2 pairs linked within the latency budget). */
  chainCount: number;
}

/** One high→low swing pair, spanning the up-leg start through the down-leg end. */
interface SwingPair {
  startAt: number;
  endAt: number;
}

/**
 * Reduce a token's alternating legs to high→low pairs, in time order. Unpaired
 * legs (a leading swing_low or trailing swing_high) are skipped.
 */
function toSwingPairs(swings: SwingLegRecord[]): SwingPair[] {
  const sorted = [...swings].sort((a, b) => a.start_at - b.start_at);
  const pairs: SwingPair[] = [];
  let i = 0;
  while (i < sorted.length) {
    const high = sorted[i];
    const low = sorted[i + 1];
    if (high.type === 'swing_high' && low?.type === 'swing_low') {
      pairs.push({ startAt: high.start_at, endAt: low.end_at });
      i += 2; // consume the pair
    } else {
      i += 1; // unpaired leg — skip
    }
  }
  return pairs;
}

/**
 * Group a token's high→low pairs into chains and summarise them.
 * `chainLatencyMs` is the maximum idle gap (ms) between two consecutive pairs
 * for them to stay in the same chain.
 */
export function computeChainStats(
  swings: SwingLegRecord[],
  chainLatencyMs: number,
): SwingChainStats {
  const pairs = toSwingPairs(swings);
  const m = pairs.length;

  let maxRun = 0; // pairs in the largest chain (≥ 2 linked pairs)
  let curRun = 0; // pairs in the currently-open chain (0 = none open)
  let chainCount = 0;

  for (let k = 0; k + 1 < m; k++) {
    const gap = pairs[k + 1].startAt - pairs[k].endAt;
    if (gap <= chainLatencyMs) {
      if (curRun === 0) {
        curRun = 2; // this link joins pairs k and k+1 — a new chain opens
        chainCount += 1;
      } else {
        curRun += 1; // extend the chain by one more pair
      }
      if (curRun > maxRun) maxRun = curRun;
    } else {
      curRun = 0; // gap too large — chain breaks here
    }
  }

  return {
    swingCount: swings.length,
    totalPairCount: m,
    maxSequentialPairCount: maxRun,
    chainCount,
  };
}
