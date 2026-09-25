// Helpers for the lab metric-pane overlay (`GET /api/tokens/{mint}/metric-series`): the
// wire shape, the reads a rule makes, and the rule's verdicts over a fetched series,
// judged the way the engine judges them (DNF: OR of AND arms, `=` band tolerance).
//
// A pane IS one read, identified by its label: `m_flow.buy_sol @!volume [10s]`
// (`refLabel`, the engine's `MetricRef::label`). A series column, a rule condition and
// an auto-labelled exit reason that name the same read therefore match by string
// equality, with no second key to keep in step.

import type {
  ChartEventMarker,
  ChartTimeBand,
  ChartTimeSpan,
  ChartValueLane,
} from 'components/token-price-chart';
import type { Condition, ConditionExpr } from './grammar';
import { formatMetricThreshold, parseSpan, refLabel } from './metricRef';
import { findMetric, type MetricUnit, type StrategyRegistry } from './registry';
import {
  allLines,
  entersOnArm,
  liveMetricConds,
  metricCond,
  newLine,
  ruleDocFromJson,
  type Cond,
  type Deadline,
  type Line,
  type MetricCond,
  type RuleDoc,
} from './ruleDoc';
import { condLabel, lineExitLabel } from './sentences';
import { windowSpecKey, type WindowSpec } from './windowSpec';

/** Registry paths the overlay treats specially (engine `Metric` ids). */
const PNL_PCT = 'm_position.pnl_pct';
const AGE_SEC = 'm_state.age_sec';
const STALL_SEC = 'm_price.stall_sec';
const STAGE_SEC = 'm_position.stage_sec';

/** Wall-clock spans for a caller who names none: browsing rather than checking a
 *  rule (the endpoint's own `DEFAULT_WINDOWS`). */
export const DEFAULT_WINDOWS: WindowSpec[] = [10, 30, 60].map((size) => ({
  size,
  lag: 0,
  unit: 'sec' as const,
}));

// ── Wire ────────────────────────────────────────────────────────────────────

/** One read's column from `/metric-series` (the endpoint's `SeriesOut`). */
export interface MetricSeriesColumn {
  /** Registry path, `m_flow.buy_sol`. */
  metric: string;
  /** Family name, `m_flow`. */
  family: string;
  unit: MetricUnit;
  /** `volume` / `!volume` / a wallet class; null untagged. */
  tag?: string | null;
  /** `10s` / `30sl@1` / `age0s`; null for the life. */
  span?: string | null;
  /** The nested slice of a two-window read. */
  slice?: string | null;
  /** `m_flow.buy_sol @!volume [10s]`: the pane's identity. */
  label: string;
  /** One value per event, aligned with `at`; non-finite = `null`. */
  values: Array<number | null>;
}

/** `/metric-series` response: every read at every **event** (trades plus the engine's
 *  `TICK_MS` grid ticks, since a time-decaying read only moves on a tick), as parallel
 *  arrays. Tagged reads need a fingerprint; `m_position` reads need an entry fill. */
export interface MetricSeriesResponse {
  mint_address: string;
  /** RFC3339 timestamps aligned with every column's `values`. */
  at: string[];
  /** Spot price (SOL) at each event; non-finite = `null`. */
  price?: Array<number | null>;
  series: MetricSeriesColumn[];
  /** The row ceiling cut the series short: it covers only
   *  `[first trade, covered_until]`. The rows present stay exact. */
  truncated?: boolean;
  /** Last instant the series reaches (RFC3339); null with no events. */
  covered_until?: string | null;
}

/** Series columns by read label. */
export function seriesByLabel(series: readonly MetricSeriesColumn[]): Map<string, MetricSeriesColumn> {
  return new Map(series.map((s) => [s.label, s]));
}

// ── The rule, read for the panes ─────────────────────────────────────────────

/** Where a condition sits: the buy (`enter.*`), a named signal, or a held line. */
export type ConditionSide = 'entry' | 'signal' | 'exit';

/** Side tag on a lane and a threshold. `entry` / `exit` both start with "e", so they
 *  get distinct words: colour alone is no readable difference at 9 px. */
export const CONDITION_SIDE_TAG: Record<ConditionSide, string> = { entry: 'IN', signal: 'SIG', exit: 'OUT' };

