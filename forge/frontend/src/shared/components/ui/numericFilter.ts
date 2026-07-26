// Per-column numeric-filter parsing for the shared DataTable.
//
// Reimplemented locally in the forge tree (do NOT import hunter's copy — forge
// and hunter frontends are separate trees and the ESLint boundary forbids it).
// A column that declares `filterNumber` gets these comparison/range operators;
// anything the parser doesn't recognize falls back to substring matching on the
// column's displayed text.
//
// Supported syntax (whitespace-tolerant):
//   >5  >=5  <10  <=10  =5  ==5  !=5    comparison
//   1..10                               inclusive range (low..high, order-agnostic)
const CMP_RE = /^(>=|<=|==|!=|>|<|=)\s*(-?\d+(?:\.\d+)?)$/;
const RANGE_RE = /^(-?\d+(?:\.\d+)?)\s*\.\.\s*(-?\d+(?:\.\d+)?)$/;

/**
 * Parse a per-column numeric-filter string into a client-side predicate over the
 * column's `filterNumber` value, or `null` when the text isn't a recognized
 * numeric expression (so the caller falls back to substring matching).
 */
export function parseNumericPredicate(text: string): ((n: number) => boolean) | null {
  const range = RANGE_RE.exec(text);
  if (range) {
    let lo = parseFloat(range[1]);
    let hi = parseFloat(range[2]);
    if (lo > hi) [lo, hi] = [hi, lo];
    return (n) => n >= lo && n <= hi;
  }
  const cmp = CMP_RE.exec(text);
  if (!cmp) return null;
  const v = parseFloat(cmp[2]);
  switch (cmp[1]) {
    case '>':
      return (n) => n > v;
    case '>=':
      return (n) => n >= v;
    case '<':
      return (n) => n < v;
    case '<=':
      return (n) => n <= v;
    case '!=':
      return (n) => n !== v;
    default: // '=', '=='
      return (n) => n === v;
  }
}
