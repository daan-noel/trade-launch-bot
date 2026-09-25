// Grouped-sweep **axes**: the frontend mirror of the backend
// `hunter/lab/src/sweep/generic/axes.rs` wire model. An axis is one swept dimension;
// each combo picks one value per axis and the picks assemble one rule:
//
//   metric       one read `metric @tag [span] <operator> value`. `side: entry` puts it
//                in `enter.filters` (the coin must pass it to be bought); `side: exit`
//                makes it its own `always` line that sells everything when it holds.
//                The value `off` (wire `null`) leaves the condition out of that combo.
//   take_profit  each value sets the rule's take-profit %.
//   stop_loss    each value sets the rule's stop-loss %.
//
// Two axes on the same read join into one condition: AND when both can hold
// (`> 5` and `< 50`), else OR (`< 5` or `> 50`). A read is checked with the rule
// editor's own `checkRef`, so an axis the engine would refuse shows its error while
// typing.

import { builtinTagNames, checkRef, parseTagRef, refLabel, type MetricRef } from 'lib/strategy/metricRef';
import { findMetric, type Operator, type StrategyRegistry } from 'lib/strategy/registry';
import { formatMetricThreshold } from 'lib/strategy/windowSpec';

/** What an axis sweeps: a metric condition, or the rule's TP / SL %. */
export type AxisKind = 'metric' | 'take_profit' | 'stop_loss';

/** Where a metric axis's condition goes: `enter.filters`, or its own sell line. */
export type MetricAxisSide = 'entry' | 'exit';

/** One editor row of the axis builder. Values stay raw text so a half-typed list or
 *  range survives keystrokes. */
export interface GenericAxisRow {
  /** Stable client id (list key, remove and drag target). */
  id: string;
  kind: AxisKind;
  /** Metric axes only. */
  side: MetricAxisSide;
  /** Metric axes only: the read. */
  ref: MetricRef;
  /** Metric axes only. */
  operator: Operator;
  /** `5, 10, 15`, ranges `10..40 step 10`, and on metric axes `off`. */
  valuesText: string;
}

/** The wire form of one axis: the backend `AxisSpec`. `null` in `values` is `off`
 *  (metric axes only). */
export interface AxisSpecWire {
  /** Absent reads as `metric`. */
  kind?: AxisKind;
  side?: MetricAxisSide;
  metric?: string;
  tag?: string;
  span?: string;
  slice?: string;
  operator?: Operator;
  values: (number | null)[];
}

/** Most values one range fragment may expand to, so `0..1000000 step 0.1` cannot
 *  lock the tab. */
const MAX_RANGE_VALUES = 1000;

const RANGE_RE =
  /^(-?\d+(?:\.\d+)?)\s*\.\.\s*(-?\d+(?:\.\d+)?)(?:\s*(?:step|:)\s*(-?\d+(?:\.\d+)?))?$/i;

/**
 * Parse a values box into a deduped list: `off` first (when present), then numbers
 * ascending, the backend's resolve order. A fragment is a number (`50`), an inclusive
 * range `lo..hi[ step s]` (`10..40 step 10` -> 10, 20, 30, 40; `1..5` -> 1, 5), or
 * `off` (-> `null`). Anything else is dropped; {@link invalidValueFragments} names it.
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
        push(lo);
        push(hi);
        continue;
      }
      // Round to the step's precision so float drift cannot add 10.000000000000002.
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

/** The fragments of a values box that parse to nothing (not a number, a range, `off`
 *  or blank), so a typo like `of` or `1O` shows instead of vanishing from the grid. */
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

/** A row's parsed values (`off` first, then ascending). */
export function rowValues(row: GenericAxisRow): (number | null)[] {
  return parseValueList(row.valuesText);
}

/**
 * Why a row is not a valid axis, or `null`: the backend `resolve_one` plus the read
 * check. `definedTags` = the tag names the run's tags document defines; the backend
 * refuses an axis reading any other tag.
 */
export function axisRowError(
  row: GenericAxisRow,
  reg: StrategyRegistry | undefined,
  definedTags: readonly string[] = [],
): string | null {
  const bad = invalidValueFragments(row.valuesText);
  if (bad.length > 0) return `unrecognized value${bad.length > 1 ? 's' : ''}: ${bad.join(', ')}`;
  const values = rowValues(row);
  if (values.length === 0) return 'add at least one value';
  if (row.kind === 'take_profit' || row.kind === 'stop_loss') {
    if (values.includes(null)) return "'off' applies to metric axes only: remove the axis instead";
    if (values.some((v) => v != null && v <= 0)) return 'TP / SL values must be above 0';
    return null;
  }
  if (values.every((v) => v == null)) return "add at least one number besides 'off'";
  if (!row.ref.metric) return 'pick a metric';
  const refErr = checkRef(reg, row.ref);
  if (refErr) return refErr;
  const tag = row.ref.tag ? parseTagRef(row.ref.tag).name : null;
  if (tag && !builtinTagNames(reg).includes(tag) && !definedTags.includes(tag)) {
    return `the run's tags define no \`${tag}\`: add it under Tags, or pick a scope fingerprint that has it`;
  }
  if (row.side === 'entry' && findMetric(reg, row.ref.metric)?.position) {
    return `${row.ref.metric} reads our position, which has no value before the buy: move it to the exit side`;
  }
  return null;
}