/** One live metric condition of a rule. */
export interface RuleCondRow {
  side: ConditionSide;
  cond: MetricCond;
  /** `refLabel(cond.ref)`: the series column it reads. */
  key: string;
}

/** A rule as the panes read it: the doc, the engine's `always` list (the `stop_loss` /
 *  `take_profit` shortcuts first, as `CompiledRule::compile` puts them), and every live
 *  metric condition in rule order. */
export interface PaneRule {
  doc: RuleDoc;
  always: Line[];
  rows: RuleCondRow[];
}

/** A stored `params` as a {@link PaneRule}, or the reason it is not a v2 rule. */
export function paneRuleFromJson(params: unknown): { rule: PaneRule | null; error: string | null } {
  try {
    return { rule: paneRule(ruleDocFromJson(params)), error: null };
  } catch (e) {
    return { rule: null, error: e instanceof Error ? e.message : String(e) };
  }
}

/** A shortcut sell line on `m_position.pnl_pct`, booked under the engine's reason. */
function pnlLine(operator: Condition['operator'], value: number, reason: string): Line {
  return newLine({ if: [metricCond({ metric: PNL_PCT }, [[{ operator, value }]])], sell: { label: reason, pct: null } });
}

export function paneRule(doc: RuleDoc): PaneRule {
  const shortcuts: Line[] = [];
  if (doc.stop_loss != null) shortcuts.push(pnlLine('<=', -doc.stop_loss, 'StopLoss'));
  if (doc.take_profit != null) shortcuts.push(pnlLine('>=', doc.take_profit, 'TakeProfit'));
  const entryIds = new Set(
    [...doc.enter.event, ...doc.enter.filters, ...doc.enter.final_filters].map((c) => c.id),
  );
  const signalIds = new Set(doc.signals.flatMap((s) => s.groups.flat()).map((c) => c.id));
  const rows: RuleCondRow[] = liveMetricConds(doc).map((cond) => ({
    side: entryIds.has(cond.id) ? 'entry' : signalIds.has(cond.id) ? 'signal' : 'exit',
    cond,
    key: refLabel(cond.ref),
  }));
  const firstExit = rows.findIndex((r) => r.side === 'exit');
  const shortcutRows: RuleCondRow[] = shortcuts.flatMap((l) =>
    l.if.flatMap((c) => (c.kind === 'metric' ? [{ side: 'exit' as const, cond: c, key: refLabel(c.ref) }] : [])),
  );
  rows.splice(firstExit < 0 ? rows.length : firstExit, 0, ...shortcutRows);
  return { doc, always: [...shortcuts, ...doc.always], rows };
}

/** The reads a rule's conditions make, in rule order, once each: its own panes. */
export function rulePaneKeys(rule: PaneRule): string[] {
  return [...new Set(rule.rows.map((r) => r.key))];
}

/** Every trailing window the rule's reads need, whole (size, lag, unit). A sliced read
 *  also needs its slice as a window: the endpoint pairs the requested windows into
 *  nested spans. The defaults when the rule reads none. */
export function ruleWindows(rule: PaneRule): WindowSpec[] {
  const out = new Map<string, WindowSpec>();
  for (const { cond } of rule.rows) {
    const m = parseSpan(cond.ref.span, cond.ref.slice);
    if (typeof m === 'string' || m.kind !== 'window') continue;
    for (const w of m.slice ? [m.window, m.slice] : [m.window]) out.set(windowSpecKey(w), w);
  }
  if (out.size === 0) return [...DEFAULT_WINDOWS];
  return [...out.values()].sort((a, b) => a.unit.localeCompare(b.unit) || a.size - b.size || a.lag - b.lag);
}

/** The since-age anchors the rule reads (`[age5s]` -> 5): the endpoint draws only
 *  `age0s` unless asked, so without these a rule's own since-age read has no column. */
export function ruleAges(rule: PaneRule): number[] {
  const out = new Set<number>();
  for (const { cond } of rule.rows) {
    const m = parseSpan(cond.ref.span, cond.ref.slice);
    if (typeof m !== 'string' && m.kind === 'since_age' && m.secs > 0) out.add(m.secs);
  }
  return [...out].sort((a, b) => a - b);
}

