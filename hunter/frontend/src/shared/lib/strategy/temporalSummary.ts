/**
 * Temporal summary fold — hold-duration × exit mix + wall-clock sold/bought/created
 * volume timeline. Shared by Simulate (server bins mirror this shape) and the
 * grouped-sweep combo drill-in (client fold over ComboTokenResult rows).
 *
 * Hold-bin **scheme** adapts to cohort density (p90 of closed holding_secs) —
 * twin of `lab::strategies::sim_query::{pick_hold_scheme, hold_bins_for}`.
 * Integer-second edges so `holding` col-filters (`15..59`) round-trip.
 * Wall grain picker is also twin'd there (`pick_wall_grain`).
 */

import { getUtcOffsetMinutes } from 'components/token-price-chart/chartTimezone';
import { dayKeyInTz } from 'components/analytics/pnlSeries';
import { isMetricExitReason } from './exitReason';
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
  /** Null while open — `exit_time` binning falls back to `entry_time` there. */
  exit_time?: string | null;
}

export type WallTimeField = 'exit_time' | 'entry_time' | 'created_at';

/**
 * The instant a row sits at on the wall-clock timeline.
 *
 * `exit_time` is the **decision instant** — sold, or bought for a position still
 * open — the same rule `PositionChartPoint.timeMs` uses, so the Wall clock lands
 * a position on the identical civil day as the equity curve, calendar and
 * heatmap beside it. Keep the two definitions in step.
 */
export function wallTimeMs(row: TemporalRow, field: WallTimeField): number | null {
  if (field === 'created_at') return parseTsMs(row.created_at);
  if (field === 'entry_time') return parseTsMs(row.entry_time);
  return parseTsMs(row.exit_time) ?? parseTsMs(row.entry_time);
}

export type TemporalMetric = 'exit_mix' | 'pnl';

/** Adaptive wall-clock bucket size — pick by cohort span (see `pickWallGrain`). */
export type WallGrain = '30m' | '1h' | '2h' | '4h' | 'day';

/** Manual override; `auto` uses `pickWallGrain` on the cohort span. */
export type WallGrainChoice = 'auto' | WallGrain;

export const WALL_GRAINS: readonly WallGrain[] = ['30m', '1h', '2h', '4h', 'day'];

/**
 * Hold-duration scale picked from the cohort's closed-hold p90.
 * Dense short snipes get sub-15s resolution; long holds stretch into hours.
 */
export type HoldScheme =
  | 'dense_15s'
  | 'dense_60s'
  | 'mid_5m'
  | 'mid_30m'
  | 'wide_2h'
  | 'wide_day';

export const HOLD_SCHEMES: readonly HoldScheme[] = [
  'dense_15s',
  'dense_60s',
  'mid_5m',
  'mid_30m',
  'wide_2h',
  'wide_day',
];

/** Manual override; `auto` uses `pickHoldScheme` on closed-hold p90. */
export type HoldSchemeChoice = 'auto' | HoldScheme;

export function parseHoldSchemeChoice(s: string | null | undefined): HoldSchemeChoice {
  if (!s || s === 'auto') return 'auto';
  if ((HOLD_SCHEMES as readonly string[]).includes(s)) return s as HoldScheme;
  return 'auto';
}

/** One hold-duration bucket — inclusive lo/hi seconds (`null` hi = open-ended). */
export interface HoldBinDef {
  id: string;
  label: string;
  lo: number | null;
  hi: number | null;
  /** Still-open positions (not a hold-duration bucket). */
  isOpen?: boolean;
}

const OPEN_HOLD_BIN: HoldBinDef = {
  id: 'open',
  label: 'Open',
  lo: null,
  hi: null,
  isOpen: true,
};

