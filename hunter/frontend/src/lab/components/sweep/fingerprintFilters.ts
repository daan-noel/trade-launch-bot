// Shared parsing helpers for fingerprint value filters — used by both the
// sweep config form and the dashboard "Creation by token group" section so the
// two surfaces accept the exact same input syntax.

import { parseIxLabelsText } from 'lib/ixLabels';

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

/** Thin wrap of the shared `parseIxLabelsText` SSOT (pretty JSON primary;
 *  newline/comma paste-compatible) so lab call sites keep a stable import path. */
export function parseIxLabelsFilter(text: string): {
  labels: string[] | null;
  error: string | null;
} {
  return parseIxLabelsText(text);
}