/** The two clocks whose sampling density the endpoint cannot infer from `windows`. */
export interface MetricClockHorizons {
  /** Largest `m_state.age_sec` threshold or age deadline, secs; 0 when unread. */
  timeHorizonSec: number;
  /** Largest `m_price.stall_sec` threshold, secs; 0 when unread. */
  stallHorizonSec: number;
}

/** The rule's clock ceilings, so the sparse tick grid stays dense up to the last
 *  instant a crossing could land on (engine `ClockHorizons::absorb_req` /
 *  `absorb_deadline`: threshold plus the `=` band, a deadline plus half a second). */
export function metricClockHorizons(rule: PaneRule, registry: StrategyRegistry | undefined): MetricClockHorizons {
  const out: MetricClockHorizons = { timeHorizonSec: 0, stallHorizonSec: 0 };
  for (const { cond } of rule.rows) {
    const key = cond.ref.metric === AGE_SEC ? 'timeHorizonSec' : cond.ref.metric === STALL_SEC ? 'stallHorizonSec' : null;
    if (!key) continue;
    const tol = tolerance(registry, cond.ref.metric);
    for (const c of cond.is.flat()) {
      if (Number.isFinite(c.value)) out[key] = Math.max(out[key], Math.abs(c.value) + tol);
    }
  }
  for (const s of rule.doc.stages) {
    if (s.ends?.basis === 'age_sec') out.timeHorizonSec = Math.max(out.timeHorizonSec, s.ends.secs + 0.5);
  }
  return out;
}

// ── Judging ─────────────────────────────────────────────────────────────────

/** Judge one condition: `hunter_engine::metrics::evaluator::eval_one`. */
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

/** DNF: any OR arm whose conditions all hold; no arms at all holds (engine `eval`). */
export function evalMetricConditions(arms: ConditionExpr, value: number, eqTolerance: number): boolean {
  if (arms.length === 0) return true;
  return arms.some((arm) => arm.every((c) => evalMetricCondition(c, value, eqTolerance)));
}

/** The registry `=` / `!=` band of a metric (0 when unknown). */
function tolerance(registry: StrategyRegistry | undefined, metric: string): number {
  return findMetric(registry, metric)?.eq_tolerance ?? 0;
}

/**
 * How a judge reads `m_position`:
 * - `null`: no position (before the buy): every position read is `NaN`, so a line on
 *   our own position never vetoes a buy (engine `sell_line_holding_before_entry`).
 * - `series`: the series' own columns (anchored on the inspected run's entry fill).
 * - a stage start: as `series`, with `m_position.stage_sec` read from that instant,
 *   since the series has no stage moves.
 */
type PositionView = null | 'series' | { stageStartSec: number };

/** Reads a rule's conditions off one fetched series. */
class SeriesJudge {
  readonly atSec: number[];
  private readonly byLabel: Map<string, MetricSeriesColumn>;
  private readonly cols = new Map<string, MetricSeriesColumn | undefined>();
  private readonly signals: Map<string, MetricCond[][]>;

  constructor(
    readonly rule: PaneRule,
    data: MetricSeriesResponse,
    private readonly registry: StrategyRegistry | undefined,
  ) {
    this.atSec = parseSeriesAtSec(data.at);
    this.byLabel = seriesByLabel(data.series);
    this.signals = new Map(rule.doc.signals.map((s) => [s.name, s.groups]));
  }

  column(c: MetricCond): MetricSeriesColumn | undefined {
    if (!this.cols.has(c.id)) this.cols.set(c.id, this.byLabel.get(refLabel(c.ref)));
    return this.cols.get(c.id);
  }

  /** A column's value by read label; `NaN` when absent. */
  readLabel(label: string, i: number): number {
    return this.byLabel.get(label)?.values[i] ?? NaN;
  }

  read(c: MetricCond, i: number, pos: PositionView): number {
    if (findMetric(this.registry, c.ref.metric)?.position) {
      if (pos == null) return NaN;
      if (pos !== 'series' && c.ref.metric === STAGE_SEC) return this.atSec[i] - pos.stageStartSec;
    }
    return this.column(c)?.values[i] ?? NaN;
  }

