// Helpers for the lab metric-pane overlay: extract which metrics/windows a rule
// uses, evaluate conditions the same way the engine does (DNF: OR of AND arms +
// `=` bucket tolerance), and find first entry/exit fire indices for chart markers.

import type {
  ChartEventMarker,
  ChartTimeBand,
  ChartTimeSpan,
  ChartValueLane,
} from 'components/token-price-chart';
import { formatConditions, type Condition, type ConditionExpr } from './grammar';
import type { RuleParams, SideConditions } from './ruleParams';
import type { StrategyRegistry } from './registry';
import type { MetricSeriesColumn, MetricSeriesResponse, StrategyRule } from './types';
import { ruleParamsFromJson, sideInstances, authoredBuySides, authoredExitSides } from './ruleParams';
import { formatMetricExitLabel, parseMetricExitTarget } from './exitReason';
import {
  formatWindowSpec,
  sameWindowSpec,
  unitSuffix,
  sliceSpecFromStrict,
  windowSpecFromStrict,
  windowSpecKey,
  readWindow,
  type WindowSpec,
} from './windowSpec';

/** Wall-clock defaults for a caller who names no window — browsing rather than
 *  checking a specific rule. */
const DEFAULT_WINDOWS: WindowSpec[] = [10, 30, 60].map((size) => ({
  size,
  lag: 0,
  unit: 'sec' as const,
}));

/**
 * Stable key for a series column (metric, optionally `@window`).
 *
 * A bare number is a wall-clock window in seconds — the shape `/metric-series`
 * returns and the shape saved pane preferences were written in, so `30` keeps
 * producing `metric@30` byte-for-byte. A span keys by its whole identity, so a
 * 30-slot read and a 30-second read are two panes rather than one, and a lag makes
 * a third.
 */
/** Every ORDERED pair of spans that can NEST — same unit, slice strictly narrower.
 *
 *  The frontend mirror of the endpoint's own pairing (`nested_pairs`), so the pane
 *  list offers exactly the two-window columns the series carries. A pair that cannot
 *  nest is not a stricter reading, it is a `NaN` one: a cross-unit ratio is not a
 *  share of anything, and a slice equal to its reference reads 100 on every token. */
export function nestedWindowPairs(windows: WindowSpec[]): [WindowSpec, WindowSpec][] {
  const out: [WindowSpec, WindowSpec][] = [];
  for (const reference of windows) {
    for (const slice of windows) {
      if (slice.unit === reference.unit && slice.size < reference.size) out.push([reference, slice]);
    }
  }
  return out;
}

export function metricColKey(
  metric: string,
  window: WindowSpec | number | null,
  slice?: WindowSpec | null,
): string {
  // The nested slice is part of the key, because it is part of the READING: two
  // `trade_share` columns over one reference window and different slices are two
  // different numbers, and a key that ignores it collapses them onto one pane.
  const nested = slice ? `/${slice.size}${unitSuffix(slice.unit)}` : '';
  if (window == null) return `${metric}${nested}`;
  if (typeof window === 'number') return `${metric}@${window}${nested}`;
  // An unlagged seconds span keeps its bare-number key, byte-for-byte, so pane
  // preferences saved before the other bases existed still resolve.
  if (window.unit === 'sec' && window.lag === 0) return `${metric}@${window.size}${nested}`;
  const lag = window.lag > 0 ? `@${window.lag}` : '';
  return `${metric}@${window.size}${unitSuffix(window.unit)}${lag}${nested}`;
}

export interface RuleMetricPrefs {
  fingerprintId: string;
  /** Metric names the rule constrains (event ∪ filters ∪ exit). */
  metrics: string[];
  /** Dynamic windows authored on the rule, as WHOLE spans; falls back to defaults
   *  when empty. A bare size cannot tell 30 slots from 30 seconds, and the endpoint
   *  computes whichever it is asked for. */
  windows: WindowSpec[];
  /** Default pane keys to enable when the user hasn't picked any. */
  paneKeys: string[];
}

/** The two monotone clocks whose sampling density the backend can't infer from
 *  `windows` alone. Mirrors the Rust `sparse_grid_for`. */
