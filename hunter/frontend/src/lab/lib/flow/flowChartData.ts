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
 *  (carry-forward at every observed bucket) so they always step together. */
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

  const byKey = new Map<number, { vol: number; nonVol: number }>();
  let volCum = 0;
  let nonVolCum = 0;
  for (const t of classified) {
    const key =
      groupMode === 'slot' ? tradeBarSlot(t.raw) : tradeBarTime(t.raw.block_time, intervalSec);
    if (key == null) continue;
    const amt = basisAmount(t.raw, basis);
    if (t.isVol) volCum += amt;
    else nonVolCum += amt;
    byKey.set(key as number, { vol: volCum, nonVol: nonVolCum });
  }

  const keys = [...byKey.keys()].sort((a, b) => a - b);
  const vol = keys.map((k) => ({ time: k as UTCTimestamp, value: byKey.get(k)!.vol }));
  const nonVol = keys.map((k) => ({ time: k as UTCTimestamp, value: byKey.get(k)!.nonVol }));
  return { vol, nonVol };
}
