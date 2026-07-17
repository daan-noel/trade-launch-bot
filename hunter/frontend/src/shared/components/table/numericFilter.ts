// Parses a per-column filter string into a numeric predicate, used when a
// column declares `filterNumber`. Returns null when the text is not a
// recognized numeric expression, so the caller can fall back to substring
// matching on the displayed text.
//
// Supported syntax (whitespace-tolerant):
//   >5  >=5  <10  <=10  =5  ==5  !=5    comparison
//   1..10                               inclusive range (low..high)
const CMP_RE = /^(>=|<=|==|!=|>|<|=)\s*(-?\d+(?:\.\d+)?)$/;
const RANGE_RE = /^(-?\d+(?:\.\d+)?)\s*\.\.\s*(-?\d+(?:\.\d+)?)$/;

/** The backend `FilterOp` set (see `trading_core::api::table_query::FilterOp`). */
export type FilterOp = 'contains' | 'eq' | 'neq' | 'in' | 'gt' | 'gte' | 'lt' | 'lte' | 'between';

/** A structured per-column filter — the parsed shape sent to the server. `between`
 *  carries `min`/`max`; `in` carries an array in `val` (set membership, e.g. a pasted
 *  mint set — the operand field is `val` to match the backend `FilterSpec.val`); every
 *  other op carries a single `val`. Operands are `string | number` so date-range
 *  filters (RFC3339 strings) share the same shape as numeric ones; the backend reads
 *  each operand by the column's type. */
export type FilterSpec =
  | { op: Exclude<FilterOp, 'between' | 'in'>; val: string | number }
  | { op: 'between'; min: string | number; max: string | number }
  | { op: 'in'; val: (string | number)[] };

/**
 * Parse a per-column numeric-filter string into a structured {@link FilterSpec}
 * for the server, or `null` when the text isn't a recognized numeric expression
 * (so the caller falls back to `{op:'contains', val:text}`).
 *
 * The mapping mirrors the backend numeric ops: `>`→gt, `>=`→gte, `<`→lt, `<=`→lte,
 * `=`/`==`→eq, `!=`→neq, `lo..hi`→between.
 */
export function parseFilterSpec(text: string): FilterSpec | null {
  const range = RANGE_RE.exec(text);
  if (range) {
    let lo = parseFloat(range[1]);
    let hi = parseFloat(range[2]);
    if (lo > hi) [lo, hi] = [hi, lo];
    return { op: 'between', min: lo, max: hi };
  }
  const cmp = CMP_RE.exec(text);
  if (cmp) {
    const v = parseFloat(cmp[2]);
    switch (cmp[1]) {
      case '>':
        return { op: 'gt', val: v };
      case '>=':
        return { op: 'gte', val: v };
      case '<':
        return { op: 'lt', val: v };
      case '<=':
        return { op: 'lte', val: v };
      case '!=':
        return { op: 'neq', val: v };
      default: // '=', '=='
        return { op: 'eq', val: v };
    }
  }
  return null;
}

// ── Compound condition grammar (strategy conditions + client filtering) ───────
//
// The rule-condition grammar (plan §2): `,` = AND, `|` = OR (DNF: OR of AND
// arms). `">10, <=30"` is one AND arm; `"<30 | >=70"` is two OR arms; `"1..10"`
// expands to `>=1 AND <=10` inside one arm. Shared SSOT the strategy
// `lib/strategy/grammar` wraps. Unlike {@link parseFilterSpec}, this is **strict**:
// a malformed fragment fails the whole parse (`null`) — no `contains` fallback.

/** The comparison operators of the condition grammar (JSON-wire form, matching
 *  the backend `hunter_engine::metrics::evaluator::Operator` renames). */
export type CompareOp = '>' | '>=' | '<' | '<=' | '=' | '!=';

/** One atomic comparison of the compound grammar (`op value`). */
export interface Comparison {
  op: CompareOp;
  value: number;
}

/** DNF: OR of AND-arms. `[[a,b],[c]]` ⇔ `(a ∧ b) ∨ c`. Empty `[]` = unconstrained. */
export type ComparisonExpr = Comparison[][];

