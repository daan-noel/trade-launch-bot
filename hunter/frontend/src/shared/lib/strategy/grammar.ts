// The strategy condition grammar — the domain-typed wrapper over the shared
// compound `,` AND / `|` OR parser in `components/table/numericFilter`. A metric
// input like `">10, <=30"` parses to one AND arm; `"<30 | >=70"` to two OR arms
// (DNF). The in-memory / form shape is always `Condition[][]`; the JSON wire is
// flat for a single arm (legacy) and nested for multi-arm. Strict: a malformed
// fragment yields `null` so the UI can red-underline it.

import {
  parseConditionList,
  formatConditionList,
  type Comparison,
  type ComparisonExpr,
} from 'components/table/numericFilter';
import type { Operator } from './registry';

/** One `{operator, value}` condition — the atomic wire atom. */
export interface Condition {
  operator: Operator;
  value: number;
}

/** DNF: OR of AND-arms. Always the form/in-memory shape. */
export type ConditionExpr = Condition[][];

/** Parse a metric-condition string into DNF arms, or `null` if malformed.
 *  Empty string ⇒ `[]` (unconstrained metric). */
export function parseConditions(text: string): ConditionExpr | null {
  const list = parseConditionList(text);
  return list && list.map((arm) => arm.map(fromComparison));
}

/** Canonical text for DNF arms (inverse of {@link parseConditions}). */
export function formatConditions(arms: ConditionExpr): string {
  return formatConditionList(arms.map((arm) => arm.map(toComparison)));
}

/** Normalize wire JSON (flat legacy list OR nested arms) → DNF. */
export function conditionExprFromJson(raw: unknown): ConditionExpr | null {
  if (!Array.isArray(raw)) return null;
  if (raw.length === 0) return [];
  // Nested: [[{op,value},…], …]
  if (Array.isArray(raw[0])) {
    const arms: ConditionExpr = [];
    for (const arm of raw) {
      if (!Array.isArray(arm) || arm.length === 0) return null;
      const parsed: Condition[] = [];
      for (const c of arm) {
        const cond = asCondition(c);
        if (!cond) return null;
        parsed.push(cond);
      }
      arms.push(parsed);
    }
    return arms;
  }
  // Flat legacy: [{op,value},…] → one AND arm
  const arm: Condition[] = [];
  for (const c of raw) {
    const cond = asCondition(c);
    if (!cond) return null;
    arm.push(cond);
  }
  return arm.length ? [arm] : [];
}

/** Serialize DNF → wire JSON (flat when one arm, nested when multi). */
export function conditionExprToJson(arms: ConditionExpr): Condition[] | Condition[][] {
  if (arms.length === 1) return arms[0];
  return arms;
}

/**
 * Same-metric multi-op normalize (mirrors `hunter_engine::normalize_condition_expr`):
 * a single AND arm that is unsatisfiable becomes OR of its atoms; feasible AND
 * and explicit multi-arm `|` exprs are unchanged.
 */
export function normalizeConditionExpr(arms: ConditionExpr, tol: number): ConditionExpr {
  if (arms.length !== 1) return arms;
  const arm = arms[0];
  if (!armUnsatisfiable(arm, tol)) return arms;
  return arm.map((c) => [c]);
}

function armUnsatisfiable(conds: Condition[], tol: number): boolean {
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
  if (lo > hi || (lo === hi && (loStrict || hiStrict))) return true;
  for (const [bLo, bHi] of neBands) {
    if (Number.isFinite(lo) && Number.isFinite(hi) && bLo <= lo && hi <= bHi) return true;
  }
  return false;
}

function asCondition(c: unknown): Condition | null {
  if (!c || typeof c !== 'object') return null;
  const o = c as Record<string, unknown>;
  if (typeof o.operator !== 'string' || typeof o.value !== 'number' || !Number.isFinite(o.value)) {
    return null;
  }
  return { operator: o.operator as Operator, value: o.value };
}

const toComparison = (c: Condition): Comparison => ({ op: c.operator, value: c.value });
const fromComparison = (c: Comparison): Condition => ({ operator: c.op, value: c.value });

// Re-export for callers that need the ComparisonExpr alias.
export type { ComparisonExpr };
