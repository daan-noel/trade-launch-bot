import type { Time, UTCTimestamp } from 'lightweight-charts';
import {
  compareTradesChronologically,
  preTradeReserves,
  tradeBarSlot,
  tradeBarTime,
  tradeSpotPriceSol,
} from 'components/token-price-chart/chartBars';
import {
  PUMP_INITIAL_VIRTUAL_SOL,
  PUMP_INITIAL_VIRTUAL_TOKEN,
} from 'components/token-price-chart/constants';
import type { ChartGroupMode, ChartTrade, OhlcBar } from 'components/token-price-chart/types';
import { formatCompact, formatDecimalTrim } from 'utils/format';
import type { TradeRecord } from 'types';
import {
  classifyFlowTrades,
  flowReasonsById,
  type FlowClassifyOptions,
  type FlowReason,
  type FlowTradeLite,
} from './classifyFlow';

/** A chart trade as the classifier reads it - the ONE mapping, shared by the lines
 *  and the trades table's reasons so the two cannot read a field differently. */
export function toFlowTrade(t: ChartTrade): FlowTradeLite {
  return {
    wallet_address: t.wallet_address ?? '',
    sol: t.amount_sol ?? 0,
    ix_labels: t.instruction_labels,
    side: t.trade_type,
    slot: t.slot ?? null,
    cu_limit: t.cu_limit,
    cu_price: t.cu_price,
    tip_lamports: t.tip_lamports,
  };
}

/**
 * The verdict per trade id over a coin's FULL history, for the trades table - the
 * same pass the chart's lines draw from. `null` without options.
 */
export function tradeFlowReasons(
  trades: readonly TradeRecord[],
  opts: FlowClassifyOptions | null | undefined,
): ReadonlyMap<string, FlowReason> | null {
  if (!opts) return null;
  const sorted = [...trades].sort(compareTradesChronologically);
  return flowReasonsById(
    sorted.map((t) => ({ id: t.id, ...toFlowTrade(t) })),
    opts,
  );
}

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
  /** The cohort's running net, in the basis unit - what the legend reads. */
  value: number;
  /** Where the line is DRAWN: {@link cohortCurvePriceSol} of that net. */
  priceSol: number;
}

/** `tagged` = the trades carrying the tag (`@tag`); `untagged` = the rest (`@!tag`).
 *  A trade without a readable amount is on neither line.
 *
 *  Both lines live on the bonding curve only: they end at the first AMM trade
 *  (`endTime` = their last point), since the curve mapping does not hold on a
 *  pool. `anchor` is the curve before the chart's first trade, where both lines
 *  start - net 0 draws at the first candle's open. */
export interface FlowLines {
  tagged: FlowLinePoint[];
  untagged: FlowLinePoint[];
  anchor: CurveReserves;
  endTime: UTCTimestamp | null;
}

/** A bonding curve's virtual reserves: SOL and raw token units. */
export interface CurveReserves {
  sol: number;
  token: number;
}

const PUMP_GENESIS_CURVE: CurveReserves = {
  sol: PUMP_INITIAL_VIRTUAL_SOL,
  token: PUMP_INITIAL_VIRTUAL_TOKEN,
};

export const EMPTY_FLOW_LINES: FlowLines = {
  tagged: [],
  untagged: [],
  anchor: PUMP_GENESIS_CURVE,
  endTime: null,
};

/** The curve before the first chart trade - pump genesis when the first curve
 *  trade carries no reserve pair. */
function curveAnchor(sorted: readonly ChartTrade[]): CurveReserves {
  const first = sorted.find((t) => t.venue !== 'amm');
  return (first && preTradeReserves(first)) ?? PUMP_GENESIS_CURVE;
}

/**
 * Cohort curve price - SOL per raw token: the spot the bonding curve would sit at
 * if, from `anchor`, only this cohort had traded. Constant product
 * `k = anchor.sol × anchor.token`, so price = `vsol / vtoken` with
 *
 *  - SOL bases (`cost_sol`): `vsol = anchor.sol + netSol` ⇒ `(anchor.sol + netSol)² / k`
 *    (`amount_sol` is the curve-side leg, so this is exact on the curve);
 *  - token bases (`token`, `value_sol`): `vtoken = anchor.token − netToken`
 *    ⇒ `k / (anchor.token − netToken)²`.
 *
 * Net 0 is the anchor's own spot. This is what puts SOL flow and price on ONE
 * axis: the flow lines are drawn in the candles' unit, not on a second scale.
 * Example: anchor 30 SOL, a cohort net +10 SOL draws at (40/30)² = 1.78× the
 * start price; net −10 at (20/30)² = 0.44×. A reserve driven to 0 or below
 * clamps to price 0 (SOL) or has no price (token) - never a negative square.
 */
