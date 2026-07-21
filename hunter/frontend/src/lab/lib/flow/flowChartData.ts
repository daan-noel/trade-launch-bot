import type { UTCTimestamp } from 'lightweight-charts';
import {
  compareTradesChronologically,
  tradeBarSlot,
  tradeBarTime,
} from 'components/token-price-chart/chartBars';
import type { ChartGroupMode } from 'components/token-price-chart/types';
import type { TradeRecord } from 'types';
import { classifyFlowTrades, type FlowClassifyOptions } from './classifyFlow';

/** Cumulative-line basis — mirrors a subset of the reference tool's SOL/Token
 *  balance tabs. (A third "real SOL" basis was dropped: `TradeRecord.amount_sol`
 *  is already the exact real lamport amount per `amount-type-by-meaning` — there
 *  is no separate virtual-vs-real trade-amount distinction to chart here.) */
export type FlowBasis = 'sol' | 'token';

export interface FlowLinePoint {
  time: UTCTimestamp;
  value: number;
}

export interface FlowLines {
  vol: FlowLinePoint[];
  nonVol: FlowLinePoint[];
}

function basisAmount(trade: TradeRecord, basis: FlowBasis): number {
  return Math.abs(basis === 'token' ? trade.token_amount : trade.amount_sol);
}

/** Cumulative vol/non-vol series over one token's trades, classified via the
 *  {@link classifyFlowTrades} preview. Both lines share one time axis
 *  (carry-forward at every observed bucket) so they always step together.
 *
 *  Bucket totals are accumulated first (order-independent `+=`), THEN
 *  prefix-summed over buckets sorted by time — not a single running total
 *  written per-trade in processing order. A live/very-recent token can have
 *  trades whose `tx_index` hasn't been backfilled yet, which the shared
 *  `compareTradesChronologically` falls back to treating as `0`; that can
 *  put a trade slightly out of true time order within its bucket. Writing a
 *  *running* cumulative value keyed by processing order would let an
 *  out-of-order trade overwrite an earlier bucket with a smaller snapshot,
 *  drawing a line that visibly DROPS — impossible for a true cumulative sum.
 *  Per-bucket totals + a final prefix-sum are immune to that: every bucket's
 *  amount lands in the right place regardless of processing order, so the
 *  displayed series is monotonic by construction. */
export function buildFlowLines(
  trades: readonly TradeRecord[],
  groupMode: ChartGroupMode,
  intervalSec: number,
  basis: FlowBasis,
  classifyOpts: FlowClassifyOptions,
): FlowLines {
  const sorted = [...trades].sort(compareTradesChronologically);
  const classified = classifyFlowTrades(
    sorted.map((t) => ({
      wallet_address: t.wallet_address,
      sol: t.amount_sol,
      ix_labels: t.instruction_labels,
      raw: t,
    })),
    classifyOpts,
  );

  const volByBucket = new Map<number, number>();
  const nonVolByBucket = new Map<number, number>();
  for (const t of classified) {
    const key =
      groupMode === 'slot' ? tradeBarSlot(t.raw) : tradeBarTime(t.raw.block_time, intervalSec);
    if (key == null) continue;
    const k = key as number;
    const amt = basisAmount(t.raw, basis);
    const bucket = t.isVol ? volByBucket : nonVolByBucket;
    bucket.set(k, (bucket.get(k) ?? 0) + amt);
  }

  const keys = [...new Set([...volByBucket.keys(), ...nonVolByBucket.keys()])].sort(
    (a, b) => a - b,
  );
  const vol: FlowLinePoint[] = [];
  const nonVol: FlowLinePoint[] = [];
  let volCum = 0;
  let nonVolCum = 0;
  for (const k of keys) {
    volCum += volByBucket.get(k) ?? 0;
    nonVolCum += nonVolByBucket.get(k) ?? 0;
    vol.push({ time: k as UTCTimestamp, value: volCum });
    nonVol.push({ time: k as UTCTimestamp, value: nonVolCum });
  }
  return { vol, nonVol };
}