export interface MetricClockHorizons {
  /** Largest `time` condition value (secs since creation), 0 when unconstrained. */
  timeHorizonSec: number;
  /** Largest `stall` condition value (secs since the last high), 0 when unconstrained. */
  stallHorizonSec: number;
}

/** Metric names whose clocks run on wall time rather than on trades, keyed to the
 *  sparse-grid horizon they size. `time` counts from creation, `stall` from the last
 *  all-time high — the backend cannot derive either from the requested windows. */
const CLOCK_METRICS = { time: 'timeHorizonSec', stall: 'stallHorizonSec' } as const;

/**
 * Largest authored condition value for each wall-clock metric — the horizons the
 * metric-series endpoint needs to keep its sparse tick grid dense far enough for a
 * `time`/`stall` crossing to land on a row.
 *
 * The backend defaults these to `0` (⇒ "not evaluated"), which only ever drops ticks
 * in quiet gaps past every other horizon. Passing the rule's real ceilings is what
 * makes a `stall > 120` marker land where the engine fires it. Mirrors the Rust
 * `sparse_grid_for` ceiling walk; the `=`-tolerance margin is added backend-side.
 */
export function metricClockHorizons(params: RuleParams): MetricClockHorizons {
  const out: MetricClockHorizons = { timeHorizonSec: 0, stallHorizonSec: 0 };
  for (const side of [...authoredBuySides(params), ...authoredExitSides(params)]) {
    if (!side) continue;
    for (const [, group] of sideInstances(side)) {
      for (const [metric, arms] of Object.entries(group.metrics)) {
        const key = CLOCK_METRICS[metric as keyof typeof CLOCK_METRICS];
        if (!key || !arms?.length) continue;
        for (const arm of arms) {
          for (const cond of arm) {
            if (Number.isFinite(cond.value)) out[key] = Math.max(out[key], cond.value);
          }
        }
      }
    }
  }
  return out;
}

/** Metrics + windows + default panes constrained by an (already-parsed) params
 *  form — the shared core of `extractRuleMetricPrefs`, also used for ad-hoc
 *  params without a saved rule (e.g. a sweep combo's blob). */
export function metricPrefsFromParams(
  params: RuleParams,
  registry: StrategyRegistry | undefined,
): Omit<RuleMetricPrefs, 'fingerprintId'> {
  const metrics = new Set<string>();
  // Keyed by the whole span, the same identity the engine dedupes buffers by, so a
  // 30-slot and a 30-second read are two requested columns rather than one.
  const windows = new Map<string, WindowSpec>();
  const paneKeys: string[] = [];

  for (const side of [...authoredBuySides(params), ...authoredExitSides(params)]) {
    if (!side) continue;
    for (const [groupName, group] of sideInstances(side)) {
      const gSpec = registry?.groups.find((g) => g.name === groupName);
      const kind = gSpec?.kind;
      const w = windowSpecFromStrict(group.strict);
      const slice = sliceSpecFromStrict(group.strict);
      // Every basis is requestable: `/metric-series` folds the span it is given, so
      // the pane a rule opens is the reading that rule actually gates on.
      if (w != null) windows.set(windowSpecKey(w), w);
      for (const [metric, arms] of Object.entries(group.metrics)) {
        if (!arms?.length) continue;
        metrics.add(metric);
        const key = metricColKey(
          metric,
          kind === 'dynamic' ? w : null,
          gSpec?.metrics.find((m) => m.name === metric)?.two_window ? slice : null,
        );
        if (!paneKeys.includes(key)) paneKeys.push(key);
      }
    }
  }

  return {
    metrics: [...metrics],
    windows: windows.size
      ? [...windows.values()].sort((a, b) => a.size - b.size || a.lag - b.lag)
      : [...DEFAULT_WINDOWS],
    paneKeys,
  };
}

/** Pull metrics + windows (+ fingerprint) from a selected rule. */
export function extractRuleMetricPrefs(
  rule: StrategyRule,
  registry: StrategyRegistry | undefined,
): RuleMetricPrefs {
  const params = ruleParamsFromJson(rule.params, registry);
  return { fingerprintId: rule.fingerprint_id, ...metricPrefsFromParams(params, registry) };
}

