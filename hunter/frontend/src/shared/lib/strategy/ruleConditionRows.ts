// Row model for the rule editor's condition builder — the analogue of the sweep's
// `genericAxes.ts` (`GenericAxisRow`). A rule is authored as a flat list of
// condition rows the user adds/deletes one by one; on save the rows fold into the
// nested `SideConditions` (`Record<group, GroupConditions[]>`) the backend stores.
//
// Because the trailing **window is a per-row field**, the same metric at two
// windows is simply two rows (`m_price_window.trail@5` + `@30`) — they group into
// two `GroupConditions` instances of the one group, exactly the engine's
// multi-window-per-group model. This is the FE half of the fix that let the sweep
// sweep two windows of one metric; the rule editor now authors the same shape.

import type { ConditionExpr } from './grammar';
import type { DisabledConditions, RuleParams, SideConditions } from './ruleParams';
import type { StrategyRegistry } from './registry';
import type { GroupConditions } from './ruleParams';
import {
  SLICE_PARAM,
  SLICE_PRINT_PARAM,
  SLICE_SLOT_PARAM,
  sliceSizeParam,
  formatWindowSpec,
  sizeParam,
  unitSuffix,
  WINDOW_LAG_PARAM,
  WINDOW_SIZE_PARAMS,
  windowSpecFromStrict,
  windowSpecKey,
  withoutAxis,
  withoutSliceAxis,
  type WindowSpec,
  type WindowUnit,
} from './windowSpec';

export { SLICE_PARAM, SLICE_PRINT_PARAM, SLICE_SLOT_PARAM };

/** Which side a condition row applies to (the column owns it). `entry_event` is
 *  the completing-print AND; `entry` is the filter AND evaluated on that print. */
export type RuleConditionSide = 'entry' | 'entry_event' | 'exit';

/** One editor row of the rule condition builder. */
export interface RuleConditionRow {
  /** Stable client id (list-render key + remove target). */
  id: string;
  side: RuleConditionSide;
  /** `false` = **parked**: the row is kept and still validated, but folds into the
   *  `disabled` bag instead of the live side, so the engine never compiles it.
   *  `undefined` reads as enabled — a row from before this field existed is live. */
  enabled?: boolean;
  /** Registry group name (e.g. `m_state`), '' until picked. */
  group: string;
  /** Registry metric name (e.g. `time`), '' until picked. */
  metric: string;
  /** Trailing window SIZE as raw text; '' when static / unset. The unit it counts
   *  in is {@link RuleConditionRow.windowUnit} — a bare number is not a window. */
  window: string;
  /** What this row's windows count in — `'sec'`, `'slot'` or `'print'`. `undefined`
   *  reads as `'sec'`, so a row from before slots existed is a wall-clock row. Both
   *  axes of a two-window group share it: a slice in slots over a reference in
   *  seconds is a ratio across two clocks, which the backend rejects at save. */
  windowUnit?: WindowUnit;
  /** How many units back from *now* the window ENDS, as raw text; '' = ends at now.
   *  This is what makes a window causal in its own terms — a gate on "the state
   *  entering this slot" must not be able to see the slot it fires in. */
  lag?: string;
  /** The metric's DNF condition arms (the `ConditionInput` value). Empty = the
   *  row carries no constraint yet and is dropped on serialize. */
  arms: ConditionExpr;
  /** Strict params of this row's group instance OTHER than the window params
   *  (which the `window` / `windowUnit` / `lag` fields own). Carried per row so the
   *  row model stays flat, and merged back per instance on serialize.
   *
   *  This exists so the editor never silently DROPS a strict param it has no
   *  dedicated control for: a rule authored with `m_position.arm_above_pct` and
   *  then opened and re-saved in the UI must come back out unchanged. New registry
   *  strict params round-trip for free; only an editing *control* is per-param work. */
  strict?: Record<string, number>;
  /**
   * Exit **way** this row belongs to (OR of ways, AND inside a way). Absent on
   * entry rows and on scale-out stage rows (those stay object-form / flat OR).
   * The rule editor always stamps one: object-form load is one way per metric.
   */
  clauseId?: string;
}

