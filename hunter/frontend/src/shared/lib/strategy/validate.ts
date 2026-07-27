// Client-side mirror of the backend rule-params validation
// (`hunter_engine::rule_params` §5). It uses the SAME error vocabulary so a
// draft that passes here passes the save endpoint too — the editor can surface
// problems inline instead of round-tripping to the server. The backend remains
// the authority; this is a fast-feedback pre-check.

import { normalizeConditionExpr, type Condition, type ConditionExpr } from './grammar';
import { sideInstances, type RuleParams, type SideConditions, type GroupConditions } from './ruleParams';
import { findGroup, findMetric, type StrategyRegistry } from './registry';

/** Validate a draft against the registry. Returns human-readable error strings
 *  (empty = valid), matching the backend's messages closely. */
export function validateRuleParams(p: RuleParams, reg: StrategyRegistry | undefined): string[] {
  const errors: string[] = [];
  for (const [name, v] of [
    ['take_profit', p.take_profit],
    ['stop_loss', p.stop_loss],
  ] as const) {
    if (v != null && (!Number.isFinite(v) || v <= 0)) {
      errors.push(`${name} must be a finite number > 0`);
    }
  }
  validateSide('entry', p.entry, reg, errors);
  validateSide('exit', p.exit, reg, errors);
  validateReentry(p.reentry, errors);
  // Mirror of the backend `parse_opt_priority`.
  if (!Number.isInteger(p.priority)) errors.push('priority must be an integer');
  errors.push(...pnlSugarDuplicateErrors(p));
  return errors;
}

/** True for the advanced `m_position.pnl` metric (TP/SL sugar is the primary surface). */
export function isPnlAdvancedMetric(group: string, metric: string): boolean {
  return group === 'm_position' && metric === 'pnl';
}

/**
 * Authored `m_position.pnl` conditions that exactly restate the TP/SL sugar
 * (`pnl >= take_profit` / `pnl <= −stop_loss`). Backend desugar produces those
 * same bounds — duplicating them is a footgun, not a second exit.
 */
export function pnlSugarDuplicateErrors(p: RuleParams): string[] {
  // `m_position` is static (single instance), but read it instance-safely.
  const arms = p.exit?.m_position?.find((g) => g.metrics.pnl?.length)?.metrics.pnl;
  if (!arms?.length) return [];
  const errors: string[] = [];
  let sawTp = false;
  let sawSl = false;
  for (const arm of arms) {
    for (const c of arm) {
      if (
        !sawTp &&
        p.take_profit != null &&
        c.operator === '>=' &&
        nearlyEqual(c.value, p.take_profit)
      ) {
        errors.push(
          'exit.m_position.pnl duplicates take_profit — use the TP % field (or clear it)',
        );
        sawTp = true;
      }
      if (
        !sawSl &&
        p.stop_loss != null &&
        c.operator === '<=' &&
        nearlyEqual(c.value, -p.stop_loss)
      ) {
        errors.push(
          'exit.m_position.pnl duplicates stop_loss — use the SL % field (or clear it)',
        );
        sawSl = true;
      }
    }
  }
  return errors;
}

function nearlyEqual(a: number, b: number): boolean {
  return Number.isFinite(a) && Number.isFinite(b) && Math.abs(a - b) < 1e-9;
}

/** Mirror of the backend `parse_opt_reentry`: `cooldown_sec` finite `>= 0`,
 *  `max_episodes_per_token` an integer `>= 1`. */
function validateReentry(re: RuleParams['reentry'], errors: string[]): void {
  if (re == null) return;
  if (!Number.isFinite(re.cooldown_sec) || re.cooldown_sec < 0) {
    errors.push('reentry.cooldown_sec must be a finite number >= 0');
  }
  if (
    !Number.isFinite(re.max_episodes_per_token) ||
    re.max_episodes_per_token < 1 ||
    !Number.isInteger(re.max_episodes_per_token)
  ) {
    errors.push('reentry.max_episodes_per_token must be an integer >= 1');
  }
}