/** Judge one condition — mirrors `hunter_engine::metrics::evaluator::eval_one`. */
export function evalMetricCondition(cond: Condition, value: number, eqTolerance: number): boolean {
  if (!Number.isFinite(value)) return false;
  switch (cond.operator) {
    case '>':
      return value > cond.value;
    case '>=':
      return value >= cond.value;
    case '<':
      return value < cond.value;
    case '<=':
      return value <= cond.value;
    case '=':
      return Math.abs(value - cond.value) <= eqTolerance / 2;
    case '!=':
      return Math.abs(value - cond.value) > eqTolerance / 2;
    default:
      return false;
  }
}

/** DNF: any OR arm whose conditions all hold (empty arms ⇒ true). */
export function evalMetricConditions(
  arms: ConditionExpr,
  value: number,
  eqTolerance: number,
): boolean {
  if (arms.length === 0) return true;
  return arms.some((arm) => arm.every((c) => evalMetricCondition(c, value, eqTolerance)));
}

function seriesLookup(series: MetricSeriesColumn[]): Map<string, MetricSeriesColumn> {
  const map = new Map<string, MetricSeriesColumn>();
  // Through `readWindow`, which prefers the span object and falls back to the legacy
  // seconds scalar — so a slot or print column keys by its own span instead of
  // collapsing onto the metric's window-less key.
  for (const s of series) map.set(metricColKey(s.metric, readWindow(s), s.slice), s);
  return map;
}

/** One authored condition: a metric in a group instance, with its DNF arms and the
 *  trailing window the instance runs at (`null` for a lifetime/static group). */
interface SideMetricRow {
  groupName: string;
  metric: string;
  arms: ConditionExpr;
  dynamic: boolean;
  /** The WHOLE span the instance runs at (`null` for a lifetime/static group) —
   *  size, lag and unit, since a bare size cannot tell 30 slots from 30 seconds. */
  window: WindowSpec | null;
  /** The nested slice, for the two-window metrics alone — the other half of the
   *  reading, so a row without it would match the wrong series column. */
  slice: WindowSpec | null;
}

/** Exit as the engine compiles it: array-form is AND-clauses; object-form is one
 *  singleton OR-term per metric. */
function exitClauseRowGroups(
  params: RuleParams,
  registry: StrategyRegistry | undefined,
): SideMetricRow[][] {
  if (params.exitClauses && params.exitClauses.length > 0) {
    return params.exitClauses.map((c) => sideMetricRows(c, registry)).filter((g) => g.length > 0);
  }
  return sideMetricRows(params.exit, registry).map((r) => [r]);
}

function anyExitClauseFires(
  groups: SideMetricRow[][],
  idx: number,
  byKey: Map<string, MetricSeriesColumn>,
  registry: StrategyRegistry | undefined,
): FiredCondition[] | null {
  for (const rows of groups) {
    const fired = firedRowsAt(rows, idx, byKey, registry, 'and');
    if (fired != null) return fired;
  }
  return null;
}

function sideMetricRows(
  side: SideConditions | undefined,
  registry: StrategyRegistry | undefined,
): SideMetricRow[] {
  if (!side) return [];
  const out: SideMetricRow[] = [];
  for (const [groupName, group] of sideInstances(side)) {
    const gSpec = registry?.groups.find((g) => g.name === groupName);
    const w = windowSpecFromStrict(group.strict);
    const b = sliceSpecFromStrict(group.strict);
    for (const [metric, arms] of Object.entries(group.metrics)) {
      if (!arms?.length) continue;
      out.push({
        groupName,
        metric,
        arms,
        dynamic: gSpec?.kind === 'dynamic',
        window: w,
        // Per METRIC: the group declares the slice for every instance, but only the
        // two-window metrics read it, so attaching it to a `gross_flow` row would key
        // that row to a column the series never emits.
        slice: gSpec?.metrics.find((m) => m.name === metric)?.two_window ? b : null,
      });
    }
  }
  return out;
}

