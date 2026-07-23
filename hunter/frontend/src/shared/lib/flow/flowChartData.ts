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
  vol: FlowLinePoint[];
  nonVol: FlowLinePoint[];
}

/** Per-trade signed magnitude: buys add, sells subtract. */
function signedAmount(trade: ChartTrade, field: 'amount_sol' | 'token_amount'): number {
  const mag = Math.abs(trade[field] ?? 0);
  return trade.trade_type === 'buy' ? mag : -mag;
}

interface FlowBucket {
  volSol: number;
  volTok: number;
  nonVolSol: number;
  nonVolTok: number;
  spot: number | null;
}

/** Cumulative vol/non-vol series over one token's trades. */
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
    const spot = tradeSpotPriceSol(raw);
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
    } else if (basis === 'value_sol') {
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

/**
 * Forward-fill cumulative flow onto every candle bar time so both series share
 * identical X keys (needed when Trim Gaps is off).
 */
export function alignFlowToBars(
  lines: FlowLines,
  bars: readonly OhlcBar[],
): FlowLines {
  if (bars.length === 0) return { vol: [], nonVol: [] };
  const vol: FlowLinePoint[] = [];
  const nonVol: FlowLinePoint[] = [];
  let i = 0;
  let lastVol = 0;
  let lastNon = 0;
  for (const bar of bars) {
    const t = bar.time as number;
    while (i < lines.vol.length && (lines.vol[i].time as number) <= t) {
      lastVol = lines.vol[i].value;
      lastNon = lines.nonVol[i].value;
      i += 1;
    }
    vol.push({ time: bar.time, value: lastVol });
    nonVol.push({ time: bar.time, value: lastNon });
  }
  return { vol, nonVol };
}

/** Vol/non-vol overlay line colors (match Flow Discovery preview). */
export const FLOW_VOL_LINE_COLOR = '#EF5350';
export const FLOW_NON_VOL_LINE_COLOR = '#F5C542';

/** lightweight-charts rejects values outside ±9.007e13; token basis is divided. */
export const TOKEN_FLOW_SERIES_SCALE = 1e6;

export function flowSeriesScale(basis: FlowBasis): number {
  return basis === 'token' ? TOKEN_FLOW_SERIES_SCALE : 1;
}

/** Find the vol/non-vol point matching a bar's time key (both arrays share
 *  the exact same time sequence — see {@link buildFlowLines} / {@link alignFlowToBars}). */
export function flowAt(
  lines: FlowLines,
  time: Time,
): { vol: number | null; nonVol: number | null } {
  const idx = lines.vol.findIndex((p) => p.time === time);
  if (idx === -1) return { vol: null, nonVol: null };
  return { vol: lines.vol[idx].value, nonVol: lines.nonVol[idx].value };
}

/** Compact token count with a trillions tier — {@link formatCompact} caps at G,
 *  but cumulative token counts reach 1e14+. */
export function formatFlowTokenCount(v: number): string {
  const abs = Math.abs(v);
  if (abs >= 1e12) return `${formatDecimalTrim(v / 1e12, 2)}T`;
  return formatCompact(v, 2);
}