  metricHolds(c: MetricCond, i: number, pos: PositionView): boolean {
    return evalMetricConditions(c.is, this.read(c, i, pos), tolerance(this.registry, c.ref.metric));
  }

  holds(c: Cond, i: number, pos: PositionView): boolean {
    if (c.kind === 'metric') return this.metricHolds(c, i, pos);
    const groups = this.signals.get(c.signal);
    const on = !!groups?.some((g) => g.every((m) => m.off || this.metricHolds(m, i, pos)));
    return on !== c.not;
  }

  /** AND of the live conditions; none holds (engine `all_hold` over the compiled list). */
  allHold(cs: Cond[], i: number, pos: PositionView): boolean {
    return cs.every((c) => c.off || this.holds(c, i, pos));
  }

  /** The first live line that holds. */
  firstLine(lines: Line[], i: number, pos: PositionView): Line | null {
    return lines.find((l) => !l.off && this.allHold(l.if, i, pos)) ?? null;
  }

  /** `m_flow.buy_sol [10s] >= 2`: the atom that satisfied a condition, as the engine's
   *  auto label spells it, so a marker reads as the lane it fired on. */
  firedLabel(c: Cond, i: number, pos: PositionView): string {
    if (c.kind === 'signal') return c.not ? `not ${c.signal}` : c.signal;
    const v = this.read(c, i, pos);
    const tol = tolerance(this.registry, c.ref.metric);
    const atom = c.is.find((arm) => arm.length && arm.every((x) => evalMetricCondition(x, v, tol)))?.[0];
    return atom ? `${refLabel(c.ref)} ${atom.operator} ${formatMetricThreshold(atom.value)}` : refLabel(c.ref);
  }
}

// ── Crosshair readout, pane thresholds ────────────────────────────────────────

/** One condition's verdict at a hovered instant. */
export interface MetricConditionState {
  side: ConditionSide;
  /** The read label: the pane it colours. */
  key: string;
  ok: boolean;
  value: number | null;
}

/** Per-condition pass/fail at one series index. Keyed by read, never by metric: a
 *  rule may read `m_flow.buy_sol` over the life and over 10 s at once. */
export function metricConditionStatesAt(
  rule: PaneRule,
  idx: number,
  data: MetricSeriesResponse,
  registry: StrategyRegistry | undefined,
): MetricConditionState[] {
  const judge = new SeriesJudge(rule, data, registry);
  return rule.rows.map(({ side, cond, key }) => {
    const v = judge.read(cond, idx, 'series');
    return { side, key, value: Number.isFinite(v) ? v : null, ok: judge.metricHolds(cond, idx, 'series') };
  });
}

/** The threshold values a rule places on ONE read: every atom of every condition on
 *  that label. */
export function metricThresholdsFor(rule: PaneRule, key: string): Array<{ side: ConditionSide; value: number }> {
  return rule.rows
    .filter((r) => r.key === key)
    .flatMap((r) => r.cond.is.flat().filter((c) => Number.isFinite(c.value)).map((c) => ({ side: r.side, value: c.value })));
}

// ── Chart lanes ─────────────────────────────────────────────────────────────

/** Neutral on purpose: a held entry condition is *why we're in*, a held exit
 *  condition *why we're leaving*, so one green would mean opposite things. */
export const CONDITION_LANE_COLOR = 'rgba(226,232,240,0.55)';

/** The condition drawn as a VALUE line: brighter than a lane, it is the subject. */
export const CONDITION_VALUE_LANE_COLOR = '#7DD3FC';

/** The chart's bottom-pane view of a rule over one token. */
export interface MetricConditionLanes {
  lanes: ChartTimeBand[];
  valueLane: ChartValueLane | null;
  /** The stretch the lanes speak for: without it "never held" and "not covered" draw
   *  identically. */
  coverage: ChartTimeSpan;
}

/**
 * Every live condition as a lane: the stretches over which its own reading held,
 * folded over the fetched series (no extra request).
 *
 * A lane is the condition's reading, not the engine's decision: it models no entry
 * lock and no stage, so a stage-two line reads as held wherever its numbers held. A
 * condition that never held keeps its empty lane, which against the coverage track
 * reads "never fired".
 */