/** First condition on the first satisfied DNF arm — spaced `name op value` labels. */
function firstSatisfiedCondition(
  arms: ConditionExpr,
  value: number,
  eqTolerance: number,
): Condition | null {
  for (const arm of arms) {
    if (arm.length && arm.every((c) => evalMetricCondition(c, value, eqTolerance))) {
      return arm[0];
    }
  }
  return null;
}

/** Registry `=`/`!=` bucket width for a row's metric (0 when the group is unknown). */
function rowTolerance(row: SideMetricRow, registry: StrategyRegistry | undefined): number {
  return (
    registry?.groups
      .find((g) => g.name === row.groupName)
      ?.metrics.find((m) => m.name === row.metric)?.eq_tolerance ?? 0
  );
}

/** The series column a row evaluates against — its metric at ITS window. A dynamic
 *  group instance reads the windowed column; a lifetime group reads the unwindowed
 *  one, and the two share a metric name. */
function rowColumnKey(row: SideMetricRow): string {
  return metricColKey(row.metric, row.dynamic ? row.window : null, row.slice);
}

/** The atom that satisfies `row` at `idx`, or `null` when the row does not hold
 *  there (including a missing / non-finite reading). */
function rowFiredAt(
  row: SideMetricRow,
  idx: number,
  byKey: Map<string, MetricSeriesColumn>,
  registry: StrategyRegistry | undefined,
): Condition | null {
  const reading = byKey.get(rowColumnKey(row))?.values[idx];
  if (reading == null || !Number.isFinite(reading)) return null;
  return firstSatisfiedCondition(row.arms, reading, rowTolerance(row, registry));
}

/** One authored condition together with the atom satisfying it at an instant. */
interface FiredCondition {
  row: SideMetricRow;
  cond: Condition;
}

/**
 * The rows that justify a side pass at `idx`, or `null` when the side does not pass.
 *
 * Combinator mirrors the engine (`arm.rs`): entry ANDs across metrics, so a pass
 * returns **every** authored row; a DNF exit way ANDs too, while object-form exit
 * ORs (one singleton way per metric). Within a metric, DNF still applies. An empty
 * side returns `null` (vacuous entry-true is handled by the caller skipping the
 * walk when there are no entry rows).
 */
function firedRowsAt(
  rows: SideMetricRow[],
  idx: number,
  byKey: Map<string, MetricSeriesColumn>,
  registry: StrategyRegistry | undefined,
  combinator: 'and' | 'or',
): FiredCondition[] | null {
  if (rows.length === 0) return null;
  const out: FiredCondition[] = [];
  for (const row of rows) {
    const cond = rowFiredAt(row, idx, byKey, registry);
    if (cond == null) {
      if (combinator === 'and') return null;
      continue;
    }
    if (combinator === 'or') return [{ row, cond }];
    out.push({ row, cond });
  }
  return combinator === 'and' ? out : null;
}

/** `untagged_buy@2s >= 0.85` — one satisfied condition in the lanes' vocabulary, so a
 *  marker reads as the legend of the lane it fired on. The window qualifier is not
 *  decoration: `untagged_buy` names both a lifetime metric and its windowed twin, and
 *  a bare name sends the reader to whichever line is on screen. */
function firedConditionLabel({ row, cond }: FiredCondition): string {
  return formatMetricExitLabel(conditionMetricName(row), cond.operator, cond.value);
}

/** How many conditions a marker spells out before it summarises the rest. A marker
 *  is drawn inside a candle's width; past two the text is wider than the chart. */
const MARKER_LABEL_MAX = 2;

/** A set of satisfied conditions as one marker label (`a + b +2`). */
function firedLabel(fired: FiredCondition[]): string | null {
  if (fired.length === 0) return null;
  const shown = fired.slice(0, MARKER_LABEL_MAX).map(firedConditionLabel).join(' + ');
  const rest = fired.length - MARKER_LABEL_MAX;
  return rest > 0 ? `${shown} +${rest}` : shown;
}

