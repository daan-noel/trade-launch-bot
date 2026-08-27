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
import type { DisabledConditions, SideConditions } from './ruleParams';
import type { StrategyRegistry } from './registry';

/** Which side a condition row applies to (the column owns it). */
export type RuleConditionSide = 'entry' | 'exit';

/** One editor row of the rule condition builder. */
export interface RuleConditionRow {
  /** Stable client id (list-render key + remove target). */
  id: string;
  side: RuleConditionSide;
  /** `false` = **parked**: the row is kept and still validated, but folds into the
   *  `disabled` bag instead of the live side, so the engine never compiles it.
   *  `undefined` reads as enabled — a row from before this field existed is live. */
  enabled?: boolean;
  /** Registry group name (e.g. `m_snapshot`), '' until picked. */
  group: string;
  /** Registry metric name (e.g. `time`), '' until picked. */
  metric: string;
  /** Trailing window (seconds) as raw text; '' when static / unset. */
  window: string;
  /** The metric's DNF condition arms (the `ConditionInput` value). Empty = the
   *  row carries no constraint yet and is dropped on serialize. */
  arms: ConditionExpr;
  /** Strict params of this row's group instance OTHER than `window_size_sec`
   *  (which is the `window` field). Carried per row so the row model stays flat,
   *  and merged back per instance on serialize.
   *
   *  This exists so the editor never silently DROPS a strict param it has no
   *  dedicated control for: a rule authored with `m_position.arm_above_pct` and
   *  then opened and re-saved in the UI must come back out unchanged. New registry
   *  strict params round-trip for free; only an editing *control* is per-param work. */
  strict?: Record<string, number>;
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
    arms: [],
  };
}

/** A row's live/parked state. `undefined` = enabled (rows predate the toggle). */
export function ruleRowEnabled(row: RuleConditionRow): boolean {
  return row.enabled !== false;
}

/** The registry group a row names, if any. */
function rowGroup(row: RuleConditionRow, reg: StrategyRegistry | undefined) {
  return reg?.groups.find((g) => g.name === row.group);
}

/** True when a row's group needs a `window_size_sec` (dynamic group). */
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

/** The parsed, positive window a row carries, or `null` (static / unset). */
function rowWindow(row: RuleConditionRow): number | null {
  const w = Number(row.window);
  return row.window.trim() !== '' && Number.isFinite(w) && w > 0 ? w : null;
}

/** Key of the `GroupConditions` instance a row folds into: rows sharing it are
 *  merged into ONE instance by {@link rowsToSide}, so they share that instance's
 *  strict bag (`window_size_sec`, `arm_above_pct`, …). Live and parked rows land in
 *  different bags, hence different instances. */
