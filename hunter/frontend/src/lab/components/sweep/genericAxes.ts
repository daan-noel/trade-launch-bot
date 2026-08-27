// Generic sweep **axes** (redesign FE5.1) — the frontend mirror of the backend
// `hunter/lab/src/sweep/generic/axes.rs` wire model. An axis is one swept
// dimension; each combo picks one value per axis, assembling one `RuleParams`.
//
// The registry (`GET /api/meta/strategy-registry`) drives which groups/metrics/
// operators an axis can name — so a metric added in Rust becomes a sweepable axis
// with zero frontend change (extensibility contract, backend plan §8).
//
// This replaces the three per-strategy static axis grids (`TPSL{1,2}_AXES` /
// `SWING1_AXES`) with one registry-driven builder.

import type { GroupSpec, Operator, StrategyRegistry } from 'lib/strategy/registry';
import { isPnlAdvancedMetric } from 'lib/strategy/validate';
import {
  formatWindowSpec,
  parseWindowSpec,
  unitSuffix,
  type WindowSpec,
  type WindowUnit,
} from 'lib/strategy/windowSpec';

/** Which kind of dimension an axis sweeps. `metric` conditions a side; the TP/SL
 *  axes set the rule's take-profit / stop-loss %. */
export type AxisKind = 'metric' | 'take_profit' | 'stop_loss';

/** Which side of the rule a metric axis conditions. */
export type MetricAxisSide = 'entry' | 'exit';

/** One editor-row of the axis builder (form state — values are raw text so a
 *  half-typed list/range survives keystrokes). */
export interface GenericAxisRow {
  /** Stable client id (list-render key + remove target). */
  id: string;
  kind: AxisKind;
  /** Metric axes only — the side its condition applies to. */
  side: MetricAxisSide;
  /** Metric axes only — the registry group name (e.g. `m_snapshot`). */
  group: string;
  /** Metric axes only — the registry metric name (e.g. `time`). */
  metric: string;
  /** Metric axes only — the comparison operator. */
  operator: Operator;
  /** Dynamic-metric axes only — the trailing window SIZE; '' = unset. The unit it
   *  counts in is {@link GenericAxisRow.windowUnit} — a bare number is not a window. */
  window: string;
  /** What this row's window counts in. `undefined` reads as `'sec'`, so a row from
   *  before the other bases existed is a wall-clock row. */
  windowUnit?: WindowUnit;
  /** How many units back from *now* the window ENDS, as raw text; '' = ends at now.
   *  What lets a sweep search a CAUSAL gate — the tape before the trigger, with no
   *  way for the trigger's own slot or print to leak into it. */
  lag?: string;
  /** Swept values: a comma list (`5, 10, 15`), ranges (`10..40 step 10`), and —
   *  on metric axes — the `off` sentinel (that combo omits the condition). */
  valuesText: string;
}

/** The wire form of one axis — matches the backend `AxisSpec` serde struct.
 *  `null` in `values` is the `off` sentinel (metric axes only). */
export interface AxisSpecWire {
  kind: AxisKind;
  side?: MetricAxisSide;
  group?: string;
  metric?: string;
  operator?: Operator;
  /** A bare number is SECONDS — what every stored sweep config holds and what this
   *  field has always meant. A string is a full span (`"30sl@1"`, `"20p"`) in the
   *  same grammar a chart legend and a persisted exit reason use. */
  window?: number | string;
  values: (number | null)[];
}

/** A safe upper bound on how many values one range fragment may expand to, so a
 *  fat-fingered `0..1000000 step 0.1` can't lock the tab. */
const MAX_RANGE_VALUES = 1000;

const RANGE_RE =
  /^(-?\d+(?:\.\d+)?)\s*\.\.\s*(-?\d+(?:\.\d+)?)(?:\s*(?:step|:)\s*(-?\d+(?:\.\d+)?))?$/i;

