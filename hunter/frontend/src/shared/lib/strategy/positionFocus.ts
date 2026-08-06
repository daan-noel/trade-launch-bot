/**
 * Stacked focus lenses for Evidence / Simulate / Sweep position summaries.
 *
 * Independent of Console History `hfocus` (single lens, URL-backed). These chips
 * sit on top of table column filters and never write into the DataTable filter row.
 */

import { formatDurationShort } from 'utils/format';
import {
  DOW_ROWS,
  dayKeyInTz,
  dowHourInTz,
  isPnlDistBucket,
  matchesPctFocus,
  pctFocusFilter,
  shiftDayKey,
} from 'components/analytics/pnlSeries';
import { parseEpisodeRowKey } from 'components/strategy/inspectTarget';
import { isMetricExitReason, normalizeExitReasonFilter } from 'lib/strategy/exitReason';
import type { FilterSpec } from 'components/table/numericFilter';

/** How the parent table addresses a row — Evidence UUIDs vs Simulate episode keys. */
export type FocusTableMode = 'positionId' | 'mint';

export type PositionFocusLens =
  | { kind: 'status'; status: 'open' | 'closed' | 'fired' }
  | { kind: 'exit'; reason: string }
  | { kind: 'outcome'; outcome: 'win' | 'loss' }
  | { kind: 'migrated'; migrated: boolean }
  | { kind: 'hold'; binId: string; mints: string[] }
  | { kind: 'wall'; cellId: string; mints: string[] }
  | { kind: 'pct'; lo: number; hi: number }
  | { kind: 'pos'; positionId: string }
  | { kind: 'band'; holdLo: number; holdHi: number; pctLo: number; pctHi: number }
  /** Day-of-week × hour (user TZ) — exit/decision instant, matching Console heatmap. */
  | { kind: 'heat'; dow: number; hour: number }
  /** Calendar day `YYYY-MM-DD` (user TZ) on the decision instant. */
  | { kind: 'day'; day: string }
  /** Calendar week keyed by its **Sunday** `YYYY-MM-DD` (user TZ). */
  | { kind: 'week'; weekStart: string };

/** The lenses a timing chart owns: they keep the parent cohort (selection ring)
 *  instead of refolding it, and they resolve through {@link timeLensMatches}. */
export type PositionTimeLens = Extract<
  PositionFocusLens,
  { kind: 'heat' } | { kind: 'day' } | { kind: 'week' }
>;

const TIME_LENS_KINDS: readonly PositionFocusLens['kind'][] = ['heat', 'day', 'week'];

export function isTimeLens(lens: PositionFocusLens): lens is PositionTimeLens {
  return TIME_LENS_KINDS.includes(lens.kind);
}

/**
 * Does a decision instant fall inside a timing lens? The ONE predicate behind the
 * chart refold, the chip, and the table filter — a calendar cell and the rows it
 * narrows to can never disagree about which day a close belongs to, because both
 * ask `dayKeyInTz` (the same call `buildDailyPnl` lays the grid out with).
 */
export function timeLensMatches(
  timeMs: number | null | undefined,
  lens: PositionTimeLens,
  timeZone: string,
): boolean {
  if (timeMs == null || !Number.isFinite(timeMs)) return false;
  if (lens.kind === 'heat') {
    const { dow, hour } = dowHourInTz(timeMs, timeZone);
    return dow === lens.dow && hour === lens.hour;
  }
  const dayKey = dayKeyInTz(timeMs, timeZone);
  if (lens.kind === 'day') return dayKey === lens.day;
  // `YYYY-MM-DD` sorts lexicographically, so a week is a plain string range.
  return dayKey >= lens.weekStart && dayKey < shiftDayKey(lens.weekStart, 7);
}

/** One lens per kind — stacking exit + hold + pct is allowed; two statuses are not. */
export function lensKind(lens: PositionFocusLens): PositionFocusLens['kind'] {
  return lens.kind;
}

