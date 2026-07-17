// The strategy condition grammar — the domain-typed wrapper over the shared
// compound comma-AND parser in `components/table/numericFilter`. A metric input
// like `">10, <=30"` parses to a list of `{operator, value}` conditions (the
// backend `hunter_engine::metrics::evaluator::Condition` JSON shape); `"1..10"`
// expands to `>=1 AND <=10`. Strict: a malformed fragment yields `null` so the
// UI can red-underline it (no substring fallback in rule context).

import {
  parseConditionList,
  formatConditionList,
  type Comparison,
} from 'components/table/numericFilter';
import type { Operator } from './registry';

/** One `{operator, value}` condition — the wire shape stored in `params` JSONB. */
export interface Condition {
  operator: Operator;
  value: number;
}

/** Parse a metric-condition string into `{operator, value}` conditions, or `null`
 *  if any fragment is malformed. Empty string ⇒ `[]` (unconstrained metric). */
export function parseConditions(text: string): Condition[] | null {
  const list = parseConditionList(text);
  return list && list.map(fromComparison);
}

/** Canonical text for a list of conditions (inverse of {@link parseConditions}). */
export function formatConditions(conds: Condition[]): string {
  return formatConditionList(conds.map(toComparison));
}

const toComparison = (c: Condition): Comparison => ({ op: c.operator, value: c.value });
const fromComparison = (c: Comparison): Condition => ({ operator: c.op, value: c.value });