/** Closed-bin edges per scheme (Open always appended by `holdBinsFor`). */
const HOLD_SCHEME_EDGES: Readonly<Record<HoldScheme, ReadonlyArray<readonly [number, number | null]>>> = {
  // p90 ≤ 15s — sub-second snipes need fine buckets
  dense_15s: [
    [0, 2],
    [3, 5],
    [6, 9],
    [10, 14],
    [15, null],
  ],
  // p90 ≤ 60s
  dense_60s: [
    [0, 9],
    [10, 19],
    [20, 39],
    [40, 59],
    [60, null],
  ],
  // p90 ≤ 5m
  mid_5m: [
    [0, 29],
    [30, 59],
    [60, 119],
    [120, 299],
    [300, null],
  ],
  // p90 ≤ 30m — classic default
  mid_30m: [
    [0, 14],
    [15, 59],
    [60, 299],
    [300, 1799],
    [1800, null],
  ],
  // p90 ≤ 2h
  wide_2h: [
    [0, 59],
    [60, 299],
    [300, 899],
    [900, 3599],
    [3600, null],
  ],
  // longer
  wide_day: [
    [0, 299],
    [300, 1799],
    [1800, 7199],
    [7200, 21_599],
    [21_600, null],
  ],
};

function fmtHoldEdge(secs: number): string {
  if (secs < 60) return `${secs}s`;
  if (secs < 3600) {
    const m = secs / 60;
    return Number.isInteger(m) ? `${m}m` : `${Number(m.toFixed(1))}m`;
  }
  const h = secs / 3600;
  return Number.isInteger(h) ? `${h}h` : `${Number(h.toFixed(1))}h`;
}

function holdBinLabel(lo: number, hi: number | null): string {
  // Labels use exclusive-looking uppers (`15–60s` for inclusive 15..59) — same
  // convention as the legacy fixed bins.
  if (hi == null) return `${fmtHoldEdge(lo)}+`;
  if (lo === 0) return `<${fmtHoldEdge(hi + 1)}`;
  return `${fmtHoldEdge(lo)}–${fmtHoldEdge(hi + 1)}`;
}

function holdBinIdFor(lo: number, hi: number | null): string {
  return hi == null ? `hold_${lo}_plus` : `hold_${lo}_${hi}`;
}

/** Bin definitions for a hold scheme (closed buckets + Open). */
export function holdBinsFor(scheme: HoldScheme): HoldBinDef[] {
  const closed = HOLD_SCHEME_EDGES[scheme].map(([lo, hi]) => ({
    id: holdBinIdFor(lo, hi),
    label: holdBinLabel(lo, hi),
    lo,
    hi,
  }));
  return [...closed, OPEN_HOLD_BIN];
}

/** p90 of closed holding_secs → scheme (empty → mid_30m). Twin of Rust `pick_hold_scheme`. */
export function pickHoldScheme(closedSecs: number[]): HoldScheme {
  if (closedSecs.length === 0) return 'mid_30m';
  const sorted = [...closedSecs].filter((s) => Number.isFinite(s) && s >= 0).sort((a, b) => a - b);
  if (sorted.length === 0) return 'mid_30m';
  // Nearest-rank p90 (`ceil(0.9·n)−1`) so small cohorts aren't stuck on the median.
  const p90 = sorted[Math.min(sorted.length - 1, Math.ceil(0.9 * sorted.length) - 1)]!;
  if (p90 <= 15) return 'dense_15s';
  if (p90 <= 60) return 'dense_60s';
  if (p90 <= 300) return 'mid_5m';
  if (p90 <= 1800) return 'mid_30m';
  if (p90 <= 7200) return 'wide_2h';
  return 'wide_day';
}

export function holdSchemeLabel(scheme: HoldScheme): string {
  switch (scheme) {
    case 'dense_15s':
      return 'dense ≤15s';
    case 'dense_60s':
      return 'dense ≤1m';
    case 'mid_5m':
      return 'mid ≤5m';
    case 'mid_30m':
      return 'mid ≤30m';
    case 'wide_2h':
      return 'wide ≤2h';
    case 'wide_day':
      return 'wide multi-hour';
  }
}

/** @deprecated Prefer `holdBinsFor(pickHoldScheme(...))` — mid_30m alias. */
export const HOLD_BINS: readonly HoldBinDef[] = holdBinsFor('mid_30m');

/** Exit reason → stack segment key (mirrors `EXIT_KEY_BY_REASON` / Rust ExitCode).
 *  `Metrics` is split by PnL in [`tallyExit`]. */