/** Next column a ⇄ flip lands in: event → filters → exit → event. */
export function flipConditionSide(side: RuleConditionSide): RuleConditionSide {
  if (side === 'entry_event') return 'entry';
  if (side === 'entry') return 'exit';
  return 'entry_event';
}

/** Human label for the column a ⇄ flip will move a row into. */
export function flipSideLabel(side: RuleConditionSide): string {
  const next = flipConditionSide(side);
  return next === 'entry_event' ? 'event' : next === 'entry' ? 'filters' : 'exit';
}

let rowSeq = 0;
/** A fresh, empty condition row for one side. The id mixes a counter with a random
 *  suffix so a freshly-added row can't collide with one restored from a prior state. */
export function newRuleConditionRow(side: RuleConditionSide): RuleConditionRow {
  return {
    id: `cond-${rowSeq++}-${Math.random().toString(36).slice(2, 7)}`,
    side,
    enabled: true,
    group: '',
    metric: '',
    window: '',
    windowUnit: 'sec',
    lag: '',
    arms: [],
  };
}

let clauseSeq = 0;
/** Fresh id for one exit way. Stable for the editor session; not persisted. */
export function newExitClauseId(): string {
  return `way-${clauseSeq++}-${Math.random().toString(36).slice(2, 7)}`;
}

/** A new empty row that starts (or joins) an exit way. */
export function newExitClauseRow(clauseId?: string): RuleConditionRow {
  return { ...newRuleConditionRow('exit'), clauseId: clauseId ?? newExitClauseId() };
}

/** A row's live/parked state. `undefined` = enabled (rows predate the toggle). */
export function ruleRowEnabled(row: RuleConditionRow): boolean {
  return row.enabled !== false;
}

/** The registry group a row names, if any. */
function rowGroup(row: RuleConditionRow, reg: StrategyRegistry | undefined) {
  return reg?.groups.find((g) => g.name === row.group);
}

/** True when a row's group needs a trailing window at all (dynamic group). Which
 *  size param spells it is the row's `windowUnit`. */
export function ruleRowNeedsWindow(
  row: RuleConditionRow,
  reg: StrategyRegistry | undefined,
): boolean {
  return rowGroup(row, reg)?.kind === 'dynamic';
}

/** The two trailing metrics `m_position.arm_above_pct` gates — mirrors the backend
 *  SSOT `hunter_engine::metrics::position::is_trailing` (hardcoded there too; no
 *  registry metadata flags "trailing" today). */
const TRAILING_METRICS = new Set(['retrace', 'bounce']);

/** True when a row is `m_position.retrace` / `.bounce` — the only metrics
 *  `arm_above_pct` does anything to. Gates the arm-control's visibility. */
export function ruleRowIsTrailing(row: RuleConditionRow): boolean {
  return row.group === 'm_position' && TRAILING_METRICS.has(row.metric);
}

/** What a row's windows count in. Absent ⇒ seconds, so a row authored before slots
 *  existed keeps its meaning. */
export function ruleRowUnit(row: RuleConditionRow): WindowUnit {
  return row.windowUnit ?? 'sec';
}

/** A row's lag — units back from now its windows END. Blank / unparseable ⇒ 0
 *  (ends at now), which is the only behaviour that existed before the param. */
function rowLag(row: RuleConditionRow): number {
  const v = Number(row.lag ?? '');
  return (row.lag ?? '').trim() !== '' && Number.isFinite(v) && v >= 0 ? v : 0;
}

/** The parsed, positive window a row carries, or `null` (static / unset). */
export function ruleRowWindowSpec(row: RuleConditionRow): WindowSpec | null {
  const size = Number(row.window);
  if (row.window.trim() === '' || !Number.isFinite(size) || size <= 0) return null;
  return { size, lag: rowLag(row), unit: ruleRowUnit(row) };
}

/** The row's slice axis (`m_flow_window`'s second window), or `null`. It rides the
 *  row's unit and lag — that pair IS the two-window basis — and differs only in size. */
