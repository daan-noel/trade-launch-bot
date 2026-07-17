// Client-side mirror of the backend rule-params validation
// (`hunter_engine::rule_params` §5). It uses the SAME error vocabulary so a
// draft that passes here passes the save endpoint too — the editor can surface
// problems inline instead of round-tripping to the server. The backend remains
// the authority; this is a fast-feedback pre-check.

import type { Condition } from './grammar';
import type { RuleParams, SideConditions, GroupConditions } from './ruleParams';
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
  return errors;
}

function validateSide(
  side: 'entry' | 'exit',
  conds: SideConditions | undefined,
  reg: StrategyRegistry | undefined,
  errors: string[],
): void {
  if (!conds) return;
  for (const [groupName, group] of Object.entries(conds)) {
    // Skip groups the user left entirely blank — an empty group is treated as
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
    for (const [metric, list] of Object.entries(group.metrics)) {
      if (list.length === 0) continue;
      const m = findMetric(reg, groupName, metric);
      if (!m) {
        errors.push(`${side}.${groupName}: unknown metric '${metric}'`);
        continue;
      }
      if (list.some((c) => !Number.isFinite(c.value))) {
        errors.push(`${side}.${groupName}.${metric}: condition value must be finite`);
        continue;
      }
      const why = unsatisfiableReason(list, m.eq_tolerance);
      if (why) errors.push(`${side}.${groupName}.${metric}: contradictory conditions (${why})`);
    }
  }
}

function groupHasConstraint(group: GroupConditions): boolean {
  return Object.values(group.metrics).some((list) => list.length > 0);
}

/**
 * Port of the backend `check_satisfiable`: conditions AND together, so the
 * feasible set is an interval intersection; `=`/`!=` contribute `±tol/2` bands.
 * Returns a reason string when no value can satisfy the set, else `null`.
 */
export function unsatisfiableReason(conds: Condition[], tol: number): string | null {
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