const EXIT_REASON_KEY: Readonly<Record<string, ExitCountKey>> = {
  TakeProfit: 'n_exit_take_profit',
  StopLoss: 'n_exit_stop_loss',
  Dead: 'n_exit_dead',
  Manual: 'n_exit_manual',
  TrailingStop: 'n_exit_trailing',
  Stall: 'n_exit_stall',
  TimeStop: 'n_exit_time',
  LiquidityExit: 'n_exit_liquidity',
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
  /** Hold-duration scale actually used (auto pick or override). */
  holdScheme: HoldScheme;
  /** What `pickHoldScheme` would choose for this cohort (even when overridden). */
  holdSchemeAuto: HoldScheme;
  wall: WallCellStats[];
  /** Grain actually used for `wall` (auto pick or override). */
  wallGrain: WallGrain;
  /** What `pickWallGrain` would choose for this cohort (even when overridden). */
  wallGrainAuto: WallGrain;
  /** max(ts) − min(ts) over timed rows; 0 when empty / single stamp. */
  wallSpanMs: number;
  wallField: WallTimeField;
  /** Fired rows that contributed (excludes NoEntry). */
  nFired: number;
}

function emptyExits(): Record<string, number> {
  const o: Record<string, number> = { other: 0, open: 0 };
  for (const k of EXIT_KINDS) o[k.key] = 0;
  return o;
}

function tallyExit(exits: Record<string, number>, reason: string, pnlSol?: number): void {
  if (reason === 'Open') {
    exits.open = (exits.open ?? 0) + 1;
    return;
  }
  // Detail label (`retrace >= 12`) or legacy bare `Metrics` / compact `stall>` —
  // the ONE membership test, same as `countExits` / the Rust `is_metric_exit_label`.
  if (isMetricExitReason(reason)) {
    const key =
      pnlSol != null && Number.isFinite(pnlSol) && pnlSol > 0
        ? 'n_exit_metrics_win'
        : 'n_exit_metrics_loss';
    exits[key] = (exits[key] ?? 0) + 1;
    exits.n_exit_metrics = (exits.n_exit_metrics ?? 0) + 1;
    return;
  }
  const key = EXIT_REASON_KEY[reason];
  if (key) exits[key] = (exits[key] ?? 0) + 1;
  else exits.other = (exits.other ?? 0) + 1;
}

function holdBinId(row: TemporalRow, bins: readonly HoldBinDef[]): string | null {
  if (!row.fired) return null;
  if (row.exit === 'Open') return 'open';
  const s = row.holding_secs;
  if (!Number.isFinite(s) || s < 0) return 'open';
  for (const b of bins) {
    if (b.isOpen) continue;
    if (b.lo == null) continue;
    if (s < b.lo) continue;
    if (b.hi != null && s > b.hi) continue;
    return b.id;
  }
  // Last closed bucket is always open-ended — fall into it if edges ever miss.
  const last = [...bins].reverse().find((b) => !b.isOpen);
  return last?.id ?? 'open';
}