export function ruleRowSliceSpec(row: RuleConditionRow): WindowSpec | null {
  const size = row.strict?.[sliceSizeParam(ruleRowUnit(row))];
  if (typeof size !== 'number' || !Number.isFinite(size) || size <= 0) return null;
  return { size, lag: rowLag(row), unit: ruleRowUnit(row) };
}

/** Key of the `GroupConditions` instance a row folds into: rows sharing it are
 *  merged into ONE instance by {@link rowsToSide}, so they share that instance's
 *  strict bag (`window_size_sec`, `arm_above_pct`, …). Live and parked rows land in
 *  different bags, hence different instances. */
export function ruleRowInstanceKey(row: RuleConditionRow): string {
  const on = ruleRowEnabled(row) ? 'on' : 'off';
  // The WHOLE span is the identity — size, lag AND unit. A bare size would merge
  // `30 slots` with `30 seconds`, and `30 slots lag 0` with `30 slots lag 1`, which
  // read different tape entirely; the later row's strict bag would then silently win
  // and one of the two gates would disappear on save.
  //
  // A two-window read's identity is the PAIR, so the slice axis is in the key too:
  // without it `m_flow_window{60,3}` and `m_flow_window{60,10}` merge the same way.
  return [
    on,
    row.side,
    row.clauseId ?? '',
    row.group,
    windowSpecKey(ruleRowWindowSpec(row)),
    windowSpecKey(ruleRowSliceSpec(row)),
  ].join('|');
}

/**
 * Set the strict bag of the instance a row belongs to, across EVERY row of that
 * instance. `rowsToSide` merges the bags of all rows folding into one instance, so
 * a bag written to a single row is silently overwritten on save by a sibling row
 * still carrying the old value — clearing `arm_above_pct` on the `retrace` row does
 * nothing while the instance's `pnl` row still holds it. The invariant this keeps:
 * **rows of one instance agree on `strict`** (which is also what `sideToRows`
 * produces when loading a rule).
 */
export function setRowInstanceStrict(
  rows: RuleConditionRow[],
  id: string,
  strict: Record<string, number>,
): RuleConditionRow[] {
  const target = rows.find((r) => r.id === id);
  if (!target) return rows;
  const key = ruleRowInstanceKey(target);
  return rows.map((r) => (ruleRowInstanceKey(r) === key ? { ...r, strict: { ...strict } } : r));
}

/** True when this row's METRIC reads the slice axis — the frontend mirror of the
 *  backend `is_two_window`.
 *
 *  Per-metric, not per-group, because `m_flow_window` declares `slice_size_*` for
 *  every instance while only `trade_share` / `sol_share` read it. Asking the group
 *  would put a slice control on a `gross_flow` row and the save would then be
 *  rejected as a span nothing reads. Both facts come off the registry payload, so a
 *  future two-window metric gets its control and its validation for free. */
export function ruleRowNeedsSlice(
  row: RuleConditionRow,
  reg: StrategyRegistry | undefined,
): boolean {
  const group = rowGroup(row, reg);
  if (!group?.strict_params.some((sp) => sp.name === SLICE_PARAM)) return false;
  return group.metrics.find((m) => m.name === row.metric)?.two_window ?? false;
}

/**
 * Per-row validation (mirrors the backend `resolve_one` / rule-save gate). Returns
 * an error string to surface under the row, or `null` when the row is a valid
 * condition. Malformed grammar is handled by the `ConditionInput` itself.
 */