export function sameLens(a: PositionFocusLens, b: PositionFocusLens): boolean {
  if (a.kind !== b.kind) return false;
  switch (a.kind) {
    case 'status':
      return a.status === (b as typeof a).status;
    case 'exit':
      return a.reason === (b as typeof a).reason;
    case 'outcome':
      return a.outcome === (b as typeof a).outcome;
    case 'migrated':
      return a.migrated === (b as typeof a).migrated;
    case 'hold':
      return a.binId === (b as typeof a).binId;
    case 'wall':
      return a.cellId === (b as typeof a).cellId;
    case 'pct':
      return a.lo === (b as typeof a).lo && a.hi === (b as typeof a).hi;
    case 'pos':
      return a.positionId === (b as typeof a).positionId;
    case 'band': {
      const o = b as typeof a;
      return (
        a.holdLo === o.holdLo &&
        a.holdHi === o.holdHi &&
        a.pctLo === o.pctLo &&
        a.pctHi === o.pctHi
      );
    }
    case 'heat':
      return a.dow === (b as typeof a).dow && a.hour === (b as typeof a).hour;
    case 'day':
      return a.day === (b as typeof a).day;
    case 'week':
      return a.weekStart === (b as typeof a).weekStart;
  }
}

/** Toggle / replace-by-kind. Re-clicking the identical lens drops it. */
export function togglePositionFocus(
  current: readonly PositionFocusLens[],
  next: PositionFocusLens,
): PositionFocusLens[] {
  const existing = current.find((l) => l.kind === next.kind);
  if (existing && sameLens(existing, next)) {
    return current.filter((l) => l.kind !== next.kind);
  }
  return [...current.filter((l) => l.kind !== next.kind), next];
}

export function removePositionFocus(
  current: readonly PositionFocusLens[],
  lens: PositionFocusLens,
): PositionFocusLens[] {
  return current.filter((l) => !sameLens(l, lens));
}

function encodeEdge(n: number): string {
  if (n === Infinity) return 'inf';
  if (n === -Infinity) return '-inf';
  return String(n);
}

export function positionFocusLabel(lens: PositionFocusLens): string {
  switch (lens.kind) {
    case 'status':
      return lens.status === 'fired' ? 'Fired' : lens.status === 'open' ? 'Open' : 'Closed';
    case 'exit':
      return `Exit: ${lens.reason}`;
    case 'outcome':
      return lens.outcome === 'win' ? 'Winners' : 'Losers';
    case 'migrated':
      return lens.migrated ? 'Migrated' : 'Not migrated';
    case 'hold':
      return `Hold: ${lens.binId}`;
    case 'wall':
      return `When: ${lens.cellId}`;
    case 'pct': {
      if (lens.lo === -Infinity) return `PnL% < ${lens.hi}`;
      if (lens.hi === Infinity) return `PnL% ≥ ${lens.lo}`;
      return `PnL% ${lens.lo}…${lens.hi}`;
    }
    case 'pos':
      return `Pos ${lens.positionId.slice(0, 8)}…`;
    case 'band':
      return `${formatDurationShort(lens.holdLo)}–${formatDurationShort(lens.holdHi)} · ${lens.pctLo.toFixed(0)}…${lens.pctHi.toFixed(0)}%`;
    case 'heat': {
      const dow = DOW_ROWS.find((r) => r.dow === lens.dow)?.label ?? `D${lens.dow}`;
      return `${dow} ${String(lens.hour).padStart(2, '0')}:00`;
    }
    case 'day':
      return lens.day;
    case 'week':
      return `${lens.weekStart} → ${shiftDayKey(lens.weekStart, 6).slice(5)}`;
  }
}

