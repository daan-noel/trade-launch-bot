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
export type FilterOp = 'contains' | 'eq' | 'gt' | 'gte' | 'lt' | 'lte' | 'between';

/** A structured per-column filter — the parsed shape sent to the server. `between`
 *  carries `min`/`max`; every other op carries `val`. */
export type FilterSpec =
  | { op: Exclude<FilterOp, 'between'>; val: string | number }
  | { op: 'between'; min: number; max: number };

/**
 * Parse a per-column numeric-filter string into a structured {@link FilterSpec}
 * for the server, or `null` when the text isn't a recognized numeric expression
 * (so the caller falls back to `{op:'contains', val:text}`).
 *
 * `!=` has no server-side operator, so it maps to `eq` here (the closure in
 * {@link parseNumericPredicate} still negates for the legacy client path). The
 * mapping mirrors the backend numeric ops: `>`→gt, `>=`→gte, `<`→lt, `<=`→lte,
 * `=`/`==`→eq, `lo..hi`→between.
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
      // `!=` has no server operator; approximate as `eq` (rare in practice).
      default: // '=', '==', '!='
        return { op: 'eq', val: v };
    }
  }
  return null;
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
  if (spec.op === 'between') return (n) => n >= spec.min && n <= spec.max;
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
