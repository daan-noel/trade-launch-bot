/**
 * Temporal summary fold — hold-duration × exit mix + wall-clock entry/create
 * heatmap. Shared by Simulate (server bins mirror this shape) and the
 * grouped-sweep combo drill-in (client fold over ComboTokenResult rows).
 *
 * Keep hold-bin edges in sync with `lab::strategies::sim_query::time_summary`
 * (Rust). Integer-second bins so `holding` col-filters (`15..59`) round-trip.
 */

import { EXIT_KINDS, type ExitCountKey } from './runSummary';

/** One fired (or open) outcome with the times the temporal fold needs. */
export interface TemporalRow {
  mint_address: string;
  fired: boolean;
  /** Exit reason string (`TakeProfit`, `Open`, …). */
  exit: string;
  pnl_sol: number;
  /** 0 when open / not fired. */
  holding_secs: number;
  entry_time: string | null;
  created_at: string | null;
}

export type WallTimeField = 'entry_time' | 'created_at';

export type TemporalMetric = 'exit_mix' | 'pnl';

/** Hold-bin edges in seconds — inclusive lo, inclusive hi (`null` hi = open-ended). */
export const HOLD_BINS: ReadonlyArray<{
  id: string;
  label: string;
  lo: number | null;
  hi: number | null;
  /** Still-open positions (not a hold-duration bucket). */
  isOpen?: boolean;
}> = [
  { id: 'lt15s', label: '<15s', lo: 0, hi: 14 },
  { id: '15to60s', label: '15–60s', lo: 15, hi: 59 },
  { id: '1to5m', label: '1–5m', lo: 60, hi: 299 },
  { id: '5to30m', label: '5–30m', lo: 300, hi: 1799 },
  { id: '30mPlus', label: '30m+', lo: 1800, hi: null },
  { id: 'open', label: 'Open', lo: null, hi: null, isOpen: true },
];

/** Exit reason → stack segment key (mirrors `EXIT_KEY_BY_REASON` / Rust ExitCode). */
const EXIT_REASON_KEY: Readonly<Record<string, ExitCountKey>> = {
  TakeProfit: 'n_exit_take_profit',
  StopLoss: 'n_exit_stop_loss',
  Metrics: 'n_exit_metrics',
  Dead: 'n_exit_dead',
  Manual: 'n_exit_manual',
  TrailingStop: 'n_exit_trailing',
  Stall: 'n_exit_stall',
  TimeStop: 'n_exit_time',
  LiquidityExit: 'n_exit_liquidity',
  NextKill: 'n_exit_next_kill',
};

export interface HoldBinStats {
  id: string;
  label: string;
  n: number;
  pnl_sol: number;
  /** Counts keyed by `ExitCountKey` (+ `other` / `open`). */
  exits: Record<string, number>;
  /** Col-filter text for the `holding` column, or null for the Open bin. */
  holdingFilter: string | null;
  /** When set, filter `exit`/`reason` instead of holding (Open bin). */
  exitFilter: string | null;
  /** Mints in this bin — used for click→table filter (`mint_address in`). */
  mints: string[];
}

export interface WallCellStats {
  id: string;
  /** Bucket start (ISO). */
  start: string;
  /** Bucket end exclusive (ISO). */
  end: string;
  n: number;
  pnl_sol: number;
  win_rate: number;
  exits: Record<string, number>;
  /** Dominant exit label for the cell glyph, or null when empty. */
  dominant: string | null;
  /** Mints in this cell — click→table filter. */
  mints: string[];
}

export interface TemporalSummaryData {
  hold: HoldBinStats[];
  wall: WallCellStats[];
  /** `hour` when span ≤ 72h, else `day`. */
  wallGrain: 'hour' | 'day';
  wallField: WallTimeField;
  /** Fired rows that contributed (excludes NoEntry). */
  nFired: number;
}

function emptyExits(): Record<string, number> {
  const o: Record<string, number> = { other: 0, open: 0 };
  for (const k of EXIT_KINDS) o[k.key] = 0;
  return o;
}