/**
 * Parse the axis values box into a deduped list: `off` first (if present), then
 * numbers ascending — mirroring the backend's resolve order so pick 0 is always
 * the off sentinel. Each comma-separated fragment is a plain number (`50`), an
 * inclusive range `lo..hi[ step s]` (`10..40 step 10` → 10,20,30,40; `1..5` →
 * 1,5), or the literal `off` (→ `null`). Blank / unrecognized fragments and a
 * non-positive step are dropped (see [`invalidValueFragments`] to surface them).
 */
export function parseValueList(text: string): (number | null)[] {
  let off = false;
  const seen = new Set<number>();
  const out: number[] = [];
  const push = (n: number) => {
    if (Number.isFinite(n) && !seen.has(n)) {
      seen.add(n);
      out.push(n);
    }
  };
  for (const raw of text.split(',')) {
    const frag = raw.trim();
    if (frag === '') continue;
    if (/^off$/i.test(frag)) {
      off = true;
      continue;
    }
    const range = RANGE_RE.exec(frag);
    if (range) {
      let lo = parseFloat(range[1]);
      let hi = parseFloat(range[2]);
      if (lo > hi) [lo, hi] = [hi, lo];
      const step = range[3] != null ? parseFloat(range[3]) : NaN;
      if (!Number.isFinite(step) || step <= 0) {
        // No usable step ⇒ just the two endpoints.
        push(lo);
        push(hi);
        continue;
      }
      // Round to the step's decimal precision so float drift doesn't spawn
      // 10.000000000000002 near the top of the range.
      const dp = (String(step).split('.')[1] ?? '').length;
      const round = (v: number) => Number(v.toFixed(Math.min(dp + 2, 12)));
      let count = 0;
      for (let v = lo; v <= hi + 1e-9 && count < MAX_RANGE_VALUES; v += step, count++) {
        push(round(v));
      }
      continue;
    }
    push(Number(frag));
  }
  out.sort((a, b) => a - b);
  return off ? [null, ...out] : out;
}

/** The fragments of a values box that parse to nothing — neither a number, a
 *  range, `off`, nor blank. Surfaced as a row error so a typo (`of`, `1O`)
 *  can't silently vanish from the grid. */
export function invalidValueFragments(text: string): string[] {
  const bad: string[] = [];
  for (const raw of text.split(',')) {
    const frag = raw.trim();
    if (frag === '' || /^off$/i.test(frag) || RANGE_RE.test(frag)) continue;
    if (Number.isFinite(Number(frag))) continue;
    bad.push(frag);
  }
  return bad;
}

/** The registry group a metric row names, if any. */
function rowGroup(row: GenericAxisRow, reg: StrategyRegistry | undefined): GroupSpec | undefined {
  return reg?.groups.find((g) => g.name === row.group);
}

/** True when a metric row's group needs a trailing window (dynamic group). Every
 *  basis is sweepable: the backend `resolve_one` resolves the row's span and
 *  `assemble` writes the size param that span's own unit spells. */
export function rowNeedsWindow(row: GenericAxisRow, reg: StrategyRegistry | undefined): boolean {
  return row.kind === 'metric' && rowGroup(row, reg)?.kind === 'dynamic';
}

/** What a row's window counts in. Absent ⇒ seconds, so a row authored before the
 *  other bases existed keeps its meaning. */
export function axisRowUnit(row: GenericAxisRow): WindowUnit {
  return row.windowUnit ?? 'sec';
}

/** The parsed, positive span a row carries, or `null` (static / unset). */
export function axisRowWindowSpec(row: GenericAxisRow): WindowSpec | null {
  const size = Number(row.window);
  if ((row.window ?? '').trim() === '' || !Number.isFinite(size) || size <= 0) return null;
  const lagText = (row.lag ?? '').trim();
  const lag = lagText === '' ? 0 : Number(lagText);
  if (!Number.isFinite(lag) || lag < 0) return null;
  return { size, lag, unit: axisRowUnit(row) };
}