export function cohortCurvePriceSol(
  anchor: CurveReserves,
  basis: FlowBasis,
  netSol: number,
  netToken: number,
): number {
  const k = anchor.sol * anchor.token;
  if (basis === 'cost_sol') {
    const vsol = Math.max(0, anchor.sol + netSol);
    return (vsol * vsol) / k;
  }
  const vtoken = anchor.token - netToken;
  return vtoken > 0 ? k / (vtoken * vtoken) : Number.NaN;
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

/** Cumulative `@tag` / `@!tag` series over one token's trades. */
export function buildFlowLines(
  trades: readonly ChartTrade[],
  groupMode: ChartGroupMode,
  intervalSec: number,
  basis: FlowBasis,
  classifyOpts: FlowClassifyOptions,
): FlowLines {
  const sorted = [...trades].sort(compareTradesChronologically);
  const classified = classifyFlowTrades(
    sorted.map((t) => ({ ...toFlowTrade(t), raw: t })),
    classifyOpts,
  );

  const anchor = curveAnchor(sorted);
  const buckets = new Map<number, FlowBucket>();
  /** Bar key of the last curve print (any half) - the lines' end on a migrated coin. */
  let lastCurveKey: number | null = null;
  /** Bar key of the first AMM print, once one is seen. */
  let migrationKey: number | null = null;
  for (const t of classified) {
    const raw = t.raw;
    const key =
      groupMode === 'slot' ? tradeBarSlot(raw) : tradeBarTime(raw.block_time, intervalSec);
    // Migration is one-way and the order is canonical: the first AMM print ends
    // the curve, and with it both lines.
    if (raw.venue === 'amm') {
      migrationKey = (key as number | null) ?? Number.NEGATIVE_INFINITY;
      break;
    }
    if (key == null) continue;
    const k = key as number;
    lastCurveKey = lastCurveKey == null ? k : Math.max(lastCurveKey, k);
    if (t.half === 'excluded') continue;
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
    tagged.push({
      time: k as UTCTimestamp,
      value: taggedVal,
      priceSol: cohortCurvePriceSol(anchor, basis, taggedSol, taggedTok),
    });
    untagged.push({
      time: k as UTCTimestamp,
      value: untaggedVal,
      priceSol: cohortCurvePriceSol(anchor, basis, untaggedSol, untaggedTok),
    });
  }
  // No curve print at all before the AMM: end just before the migration bar.
  const endTime =
    migrationKey == null ? null : ((lastCurveKey ?? migrationKey - 1) as UTCTimestamp);
  return { tagged, untagged, anchor, endTime };
}

/**
 * Forward-fill cumulative flow onto every candle bar time so both series share
 * identical X keys (needed when Trim Gaps is off). Bars before the first point
 * sit at net 0 (the anchor's spot); bars after `endTime` get no point.
 */
export function alignFlowToBars(
  lines: FlowLines,
  bars: readonly OhlcBar[],
): FlowLines {
  if (bars.length === 0) return { ...lines, tagged: [], untagged: [] };
  const start = lines.anchor.sol / lines.anchor.token;
  const tagged: FlowLinePoint[] = [];
  const untagged: FlowLinePoint[] = [];
  let i = 0;
  let lastTagged: FlowLinePoint | null = null;
  let lastNon: FlowLinePoint | null = null;
  for (const bar of bars) {
    const t = bar.time as number;
    if (lines.endTime != null && t > (lines.endTime as number)) break;
    while (i < lines.tagged.length && (lines.tagged[i].time as number) <= t) {
      lastTagged = lines.tagged[i];
      lastNon = lines.untagged[i];
      i += 1;
    }
    tagged.push({ time: bar.time, value: lastTagged?.value ?? 0, priceSol: lastTagged?.priceSol ?? start });
    untagged.push({ time: bar.time, value: lastNon?.value ?? 0, priceSol: lastNon?.priceSol ?? start });
  }
  return { ...lines, tagged, untagged };
}

/** `@tag` / `@!tag` overlay line colors (match Flow Discovery preview). */
export const FLOW_VOL_LINE_COLOR = '#EF5350';
export const FLOW_NON_VOL_LINE_COLOR = '#F5C542';

/** Find the `@tag` / `@!tag` point matching a bar's time key (both arrays share
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