// Same numeric shapes as CMP_RE/RANGE_RE but keeping the operator token verbatim
// (`==` normalized to `=` below) so the parsed list round-trips to the wire form.
const COMPARE_RE = /^(>=|<=|==|!=|>|<|=)\s*(-?\d+(?:\.\d+)?)$/;

/** Parse one AND arm (`", "`-joined fragments). Empty/trailing comma → `null`. */
function parseAndArm(text: string): Comparison[] | null {
  const trimmed = text.trim();
  if (trimmed === '') return null; // empty OR arm is malformed
  const out: Comparison[] = [];
  for (const rawFrag of trimmed.split(',')) {
    const frag = rawFrag.trim();
    if (frag === '') return null;
    const range = RANGE_RE.exec(frag);
    if (range) {
      let lo = parseFloat(range[1]);
      let hi = parseFloat(range[2]);
      if (lo > hi) [lo, hi] = [hi, lo];
      out.push({ op: '>=', value: lo }, { op: '<=', value: hi });
      continue;
    }
    const cmp = COMPARE_RE.exec(frag);
    if (!cmp) return null;
    const op = (cmp[1] === '==' ? '=' : cmp[1]) as CompareOp;
    out.push({ op, value: parseFloat(cmp[2]) });
  }
  return out;
}

/**
 * Parse a compound condition string into DNF arms.
 * `">10, <=30"` → `[[{'>',10},{'<=',30}]]`; `"<30 | >=70"` → `[[{'<',30}],[{'>=',70}]]`;
 * `"1..10"` → `[[{'>=',1},{'<=',10}]]`; `""` → `[]` (unconstrained).
 * Returns `null` if any arm/fragment is malformed (strict).
 */
export function parseConditionList(text: string): ComparisonExpr | null {
  const trimmed = text.trim();
  if (trimmed === '') return [];
  const arms: ComparisonExpr = [];
  for (const rawArm of trimmed.split('|')) {
    const arm = parseAndArm(rawArm);
    if (!arm) return null;
    arms.push(arm);
  }
  return arms;
}

/** Inverse of {@link parseConditionList}: canonical `"op value, op value | …"` text. */
export function formatConditionList(arms: ComparisonExpr): string {
  return arms.map((arm) => arm.map((c) => `${c.op} ${c.value}`).join(', ')).join(' | ');
}

/** Client-side predicate for a DNF condition expr (any arm all-true). Empty ⇒ true. */
export function conditionListPredicate(arms: ComparisonExpr): (n: number) => boolean {
  return (n) =>
    arms.length === 0 ||
    arms.some((arm) =>
      arm.every((c) => {
        switch (c.op) {
          case '>':
            return n > c.value;
          case '>=':
            return n >= c.value;
          case '<':
            return n < c.value;
          case '<=':
            return n <= c.value;
          case '=':
            return n === c.value;
          case '!=':
            return n !== c.value;
        }
      }),
    );
}

/**
 * Legacy client-side numeric predicate (used by fully client-side tables that
 * still filter in-browser). Wraps {@link parseFilterSpec}; returns `null` when the
 * text isn't numeric so the caller can fall back to substring matching.
 */
export function parseNumericPredicate(text: string): ((n: number) => boolean) | null {
  // `!=` needs a genuine negation the FilterSpec can't express, so handle it here.
  const cmp = CMP_RE.exec(text);
  if (cmp && cmp[1] === '!=') {
    const v = parseFloat(cmp[2]);
    return (n) => n !== v;
  }
  const spec = parseFilterSpec(text);
  if (!spec) return null;
  if (spec.op === 'between') {
    const lo = Number(spec.min);
    const hi = Number(spec.max);
    return (n) => n >= lo && n <= hi;
  }
  const v = Number(spec.val);
  switch (spec.op) {
    case 'gt':
      return (n) => n > v;
    case 'gte':
      return (n) => n >= v;
    case 'lt':
      return (n) => n < v;
    case 'lte':
      return (n) => n <= v;
    default: // 'eq'
      return (n) => n === v;
  }
}
