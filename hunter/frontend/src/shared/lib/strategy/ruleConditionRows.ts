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
import type { SideConditions } from './ruleParams';
import type { StrategyRegistry } from './registry';

/** Which side a condition row applies to (the column owns it). */
export type RuleConditionSide = 'entry' | 'exit';

/** One editor row of the rule condition builder. */
export interface RuleConditionRow {
  /** Stable client id (list-render key + remove target). */
  id: string;
  side: RuleConditionSide;
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
    group: '',
    metric: '',
    window: '',
    arms: [],
  };
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
    const key = `${row.side}|${rowWindow(row) ?? '∅'}`;
    const inst = instances.get(key) ?? { hasArm: false, hasTrailing: false };
    if (row.strict?.arm_above_pct != null) inst.hasArm = true;
    if (ruleRowIsTrailing(row)) inst.hasTrailing = true;
    instances.set(key, inst);
  }
  for (const [key, inst] of instances) {
    if (inst.hasArm && !inst.hasTrailing) {
      const [side] = key.split('|');
      return `${side} m_position: arm_above_pct gates the trailing metrics (retrace / bounce) — add one or remove arm_above_pct`;
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
    const key = `${row.side}|${row.group}|${w ?? '∅'}|${row.metric}`;
    if (seen.has(key)) {
      const at = w != null ? `@${w}s` : '';
      return `${row.side} ${row.group}.${row.metric}${at} is set twice — merge the conditions into one row`;
    }
    seen.add(key);
  }
  return null;
}

/**
 * Fold the rows of one side into `SideConditions` — group by (group, window) into
 * one `GroupConditions` instance each (the engine's multi-window-per-group model).
 * Half-authored rows (no group/metric) and empty-condition rows are skipped, so the
 * output only ever carries constraints the backend will accept.
 */
export function rowsToSide(rows: RuleConditionRow[], side: RuleConditionSide): SideConditions {
  const out: SideConditions = {};
  for (const row of rows) {
    if (row.side !== side) continue;
    if (!row.group || !row.metric || row.arms.length === 0) continue;
    const w = rowWindow(row);
    const instances = out[row.group] ?? (out[row.group] = []);
    let inst = instances.find((g) => sameWindow(g.strict.window_size_sec, w));
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

/** Both sides folded — the `entry`/`exit` of a `RuleParams`. */
export function rowsToSides(rows: RuleConditionRow[]): {
  entry: SideConditions;
  exit: SideConditions;
} {
  return { entry: rowsToSide(rows, 'entry'), exit: rowsToSide(rows, 'exit') };
}

/** Expand a nested side back into editor rows — one row per (group, window, metric).
 *  The inverse of {@link rowsToSide} for loading an existing rule / JSON edit. */
export function sideToRows(
  side: SideConditions | undefined,
  sideName: RuleConditionSide,
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

/** Load both sides of a parsed `RuleParams` into a single row list. */
export function sidesToRows(
  entry: SideConditions | undefined,
  exit: SideConditions | undefined,
): RuleConditionRow[] {
  return [...sideToRows(entry, 'entry'), ...sideToRows(exit, 'exit')];
}
