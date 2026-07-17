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

// ── Compound comma-AND grammar (strategy conditions + client filtering) ───────
//
// The rule-condition grammar (plan §2): comma = AND, so `">10, <=30"` is two
// ANDed comparisons and `"1..10"` is `>=1 AND <=10`. This is the shared SSOT the
// strategy `lib/strategy/grammar` wraps and DataTable client-side filtering can
// adopt. Unlike {@link parseFilterSpec}, this is **strict**: a fragment that
// isn't a recognized comparison/range makes the whole parse fail (`null`) — there
// is no `contains` fallback, so a malformed rule fragment surfaces as an error
// instead of silently matching nothing.

/** The comparison operators of the condition grammar (JSON-wire form, matching
 *  the backend `hunter_engine::metrics::evaluator::Operator` renames). */
export type CompareOp = '>' | '>=' | '<' | '<=' | '=' | '!=';

/** One atomic comparison of the compound grammar (`op value`). */
export interface Comparison {
  op: CompareOp;
  value: number;
}

// Same numeric shapes as CMP_RE/RANGE_RE but keeping the operator token verbatim
// (`==` normalized to `=` below) so the parsed list round-trips to the wire form.
const COMPARE_RE = /^(>=|<=|==|!=|>|<|=)\s*(-?\d+(?:\.\d+)?)$/;

/**
 * Parse a compound comma-AND condition string into a list of {@link Comparison}s.
 * `">10, <=30"` → `[{'>',10},{'<=',30}]`; `"1..10"` → `[{'>=',1},{'<=',10}]`;
 * `""` (or whitespace) → `[]` (unconstrained). Returns `null` if ANY fragment is
 * malformed (strict — no substring fallback), so callers can flag the input.
 */
export function parseConditionList(text: string): Comparison[] | null {
  const trimmed = text.trim();
  if (trimmed === '') return [];
  const out: Comparison[] = [];
  for (const rawFrag of trimmed.split(',')) {
    const frag = rawFrag.trim();
    if (frag === '') return null; // empty/trailing-comma fragment is malformed
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

/** Inverse of {@link parseConditionList}: canonical `"op value, op value"` text
 *  (single-spaced, comma-joined). Order = list order — a save may reorder vs the
 *  raw text the user typed (documented in the input hint). */
export function formatConditionList(list: Comparison[]): string {
  return list.map((c) => `${c.op} ${c.value}`).join(', ');
}

/** A client-side predicate for a compound condition list (all AND). */
export function conditionListPredicate(list: Comparison[]): (n: number) => boolean {
  return (n) =>
    list.every((c) => {
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
    });
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