export function ruleRowInstanceKey(row: RuleConditionRow): string {
  const on = ruleRowEnabled(row) ? 'on' : 'off';
  // A two-window group's identity is the PAIR, so the burst axis is part of the key.
  // Without it `m_flow_burst{60,3}` and `m_flow_burst{60,10}` merge into one instance
  // and the later row's strict bag silently wins — one of the two gates disappears.
  const burst = row.strict?.[BURST_PARAM];
  const b = typeof burst === 'number' ? burst : '∅';
  return `${on}|${row.side}|${row.group}|${rowWindow(row) ?? '∅'}|${b}`;
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

/** The second trailing-window strict param (`m_flow_burst`). Mirrors the backend
 *  SSOT `hunter_engine::metrics::flow_burst::BURST_PARAM`. */
export const BURST_PARAM = 'burst_size_sec';

function sameWindow(a: number | undefined, b: number | null): boolean {
  const av = typeof a === 'number' ? a : null;
  if (av == null && b == null) return true;
  if (av == null || b == null) return false;
  return Math.abs(av - b) < 1e-9;
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
  // Position-scoped groups read NaN before entry — an entry condition can never
  // fire, so the backend rejects it. Flag it here (mirrors the sweep axis rule).
  if (row.side === 'entry' && group.scope === 'position')
    return `${group.name} is exit-only (no value before entry) — move it to the exit column`;
  if (!row.metric || !group.metrics.some((m) => m.name === row.metric)) return 'pick a metric';
  if (group.kind === 'dynamic' && rowWindow(row) == null) return 'window (s) > 0 required';
  // Second window axis (`m_flow_burst`): required by the registry, and it must nest
  // inside the reference window or the share counts trades the denominator does not.
  if (group.strict_params.some((sp) => sp.name === BURST_PARAM && sp.required)) {
    const burst = row.strict?.[BURST_PARAM];
    if (typeof burst !== 'number' || !Number.isFinite(burst) || burst <= 0)
      return 'burst (s) > 0 required';
    const w = rowWindow(row);
    if (w != null && burst > w) return `burst (s) must nest inside window ${w}`;
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
    const w = rowWindow(row);
    const on = ruleRowEnabled(row);
    // Keyed by the instance (so live/parked and each window are their own bag — that
    // pairing is exactly what the toggle is for: park a value, try another) plus the
    // metric, which is what actually collides inside one instance.
    const key = `${ruleRowInstanceKey(row)}|${row.metric}`;
    if (seen.has(key)) {
      const at = w != null ? `@${w}s` : '';
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
  for (const side of ['entry', 'exit'] as const) {
    const ofSide = rows.filter((r) => r.side === side && authored(r));
    if (ofSide.length === 0 || ofSide.some(ruleRowEnabled)) continue;
    out.push(
      side === 'entry'
        ? 'every entry condition is off — the rule now buys on the fingerprint alone'
        : 'every exit condition is off — only TP / SL / death can close a position',
    );
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
  for (const row of rows) {
    if (row.side !== side || ruleRowEnabled(row) !== enabled) continue;
    if (!row.group || !row.metric || row.arms.length === 0) continue;
    const w = rowWindow(row);
    const instances = out[row.group] ?? (out[row.group] = []);
    const burst = row.strict?.[BURST_PARAM] ?? null;
    let inst = instances.find(
      (g) =>
        sameWindow(g.strict.window_size_sec, w) && sameWindow(g.strict[BURST_PARAM], burst),
    );
    if (!inst) {
      inst = { strict: {}, metrics: {} };
      if (w != null) inst.strict.window_size_sec = w;
      instances.push(inst);
    }
    // Non-window strict params ride on the row so nothing the editor has no control
    // for is lost on re-save. Rows of one instance agree (sideToRows copies the same
    // bag onto each), so a later row merging in is a no-op rather than a conflict.
    Object.assign(inst.strict, row.strict ?? {});
    inst.metrics[row.metric] = row.arms;
  }
  return out;
}

/** Both sides folded — the `entry`/`exit`/`disabled` of a `RuleParams`. Parked rows
 *  fold into `disabled` with the identical shape, so nothing is dropped on save and
 *  the live sides stay exactly what the engine will compile. */
export function rowsToSides(rows: RuleConditionRow[]): {
  entry: SideConditions;
  exit: SideConditions;
  disabled: DisabledConditions | null;
} {
  const off: DisabledConditions = {
    entry: rowsToSide(rows, 'entry', false),
    exit: rowsToSide(rows, 'exit', false),
  };
  const parked = Object.keys(off.entry ?? {}).length || Object.keys(off.exit ?? {}).length;
  return {
    entry: rowsToSide(rows, 'entry'),
    exit: rowsToSide(rows, 'exit'),
    disabled: parked ? off : null,
  };
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
      const w = inst.strict.window_size_sec;
      const windowText = typeof w === 'number' && w > 0 ? String(w) : '';
      // Everything except the window, which the `window` field already owns.
      const strict = Object.fromEntries(
        Object.entries(inst.strict).filter(([k]) => k !== 'window_size_sec'),
      );
      for (const [metric, arms] of Object.entries(inst.metrics)) {
        if (!arms?.length) continue;
        rows.push({
          ...newRuleConditionRow(sideName),
          enabled,
          group: groupName,
          metric,
          window: windowText,
          arms,
          strict,
        });
      }
    }
  }
  return rows;
}

/** Load a parsed `RuleParams` into a single row list — live sides first, then the
 *  parked bag as rows with `enabled: false`. Inverse of {@link rowsToSides}. */
export function sidesToRows(
  entry: SideConditions | undefined,
  exit: SideConditions | undefined,
  disabled?: DisabledConditions | null,
): RuleConditionRow[] {
  return [
    ...sideToRows(entry, 'entry'),
    ...sideToRows(exit, 'exit'),
    ...sideToRows(disabled?.entry, 'entry', false),
    ...sideToRows(disabled?.exit, 'exit', false),
  ];
}