export function metricConditionBands(
  rule: PaneRule,
  data: MetricSeriesResponse,
  registry: StrategyRegistry | undefined,
  /** The run's exit reason: picks the condition drawn as a value line. */
  exitReason?: string | null,
): MetricConditionLanes | null {
  const judge = new SeriesJudge(rule, data, registry);
  const { atSec } = judge;
  if (atSec.length === 0 || rule.rows.length === 0) return null;

  const lanes: ChartTimeBand[] = rule.rows.map(({ side, cond }) => {
    const spans: ChartTimeSpan[] = [];
    let start = -1;
    for (let i = 0; i < atSec.length; i++) {
      const on = Number.isFinite(judge.read(cond, i, 'series')) && judge.metricHolds(cond, i, 'series');
      if (on && start < 0) start = i;
      else if (!on && start >= 0) {
        spans.push({ from: atSec[start], to: atSec[i - 1] });
        start = -1;
      }
    }
    if (start >= 0) spans.push({ from: atSec[start], to: atSec[atSec.length - 1] });
    return {
      key: `${side}-${cond.id}`,
      label: `${CONDITION_SIDE_TAG[side]} ${condLabel(cond)}`,
      color: CONDITION_LANE_COLOR,
      spans,
    };
  });

  const drawn = valueLaneRow(rule, exitReason);
  const col = drawn ? judge.column(drawn.cond) : undefined;
  return {
    lanes,
    valueLane:
      drawn && col
        ? {
          key: `value-${drawn.cond.id}`,
          label: condLabel(drawn.cond),
          color: CONDITION_VALUE_LANE_COLOR,
          points: atSec.map((timeSec, i) => ({ timeSec, value: col.values[i] ?? null })),
          thresholds: conditionThresholds(drawn.cond.is),
        }
        : null,
    coverage: { from: atSec[0], to: atSec[atSec.length - 1] },
  };
}

/** Every held line in the engine's order, the shortcuts first. */
function heldLines(rule: PaneRule): Line[] {
  return [...rule.always, ...allLines({ ...rule.doc, always: [] })];
}

/** The condition whose reading is drawn: the first live metric condition of the line
 *  that booked the exit reason (a line's own label, or the auto label the engine gives
 *  an unlabelled one), else the first exit condition. */
function valueLaneRow(rule: PaneRule, exitReason: string | null | undefined): RuleCondRow | null {
  const exits = rule.rows.filter((r) => r.side === 'exit');
  if (exits.length === 0) return null;
  const reason = exitReason?.trim();
  if (reason) {
    const line = heldLines(rule).find((l) => !l.off && l.sell && lineExitLabel(l) === reason);
    const cond = line?.if.find((c) => c.kind === 'metric' && !c.off);
    const row = cond && exits.find((r) => r.cond.id === cond.id);
    if (row) return row;
  }
  return exits[0];
}

/** The lines a condition is judged against: every threshold of its ONE AND arm, so a
 *  band (`> 20, < 50`) draws both edges. Several OR arms disagree about where the line
 *  sits, so they draw none. */
function conditionThresholds(arms: ConditionExpr): number[] {
  if (arms.length !== 1) return [];
  return arms[0].filter((a) => Number.isFinite(a.value)).map((a) => a.value);
}

// ── Fire markers ────────────────────────────────────────────────────────────

/** How many conditions a marker spells out before it summarises the rest. */
const MARKER_LABEL_MAX = 2;

function joinFired(labels: string[]): string {
  const shown = labels.slice(0, MARKER_LABEL_MAX).join(' + ');
  const rest = labels.length - MARKER_LABEL_MAX;
  return rest > 0 ? `${shown} +${rest}` : shown;
}

/** Whether a stage deadline is reached at row `i` (engine `deadline_reached`). */
function deadlineReached(
  d: Deadline,
  judge: SeriesJudge,
  i: number,
  entrySec: number,
  stageStartSec: number,
): boolean {
  const t = judge.atSec[i];
  switch (d.basis) {
    case 'age_sec':
      return judge.readLabel(AGE_SEC, i) >= d.secs;
    case 'held_sec':
      return t - entrySec >= d.secs;
    case 'stage_sec':
      return t - stageStartSec >= d.secs;
  }
}