/**
 * The conditions that *changed the answer* at `idx` — satisfied here and not on the
 * row before.
 *
 * Entry is a conjunction, so the rows that were already holding decided nothing about
 * the timing. Naming one of them anyway is how a **monotone lifetime** metric comes to
 * label a fire that two trailing windows produced: it crosses once, stays true forever,
 * and (being first in the params JSON) wins every label from then on. The reader then
 * compares the marker against a line that crossed minutes earlier and concludes the
 * engine entered late.
 *
 * Empty when nothing flipped — `idx` 0, or entry unblocked because an exit metric
 * stopped vetoing it. Callers fall back to the whole conjunction, which is still true.
 */
function newlyFiredRows(
  fired: FiredCondition[],
  idx: number,
  byKey: Map<string, MetricSeriesColumn>,
  registry: StrategyRegistry | undefined,
): FiredCondition[] {
  if (idx === 0) return fired;
  return fired.filter((f) => rowFiredAt(f.row, idx - 1, byKey, registry) == null);
}

/** One authored condition's pass/fail at a hovered instant. */
export interface MetricConditionState {
  side: 'entry' | 'exit';
  metric: string;
  /** The trailing span the condition runs at; `null` for a lifetime/static group. */
  window: WindowSpec | null;
  /**
   * Series-column key (`metric` or `metric@Ns`) — the identity a pane is keyed by.
   * A rule that constrains `untagged_buy` lifetime AND `untagged_buy` at 2 s produces two
   * states with the same `metric`, and keying a readout by the name alone paints the
   * windowed pane with the lifetime verdict.
   */
  key: string;
  ok: boolean;
  value: number | null;
}

/** Per-condition pass/fail at one series index (for the crosshair readout). */
export function metricConditionStatesAt(
  params: RuleParams,
  idx: number,
  data: MetricSeriesResponse,
  registry: StrategyRegistry | undefined,
): MetricConditionState[] {
  const byKey = seriesLookup(data.series);
  const out: MetricConditionState[] = [];
  for (const sideName of ['entry', 'exit'] as const) {
    const sides =
      sideName === 'exit' ? authoredExitSides(params) : authoredBuySides(params);
    for (const side of sides) {
      for (const row of sideMetricRows(side, registry)) {
      const key = rowColumnKey(row);
      const value = byKey.get(key)?.values[idx] ?? null;
      out.push({
        side: sideName,
        metric: row.metric,
        window: row.dynamic ? row.window : null,
        key,
        value,
        ok: value != null && evalMetricConditions(row.arms, value, rowTolerance(row, registry)),
      });
      }
    }
  }
  return out;
}

/**
 * Entry/exit threshold values a rule places on ONE series column — the lines a pane
 * draws under its sparkline.
 *
 * Scoped by window, not by metric name: `m_flow_ix.untagged_buy >= 5.5` and
 * `m_flow_ix_window.untagged_buy >= 0.9` are different conditions on different
 * readings, and drawing both on both panes puts a line on a chart the rule never
 * placed there.
 */
export function metricThresholdsFor(
  params: RuleParams,
  metric: string,
  /** A bare number is a wall-clock window in seconds — what the chart pane, whose
   *  columns `/metric-series` computes, is keyed by. */
  window: WindowSpec | number | null,
  registry: StrategyRegistry | undefined,
): Array<{ side: 'entry' | 'exit'; value: number }> {
  const key = metricColKey(metric, window);
  const out: Array<{ side: 'entry' | 'exit'; value: number }> = [];
  for (const sideName of ['entry', 'exit'] as const) {
    const sides =
      sideName === 'exit' ? authoredExitSides(params) : authoredBuySides(params);
    for (const side of sides) {
      for (const row of sideMetricRows(side, registry)) {
      if (rowColumnKey(row) !== key) continue;
      // `arms` is DNF (`Condition[][]`) — flatten arms → atoms.
      for (const arm of row.arms) {
        for (const c of arm) {
          if (Number.isFinite(c.value)) out.push({ side: sideName, value: c.value });
        }
      }
      }
    }
  }
  return out;
}