export function ruleConditionRowError(
  row: RuleConditionRow,
  reg: StrategyRegistry | undefined,
): string | null {
  if (!row.group) return 'pick a metric group';
  const group = rowGroup(row, reg);
  if (!group) return 'pick a metric group';
  // Position-scoped groups read NaN before entry — an entry or event condition can
  // never fire, so the backend rejects it. Flag it here (mirrors the sweep axis rule).
  if (row.side !== 'exit' && group.scope === 'position')
    return `${group.name} is exit-only (no value before entry) — move it to the exit column`;
  if (!row.metric || !group.metrics.some((m) => m.name === row.metric)) return 'pick a metric';
  const unit = ruleRowUnit(row);
  // The short suffix, so an error names the field exactly as its label does
  // (`window s` / `window sl`) rather than in a second vocabulary.
  const u = unitSuffix(unit);
  const win = ruleRowWindowSpec(row);
  if (group.kind === 'dynamic' && win == null) return `window (${u}) > 0 required`;
  // The lag is what makes a window causal in its own terms, and `0` is a real value
  // of its domain (end at now) rather than "absent" — so only a negative or
  // unparseable entry is an error.
  const lagText = (row.lag ?? '').trim();
  if (lagText !== '') {
    const lag = Number(lagText);
    if (!Number.isFinite(lag) || lag < 0) return `lag (${u}) must be a number ≥ 0`;
  }
  // Second window axis: required by the METRICS that read it, in the SAME unit as
  // the reference, and it must nest inside it or the share counts trades the
  // denominator does not. A row whose metric does not read it must not carry one —
  // the backend rejects that as a span nothing reads.
  const slice = ruleRowSliceSpec(row);
  if (ruleRowNeedsSlice(row, reg)) {
    if (slice == null) return `slice (${u}) > 0 required`;
    if (win != null && slice.size > win.size)
      return `slice (${u}) must nest inside window ${win.size}`;
  } else if (slice != null) {
    return `slice (${u}) is read only by trade_share / sol_share`;
  }
  if (row.arms.length === 0) return 'add a condition (e.g. > 10)';
  const arm = row.strict?.arm_above_pct;
  if (arm != null && (!Number.isFinite(arm) || arm < 0)) return 'arm ≥ % must be a number ≥ 0';
  return null;
}

/**
 * Cross-row check: `arm_above_pct` gates only the trailing metrics (`retrace` /
 * `bounce`) on an `m_position` instance — authored without one it is a silent
 * no-op, so the backend rejects it at save (`rule_params.rs`). Mirror that here
 * so a rule imported via the JSON view surfaces the same problem before save.
 */
export function armAbovePctOrphanError(rows: RuleConditionRow[]): string | null {
  const instances = new Map<string, { hasArm: boolean; hasTrailing: boolean }>();
  for (const row of rows) {
    if (row.group !== 'm_position') continue;
    // Live and parked rows land in different bags, so they form different group
    // instances — an arm on a live row is NOT satisfied by a parked trailing row.
    // (This is the check that catches "I parked `retrace` and orphaned its arm".)
    const key = ruleRowInstanceKey(row);
    const inst = instances.get(key) ?? { hasArm: false, hasTrailing: false };
    if (row.strict?.arm_above_pct != null) inst.hasArm = true;
    if (ruleRowIsTrailing(row)) inst.hasTrailing = true;
    instances.set(key, inst);
  }
  for (const [key, inst] of instances) {
    if (inst.hasArm && !inst.hasTrailing) {
      const [on, side] = key.split('|');
      const where = on === 'on' ? side : `disabled ${side}`;
      return `${where} m_position: arm_above_pct gates the trailing metrics (retrace / bounce) — add one or remove arm_above_pct`;
    }
  }
  return null;
}

/**
 * Cross-row check: two rows on the same (side, group, window, metric) both write
 * `metrics[metric]` of the one instance, so the later silently overwrites the
 * earlier. Surface it rather than drop a condition. Returns a form-level error, or
 * `null`. (Different windows are fine — they land in different instances.)
 */
export function duplicateConditionRowError(rows: RuleConditionRow[]): string | null {
  const seen = new Set<string>();
  for (const row of rows) {
    if (!row.group || !row.metric) continue;
    const w = ruleRowWindowSpec(row);
    const on = ruleRowEnabled(row);
    // Keyed by the instance (so live/parked, each window, AND each exit way are
    // their own bag — two ways may both name `m_position.armed`) plus the metric,
    // which is what actually collides inside one instance.
    const key = `${ruleRowInstanceKey(row)}|${row.metric}`;
    if (seen.has(key)) {
      const at = w ? `@${formatWindowSpec(w)}` : '';
      const where = on ? row.side : `disabled ${row.side}`;
      return `${where} ${row.group}.${row.metric}${at} is set twice — merge the conditions into one row`;
    }
    seen.add(key);
  }
  return null;
}

