/**
 * Chart → table (+ sibling charts) drill-down for Console History.
 *
 * A focus is a **lens on top of the parent cohort**. Timing charts (calendar /
 * heatmap) keep the parent grid with a selection ring; equity / distribution /
 * hold scatter / rule comparison refold on the matching slice — same predicate
 * as the table + filter-bar chip. Encoded in `hfocus` so reload / deep-links keep it.
 *
 * Wire forms:
 *   day:YYYY-MM-DD
 *   week:YYYY-MM-DD       the week's SUNDAY (the calendar column's first cell)
 *   heat:<dow>:<hour>     dow 0=Sun..6=Sat, hour 0..23 (user TZ)
 *   pct:<lo>:<hi>         adjacent edges from any `PNL_DIST_EDGE_SETS` preset;
 *                         `inf` / `-inf` for open ends
 *   rule:<uuid>
 *   pos:<uuid>            one close from the Hold-vs-PnL scatter
 *   band:<holdLo>:<holdHi>:<pctLo>:<pctHi>   drag-zoom on Hold vs PnL
 */

import {
  DOW_ROWS,
  dowHourInTz,
  isPnlDistBucket,
  shiftDayKey,
} from 'components/analytics/pnlSeries';
import { datetimeLocalToUtcWallClock } from 'utils/date';
import { formatDurationShort } from 'utils/format';
import type { FilterSpec } from 'components/table/numericFilter';

export type HistoryFocus =
  | { kind: 'day'; day: string }
  | { kind: 'week'; weekStart: string }
  | { kind: 'heat'; dow: number; hour: number }
  | { kind: 'pct'; lo: number; hi: number }
  | { kind: 'rule'; ruleId: string }
  | { kind: 'pos'; positionId: string }
  | { kind: 'holdBand'; holdLo: number; holdHi: number; pctLo: number; pctHi: number };

const DAY_RE = /^day:(\d{4}-\d{2}-\d{2})$/;
const WEEK_RE = /^week:(\d{4}-\d{2}-\d{2})$/;
const HEAT_RE = /^heat:([0-6]):(\d|1\d|2[0-3])$/;
const PCT_RE = /^pct:(-?inf|-?\d+(?:\.\d+)?):(-?inf|-?\d+(?:\.\d+)?)$/;
const RULE_RE = /^rule:([0-9a-fA-F-]{36})$/;
const POS_RE = /^pos:([0-9a-fA-F-]{36})$/;
const BAND_RE =
  /^band:(-?\d+(?:\.\d+)?):(-?\d+(?:\.\d+)?):(-?\d+(?:\.\d+)?):(-?\d+(?:\.\d+)?)$/;

function roundBand(n: number): number {
  // Stable URL + enough precision for brush zoom.
  return Math.round(n * 1e4) / 1e4;
}

function encodeEdge(n: number): string {
  if (n === Infinity) return 'inf';
  if (n === -Infinity) return '-inf';
  return String(n);
}

function parseEdge(s: string): number | null {
  if (s === 'inf') return Infinity;
  if (s === '-inf') return -Infinity;
  const n = Number(s);
  return Number.isFinite(n) ? n : null;
}

export function parseHistoryFocus(raw: string | null | undefined): HistoryFocus | null {
  if (!raw) return null;
  let m = DAY_RE.exec(raw);
  if (m) return { kind: 'day', day: m[1]! };
  m = WEEK_RE.exec(raw);
  if (m) return { kind: 'week', weekStart: m[1]! };
  m = HEAT_RE.exec(raw);
  if (m) return { kind: 'heat', dow: Number(m[1]), hour: Number(m[2]) };
  m = PCT_RE.exec(raw);
  if (m) {
    const lo = parseEdge(m[1]!);
    const hi = parseEdge(m[2]!);
    if (lo == null || hi == null || !(lo < hi)) return null;
    // Only accept adjacent pairs from a density preset so focus ↔ bar stay 1:1.
    if (!isPnlDistBucket(lo, hi)) return null;
    return { kind: 'pct', lo, hi };
  }
  m = RULE_RE.exec(raw);
  if (m) return { kind: 'rule', ruleId: m[1]! };
  m = POS_RE.exec(raw);
  if (m) return { kind: 'pos', positionId: m[1]! };
  m = BAND_RE.exec(raw);
  if (m) {
    const holdLo = Number(m[1]);
    const holdHi = Number(m[2]);
    const pctLo = Number(m[3]);
    const pctHi = Number(m[4]);
    if (!(holdLo < holdHi) || !(pctLo < pctHi)) return null;
    return { kind: 'holdBand', holdLo, holdHi, pctLo, pctHi };
  }
  return null;
}