/**
 * Side tag on a condition's chart lane / threshold line. `entry`/`exit` both start
 * with "e", so they get distinct words — the color alone isn't a readable
 * difference at 9px, and a lane stack that mixes both sides is unreadable without
 * one.
 */
export const CONDITION_SIDE_TAG = { entry: 'IN', exit: 'OUT' } as const;

/**
 * Neutral on purpose. A lane refuses a good/bad tone — a satisfied entry condition
 * is *why we're in* and a satisfied exit condition is *why we're leaving*, so one
 * green would mean opposite things two lanes apart.
 */
export const CONDITION_LANE_COLOR = 'rgba(226,232,240,0.55)';

/** The condition drawn as a VALUE line in its own pane — brighter than a lane,
 *  because it is the subject rather than the backdrop. */
export const CONDITION_VALUE_LANE_COLOR = '#7DD3FC';

/** The chart's bottom-pane view of a rule over one token: one on/off lane per
 *  authored condition, plus the one condition whose reading is drawn as a line. */
export interface MetricConditionLanes {
  lanes: ChartTimeBand[];
  valueLane: ChartValueLane | null;
  /** The stretch the lanes speak for — without it "never held" and "not covered"
   *  draw identically. */
  coverage: ChartTimeSpan;
}

/**
 * `untagged_buy` / `untagged_buy@2s` — the name half of every condition label the lab
 * draws: lanes, the value line, and the metric-fire markers.
 *
 * ONE namer for all three, and it always carries the window. The lifetime and windowed
 * registry entries share every metric name, so a surface that drops the qualifier
 * cannot say which reading it means — and the lifetime twin is usually monotone, which
 * makes the wrong reading look permanently satisfied. Matches the live position
 * modal's chips (`conditionLabel`), so a lane reads as their legend.
 */
function conditionMetricName(row: {
  metric: string;
  dynamic: boolean;
  window: WindowSpec | null;
}): string {
  return row.dynamic && row.window != null
    ? `${row.metric}@${formatWindowSpec(row.window)}`
    : row.metric;
}

/** `untagged_buy@2s >= 0.9` — an authored condition, human-side. */
function conditionLaneLabel(row: SideMetricRow): string {
  const name = conditionMetricName(row);
  const expr = formatConditions(row.arms);
  return expr ? `${name} ${expr}` : name;
}

/**
 * Every authored condition as a chart lane: the stretches over which it held,
 * folded over the whole metric series rather than one hovered instant.
 *
 * The lab twin of the live position modal's condition timeline. Both draw the same
 * `ChartTimeBand` vocabulary, but from different sources — live replays a persisted
 * position server-side, this folds the metric series the panes already fetched, so
 * turning it on costs no request.
 *
 * It is the panes' answer, with the panes' limits: no arming gate (`arm_above_pct`)
 * and no ladder-stage state, exactly like {@link findRuleFireMarkers} beside it. A
 * gated trailing stop therefore reads as satisfied over stretches the engine was
 * skipping it — read the lane as "the condition's own reading crossed", not as
 * "the engine would have sold here".
 *
 * A condition with no crossing keeps its (empty) lane: against the coverage track
 * that reads as "never fired", which is a real answer and the common one for the
 * exits that did not close the position.
 */
