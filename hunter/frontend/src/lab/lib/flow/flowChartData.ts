import type { UTCTimestamp } from 'lightweight-charts';
import {
  compareTradesChronologically,
  poolSpotPriceSol,
  tradeBarSlot,
  tradeBarTime,
} from 'components/token-price-chart/chartBars';
import type { ChartGroupMode } from 'components/token-price-chart/types';
import type { TradeRecord } from 'types';
import { classifyFlowTrades, type FlowClassifyOptions } from './classifyFlow';

/** Cumulative-line basis — each cohort's line is the running NET (buy − sell),
 *  not gross turnover, so a line legitimately drops when that cohort sells.
 *
 *  - `net_sol`  — net SOL cash-flow: Σ(buy − sell) of `amount_sol`. What the
 *                 cohort has net *paid in*.
 *  - `token`    — net token balance: Σ(buy − sell) of `token_amount`. How many
 *                 tokens the cohort is net *holding*.
 *  - `real_sol` — mark-to-market SOL value of that net token balance, priced at
 *                 each candle's real-reserve spot (`poolSpotPriceSol` =
 *                 `real_sol_reserves / real_token_reserves`). What the cohort's
 *                 bag is currently *worth*, vs `net_sol` = what it cost. The gap
 *                 between the two lines is unrealized PnL. */
export type FlowBasis = 'net_sol' | 'token' | 'real_sol';

export interface FlowLinePoint {
  time: UTCTimestamp;
  value: number;
}

export interface FlowLines {
  vol: FlowLinePoint[];
  nonVol: FlowLinePoint[];
}

/** Per-trade signed magnitude: buys add, sells subtract. */
function signedAmount(trade: TradeRecord, field: 'amount_sol' | 'token_amount'): number {
  const mag = Math.abs(trade[field]);
  return trade.trade_type === 'buy' ? mag : -mag;
}

interface FlowBucket {
  volSol: number;
  volTok: number;
  nonVolSol: number;
  nonVolTok: number;
  /** Real-reserve spot of the LAST trade in the bucket (candle close). Null
   *  until a trade carrying real reserves lands in it. */
  spot: number | null;
}

/** Cumulative vol/non-vol series over one token's trades, classified via the
 *  {@link classifyFlowTrades} preview. Both lines share one time axis (a point
 *  at every observed bucket) so they always step together.
 *
 *  Per-bucket signed deltas are accumulated first (order-independent `+=`), THEN
 *  prefix-summed over buckets sorted by time. A live/very-recent token can have
 *  trades whose `tx_index` hasn't been backfilled yet, which
 *  `compareTradesChronologically` falls back to treating as `0`; that can put a
 *  trade slightly out of true time order within its bucket. Accumulating a
 *  per-bucket total and prefix-summing at the end is immune to that: every
 *  bucket's net amount lands in the right place regardless of processing order.
 *  (Unlike the old gross-flow version, the resulting line is NOT monotonic — a
 *  net balance falls when the cohort sells; that's the point.)
 *
 *  For `real_sol` the value at a candle is the cohort's cumulative net TOKEN
 *  balance × the candle-close real-reserve spot, carried forward across buckets
 *  with no reserve data. */
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

  const buckets = new Map<number, FlowBucket>();
  for (const t of classified) {
    const raw = t.raw;
    const key =
      groupMode === 'slot' ? tradeBarSlot(raw) : tradeBarTime(raw.block_time, intervalSec);
    if (key == null) continue;
    const k = key as number;
    let bucket = buckets.get(k);
    if (!bucket) {
      bucket = { volSol: 0, volTok: 0, nonVolSol: 0, nonVolTok: 0, spot: null };
      buckets.set(k, bucket);
    }
    const solDelta = signedAmount(raw, 'amount_sol');
    const tokDelta = signedAmount(raw, 'token_amount');
    if (t.isVol) {
      bucket.volSol += solDelta;
      bucket.volTok += tokDelta;
    } else {
      bucket.nonVolSol += solDelta;
      bucket.nonVolTok += tokDelta;
    }
    // Trades are processed in chronological order, so the last real-reserve
    // spot written wins → the candle-close price.
    const spot = poolSpotPriceSol(raw);
    if (spot != null) bucket.spot = spot;
  }

  const keys = [...buckets.keys()].sort((a, b) => a - b);
  const vol: FlowLinePoint[] = [];
  const nonVol: FlowLinePoint[] = [];
  let volSol = 0;
  let volTok = 0;
  let nonVolSol = 0;
  let nonVolTok = 0;
  let lastSpot: number | null = null;
  for (const k of keys) {
    const bucket = buckets.get(k)!;
    volSol += bucket.volSol;
    volTok += bucket.volTok;
    nonVolSol += bucket.nonVolSol;
    nonVolTok += bucket.nonVolTok;
    if (bucket.spot != null) lastSpot = bucket.spot;

    let volVal: number;
    let nonVolVal: number;
    if (basis === 'token') {
      volVal = volTok;
      nonVolVal = nonVolTok;
    } else if (basis === 'real_sol') {
      const spot = lastSpot ?? 0;
      volVal = volTok * spot;
      nonVolVal = nonVolTok * spot;
    } else {
      volVal = volSol;
      nonVolVal = nonVolSol;
    }
    vol.push({ time: k as UTCTimestamp, value: volVal });
    nonVol.push({ time: k as UTCTimestamp, value: nonVolVal });
  }
  return { vol, nonVol };
}