/** Stable id for React keys / active highlighting. */
export function positionFocusKey(lens: PositionFocusLens): string {
  switch (lens.kind) {
    case 'status':
      return `status:${lens.status}`;
    case 'exit':
      return `exit:${lens.reason}`;
    case 'outcome':
      return `outcome:${lens.outcome}`;
    case 'migrated':
      return `migrated:${lens.migrated ? 'yes' : 'no'}`;
    case 'hold':
      return `hold:${lens.binId}`;
    case 'wall':
      return `wall:${lens.cellId}`;
    case 'pct':
      return `pct:${encodeEdge(lens.lo)}:${encodeEdge(lens.hi)}`;
    case 'pos':
      return `pos:${lens.positionId}`;
    case 'band':
      return `band:${lens.holdLo}:${lens.holdHi}:${lens.pctLo}:${lens.pctHi}`;
    case 'heat':
      return `heat:${lens.dow}:${lens.hour}`;
    case 'day':
      return `day:${lens.day}`;
    case 'week':
      return `week:${lens.weekStart}`;
  }
}

export function activeLensOfKind<K extends PositionFocusLens['kind']>(
  lenses: readonly PositionFocusLens[],
  kind: K,
): Extract<PositionFocusLens, { kind: K }> | null {
  const found = lenses.find((l) => l.kind === kind);
  return (found as Extract<PositionFocusLens, { kind: K }> | undefined) ?? null;
}

/** Row shape every surface can project into for client-side focus predicates. */
export interface PositionFocusRow {
  id?: string;
  mint_address: string;
  /** Entered a position (sim/sweep `fired`; evidence: entry filled). */
  fired: boolean;
  /** Still open (unrealized). */
  isOpen: boolean;
  /** Closed cleanly (`End` / has exit). */
  isClosed: boolean;
  exit_reason: string | null;
  pnl_sol: number | null;
  pnl_pct: number | null;
  hold_secs: number | null;
  is_migrated?: boolean | null;
  /** Decision instant (exit time, or entry for open marks) — required for the
   *  timing lenses (`heat` / `day` / `week`). */
  timeMs?: number | null;
}

export interface FocusMatchOpts {
  /** IANA zone for the timing lenses. Required when one of them is active. */
  timeZone?: string;
}

/** Keys whose `timeMs` lands inside a timing lens (user TZ). */
export function timeLensMatchingKeys(
  points: readonly { key: string; timeMs: number }[],
  lens: PositionTimeLens,
  timeZone: string,
): string[] {
  const out: string[] = [];
  for (const p of points) {
    if (timeLensMatches(p.timeMs, lens, timeZone)) out.push(p.key);
  }
  return out;
}

/** Compact chart-point shape for key-resolved focus → table filters. */
export type FocusTablePoint = {
  key: string;
  mint_address: string;
  timeMs: number;
  pnlPct?: number | null;
  holdSeconds?: number | null;
  pnlSol?: number | null;
  exit_reason?: string | null;
  isOpen?: boolean;
};

function pointToFocusRow(p: FocusTablePoint): PositionFocusRow {
  return {
    id: p.key,
    mint_address: p.mint_address,
    fired: true,
    isOpen: !!p.isOpen,
    isClosed: !p.isOpen,
    exit_reason: p.exit_reason ?? null,
    pnl_sol: p.pnlSol ?? null,
    pnl_pct: p.pnlPct ?? null,
    hold_secs: p.holdSeconds ?? null,
    timeMs: p.timeMs,
  };
}

/**
 * Lenses that cannot be expressed as a single SQL predicate on every surface —
 * resolve matching chart-point keys, then emit `id in` / `mint_address in`.
 * Timing always; `exit:Other` always; `band` on Evidence (no hold-seconds column).
 */
function needsKeyResolution(lens: PositionFocusLens, mode: FocusTableMode): boolean {
  if (isTimeLens(lens)) return true;
  if (lens.kind === 'exit' && lens.reason === 'Other') return true;
  if (lens.kind === 'band' && mode === 'positionId') return true;
  // Simulate has no `status` column; "Closed" must exclude both Open and NoEntry.
  if (lens.kind === 'status' && mode === 'mint' && lens.status === 'closed') return true;
  return false;
}