/**
 * Warnings (not errors) for a side whose conditions are ALL parked. Turning off the
 * last row is a rule rewrite dressed as unchecking a box: an empty entry side means
 * the fingerprint alone fires the buy (`enter_on_arm`), and an empty exit side means
 * only TP/SL/death can close. Both are legal rules — hence a warning — but nobody
 * should discover it from a fill.
 */
export function parkedSideWarnings(rows: RuleConditionRow[]): string[] {
  const out: string[] = [];
  const authored = (r: RuleConditionRow) => r.group && r.metric && r.arms.length > 0;
  const labels: Record<RuleConditionSide, string> = {
    entry:
      'every filter is off — the rule now buys on the fingerprint and event alone',
    entry_event:
      'every event condition is off — the completing-print gate is gone (and entry_lock will not save)',
    exit: 'every exit condition is off — only TP / SL / death can close a position',
  };
  for (const side of ['entry_event', 'entry', 'exit'] as const) {
    const ofSide = rows.filter((r) => r.side === side && authored(r));
    if (ofSide.length === 0 || ofSide.some(ruleRowEnabled)) continue;
    out.push(side === 'entry' ? labels.entry : labels[side]);
  }
  return out;
}

/**
 * Fold the rows of one side into `SideConditions` — group by (group, window) into
 * one `GroupConditions` instance each (the engine's multi-window-per-group model).
 * Half-authored rows (no group/metric) and empty-condition rows are skipped, so the
 * output only ever carries constraints the backend will accept.
 */
export function rowsToSide(
  rows: RuleConditionRow[],
  side: RuleConditionSide,
  enabled = true,
): SideConditions {
  const out: SideConditions = {};
  const instanceKeys = new Map<string, GroupConditions>();
  for (const row of rows) {
    if (row.side !== side || ruleRowEnabled(row) !== enabled) continue;
    if (!row.group || !row.metric || row.arms.length === 0) continue;
    const w = ruleRowWindowSpec(row);
    const instances = out[row.group] ?? (out[row.group] = []);
    // Rows share an instance only when they name the SAME span on both axes — the
    // same identity `ruleRowInstanceKey` uses, so what the editor validates as one
    // instance is what gets written as one.
    const key = ruleRowInstanceKey(row);
    let inst = instanceKeys.get(key);
    if (!inst) {
      inst = { strict: {}, metrics: {} };
      if (w != null) {
        // Exactly ONE size param, spelled in the row's unit — writing both is what
        // the backend rejects as "two spans claiming one axis".
        inst.strict[sizeParam(w.unit)] = w.size;
        // A zero lag is the default and the only behaviour that existed before the
        // param, so it stays absent — that is what keeps a pre-slot rule
        // byte-identical through a load-and-save.
        if (w.lag > 0) inst.strict[WINDOW_LAG_PARAM] = w.lag;
      }
      instances.push(inst);
      instanceKeys.set(key, inst);
    }
    // Non-window strict params ride on the row so nothing the editor has no control
    // for is lost on re-save. Rows of one instance agree (sideToRows copies the same
    // bag onto each), so a later row merging in is a no-op rather than a conflict.
    // The slice axis is re-spelled in the row's unit for the same reason the
    // reference span is: a unit flip must not leave the old param behind.
    Object.assign(inst.strict, withoutSliceAxis(row.strict));
    const slice = ruleRowSliceSpec(row);
    if (slice != null) inst.strict[sliceSizeParam(slice.unit)] = slice.size;
    inst.metrics[row.metric] = row.arms;
  }
  return out;
}

