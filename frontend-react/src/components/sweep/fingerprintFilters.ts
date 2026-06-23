// Shared parsing helpers for fingerprint value filters — used by both the TPSL2
// sweep config form (`SweepConfigForm.tsx`) and the dashboard "Creation by token
// group" section (`GroupedCreationSection.tsx`) so the two surfaces accept the
// exact same input syntax (comma-separated numbers; an ix_labels JSON array).

/** Parse comma-separated number text into a deduped number array. Non-numbers
 *  and blanks are dropped. */
export function parseNumbers(text: string): number[] {
  const seen = new Set<number>();
  const out: number[] = [];
  for (const s of text.split(',')) {
    const n = Number(s.trim());
    if (s.trim() && !isNaN(n) && !seen.has(n)) {
      seen.add(n);
      out.push(n);
    }
  }
  return out;
}

/** Parse the ix_labels filter textarea (a JSON array of strings) into a label
 *  set, or an error message. Empty text ⇒ no filter (`null` labels, no error).
 *  Whitespace-only labels are dropped; an array that parses to no usable labels
 *  is treated as no filter so an empty `[]` doesn't accidentally pin "no labels". */
export function parseIxLabelsFilter(text: string): {
  labels: string[] | null;
  error: string | null;
} {
  const t = text.trim();
  if (t === '') return { labels: null, error: null };
  let parsed: unknown;
  try {
    parsed = JSON.parse(t);
  } catch {
    return { labels: null, error: 'Invalid JSON' };
  }
  if (!Array.isArray(parsed) || !parsed.every((x) => typeof x === 'string')) {
    return { labels: null, error: 'Expected a JSON array of strings, e.g. ["Pump.Fun: Buy"]' };
  }
  const labels = (parsed as string[]).map((s) => s.trim()).filter((s) => s !== '');
  return { labels: labels.length > 0 ? labels : null, error: null };
}