/**
 * Fold key-resolved focus lenses into structured table filters.
 * - `positionId`: Evidence (`id` UUID column).
 * - `mint`: Simulate (mint set; may over-include re-entries when many keys share a mint).
 * Intersects with any prior `mint_address in` from hold/wall chips.
 *
 * Prefer {@link buildFocusTableFilters} at call sites — it composes structured + keys.
 */
export function applyTimeLensTableFilter(
  filters: Record<string, FilterSpec>,
  lenses: readonly PositionFocusLens[],
  points: readonly FocusTablePoint[],
  timeZone: string,
  mode: FocusTableMode,
): Record<string, FilterSpec> {
  return applyFocusKeyTableFilter(filters, lenses, points, timeZone, mode);
}

export function applyFocusKeyTableFilter(
  filters: Record<string, FilterSpec>,
  lenses: readonly PositionFocusLens[],
  points: readonly FocusTablePoint[],
  timeZone: string,
  mode: FocusTableMode,
): Record<string, FilterSpec> {
  const keyLenses = lenses.filter((l) => needsKeyResolution(l, mode));
  if (keyLenses.length === 0) return filters;
  const matchOpts: FocusMatchOpts = { timeZone };
  const matched = points.filter((p) =>
    keyLenses.every((l) => rowMatchesLens(pointToFocusRow(p), l, matchOpts)),
  );
  if (matched.length === 0) {
    return {
      ...filters,
      ...(mode === 'positionId'
        ? { id: { op: 'eq' as const, val: '00000000-0000-0000-0000-000000000000' } }
        : { mint_address: { op: 'in' as const, val: ['__focus_empty__'] } }),
    };
  }
  if (mode === 'positionId') {
    return { ...filters, id: { op: 'in', val: matched.map((p) => p.key) } };
  }
  let mints = [...new Set(matched.map((p) => p.mint_address))];
  const prior = filters.mint_address;
  if (prior?.op === 'in' && Array.isArray(prior.val)) {
    const allow = new Set(prior.val.map(String));
    mints = mints.filter((m) => allow.has(m));
    if (mints.length === 0) {
      return { ...filters, mint_address: { op: 'in', val: ['__focus_empty__'] } };
    }
  }
  return { ...filters, mint_address: { op: 'in', val: mints } };
}

/** Structured filters + key-resolved cohort — ONE call for Evidence / Simulate. */
export function buildFocusTableFilters(
  lenses: readonly PositionFocusLens[],
  points: readonly FocusTablePoint[],
  timeZone: string,
  mode: FocusTableMode,
): Record<string, FilterSpec> {
  return applyFocusKeyTableFilter(
    focusToStructuredFilters(lenses, { mode }),
    lenses,
    points,
    timeZone,
    mode,
  );
}

function exitMatches(reason: string | null, needle: string): boolean {
  if (needle === 'Other') {
    // "Other" is the residual closed bucket — handled by callers that already
    // know the row is closed; here we only reject known system/metric reasons.
    if (!reason) return true;
    const n = normalizeExitReasonFilter(reason);
    if (isMetricExitReason(n)) return false;
    return ![
      'TakeProfit',
      'StopLoss',
      'Dead',
      'Manual',
      'TrailingStop',
      'Stall',
      'TimeStop',
      'LiquidityExit',
      'Open',
      'NoEntry',
    ].includes(n);
  }
  if (needle === 'Metric+' || needle === 'Metric-') {
    if (!isMetricExitReason(reason)) return false;
    // Win/loss split needs pnl; without it accept any metric exit.
    return true;
  }
  if (needle === 'Metric' || needle === 'Metrics') {
    return isMetricExitReason(reason);
  }
  const normalizedNeedle = normalizeExitReasonFilter(needle);
  const normalizedReason = reason ? normalizeExitReasonFilter(reason) : '';
  if (!normalizedReason) return false;
  if (normalizedReason === normalizedNeedle) return true;
  // History-style substring needles (Trailing / Time / Liquidity).
  return normalizedReason.toLowerCase().includes(normalizedNeedle.toLowerCase());
}