function holdingFilterFor(bin: HoldBinDef): string | null {
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

const H = 3_600_000;
const D = 86_400_000;

/** Step size for a wall grain (ms). */
export function wallGrainStepMs(grain: WallGrain): number {
  switch (grain) {
    case '30m':
      return 30 * 60_000;
    case '1h':
      return H;
    case '2h':
      return 2 * H;
    case '4h':
      return 4 * H;
    case 'day':
      return D;
  }
}

/**
 * Choose a wall bucket so short cohorts stay readable (~dozen bars) and long
 * ones don't explode into hundreds of hour cells. Twin of Rust `pick_wall_grain`.
 */
export function pickWallGrain(spanMs: number): WallGrain {
  const span = Math.max(0, spanMs);
  if (span <= 6 * H) return '30m';
  if (span <= 24 * H) return '1h';
  if (span <= 3 * D) return '2h';
  if (span <= 7 * D) return '4h';
  return 'day';
}

/**
 * Floor an instant to its wall-clock bucket **in `timeZone`** — twin of Rust
 * `floor_to_grain_in_zone`.
 *
 * Epoch-aligned flooring (`ms % step`) buckets in UTC, which silently disagrees
 * with every other chart on this page: the calendar and the dow×hour heatmap
 * resolve civil dates through `Intl` in the user's zone, so a UTC-floored `day`
 * bar starts at 19:00 the previous local day for a UTC-5 user and its total can
 * never match the calendar square carrying the same date. Half-hour zones
 * (+05:30) and the 2h/4h grains straddle local boundaries the same way.
 *
 * DST-safe in two passes: floor the local wall-clock, map it back with the
 * offset in effect *there*, and re-resolve if crossing the boundary changed the
 * offset (`getUtcOffsetMinutes` reads the offset at a given instant, never a
 * fixed value).
 */
export function floorToWallGrain(ms: number, grain: WallGrain, timeZone: string): number {
  const step = wallGrainStepMs(grain);
  const offMs = getUtcOffsetMinutes(timeZone, new Date(ms)) * 60_000;
  const local = ms + offMs;
  const flooredLocal = local - (((local % step) + step) % step);
  const out = flooredLocal - offMs;
  const offAfter = getUtcOffsetMinutes(timeZone, new Date(out)) * 60_000;
  return offAfter === offMs ? out : flooredLocal - offAfter;
}

/**
 * Ascending bucket boundaries covering `[minT, maxT]` in `timeZone`.
 *
 * Civil buckets are NOT uniformly `step` apart — a DST day is 23 h or 25 h — so
 * a `t += step` grid drifts off the boundaries `floorToWallGrain` produces, and
 * every row landing on a drifted boundary would be silently dropped at lookup.
 * The real row keys are therefore seeded first (they are boundaries by
 * construction) and the walk only fills the empty gaps between them; the walk
 * is forced monotonic so a repeated local hour can't spin.
 */
function wallBoundaries(
  minT: number,
  maxT: number,
  grain: WallGrain,
  timeZone: string,
  rowKeys: Iterable<number>,
): number[] {
  const step = wallGrainStepMs(grain);
  const set = new Set<number>(rowKeys);
  let t = floorToWallGrain(minT, grain, timeZone);
  const last = floorToWallGrain(maxT, grain, timeZone);
  set.add(t);
  set.add(last);
  // Bounded: each step advances by at least `step`, so the walk is O(span/step).
  while (t < last) {
    const snapped = floorToWallGrain(t + step, grain, timeZone);
    t = snapped > t ? snapped : t + step;
    if (t <= last) set.add(t);
  }
  return [...set].sort((a, b) => a - b);
}

function dominantExit(exits: Record<string, number>): string | null {
  let best: string | null = null;
  let n = 0;
  const metricsSplit =
    (exits.n_exit_metrics_win ?? 0) + (exits.n_exit_metrics_loss ?? 0);
  for (const k of EXIT_KINDS) {
    if (k.legacyMetricsTotal && metricsSplit > 0) continue;
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
 * Fold fired rows into hold bins + a wall-clock volume timeline over `wallField`.
 * Not-fired (`NoEntry`) rows are skipped.
 * `grainChoice` / `holdChoice` override the adaptive pickers when not `'auto'`.
 *
 * `timeZone` is required and positioned early on purpose: the wall buckets are
 * civil-time buckets, and a caller that forgets one silently re-introduces the
 * UTC-vs-local split with the calendar (see {@link floorToWallGrain}). Pass the
 * app zone from `useTimezone()`; `'UTC'` is only correct in tests.
 */
export function buildTemporalSummary(
  rows: TemporalRow[],
  timeZone: string,
  wallField: WallTimeField = 'entry_time',
  grainChoice: WallGrainChoice = 'auto',
  holdChoice: HoldSchemeChoice = 'auto',
): TemporalSummaryData {
  const fired = rows.filter((r) => r.fired);
  const closedSecs = fired
    .filter((r) => r.exit !== 'Open')
    .map((r) => r.holding_secs)
    .filter((s) => Number.isFinite(s) && s >= 0);
  const holdSchemeAuto = pickHoldScheme(closedSecs);
  const holdScheme = holdChoice === 'auto' ? holdSchemeAuto : holdChoice;
  const bins = holdBinsFor(holdScheme);

  const holdMap = new Map<string, HoldBinStats>();
  for (const b of bins) {
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
    const id = holdBinId(r, bins);
    if (!id) continue;
    const bin = holdMap.get(id)!;
    bin.n += 1;
    bin.pnl_sol += r.pnl_sol;
    bin.mints.push(r.mint_address);
    tallyExit(bin.exits, r.exit, r.pnl_sol);
    const ts = wallTimeMs(r, wallField);
    if (ts != null) times.push(ts);
  }

  let wallGrain: WallGrain = 'day';
  let wallGrainAuto: WallGrain = 'day';
  let wallSpanMs = 0;
  const wall: WallCellStats[] = [];
  if (times.length > 0) {
    const minT = Math.min(...times);
    const maxT = Math.max(...times);
    wallSpanMs = Math.max(0, maxT - minT);
    wallGrainAuto = pickWallGrain(wallSpanMs);
    wallGrain = grainChoice === 'auto' ? wallGrainAuto : grainChoice;
    const step = wallGrainStepMs(wallGrain);
    const rowKeys = times.map((t) => floorToWallGrain(t, wallGrain, timeZone));
    const boundaries = wallBoundaries(minT, maxT, wallGrain, timeZone, rowKeys);
    const cellMap = new Map<number, WallCellStats>();
    boundaries.forEach((t, i) => {
      // End at the next boundary so cells stay contiguous across a DST step —
      // `rowMatchesWallCell` filters on this exact `[start, end)`.
      const end = boundaries[i + 1] ?? t + step;
      cellMap.set(t, {
        id: `${wallField}:${t}`,
        start: new Date(t).toISOString(),
        end: new Date(end).toISOString(),
        n: 0,
        pnl_sol: 0,
        win_rate: 0,
        exits: emptyExits(),
        dominant: null,
        mints: [],
      });
    });
    const wins = new Map<number, number>();
    for (const r of fired) {
      const ts = wallTimeMs(r, wallField);
      if (ts == null) continue;
      const key = floorToWallGrain(ts, wallGrain, timeZone);
      const cell = cellMap.get(key);
      if (!cell) continue;
      cell.n += 1;
      cell.pnl_sol += r.pnl_sol;
      cell.mints.push(r.mint_address);
      tallyExit(cell.exits, r.exit, r.pnl_sol);
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
    hold: bins.map((b) => holdMap.get(b.id)!),
    holdScheme,
    holdSchemeAuto,
    wall,
    wallGrain,
    wallGrainAuto,
    wallSpanMs,
    wallField,
    nFired: fired.length,
  };
}

/**
 * Re-fold rows into a fixed hold scheme (same edges as the base chart).
 * Used for linked-brush: wall selection → hold distribution of that slice.
 */
export function rebucketHold(scheme: HoldScheme, rows: TemporalRow[]): HoldBinStats[] {
  // Hold bins are durations, not civil times — the zone can't change them, so the
  // wall half of this fold is discarded and 'UTC' is a real answer here.
  return buildTemporalSummary(rows, 'UTC', 'entry_time', 'auto', scheme).hold;
}

/**
 * Re-fold rows into the base wall cell grid (same ids / grain / span).
 * Used for linked-brush: hold selection → wall distribution of that slice.
 */
export function rebucketWall(
  baseWall: WallCellStats[],
  wallField: WallTimeField,
  grain: WallGrain,
  rows: TemporalRow[],
  timeZone: string,
): WallCellStats[] {
  const cells = baseWall.map((c) => ({
    ...c,
    n: 0,
    pnl_sol: 0,
    win_rate: 0,
    exits: emptyExits(),
    dominant: null as string | null,
    mints: [] as string[],
  }));
  const byId = new Map(cells.map((c) => [c.id, c]));
  const wins = new Map<string, number>();
  for (const r of rows) {
    if (!r.fired) continue;
    const ts = wallTimeMs(r, wallField);
    if (ts == null) continue;
    const key = floorToWallGrain(ts, grain, timeZone);
    const id = `${wallField}:${key}`;
    const cell = byId.get(id);
    if (!cell) continue;
    cell.n += 1;
    cell.pnl_sol += r.pnl_sol;
    cell.mints.push(r.mint_address);
    tallyExit(cell.exits, r.exit, r.pnl_sol);
    if (r.exit !== 'Open' && r.pnl_sol > 0) {
      wins.set(id, (wins.get(id) ?? 0) + 1);
    }
  }
  for (const cell of cells) {
    const closed = cell.n - (cell.exits.open ?? 0);
    const w = wins.get(cell.id) ?? 0;
    cell.win_rate = closed > 0 ? w / closed : 0;
    cell.dominant = cell.n > 0 ? dominantExit(cell.exits) : null;
  }
  return cells;
}

/** Map a linked hold payload onto base bin edges by id (zeros when missing). */
export function alignHoldBins(base: HoldBinStats[], linked: HoldBinStats[]): HoldBinStats[] {
  const map = new Map(linked.map((b) => [b.id, b]));
  return base.map(
    (b) =>
      map.get(b.id) ?? {
        ...b,
        n: 0,
        pnl_sol: 0,
        exits: emptyExits(),
        mints: [],
      },
  );
}

/** Map a linked wall payload onto base cells by id (zeros when missing). */
export function alignWallCells(base: WallCellStats[], linked: WallCellStats[]): WallCellStats[] {
  const map = new Map(linked.map((c) => [c.id, c]));
  return base.map(
    (c) =>
      map.get(c.id) ?? {
        ...c,
        n: 0,
        pnl_sol: 0,
        win_rate: 0,
        exits: emptyExits(),
        dominant: null,
        mints: [],
      },
  );
}

/** Short human span for glance chips (`45m`, `6h`, `2.5d`). */
export function formatWallSpan(spanMs: number): string {
  const ms = Math.max(0, spanMs);
  if (ms < 60_000) return '<1m';
  if (ms < H) return `${Math.max(1, Math.round(ms / 60_000))}m`;
  if (ms < D) {
    const h = ms / H;
    return h >= 10 ? `${Math.round(h)}h` : `${Number(h.toFixed(1))}h`;
  }
  const d = ms / D;
  return d >= 10 ? `${Math.round(d)}d` : `${Number(d.toFixed(1))}d`;
}

/** Busiest wall cell (first max on ties). */
export function peakWallCell(cells: WallCellStats[]): WallCellStats | null {
  let best: WallCellStats | null = null;
  for (const c of cells) {
    if (c.n <= 0) continue;
    if (!best || c.n > best.n) best = c;
  }
  return best;
}

/** Highest-PnL wall cell among non-empty (first max on ties). */
export function bestPnlWallCell(cells: WallCellStats[]): WallCellStats | null {
  let best: WallCellStats | null = null;
  for (const c of cells) {
    if (c.n <= 0) continue;
    if (!best || c.pnl_sol > best.pnl_sol) best = c;
  }
  return best;
}

/** Lowest-PnL wall cell among non-empty (first min on ties). */
export function worstPnlWallCell(cells: WallCellStats[]): WallCellStats | null {
  let worst: WallCellStats | null = null;
  for (const c of cells) {
    if (c.n <= 0) continue;
    if (!worst || c.pnl_sol < worst.pnl_sol) worst = c;
  }
  return worst;
}

/** Closed count for a bin/cell (`n` minus still-open). */
export function closedCount(n: number, exits: Record<string, number>): number {
  return Math.max(0, n - (exits.open ?? 0));
}

/** Average PnL per position, or null when empty. */
export function avgPnlSol(pnlSol: number, n: number): number | null {
  if (n <= 0) return null;
  return pnlSol / n;
}

/** Whether a row falls in a hold bin (for client-side cohort filter). */
export function rowMatchesHoldBin(
  row: TemporalRow,
  binId: string,
  bins: readonly HoldBinDef[] = HOLD_BINS,
): boolean {
  return holdBinId(row, bins) === binId;
}

/** Whether a row's wall-clock field falls in `[start, end)`. */
export function rowMatchesWallCell(
  row: TemporalRow,
  field: WallTimeField,
  startIso: string,
  endIso: string,
): boolean {
  const ts = wallTimeMs(row, field);
  if (ts == null) return false;
  const a = parseTsMs(startIso);
  const b = parseTsMs(endIso);
  if (a == null || b == null) return false;
  return ts >= a && ts < b;
}

/** Diverging red←0→green wash for pnl-colored bars. `maxAbs` is the cohort peak |pnl|.
 *  Exact zero uses a neutral slate (same rule as `signedToneClass`: 0 ≠ green). */
export function pnlHeatBackground(pnl: number, maxAbs: number, n: number): string {
  if (n <= 0) return 'rgba(255,255,255,0.04)';
  if (pnl === 0) return 'rgba(148,163,184,0.4)';
  if (!(maxAbs > 0)) return 'rgba(255,255,255,0.1)';
  const t = Math.min(1, Math.abs(pnl) / maxAbs);
  const a = (0.22 + 0.78 * t).toFixed(3);
  return pnl > 0 ? `rgba(34,197,94,${a})` : `rgba(239,68,68,${a})`;
}

/** Count intensity (cyan) for volume-first wall bars when not coloring by PnL. */
export function countHeatBackground(n: number, maxN: number): string {
  if (n <= 0) return 'rgba(255,255,255,0.04)';
  const t = Math.min(1, n / Math.max(1, maxN));
  const a = (0.28 + 0.72 * t).toFixed(3);
  return `rgba(56,189,248,${a})`;
}

/** Stack segments for a hold bar — EXIT_KINDS order, drop zeros. */
export function holdBarSegments(exits: Record<string, number>): Array<{
  key: string;
  n: number;
  bar: string;
  label: string;
}> {
  const out: Array<{ key: string; n: number; bar: string; label: string }> = [];
  const metricsSplit =
    (exits.n_exit_metrics_win ?? 0) + (exits.n_exit_metrics_loss ?? 0);
  for (const k of EXIT_KINDS) {
    // Prefer Metric+/Metric-; skip the unsplittable total when the split is present.
    if (k.legacyMetricsTotal && metricsSplit > 0) continue;
    const n = exits[k.key] ?? 0;
    if (n > 0) out.push({ key: k.key, n, bar: k.bar, label: k.label });
  }
  const open = exits.open ?? 0;
  if (open > 0) out.push({ key: 'open', n: open, bar: 'bg-warning', label: 'Open' });
  const other = exits.other ?? 0;
  if (other > 0) out.push({ key: 'other', n: other, bar: 'bg-text-mid', label: 'Other' });
  return out;
}

export function wallGrainLabel(grain: WallGrain): string {
  switch (grain) {
    case '30m':
      return '30m';
    case '1h':
      return '1h';
    case '2h':
      return '2h';
    case '4h':
      return '4h';
    case 'day':
      return 'day';
  }
}

export function parseWallGrainChoice(s: string | null | undefined): WallGrainChoice {
  if (!s || s === 'auto') return 'auto';
  if ((WALL_GRAINS as readonly string[]).includes(s)) return s as WallGrain;
  return 'auto';
}

/*
 * Every label below takes the **app** timezone (`useTimezone()`), not the
 * browser's. The zone is user-selectable and server-persisted, so a bare
 * `toLocaleString()` renders the axis in a different zone than the calendar +
 * heatmap grid in — the dates themselves then disagree on the same page.
 */

export function formatWallTick(iso: string, grain: WallGrain, timeZone: string): string {
  const d = new Date(iso);
  if (grain === 'day') {
    return d.toLocaleDateString(undefined, { timeZone, month: 'short', day: 'numeric' });
  }
  return d.toLocaleString(undefined, {
    timeZone,
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: grain === '30m' || grain === '1h' ? '2-digit' : undefined,
    hour12: false,
  });
}

/** Compact clock for the chart axis (`14:30` / `14:00`). */
export function formatWallClock(iso: string, grain: WallGrain, timeZone: string): string {
  const d = new Date(iso);
  if (grain === 'day') {
    return d.toLocaleDateString(undefined, { timeZone, month: 'short', day: 'numeric' });
  }
  return d.toLocaleTimeString(undefined, {
    timeZone,
    hour: '2-digit',
    minute: '2-digit',
    hour12: false,
  });
}

/** Short date for axis day-breaks (`Jul 15`). */
export function formatWallDate(iso: string, timeZone: string): string {
  return new Date(iso).toLocaleDateString(undefined, {
    timeZone,
    month: 'short',
    day: 'numeric',
  });
}

/** Full range caption above the wall chart. */
export function formatWallRange(
  cells: WallCellStats[],
  grain: WallGrain,
  timeZone: string,
): string | null {
  const timed = cells.filter((c) => c.n > 0);
  if (timed.length === 0) return null;
  const first = timed[0]!;
  const last = timed[timed.length - 1]!;
  if (first.id === last.id) return formatWallTick(first.start, grain, timeZone);
  return `${formatWallTick(first.start, grain, timeZone)}  →  ${formatWallTick(last.start, grain, timeZone)}`;
}

/** Whether `iso` falls on a different calendar day than `prevIso` in `timeZone`. */
export function isWallDayBreak(iso: string, prevIso: string | null, timeZone: string): boolean {
  if (prevIso == null) return true;
  return dayKeyInTz(Date.parse(iso), timeZone) !== dayKeyInTz(Date.parse(prevIso), timeZone);
}