function tallyExit(exits: Record<string, number>, reason: string): void {
  if (reason === 'Open') {
    exits.open = (exits.open ?? 0) + 1;
    return;
  }
  const key = EXIT_REASON_KEY[reason];
  if (key) exits[key] = (exits[key] ?? 0) + 1;
  else exits.other = (exits.other ?? 0) + 1;
}

function holdBinId(row: TemporalRow): string | null {
  if (!row.fired) return null;
  if (row.exit === 'Open') return 'open';
  const s = row.holding_secs;
  if (!Number.isFinite(s) || s < 0) return 'open';
  for (const b of HOLD_BINS) {
    if (b.isOpen) continue;
    if (b.lo == null) continue;
    if (s < b.lo) continue;
    if (b.hi != null && s > b.hi) continue;
    return b.id;
  }
  return '30mPlus';
}

function holdingFilterFor(bin: (typeof HOLD_BINS)[number]): string | null {
  if (bin.isOpen) return null;
  if (bin.lo == null) return null;
  if (bin.hi == null) return `>=${bin.lo}`;
  return `${bin.lo}..${bin.hi}`;
}

/** Parse an RFC3339 / ISO timestamp → ms, or null. */
export function parseTsMs(iso: string | null | undefined): number | null {
  if (!iso) return null;
  const t = Date.parse(iso);
  return Number.isFinite(t) ? t : null;
}

function floorToGrain(ms: number, grain: 'hour' | 'day'): number {
  const d = new Date(ms);
  if (grain === 'day') {
    d.setUTCHours(0, 0, 0, 0);
  } else {
    d.setUTCMinutes(0, 0, 0);
  }
  return d.getTime();
}

function stepMs(grain: 'hour' | 'day'): number {
  return grain === 'hour' ? 3_600_000 : 86_400_000;
}

function dominantExit(exits: Record<string, number>): string | null {
  let best: string | null = null;
  let n = 0;
  for (const k of EXIT_KINDS) {
    const c = exits[k.key] ?? 0;
    if (c > n) {
      n = c;
      best = k.label;
    }
  }
  if ((exits.open ?? 0) > n) return 'Open';
  if ((exits.other ?? 0) > n) return 'Other';
  return best;
}

/**
 * Fold fired rows into hold bins + a wall-clock heatmap over `wallField`.
 * Not-fired (`NoEntry`) rows are skipped.
 */