export function serializeHistoryFocus(focus: HistoryFocus | null | undefined): string | null {
  if (!focus) return null;
  switch (focus.kind) {
    case 'day':
      return `day:${focus.day}`;
    case 'week':
      return `week:${focus.weekStart}`;
    case 'heat':
      return `heat:${focus.dow}:${focus.hour}`;
    case 'pct':
      return `pct:${encodeEdge(focus.lo)}:${encodeEdge(focus.hi)}`;
    case 'rule':
      return `rule:${focus.ruleId}`;
    case 'pos':
      return `pos:${focus.positionId}`;
    case 'holdBand':
      return `band:${roundBand(focus.holdLo)}:${roundBand(focus.holdHi)}:${roundBand(focus.pctLo)}:${roundBand(focus.pctHi)}`;
  }
}

/** Toggle helper — clicking the active cell again clears the focus. */
export function toggleHistoryFocus(
  current: HistoryFocus | null,
  next: HistoryFocus,
): HistoryFocus | null {
  if (!current) return next;
  if (serializeHistoryFocus(current) === serializeHistoryFocus(next)) return null;
  return next;
}

export function historyFocusLabel(
  focus: HistoryFocus,
  ruleNameOf?: (id: string) => string | null,
): string {
  switch (focus.kind) {
    case 'day':
      return focus.day;
    case 'week':
      return `${focus.weekStart} → ${shiftDayKey(focus.weekStart, 6).slice(5)}`;
    case 'heat': {
      const dow = DOW_ROWS.find((r) => r.dow === focus.dow)?.label ?? `D${focus.dow}`;
      return `${dow} ${String(focus.hour).padStart(2, '0')}:00`;
    }
    case 'pct': {
      if (focus.lo === -Infinity) return `< ${focus.hi}%`;
      if (focus.hi === Infinity) return `≥ ${focus.lo}%`;
      return `${focus.lo}…${focus.hi}%`;
    }
    case 'rule':
      return ruleNameOf?.(focus.ruleId) ?? `${focus.ruleId.slice(0, 8)}…`;
    case 'pos':
      return `close ${focus.positionId.slice(0, 8)}…`;
    case 'holdBand':
      return `${formatDurationShort(focus.holdLo)}–${formatDurationShort(focus.holdHi)} · ${focus.pctLo.toFixed(0)}…${focus.pctHi.toFixed(0)}%`;
  }
}

/** `[from, to)` UTC ISO bounds of `days` civil days starting at `startDay`, in
 *  `timeZone`. Both the day lens and the week lens derive from this one span so
 *  they round DST the same way. The date walk is the shared `shiftDayKey` the
 *  calendar grid lays its columns out with — a focus window can never address a
 *  date the cell that produced it wasn't showing. */
export function spanBoundsUtcIso(
  startDay: string,
  days: number,
  timeZone: string,
): { fromIso: string; toIso: string } {
  const fromWall = datetimeLocalToUtcWallClock(`${startDay}T00:00:00`, timeZone, 'lower');
  const toWall = datetimeLocalToUtcWallClock(
    `${shiftDayKey(startDay, days)}T00:00:00`,
    timeZone,
    'lower',
  );
  return { fromIso: `${fromWall}.000Z`, toIso: `${toWall}.000Z` };
}

/** `[from, to)` UTC ISO bounds of a civil day in `timeZone`. */
export function dayBoundsUtcIso(
  day: string,
  timeZone: string,
): { fromIso: string; toIso: string } {
  return spanBoundsUtcIso(day, 1, timeZone);
}

/** `[from, to)` UTC ISO bounds of the calendar week starting at `weekStart`
 *  (its Sunday) in `timeZone`. */
export function weekBoundsUtcIso(
  weekStart: string,
  timeZone: string,
): { fromIso: string; toIso: string } {
  return spanBoundsUtcIso(weekStart, 7, timeZone);
}

/** Intersect two half-open UTC windows; either bound may be null (= unbounded). */
export function intersectUtcWindow(
  aFrom: string | null,
  aTo: string | null,
  bFrom: string | null,
  bTo: string | null,
): { fromIso: string | null; toIso: string | null } | null {
  const from = Math.max(
    aFrom ? Date.parse(aFrom) : -Infinity,
    bFrom ? Date.parse(bFrom) : -Infinity,
  );
  const to = Math.min(aTo ? Date.parse(aTo) : Infinity, bTo ? Date.parse(bTo) : Infinity);
  if (!(from < to)) return null;
  return {
    fromIso: from === -Infinity ? null : new Date(from).toISOString(),
    toIso: to === Infinity ? null : new Date(to).toISOString(),
  };
}