/**
 * The rule's first buy and first sell over the series, as `signal` markers: the
 * frontend's estimate, never the backend's fills.
 *
 * Entry mirrors `CompiledRule::can_enter`: `enter.event`, `final_filters` and `filters`
 * hold, and no sell line that could act right after a buy (`always`, stage one's `on`)
 * already holds. Its label names the conditions that FLIPPED on that row: one that had
 * held for minutes decided nothing about the timing. A rule with no entry condition
 * buys on arming, at the first row, with no marker.
 *
 * The held side mirrors `held_line`: `always` lines, then the stage deadline (its
 * `at_end` lines, else the move to `then`), else the stage's `on` lines; a `go` moves
 * the stage. The first sell ends the walk and is labelled with the exit reason it books.
 * Not modelled: the entry lock, and `m_position` reads other than the stage clock,
 * which come from the series (the inspected run's entry fill, absent without one).
 */
export function findRuleFireMarkers(
  rule: PaneRule,
  data: MetricSeriesResponse,
  registry: StrategyRegistry | undefined,
): ChartEventMarker[] {
  const n = data.at.length;
  if (n === 0) return [];
  const judge = new SeriesJudge(rule, data, registry);
  const { doc } = rule;
  const entryConds = [...doc.enter.event, ...doc.enter.final_filters, ...doc.enter.filters].filter((c) => !c.off);
  const vetoLines = [...rule.always, ...(doc.stages[0]?.on ?? [])].filter((l) => l.sell);

  let entryIdx: number | null = entersOnArm(doc) ? 0 : null;
  let entryLabel: string | null = null;
  if (entryIdx == null) {
    for (let i = 0; i < n; i++) {
      if (!judge.allHold(entryConds, i, null) || judge.firstLine(vetoLines, i, null)) continue;
      entryIdx = i;
      const flipped = i === 0 ? [] : entryConds.filter((c) => !judge.holds(c, i - 1, null));
      entryLabel = joinFired((flipped.length ? flipped : entryConds).map((c) => judge.firedLabel(c, i, null)));
      break;
    }
  }

  let exitIdx: number | null = null;
  let exitLabel: string | null = null;
  if (entryIdx != null) {
    const entrySec = judge.atSec[entryIdx];
    let stage = 0;
    let stageStartSec = entrySec;
    for (let i = entryIdx + 1; i < n && exitIdx == null; i++) {
      const pos = { stageStartSec };
      let acted = judge.firstLine(rule.always, i, pos);
      const st = doc.stages[stage];
      if (!acted && st) {
        if (st.ends && deadlineReached(st.ends, judge, i, entrySec, stageStartSec)) {
          acted = judge.firstLine(st.at_end, i, pos);
          if (!acted) {
            stage = st.then != null ? doc.stages.findIndex((s) => s.name === st.then) : stage + 1;
            if (stage < 0) stage = doc.stages.length;
            stageStartSec = judge.atSec[i];
            continue;
          }
        } else {
          acted = judge.firstLine(st.on, i, pos);
        }
      }
      if (!acted) continue;
      if (acted.sell) {
        exitIdx = i;
        exitLabel = lineExitLabel(acted);
      } else if (acted.go) {
        const next = doc.stages.findIndex((s) => s.name === acted.go);
        stage = next < 0 ? doc.stages.length : next;
        stageStartSec = judge.atSec[i];
      }
    }
  }

  const markers: ChartEventMarker[] = [];
  const push = (kind: 'entry' | 'exit', idx: number | null, label: string | null) => {
    if (idx == null || label == null) return;
    const price = data.price?.[idx];
    if (price == null || !Number.isFinite(price)) return;
    markers.push({ kind, role: 'signal', time: data.at[idx], priceInSol: price, label });
  };
  push('entry', entryIdx, entryLabel);
  push('exit', exitIdx, exitLabel);
  return markers;
}

// ── Time axis ───────────────────────────────────────────────────────────────

/**
 * Last series index at or before a wall-clock unix second: the state **as of**
 * `timeSec`, which is what a hovered candle asks for.
 *
 * Distinct from {@link nearestSeriesIndex}: a row lands on every trade, so "nearest"
 * can jump FORWARD past trades the hovered instant had not seen yet. `null` only when
 * every row is later than `timeSec`, a real answer the caller renders as "no reading
 * here" rather than clamping to row 0.
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
