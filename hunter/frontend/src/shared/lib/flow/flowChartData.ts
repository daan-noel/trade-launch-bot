import type { Time, UTCTimestamp } from 'lightweight-charts';
import {
  compareTradesChronologically,
  tradeBarSlot,
  tradeBarTime,
  tradeSpotPriceSol,
} from 'components/token-price-chart/chartBars';
import type { ChartGroupMode, ChartTrade, OhlcBar } from 'components/token-price-chart/types';
import { formatCompact, formatDecimalTrim } from 'utils/format';
import { classifyFlowTrades, type FlowClassifyOptions } from './classifyFlow';

/** Cumulative-line basis — each cohort's line is the running NET (buy − sell),
 *  not gross turnover, so a line legitimately drops when that cohort sells.
 *
 *  - `cost_sol`  — net SOL cash-flow: Σ(buy − sell) of `amount_sol`.
 *  - `token`     — net token balance: Σ(buy − sell) of `token_amount`.
 *  - `value_sol` — mark-to-market SOL value of that net token balance at the
 *                  candle-close canonical spot (`tradeSpotPriceSol`). */
export type FlowBasis = 'cost_sol' | 'token' | 'value_sol';

export interface FlowLinePoint {
  time: UTCTimestamp;
  value: number;
}

export interface FlowLines {
  tagged: FlowLinePoint[];
  untagged: FlowLinePoint[];
}

/** Per-trade signed magnitude: buys add, sells subtract. */
function signedAmount(trade: ChartTrade, field: 'amount_sol' | 'token_amount'): number {
  const mag = Math.abs(trade[field] ?? 0);
  return trade.trade_type === 'buy' ? mag : -mag;
}

interface FlowBucket {
  taggedSol: number;
  taggedTok: number;
  untaggedSol: number;
  untaggedTok: number;
  spot: number | null;
}

/** Cumulative tagged/non-tagged series over one token's trades. */
export function buildFlowLines(
  trades: readonly ChartTrade[],
  groupMode: ChartGroupMode,
  intervalSec: number,
  basis: FlowBasis,
  classifyOpts: FlowClassifyOptions,
): FlowLines {
  const sorted = [...trades].sort(compareTradesChronologically);
  const classified = classifyFlowTrades(
    sorted.map((t) => ({
      wallet_address: t.wallet_address ?? '',
      sol: t.amount_sol ?? 0,
      ix_labels: t.instruction_labels,
      side: t.trade_type,
      cu_limit: t.cu_limit,
      cu_price: t.cu_price,
      tip_lamports: t.tip_lamports,
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
      bucket = { taggedSol: 0, taggedTok: 0, untaggedSol: 0, untaggedTok: 0, spot: null };
      buckets.set(k, bucket);
    }
    const solDelta = signedAmount(raw, 'amount_sol');
    const tokDelta = signedAmount(raw, 'token_amount');
    if (t.isTagged) {
      bucket.taggedSol += solDelta;
      bucket.taggedTok += tokDelta;
    } else {
      bucket.untaggedSol += solDelta;
      bucket.untaggedTok += tokDelta;
    }
    const spot = tradeSpotPriceSol(raw);
    if (spot != null) bucket.spot = spot;
  }

  const keys = [...buckets.keys()].sort((a, b) => a - b);
  const tagged: FlowLinePoint[] = [];
  const untagged: FlowLinePoint[] = [];
  let taggedSol = 0;
  let taggedTok = 0;
  let untaggedSol = 0;
  let untaggedTok = 0;
  let lastSpot: number | null = null;
  for (const k of keys) {
    const bucket = buckets.get(k)!;
    taggedSol += bucket.taggedSol;
    taggedTok += bucket.taggedTok;
    untaggedSol += bucket.untaggedSol;
    untaggedTok += bucket.untaggedTok;
    if (bucket.spot != null) lastSpot = bucket.spot;

    let taggedVal: number;
    let untaggedVal: number;
    if (basis === 'token') {
      taggedVal = taggedTok;
      untaggedVal = untaggedTok;
    } else if (basis === 'value_sol') {
      const spot = lastSpot ?? 0;
      taggedVal = taggedTok * spot;
      untaggedVal = untaggedTok * spot;
    } else {
      taggedVal = taggedSol;
      untaggedVal = untaggedSol;
    }
    tagged.push({ time: k as UTCTimestamp, value: taggedVal });
    untagged.push({ time: k as UTCTimestamp, value: untaggedVal });
  }
  return { tagged, untagged };
}

/**
 * Forward-fill cumulative flow onto every candle bar time so both series share
 * identical X keys (needed when Trim Gaps is off).
 */
export function alignFlowToBars(
  lines: FlowLines,
  bars: readonly OhlcBar[],
): FlowLines {
  if (bars.length === 0) return { tagged: [], untagged: [] };
  const tagged: FlowLinePoint[] = [];
  const untagged: FlowLinePoint[] = [];
  let i = 0;
  let lastTagged = 0;
  let lastNon = 0;
  for (const bar of bars) {
    const t = bar.time as number;
    while (i < lines.tagged.length && (lines.tagged[i].time as number) <= t) {
      lastTagged = lines.tagged[i].value;
      lastNon = lines.untagged[i].value;
      i += 1;
    }
    tagged.push({ time: bar.time, value: lastTagged });
    untagged.push({ time: bar.time, value: lastNon });
  }
  return { tagged, untagged };
}

/** Vol/non-tagged overlay line colors (match Flow Discovery preview). */
export const FLOW_VOL_LINE_COLOR = '#EF5350';
export const FLOW_NON_VOL_LINE_COLOR = '#F5C542';

/** lightweight-charts rejects values outside ±9.007e13; token basis is divided. */
export const TOKEN_FLOW_SERIES_SCALE = 1e6;

export function flowSeriesScale(basis: FlowBasis): number {
  return basis === 'token' ? TOKEN_FLOW_SERIES_SCALE : 1;
}

/** Find the tagged/non-tagged point matching a bar's time key (both arrays share
 *  the exact same time sequence — see {@link buildFlowLines} / {@link alignFlowToBars}). */
export function flowAt(
  lines: FlowLines,
  time: Time,
): { tagged: number | null; untagged: number | null } {
  const idx = lines.tagged.findIndex((p) => p.time === time);
  if (idx === -1) return { tagged: null, untagged: null };
  return { tagged: lines.tagged[idx].value, untagged: lines.untagged[idx].value };
}

/** Compact token count with a trillions tier — {@link formatCompact} caps at G,
 *  but cumulative token counts reach 1e14+. */
export function formatFlowTokenCount(v: number): string {
  const abs = Math.abs(v);
  if (abs >= 1e12) return `${formatDecimalTrim(v / 1e12, 2)}T`;
  return formatCompact(v, 2);
}