/** Server `pnl_pct` filter for a distribution bucket (half-open ≈ via between/lt/gte). */
export function pctFocusFilter(lo: number, hi: number): FilterSpec {
  if (lo === -Infinity) return { op: 'lt', val: hi };
  if (hi === Infinity) return { op: 'gte', val: lo };
  // BETWEEN is inclusive; nudge the top so the next bucket's edge isn't double-counted.
  return { op: 'between', min: lo, max: hi - 1e-6 };
}

/** True when `exitIso` lands in the focused heat cell (user TZ). */
export function matchesHeatFocus(
  exitIso: string | null | undefined,
  focus: Extract<HistoryFocus, { kind: 'heat' }>,
  timeZone: string,
): boolean {
  if (!exitIso) return false;
  const ms = Date.parse(exitIso);
  if (!Number.isFinite(ms)) return false;
  const { dow, hour } = dowHourInTz(ms, timeZone);
  return dow === focus.dow && hour === focus.hour;
}

/** True when hold span + PnL% fall inside a Hold-vs-PnL brush band. */
export function matchesHoldBandFocus(
  entryIso: string | null | undefined,
  exitIso: string | null | undefined,
  pnlPct: number | null | undefined,
  focus: Extract<HistoryFocus, { kind: 'holdBand' }>,
): boolean {
  if (!entryIso || !exitIso || pnlPct == null || !Number.isFinite(pnlPct)) return false;
  const start = Date.parse(entryIso);
  const end = Date.parse(exitIso);
  if (!Number.isFinite(start) || !Number.isFinite(end)) return false;
  return matchesHoldBandSecs((end - start) / 1000, pnlPct, focus);
}

/** Same band test when hold seconds are already on the wire (closes-series). */
export function matchesHoldBandSecs(
  holdSecs: number | null | undefined,
  pnlPct: number | null | undefined,
  focus: Extract<HistoryFocus, { kind: 'holdBand' }>,
): boolean {
  if (holdSecs == null || !Number.isFinite(holdSecs) || pnlPct == null || !Number.isFinite(pnlPct)) {
    return false;
  }
  return (
    holdSecs >= focus.holdLo &&
    holdSecs <= focus.holdHi &&
    pnlPct >= focus.pctLo &&
    pnlPct <= focus.pctHi
  );
}

/** Matches `pctFocusFilter` (half-open on the closed top edge). */
export function matchesPctFocus(
  pnlPct: number | null | undefined,
  lo: number,
  hi: number,
): boolean {
  if (pnlPct == null || !Number.isFinite(pnlPct)) return false;
  if (lo === -Infinity) return pnlPct < hi;
  if (hi === Infinity) return pnlPct >= lo;
  return pnlPct >= lo && pnlPct < hi;
}

/** Minimal close shape for client-side focus filtering (B2 series + table rows). */
export interface FocusableClose {
  id: string;
  exit_time: string;
  rule_id: string | null;
  entry_sol: number;
  pnl_sol: number;
  hold_secs: number | null;
}

/**
 * Narrow a closes list to the focused slice — SSOT for History chart refolds.
 * Timing charts keep the parent cohort; everything else + the table use this.
 */
export function filterClosesForFocus<T extends FocusableClose>(
  closes: readonly T[],
  focus: HistoryFocus | null,
  timeZone: string,
): T[] {
  if (!focus) return closes as T[];
  switch (focus.kind) {
    case 'day':
    case 'week': {
      const span =
        focus.kind === 'day'
          ? dayBoundsUtcIso(focus.day, timeZone)
          : weekBoundsUtcIso(focus.weekStart, timeZone);
      const from = Date.parse(span.fromIso);
      const to = Date.parse(span.toIso);
      return closes.filter((c) => {
        const t = Date.parse(c.exit_time);
        return Number.isFinite(t) && t >= from && t < to;
      });
    }
    case 'heat':
      return closes.filter((c) => matchesHeatFocus(c.exit_time, focus, timeZone));
    case 'pct':
      return closes.filter((c) =>
        matchesPctFocus(
          c.entry_sol > 0 ? (c.pnl_sol / c.entry_sol) * 100 : null,
          focus.lo,
          focus.hi,
        ),
      );
    case 'rule':
      return closes.filter((c) => c.rule_id === focus.ruleId);
    case 'pos':
      return closes.filter((c) => c.id === focus.positionId);
    case 'holdBand':
      return closes.filter((c) =>
        matchesHoldBandSecs(
          c.hold_secs,
          c.entry_sol > 0 ? (c.pnl_sol / c.entry_sol) * 100 : null,
          focus,
        ),
      );
  }
}