function validateSide(
  side: 'entry' | 'exit',
  conds: SideConditions | undefined,
  reg: StrategyRegistry | undefined,
  errors: string[],
): void {
  if (!conds) return;
  // One pass per group instance (a group may hold several — one per window).
  for (const [groupName, group] of sideInstances(conds)) {
    // Skip instances the user left entirely blank — an empty group is treated as
    // absent (only groups that carry a constraint are serialized).
    if (!groupHasConstraint(group)) continue;
    const spec = findGroup(reg, groupName);
    if (!spec) {
      errors.push(`${side}: unknown metric group '${groupName}'`);
      continue;
    }
    for (const sp of spec.strict_params) {
      if (sp.required && group.strict[sp.name] == null) {
        errors.push(`${side}.${groupName}: missing required param '${sp.name}'`);
      }
    }
    for (const [name, v] of Object.entries(group.strict)) {
      if (v != null && (!Number.isFinite(v) || v <= 0)) {
        errors.push(`${side}.${groupName}.${name} must be a finite number > 0`);
      }
    }
    for (const [metric, arms] of Object.entries(group.metrics)) {
      if (arms.length === 0) continue;
      const m = findMetric(reg, groupName, metric);
      if (!m) {
        errors.push(`${side}.${groupName}: unknown metric '${metric}'`);
        continue;
      }
      // Same metric, different ops: validate the normalized form (AND→OR when needed).
      const normalized = normalizeConditionExpr(arms, m.eq_tolerance);
      if (normalized.some((arm) => arm.length === 0 || arm.some((c) => !Number.isFinite(c.value)))) {
        errors.push(`${side}.${groupName}.${metric}: condition value must be finite`);
        continue;
      }
      const why = unsatisfiableReason(normalized, m.eq_tolerance);
      if (why) errors.push(`${side}.${groupName}.${metric}: contradictory conditions (${why})`);
    }
  }
}

function groupHasConstraint(group: GroupConditions): boolean {
  return Object.values(group.metrics).some((arms) => arms.length > 0);
}

/**
 * Port of the backend `check_satisfiable` over DNF: an expr is unsatisfiable
 * only when every OR arm is. Within an arm, conditions AND (interval
 * intersection); `=`/`!=` contribute `±tol/2` bands.
 */
export function unsatisfiableReason(arms: ConditionExpr, tol: number): string | null {
  if (arms.length === 0) return null;
  const reasons: string[] = [];
  for (const arm of arms) {
    const why = armUnsatisfiableReason(arm, tol);
    if (!why) return null; // at least one arm is feasible
    reasons.push(why);
  }
  return reasons[0] ?? 'all OR arms unsatisfiable';
}

export { normalizeConditionExpr } from './grammar';

function armUnsatisfiableReason(conds: Condition[], tol: number): string | null {
  const half = tol / 2;
  let lo = -Infinity;
  let loStrict = false;
  let hi = Infinity;
  let hiStrict = false;
  const neBands: Array<[number, number]> = [];
  const raiseLo = (v: number, strict: boolean) => {
    if (v > lo || (v === lo && strict)) {
      lo = v;
      loStrict = strict;
    }
  };
  const dropHi = (v: number, strict: boolean) => {
    if (v < hi || (v === hi && strict)) {
      hi = v;
      hiStrict = strict;
    }
  };
  for (const c of conds) {
    switch (c.operator) {
      case '>':
        raiseLo(c.value, true);
        break;
      case '>=':
        raiseLo(c.value, false);
        break;
      case '<':
        dropHi(c.value, true);
        break;
      case '<=':
        dropHi(c.value, false);
        break;
      case '=':
        raiseLo(c.value - half, false);
        dropHi(c.value + half, false);
        break;
      case '!=':
        neBands.push([c.value - half, c.value + half]);
        break;
    }
  }
  if (lo > hi || (lo === hi && (loStrict || hiStrict))) {
    return `bounds cross: feasible range is empty around ${lo}`;
  }
  for (const [bLo, bHi] of neBands) {
    if (Number.isFinite(lo) && Number.isFinite(hi) && bLo <= lo && hi <= bHi) {
      return `'!=' band [${bLo}, ${bHi}] covers the feasible range`;
    }
  }
  return null;
}