/** The rows as wire axes. Rows without a parsed value are skipped. */
export function serializeAxisRows(rows: GenericAxisRow[]): AxisSpecWire[] {
  const out: AxisSpecWire[] = [];
  for (const row of rows) {
    const values = rowValues(row);
    if (values.length === 0) continue;
    if (row.kind === 'take_profit' || row.kind === 'stop_loss') {
      out.push({ kind: row.kind, values });
      continue;
    }
    out.push({
      kind: 'metric',
      side: row.side,
      metric: row.ref.metric,
      ...(row.ref.tag ? { tag: row.ref.tag } : {}),
      ...(row.ref.span ? { span: row.ref.span } : {}),
      ...(row.ref.slice ? { slice: row.ref.slice } : {}),
      operator: row.operator,
      values,
    });
  }
  return out;
}

/** Combos = the product of every axis's value count (0 with no axis). */
export function comboCount(rows: GenericAxisRow[]): number {
  const specs = serializeAxisRows(rows);
  return specs.length === 0 ? 0 : specs.reduce((acc, a) => acc * a.values.length, 1);
}

/** The fingerprint tags the rows read (without `!`; built-in wallet classes left out):
 *  what the run's tags document has to define. */
export function axisTagNames(rows: GenericAxisRow[], reg: StrategyRegistry | undefined): string[] {
  const builtin = builtinTagNames(reg);
  const out: string[] = [];
  for (const r of rows) {
    if (r.kind !== 'metric' || !r.ref.tag) continue;
    const { name } = parseTagRef(r.ref.tag);
    if (!builtin.includes(name) && !out.includes(name)) out.push(name);
  }
  return out;
}

/** Editor rows from wire axes (a stored run's `axes_spec.axes` or a discovery seed). */
export function axesSpecToRows(spec: { axes?: AxisSpecWire[] } | AxisSpecWire[] | null | undefined): GenericAxisRow[] {
  const axes = Array.isArray(spec) ? spec : spec?.axes;
  if (!Array.isArray(axes)) return [];
  return axes.map((a) => {
    const ref: MetricRef = { metric: a.metric ?? '' };
    if (a.tag) ref.tag = a.tag;
    if (a.span) ref.span = a.span;
    if (a.slice) ref.slice = a.slice;
    return {
      ...newAxisRow(a.kind ?? 'metric', undefined, a.side ?? 'entry'),
      ref,
      operator: a.operator ?? '>=',
      valuesText: (a.values ?? []).map((v) => (v == null ? 'off' : v)).join(', '),
    };
  });
}

/** Rows restored from storage, or `null` when they are not this shape (a row saved
 *  before the read replaced `group` / `metric` / `window`). */
export function storedAxisRows(v: unknown): GenericAxisRow[] | null {
  if (!Array.isArray(v)) return null;
  const ok = v.every(
    (r) =>
      !!r &&
      typeof r === 'object' &&
      typeof (r as GenericAxisRow).id === 'string' &&
      typeof (r as GenericAxisRow).valuesText === 'string' &&
      !!(r as GenericAxisRow).ref &&
      typeof (r as GenericAxisRow).ref.metric === 'string',
  );
  return ok ? (v as GenericAxisRow[]) : null;
}

/** What an axis does, in one line:
 *  `entry filter: m_flow.buy_sol @!volume [10s] >= each of 1, 2, 3`. */
export function axisSummary(row: GenericAxisRow): string {
  const values = rowValues(row);
  const nums = values.filter((v): v is number => v != null).map(formatMetricThreshold);
  const list = nums.length > 1 ? `each of ${nums.join(', ')}` : (nums[0] ?? '...');
  const off = values.includes(null) ? ', or left out' : '';
  if (row.kind === 'take_profit') return `take profit at ${list} %`;
  if (row.kind === 'stop_loss') return `stop loss at ${list} %`;
  const where = row.side === 'entry' ? 'entry filter' : 'exit line, sells everything when';
  return `${where}: ${refLabel(row.ref)} ${row.operator} ${list}${off}`;
}

let rowSeq = 0;
/** A fresh axis row. The id mixes a counter with a random suffix so a new row never
 *  collides with a row restored from storage (minted in an earlier session). */
export function newAxisRow(
  kind: AxisKind,
  reg?: StrategyRegistry,
  side: MetricAxisSide = 'entry',
  ref: MetricRef = { metric: '' },
): GenericAxisRow {
  return {
    id: `axis-${rowSeq++}-${Math.random().toString(36).slice(2, 7)}`,
    kind,
    side,
    ref,
    operator: reg?.operators.includes('>=') ? '>=' : (reg?.operators[0] ?? '>='),
    valuesText: kind === 'take_profit' ? '50, 100, 200' : kind === 'stop_loss' ? '30, 50' : '',
  };
}