export function rowMatchesLens(
  row: PositionFocusRow,
  lens: PositionFocusLens,
  opts?: FocusMatchOpts,
): boolean {
  switch (lens.kind) {
    case 'status':
      if (lens.status === 'open') return row.isOpen;
      if (lens.status === 'closed') return row.isClosed;
      return row.fired;
    case 'exit': {
      if (lens.reason === 'Metric+') {
        return isMetricExitReason(row.exit_reason) && (row.pnl_sol ?? 0) > 0;
      }
      if (lens.reason === 'Metric-') {
        return isMetricExitReason(row.exit_reason) && (row.pnl_sol ?? 0) <= 0;
      }
      return row.isClosed && exitMatches(row.exit_reason, lens.reason);
    }
    case 'outcome': {
      const pnl = row.pnl_sol;
      if (pnl == null || !Number.isFinite(pnl)) return false;
      return lens.outcome === 'win' ? pnl > 0 : pnl <= 0;
    }
    case 'migrated':
      return !!row.is_migrated === lens.migrated;
    case 'hold':
      return lens.mints.includes(row.mint_address);
    case 'wall':
      return lens.mints.includes(row.mint_address);
    case 'pct':
      return matchesPctFocus(row.pnl_pct, lens.lo, lens.hi);
    case 'pos':
      return row.id != null && row.id === lens.positionId;
    case 'band': {
      const hold = row.hold_secs;
      const pct = row.pnl_pct;
      if (hold == null || !Number.isFinite(hold) || pct == null || !Number.isFinite(pct)) {
        return false;
      }
      return (
        hold >= lens.holdLo &&
        hold <= lens.holdHi &&
        pct >= lens.pctLo &&
        pct <= lens.pctHi
      );
    }
    case 'heat':
    case 'day':
    case 'week': {
      const tz = opts?.timeZone;
      if (!tz) return false;
      return timeLensMatches(row.timeMs, lens, tz);
    }
  }
}

export function filterRowsByFocus<T extends PositionFocusRow>(
  rows: readonly T[],
  lenses: readonly PositionFocusLens[],
  opts?: FocusMatchOpts,
): T[] {
  if (lenses.length === 0) return rows as T[];
  return rows.filter((r) => lenses.every((l) => rowMatchesLens(r, l, opts)));
}

export interface FocusStructuredOpts {
  /** Evidence = `positionId`; Simulate = `mint` (episode keys). Default `positionId`. */
  mode?: FocusTableMode;
}

/**
 * Map lenses onto DataTable structured filters where the server can apply them.
 * Key-resolved leftovers (`heat` / `day` / `week` / `exit:Other` / Evidence `band`)
 * are merged by {@link applyFocusKeyTableFilter}.
 */