/** Buy + sell sides folded — the `entry`/`entry_event`/`exit`/`exitClauses`/`disabled`
 *  of a `RuleParams`. Parked rows fold into `disabled` with the identical shape, so
 *  nothing is dropped on save and the live sides stay exactly what the engine will
 *  compile.
 *
 *  Exit rows **without** `clauseId` (scale-out stages, legacy tests) fold as today's
 *  object-form OR. Rows **with** `clauseId` group into ways: all-singleton ways with
 *  no metric collision collapse back to object-form so stored rules round-trip;
 *  any way with two-or-more metrics (or a colliding singleton pair) serializes as
 *  array-form DNF. */
export function rowsToSides(rows: RuleConditionRow[]): {
  entry: SideConditions;
  entry_event: SideConditions;
  exit: SideConditions;
  exitClauses?: SideConditions[];
  disabled: DisabledConditions | null;
} {
  const liveExit = foldExitRows(rows, true);
  const parkedExit = foldExitRows(rows, false);
  const off: DisabledConditions = {
    entry: rowsToSide(rows, 'entry', false),
    entry_event: rowsToSide(rows, 'entry_event', false),
    exit: parkedExit.exit,
    exitClauses: parkedExit.exitClauses,
  };
  const parked =
    Object.keys(off.entry ?? {}).length ||
    Object.keys(off.entry_event ?? {}).length ||
    Object.keys(off.exit ?? {}).length ||
    (off.exitClauses?.length ?? 0);
  return {
    entry: rowsToSide(rows, 'entry'),
    entry_event: rowsToSide(rows, 'entry_event'),
    exit: liveExit.exit,
    exitClauses: liveExit.exitClauses,
    disabled: parked ? off : null,
  };
}

function sideMetricCount(side: SideConditions): number {
  let n = 0;
  for (const insts of Object.values(side)) {
    for (const g of insts) {
      n += Object.values(g.metrics).filter((a) => a?.length).length;
    }
  }
  return n;
}

function exitMergeCollides(clauses: SideConditions[]): boolean {
  const seen = new Set<string>();
  for (const side of clauses) {
    for (const [group, insts] of Object.entries(side)) {
      for (const inst of insts) {
        const w = windowSpecKey(windowSpecFromStrict(inst.strict));
        for (const metric of Object.keys(inst.metrics)) {
          const key = `${group}|${w}|${metric}`;
          if (seen.has(key)) return true;
          seen.add(key);
        }
      }
    }
  }
  return false;
}

/** Fold exit rows: clause-stamped → DNF (or object collapse); unstamped → object. */
function foldExitRows(
  rows: RuleConditionRow[],
  enabled: boolean,
): { exit: SideConditions; exitClauses?: SideConditions[] } {
  const ofSide = rows.filter((r) => r.side === 'exit' && ruleRowEnabled(r) === enabled);
  if (ofSide.length === 0) return { exit: {} };
  const stamped = ofSide.some((r) => r.clauseId);
  if (!stamped) return { exit: rowsToSide(rows, 'exit', enabled) };

  const order: string[] = [];
  const byClause = new Map<string, RuleConditionRow[]>();
  const unclaused: RuleConditionRow[] = [];
  for (const r of ofSide) {
    const id = r.clauseId;
    if (!id) {
      unclaused.push(r);
      continue;
    }
    if (!byClause.has(id)) {
      order.push(id);
      byClause.set(id, []);
    }
    byClause.get(id)!.push(r);
  }
  const clauses: SideConditions[] = [];
  for (const id of order) {
    const side = rowsToSide(byClause.get(id)!, 'exit', enabled);
    if (Object.keys(side).length) clauses.push(side);
  }
  for (const r of unclaused) {
    const side = rowsToSide([r], 'exit', enabled);
    if (Object.keys(side).length) clauses.push(side);
  }
  if (clauses.length === 0) return { exit: {} };
  const allSingletons = clauses.every((c) => sideMetricCount(c) === 1);
  if (allSingletons && !exitMergeCollides(clauses)) {
    // Strip clause identity so two singleton ways on `m_position` merge into ONE
    // static instance (object-form). Leaving the id in the instance key would emit
    // two instances of a static group, which the backend rejects.
    return {
      exit: rowsToSide(
        ofSide.map((r) => ({ ...r, clauseId: undefined })),
        'exit',
        enabled,
      ),
    };
  }
  return { exit: {}, exitClauses: clauses };
}