export function metricConditionBands(
  params: RuleParams,
  data: MetricSeriesResponse,
  registry: StrategyRegistry | undefined,
  /** The run's persisted exit reason — selects which condition gets the value
   *  line. Without it the pane falls back to the first exit condition, which may
   *  not be the one that closed the position. */
  exitReason?: string | null,
): MetricConditionLanes | null {
  const atSec = parseSeriesAtSec(data.at);
  if (atSec.length === 0) return null;
  const byKey = seriesLookup(data.series);

  const rows = (['entry', 'exit'] as const).flatMap((side) => {
    const sides = side === 'exit' ? authoredExitSides(params) : authoredBuySides(params);
    return sides.flatMap((s) => sideMetricRows(s, registry).map((row) => ({ side, ...row })));
  });
  if (rows.length === 0) return null;

  const lanes: ChartTimeBand[] = rows.map((row, idx) => {
    const col = byKey.get(rowColumnKey(row));
    const tol = rowTolerance(row, registry);
    const spans: ChartTimeSpan[] = [];
    let start = -1;
    for (let i = 0; i < atSec.length; i++) {
      const value = col?.values[i];
      const on =
        value != null && Number.isFinite(value) && evalMetricConditions(row.arms, value, tol);
      if (on && start < 0) start = i;
      else if (!on && start >= 0) {
        spans.push({ from: atSec[start], to: atSec[i - 1] });
        start = -1;
      }
    }
    if (start >= 0) spans.push({ from: atSec[start], to: atSec[atSec.length - 1] });
    return {
      key: `${row.side}-${row.groupName}-${row.metric}-${row.window ?? ''}-${idx}`,
      label: `${CONDITION_SIDE_TAG[row.side]} ${conditionLaneLabel(row)}`,
      color: CONDITION_LANE_COLOR,
      spans,
    };
  });

  const drawn = valueLaneRow(rows, exitReason);
  const drawnCol = drawn ? byKey.get(rowColumnKey(drawn)) : null;

  return {
    lanes,
    valueLane:
      drawn && drawnCol
        ? {
          key: `value-${drawn.metric}-${drawn.window ?? ''}`,
          label: conditionLaneLabel(drawn),
          color: CONDITION_VALUE_LANE_COLOR,
          points: atSec.map((timeSec, i) => ({
            timeSec,
            value: drawnCol.values[i] ?? null,
          })),
          thresholds: conditionThresholds(drawn.arms),
        }
        : null,
    coverage: { from: atSec[0], to: atSec[atSec.length - 1] },
  };
}

/** The condition whose reading gets drawn: the one the exit reason names when it
 *  names one, else the first exit condition — an inspect is opened on "why did it
 *  leave", so the exit side is the useful default. */
function valueLaneRow<T extends SideMetricRow & { side: 'entry' | 'exit' }>(
  rows: T[],
  exitReason: string | null | undefined,
): T | null {
  const exits = rows.filter((r) => r.side === 'exit');
  if (exits.length === 0) return null;
  const target = parseMetricExitTarget(exitReason);
  if (!target) return exits[0];
  const byName = exits.filter((r) => r.metric === target.metric);
  if (byName.length === 0) return exits[0];
  if (target.window != null) {
    // Matching by NAME alone is free to pick the lifetime twin of a windowed exit —
    // the mismatch the window qualifier exists to make impossible. The whole span
    // has to agree: a 30-slot read is not the 30-second one.
    return byName.find((r) => sameWindowSpec(r.window, target.window)) ?? exits[0];
  }
  return byName.length === 1 ? byName[0] : exits[0];
}

/**
 * The horizontal lines a condition is judged against: every threshold of its ONE AND
 * arm, so a band (`> 20, < 50`) draws both of its edges rather than none.
 *
 * Several OR arms is deliberately empty — they disagree about where the line sits, so
 * any single set would misstate the rule. Requiring a single *atom* instead left
 * every two-sided condition unlabelled, which is most entry conditions.
 */
function conditionThresholds(arms: ConditionExpr): number[] {
  if (arms.length !== 1) return [];
  return arms[0].filter((a) => Number.isFinite(a.value)).map((a) => a.value);
}

/**
 * First trade index where entry may fire (entry AND holds and exit OR does not —
 * mirrors `CompiledRule::can_enter`); then first later exit.
 *
 * Markers are `role: 'signal'` with spaced `name[@Ns] op value` labels (e.g.
 * `tagged_buy@2s >= 0.85`) — the frontend condition-fire estimate, never the backend
 * fill pointers.
 *
 * **The entry label names what fired, not what was authored first.** Entry is a
 * conjunction, so the marker carries the condition(s) that flipped true *at* the
 * marker's instant; a condition already holding did not decide the timing, and
 * labelling the fire with one is how the monotone `m_flow_ix.untagged_buy` came to
 * explain entries produced by two trailing windows. The rest of the conjunction is
 * still on the chart — as lanes ({@link metricConditionBands}).
 */