/** The parsed values for one row (deduped; `off` first, then ascending). */
export function rowValues(row: GenericAxisRow): (number | null)[] {
  return parseValueList(row.valuesText);
}

/**
 * Per-row validation, mirroring the backend `resolve_one`. Returns an error
 * string to surface under the row, or `null` when the row is a valid axis.
 */
export function axisRowError(
  row: GenericAxisRow,
  reg: StrategyRegistry | undefined,
): string | null {
  const bad = invalidValueFragments(row.valuesText);
  if (bad.length > 0) return `unrecognized value${bad.length > 1 ? 's' : ''}: ${bad.join(', ')}`;
  const values = rowValues(row);
  if (values.length === 0) return 'add at least one value';
  if (row.kind === 'take_profit' || row.kind === 'stop_loss') {
    if (values.includes(null)) return "'off' only applies to metric axes — omit the axis instead";
    if (values.some((v) => v != null && v <= 0)) return 'TP / SL values must be > 0';
    return null;
  }
  // metric axis
  if (values.every((v) => v == null)) return "add at least one number besides 'off'";
  const group = rowGroup(row, reg);
  if (!row.group || !group) return 'pick a metric group';
  // Position-scoped groups (`m_position`) read NaN before entry, so an entry
  // condition on one can never fire — the backend `resolve_one` rejects it. Flag
  // it here rather than admit a dead axis (mirrors the sweep backend contract).
  if (row.side === 'entry' && group.scope === 'position')
    return `${group.name} is exit-only (no value before entry) — move it to the exit column`;
  if (!row.metric || !group.metrics.some((m) => m.name === row.metric)) return 'pick a metric';
  if (!row.operator) return 'pick an operator';
  if (group.kind === 'dynamic') {
    const u = unitSuffix(axisRowUnit(row));
    const w = Number(row.window);
    if (!row.window || !Number.isFinite(w) || w <= 0) return `window (${u}) > 0 required`;
    // `0` is a real value of the lag's domain (end at now), so only a negative or
    // unparseable entry is an error.
    const lagText = (row.lag ?? '').trim();
    if (lagText !== '') {
      const lag = Number(lagText);
      if (!Number.isFinite(lag) || lag < 0) return `lag (${u}) must be a number ≥ 0`;
    }
  }
  return null;
}

/**
 * Cross-row check: an exit `m_position.pnl` axis that restates TP/SL sugar
 * (`>=` overlapping a take_profit value, or `<=` overlapping `−stop_loss`) is
 * the same footgun as duplicating sugar in the rule editor. Prefer the TP/SL
 * axes for those bounds.
 */
export function pnlAxisSugarDuplicateError(rows: GenericAxisRow[]): string | null {
  const tpValues = new Set<number>();
  const slValues = new Set<number>();
  for (const r of rows) {
    if (r.kind === 'take_profit') {
      for (const v of rowValues(r)) if (v != null && v > 0) tpValues.add(v);
    } else if (r.kind === 'stop_loss') {
      for (const v of rowValues(r)) if (v != null && v > 0) slValues.add(v);
    }
  }
  if (tpValues.size === 0 && slValues.size === 0) return null;

  for (const r of rows) {
    if (r.kind !== 'metric' || r.side !== 'exit') continue;
    if (!isPnlAdvancedMetric(r.group, r.metric)) continue;
    const values = rowValues(r).filter((v): v is number => v != null);
    if (r.operator === '>=' && values.some((v) => [...tpValues].some((t) => Math.abs(v - t) < 1e-9))) {
      return 'exit m_position.pnl duplicates a TP axis value — use the TP % axis (or drop that pnl value)';
    }
    if (
      r.operator === '<=' &&
      values.some((v) => [...slValues].some((s) => Math.abs(v - -s) < 1e-9))
    ) {
      return 'exit m_position.pnl duplicates an SL axis value — use the SL % axis (or drop that pnl value)';
    }
  }
  return null;
}