export function focusToStructuredFilters(
  lenses: readonly PositionFocusLens[],
  opts?: FocusStructuredOpts,
): Record<string, FilterSpec> {
  const mode: FocusTableMode = opts?.mode ?? 'positionId';
  // An exit needle owns `exit_reason` — don't let `status:fired` overwrite it.
  const exitOwnsReason = lenses.some((l) => l.kind === 'exit' && l.reason !== 'Other');
  const out: Record<string, FilterSpec> = {};
  for (const lens of lenses) {
    switch (lens.kind) {
      case 'status':
        if (mode === 'mint') {
          // Simulate rows carry `exit_reason`, not Evidence's `status` enum.
          if (lens.status === 'open' && !exitOwnsReason) {
            out.exit_reason = { op: 'eq', val: 'Open' };
          } else if (lens.status === 'closed') {
            // Key-resolved — must exclude Open and NoEntry together.
          } else if (lens.status === 'fired' && !exitOwnsReason) {
            out.exit_reason = { op: 'neq', val: 'NoEntry' };
          }
        } else if (lens.status === 'open') {
          out.status = { op: 'neq', val: 'End' };
        } else if (lens.status === 'closed') {
          out.status = { op: 'eq', val: 'End' };
        } else if (!exitOwnsReason) {
          // Fired = entered a position (not a NoEntry pad row).
          out.exit_reason = { op: 'neq', val: 'NoEntry' };
        }
        break;
      case 'exit': {
        if (lens.reason === 'Other') {
          // Residual closed bucket — key-resolved via chart points.
          break;
        }
        if (lens.reason === 'Metric+' || lens.reason === 'Metric-') {
          // Same `Metrics` contains as the unsplit tile, plus the win/loss cut the
          // chip promises (otherwise the table shows every metric exit).
          out.exit_reason = { op: 'contains', val: 'Metrics' };
          out.pnl_sol =
            lens.reason === 'Metric+' ? { op: 'gt', val: 0 } : { op: 'lte', val: 0 };
          break;
        }
        const needle =
          lens.reason === 'Take profit'
            ? 'TakeProfit'
            : lens.reason === 'Stop loss'
              ? 'StopLoss'
              : lens.reason === 'Trailing'
                ? 'Trailing'
                : lens.reason === 'Time'
                  ? 'Time'
                  : lens.reason === 'Liquidity'
                    ? 'Liquidity'
                    : lens.reason === 'Metric'
                      ? 'Metrics'
                      : lens.reason;
        out.exit_reason = { op: 'contains', val: needle };
        break;
      }
      case 'outcome':
        out.pnl_sol = lens.outcome === 'win' ? { op: 'gt', val: 0 } : { op: 'lte', val: 0 };
        break;
      case 'migrated':
        out.is_migrated = { op: 'eq', val: lens.migrated ? 'true' : 'false' };
        break;
      case 'hold':
      case 'wall':
        if (lens.mints.length > 0) {
          out.mint_address = { op: 'in', val: lens.mints };
        }
        break;
      case 'pct':
        // Open tails (`< -50%` / `≥ 500%`) use ±Infinity — wire via pctFocusFilter
        // (`lt` / `gte`). Column key is `pnl_pct` (Evidence whitelist).
        if (isPnlDistBucket(lens.lo, lens.hi)) {
          out.pnl_pct = pctFocusFilter(lens.lo, lens.hi);
        }
        break;
      case 'pos':
        if (mode === 'mint') {
          // Simulate episode key `mint::entry_time` — exact pair, not a UUID.
          const { mint_address, entry_time } = parseEpisodeRowKey(lens.positionId);
          out.mint_address = { op: 'eq', val: mint_address };
          if (entry_time) out.entry_time = { op: 'eq', val: entry_time };
        } else {
          out.id = { op: 'eq', val: lens.positionId };
        }
        break;
      case 'band':
        if (mode === 'mint') {
          // Simulate has `holding` → holding_secs; Evidence's `holding` is token
          // count — that surface key-resolves the band instead.
          out.holding = {
            op: 'between',
            min: lens.holdLo,
            max: lens.holdHi,
          };
          out.pnl_pct = { op: 'between', min: lens.pctLo, max: lens.pctHi };
        }
        break;
      case 'heat':
      case 'day':
      case 'week':
        // Key-resolved via {@link applyFocusKeyTableFilter}.
        break;
    }
  }
  return out;
}

/** EXIT_KINDS tile label → focus exit reason token. */
export function exitTileToFocusReason(label: string): string {
  switch (label) {
    case 'Take profit':
      return 'TakeProfit';
    case 'Stop loss':
      return 'StopLoss';
    case 'Metric+':
      return 'Metric+';
    case 'Metric-':
      return 'Metric-';
    case 'Metric':
      return 'Metrics';
    case 'Dead':
      return 'Dead';
    case 'Manual':
      return 'Manual';
    case 'Trailing':
      return 'TrailingStop';
    case 'Stall':
      return 'Stall';
    case 'Time':
      return 'TimeStop';
    case 'Liquidity':
      return 'LiquidityExit';
    default:
      return label;
  }
}
