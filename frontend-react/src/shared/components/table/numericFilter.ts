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

export function parseNumericPredicate(text: string): ((n: number) => boolean) | null {
  const range = RANGE_RE.exec(text);
  if (range) {
    let lo = parseFloat(range[1]);
    let hi = parseFloat(range[2]);
    if (lo > hi) [lo, hi] = [hi, lo];
    return (n) => n >= lo && n <= hi;
  }

  const cmp = CMP_RE.exec(text);
  if (cmp) {
    const op = cmp[1];
    const v = parseFloat(cmp[2]);
    switch (op) {
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
      default: // '=' or '=='
        return (n) => n === v;
    }
  }

  return null;
}