export function buildTemporalSummary(
  rows: TemporalRow[],
  wallField: WallTimeField = 'entry_time',
): TemporalSummaryData {
  const fired = rows.filter((r) => r.fired);
  const holdMap = new Map<string, HoldBinStats>();
  for (const b of HOLD_BINS) {
    holdMap.set(b.id, {
      id: b.id,
      label: b.label,
      n: 0,
      pnl_sol: 0,
      exits: emptyExits(),
      holdingFilter: holdingFilterFor(b),
      exitFilter: b.isOpen ? 'Open' : null,
      mints: [],
    });
  }

  const times: number[] = [];
  for (const r of fired) {
    const id = holdBinId(r);
    if (!id) continue;
    const bin = holdMap.get(id)!;
    bin.n += 1;
    bin.pnl_sol += r.pnl_sol;
    bin.mints.push(r.mint_address);
    tallyExit(bin.exits, r.exit);
    const ts = parseTsMs(wallField === 'entry_time' ? r.entry_time : r.created_at);
    if (ts != null) times.push(ts);
  }

  let wallGrain: 'hour' | 'day' = 'day';
  const wall: WallCellStats[] = [];
  if (times.length > 0) {
    const minT = Math.min(...times);
    const maxT = Math.max(...times);
    wallGrain = maxT - minT <= 72 * 3_600_000 ? 'hour' : 'day';
    const step = stepMs(wallGrain);
    const start0 = floorToGrain(minT, wallGrain);
    const end0 = floorToGrain(maxT, wallGrain) + step;
    const cellMap = new Map<number, WallCellStats>();
    for (let t = start0; t < end0; t += step) {
      cellMap.set(t, {
        id: `${wallField}:${t}`,
        start: new Date(t).toISOString(),
        end: new Date(t + step).toISOString(),
        n: 0,
        pnl_sol: 0,
        win_rate: 0,
        exits: emptyExits(),
        dominant: null,
        mints: [],
      });
    }
    const wins = new Map<number, number>();
    for (const r of fired) {
      const ts = parseTsMs(wallField === 'entry_time' ? r.entry_time : r.created_at);
      if (ts == null) continue;
      const key = floorToGrain(ts, wallGrain);
      const cell = cellMap.get(key);
      if (!cell) continue;
      cell.n += 1;
      cell.pnl_sol += r.pnl_sol;
      cell.mints.push(r.mint_address);
      tallyExit(cell.exits, r.exit);
      if (r.exit !== 'Open' && r.pnl_sol > 0) {
        wins.set(key, (wins.get(key) ?? 0) + 1);
      }
    }
    for (const [key, cell] of cellMap) {
      const closed = cell.n - (cell.exits.open ?? 0);
      const w = wins.get(key) ?? 0;
      cell.win_rate = closed > 0 ? w / closed : 0;
      cell.dominant = cell.n > 0 ? dominantExit(cell.exits) : null;
      wall.push(cell);
    }
  }

  return {
    hold: HOLD_BINS.map((b) => holdMap.get(b.id)!),
    wall,
    wallGrain,
    wallField,
    nFired: fired.length,
  };
}

/** Whether a row falls in a hold bin (for client-side cohort filter). */
export function rowMatchesHoldBin(row: TemporalRow, binId: string): boolean {
  return holdBinId(row) === binId;
}

/** Whether a row's wall-clock field falls in `[start, end)`. */
export function rowMatchesWallCell(
  row: TemporalRow,
  field: WallTimeField,
  startIso: string,
  endIso: string,
): boolean {
  const ts = parseTsMs(field === 'entry_time' ? row.entry_time : row.created_at);
  if (ts == null) return false;
  const a = parseTsMs(startIso);
  const b = parseTsMs(endIso);
  if (a == null || b == null) return false;
  return ts >= a && ts < b;
}

/** Diverging red←0→green wash for heatmap cells. `maxAbs` is the cohort peak |pnl|. */
export function pnlHeatBackground(pnl: number, maxAbs: number, n: number): string {
  if (n <= 0) return 'rgba(255,255,255,0.02)';
  if (!(maxAbs > 0)) return 'rgba(255,255,255,0.06)';
  const t = Math.min(1, Math.abs(pnl) / maxAbs);
  const a = (0.1 + 0.75 * t).toFixed(3);
  return pnl >= 0 ? `rgba(34,197,94,${a})` : `rgba(239,68,68,${a})`;
}

/** Stack segments for a hold bar — EXIT_KINDS order, drop zeros. */
export function holdBarSegments(exits: Record<string, number>): Array<{
  key: string;
  n: number;
  bar: string;
  label: string;
}> {
  const out: Array<{ key: string; n: number; bar: string; label: string }> = [];
  for (const k of EXIT_KINDS) {
    const n = exits[k.key] ?? 0;
    if (n > 0) out.push({ key: k.key, n, bar: k.bar, label: k.label });
  }
  const open = exits.open ?? 0;
  if (open > 0) out.push({ key: 'open', n: open, bar: 'bg-warning', label: 'Open' });
  const other = exits.other ?? 0;
  if (other > 0) out.push({ key: 'other', n: other, bar: 'bg-text-mid', label: 'Other' });
  return out;
}

export function formatWallTick(iso: string, grain: 'hour' | 'day'): string {
  const d = new Date(iso);
  if (grain === 'hour') {
    return d.toLocaleString(undefined, {
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      hour12: false,
    });
  }
  return d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
}