/** Serialize the editor rows into the wire `AxisSpec[]` (metric rows drop the
 *  window on static groups; TP/SL rows drop the metric fields). Skips rows with
 *  no parsed values. */
export function serializeAxisRows(
  rows: GenericAxisRow[],
  reg: StrategyRegistry | undefined,
): AxisSpecWire[] {
  const out: AxisSpecWire[] = [];
  for (const row of rows) {
    const values = rowValues(row);
    if (values.length === 0) continue;
    if (row.kind === 'take_profit' || row.kind === 'stop_loss') {
      out.push({ kind: row.kind, values });
      continue;
    }
    const spec: AxisSpecWire = {
      kind: 'metric',
      side: row.side,
      group: row.group,
      metric: row.metric,
      operator: row.operator,
      values,
    };
    if (rowNeedsWindow(row, reg)) {
      const w = axisRowWindowSpec(row);
      // An unlagged wall-clock span stays a bare NUMBER on the wire, so a config
      // saved before the other bases existed round-trips byte-identically.
      spec.window =
        w == null
          ? undefined
          : w.unit === 'sec' && w.lag === 0
            ? w.size
            : formatWindowSpec(w);
    }
    out.push(spec);
  }
  return out;
}

/** Projected combos = product of every axis's value count (0 if any axis is
 *  empty). Ignores rows still missing a group/metric so the badge tracks what
 *  would actually be sent. */
export function comboCount(rows: GenericAxisRow[], reg: StrategyRegistry | undefined): number {
  const specs = serializeAxisRows(rows, reg);
  if (specs.length === 0) return 0;
  return specs.reduce((acc, a) => acc * a.values.length, 1);
}

/** Rebuild editor rows from a wire `{ axes: AxisSpec[] }` (sweep run or discovery seed). */
export function axesSpecToRows(spec: { axes?: AxisSpecWire[] } | AxisSpecWire[] | null | undefined): GenericAxisRow[] {
  const axes = Array.isArray(spec) ? spec : spec?.axes;
  if (!Array.isArray(axes)) return [];
  return axes.map((a) => ({
    ...newAxisRow((a.kind as AxisKind) ?? 'metric'),
    side: (a.side as MetricAxisSide) ?? 'entry',
    group: a.group ?? '',
    metric: a.metric ?? '',
    operator: a.operator ?? '>',
    // A number is seconds; a string carries its own basis. Anything unparseable
    // leaves the row blank rather than inventing a span the config never named.
    ...(() => {
      const w = a.window == null ? null : parseWindowSpec(String(a.window));
      return {
        window: w ? String(w.size) : '',
        windowUnit: w?.unit ?? ('sec' as const),
        lag: w && w.lag > 0 ? String(w.lag) : '',
      };
    })(),
    valuesText: (a.values ?? []).map((v) => (v == null ? 'off' : v)).join(', '),
  }));
}

let rowSeq = 0;
/** A fresh axis row with sensible defaults for its kind. The id mixes a counter
 *  with a random suffix so a freshly-added row can never collide with a row
 *  restored from `localStorage` (whose ids were minted in a prior session). */
export function newAxisRow(
  kind: AxisKind,
  reg?: StrategyRegistry,
  side: MetricAxisSide = 'entry',
): GenericAxisRow {
  const firstOp = reg?.operators[0] ?? '>';
  return {
    id: `axis-${rowSeq++}-${Math.random().toString(36).slice(2, 7)}`,
    kind,
    side,
    group: '',
    metric: '',
    operator: firstOp,
    window: '',
    windowUnit: 'sec',
    lag: '',
    valuesText: kind === 'take_profit' ? '50, 100, 200' : kind === 'stop_loss' ? '30, 50' : '',
  };
}