export function findRuleFireMarkers(
  params: RuleParams,
  data: MetricSeriesResponse,
  registry: StrategyRegistry | undefined,
): ChartEventMarker[] {
  const n = data.at.length;
  if (n === 0) return [];
  const byKey = seriesLookup(data.series);
  const entryRows = authoredBuySides(params).flatMap((s) => sideMetricRows(s, registry));
  const exitGroups = exitClauseRowGroups(params, registry);
  const hasEntry = entryRows.length > 0;
  const hasExit = exitGroups.length > 0;

  let entryIdx: number | null = null;
  let entryLabel: string | null = null;
  if (hasEntry) {
    for (let i = 0; i < n; i++) {
      const fired = firedRowsAt(entryRows, i, byKey, registry, 'and');
      if (fired == null) continue;
      // Engine refuses entry while any exit clause already holds.
      if (hasExit && anyExitClauseFires(exitGroups, i, byKey, registry) != null) continue;
      entryIdx = i;
      // Nothing flipped ⇒ the conjunction was already whole and an exit metric had
      // been vetoing; naming the whole conjunction is then the honest answer.
      const flipped = newlyFiredRows(fired, i, byKey, registry);
      entryLabel = firedLabel(flipped.length ? flipped : fired);
      break;
    }
  }

  let exitIdx: number | null = null;
  let exitLabel: string | null = null;
  if (hasExit) {
    const start = entryIdx != null ? entryIdx + 1 : 0;
    for (let i = start; i < n; i++) {
      const fired = anyExitClauseFires(exitGroups, i, byKey, registry);
      if (fired != null) {
        exitIdx = i;
        exitLabel = firedLabel(fired);
        break;
      }
    }
  }

  const markers: ChartEventMarker[] = [];
  if (entryIdx != null) {
    const price = data.price?.[entryIdx];
    if (price != null && Number.isFinite(price)) {
      markers.push({
        kind: 'entry',
        role: 'signal',
        time: data.at[entryIdx],
        priceInSol: price,
        label: entryLabel ?? 'entry',
      });
    }
  }
  if (exitIdx != null) {
    const price = data.price?.[exitIdx];
    if (price != null && Number.isFinite(price)) {
      markers.push({
        kind: 'exit',
        role: 'signal',
        time: data.at[exitIdx],
        priceInSol: price,
        label: exitLabel ?? 'exit',
      });
    }
  }
  return markers;
}

/**
 * Last series index at or before a wall-clock unix second — the state **as of**
 * `timeSec`, which is what a hovered candle is asking for.
 *
 * Distinct from {@link nearestSeriesIndex}, and the difference is not cosmetic: a
 * series row lands on every trade, so "nearest" can jump FORWARD past trades the
 * hovered instant had not seen yet, reporting a reading that had not happened. Rows
 * are ascending, so this is a binary search.
 *
 * `null` only when every row is later than `timeSec` (the pointer is left of the
 * recorded span) — a real answer the caller must render as "no reading here" rather
 * than clamping to row 0.
 */
export function seriesIndexAsOf(atSec: number[], timeSec: number): number | null {
  if (!atSec.length || timeSec < atSec[0]) return null;
  let lo = 0;
  let hi = atSec.length - 1;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (atSec[mid] <= timeSec) lo = mid;
    else hi = mid - 1;
  }
  return lo;
}

/** Nearest series index to a wall-clock unix second (or null). */
export function nearestSeriesIndex(atSec: number[], timeSec: number): number | null {
  if (!atSec.length) return null;
  let best = 0;
  let bestDist = Math.abs(atSec[0] - timeSec);
  for (let i = 1; i < atSec.length; i++) {
    const d = Math.abs(atSec[i] - timeSec);
    if (d < bestDist) {
      best = i;
      bestDist = d;
    }
  }
  return best;
}

export function parseSeriesAtSec(at: string[]): number[] {
  return at.map((s) => {
    const ms = Date.parse(s);
    return Number.isFinite(ms) ? ms / 1000 : NaN;
  });
}

export { DEFAULT_WINDOWS };