/** First-seen exit way ids, including empty draft ways the user just added. */
export function exitClauseOrder(rows: RuleConditionRow[]): string[] {
  const out: string[] = [];
  const seen = new Set<string>();
  for (const r of rows) {
    if (r.side !== 'exit' || !r.clauseId || seen.has(r.clauseId)) continue;
    seen.add(r.clauseId);
    out.push(r.clauseId);
  }
  return out;
}

/** Expand a nested side back into editor rows — one row per (group, window, metric).
 *  The inverse of {@link rowsToSide} for loading an existing rule / JSON edit. */
export function sideToRows(
  side: SideConditions | undefined,
  sideName: RuleConditionSide,
  enabled = true,
): RuleConditionRow[] {
  const rows: RuleConditionRow[] = [];
  for (const [groupName, instances] of Object.entries(side ?? {})) {
    for (const inst of instances) {
      const spec = windowSpecFromStrict(inst.strict);
      const windowText = spec ? String(spec.size) : '';
      const lagText = spec && spec.lag > 0 ? String(spec.lag) : '';
      // Everything except the reference span, which the `window` / `windowUnit` /
      // `lag` fields own. The BURST axis stays in the bag (there is a control for
      // it) but the reference lag does not — it is shared by both axes, so leaving a
      // copy here would let a stale value outlive a lag edit.
      const strict = withoutAxis(inst.strict, [...WINDOW_SIZE_PARAMS, WINDOW_LAG_PARAM]);
      for (const [metric, arms] of Object.entries(inst.metrics)) {
        if (!arms?.length) continue;
        rows.push({
          ...newRuleConditionRow(sideName),
          enabled,
          group: groupName,
          metric,
          window: windowText,
          // A group with no reference span authors no windows at all, so it keeps
          // the default unit rather than inventing one.
          windowUnit: spec?.unit ?? 'sec',
          lag: lagText,
          arms,
          strict,
        });
      }
    }
  }
  return rows;
}

/** Load a parsed `RuleParams` into a single row list — live sides first, then the
 *  parked bag as rows with `enabled: false`. Inverse of {@link rowsToSides}.
 *  Object-form exit becomes one way per metric; array-form keeps AND grouping. */
export function paramsToConditionRows(p: RuleParams): RuleConditionRow[] {
  return [
    ...sideToRows(p.entry_event, 'entry_event'),
    ...sideToRows(p.entry, 'entry'),
    ...exitToRows(p.exit, p.exitClauses, true),
    ...sideToRows(p.disabled?.entry_event, 'entry_event', false),
    ...sideToRows(p.disabled?.entry, 'entry', false),
    ...exitToRows(p.disabled?.exit, p.disabled?.exitClauses, false),
  ];
}

function exitToRows(
  exit: SideConditions | undefined,
  clauses: SideConditions[] | undefined,
  enabled: boolean,
): RuleConditionRow[] {
  if (clauses && clauses.length > 0) {
    return clauses.flatMap((c) => {
      const id = newExitClauseId();
      return sideToRows(c, 'exit', enabled).map((r) => ({ ...r, clauseId: id }));
    });
  }
  return sideToRows(exit, 'exit', enabled).map((r) => ({
    ...r,
    clauseId: newExitClauseId(),
  }));
}

/** Load a parsed `RuleParams` into a single row list — live sides first, then the
 *  parked bag as rows with `enabled: false`. Inverse of {@link rowsToSides}. */
export function sidesToRows(
  entry: SideConditions | undefined,
  exit: SideConditions | undefined,
  disabled?: DisabledConditions | null,
  entryEvent?: SideConditions,
): RuleConditionRow[] {
  return [
    ...sideToRows(entryEvent, 'entry_event'),
    ...sideToRows(entry, 'entry'),
    ...sideToRows(exit, 'exit'),
    ...sideToRows(disabled?.entry_event, 'entry_event', false),
    ...sideToRows(disabled?.entry, 'entry', false),
    ...sideToRows(disabled?.exit, 'exit', false),
  ];
}
