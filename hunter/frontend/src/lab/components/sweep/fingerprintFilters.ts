// Shared parsing helpers for fingerprint value filters — used by both the
// sweep config form and the dashboard "Creation by token group" section so the
// two surfaces accept the exact same input syntax.

import { parseIxLabelsText } from 'lib/ixLabels';

/** Parse comma-separated number text into a deduped number array. Non-numbers
 *  and blanks are dropped. Used by the discrete fields (CU limit / CU price),
 *  whose typed value IS the group-key value — see {@link parseSolFilter} for the
 *  bucketed SOL fields, which accept a range as well. */
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

/** A finite, non-negative human-SOL literal, or `null`. */
function solLiteral(s: string): number | null {
  const t = s.trim();
  if (!t) return null;
  const n = Number(t);
  return Number.isFinite(n) && n >= 0 ? n : null;
}

/**
 * Parse one comma-separated entry of a **bucketed SOL field's** value filter.
 * Mirrors Rust `hunter_engine::grouping::SolFilter::parse` — keep the two in
 * lockstep; that parser is what both backends actually apply, this one only
 * decides what the box accepts and what the summary shows.
 *
 * Two forms, both human SOL:
 * - `"1.515"` → an exact amount (whatever the bucket width).
 * - `"1.5–1.6"` / `"1.5-1.6"` → the half-open `[lo, hi)` window — i.e. the text a
 *   group chip displays, so a chip can be copied straight into the box.
 *
 * Returns the entry in its **canonical string form** (what goes on the wire —
 * a range cannot be sent as a number), or `null` if it's neither.
 */
export function parseSolFilter(entry: string): string | null {
  const t = entry.trim();
  if (!t) return null;
  for (const sep of ['–', '-']) {
    const at = t.indexOf(sep);
    if (at <= 0) continue; // no separator, or a leading sign — not a range
    const lo = solLiteral(t.slice(0, at));
    const hi = solLiteral(t.slice(at + sep.length));
    // A second separator means the rest isn't a bare number ⇒ solLiteral rejects.
    if (lo == null || hi == null || hi <= lo) return null;
    return `${lo}–${hi}`;
  }
  const exact = solLiteral(t);
  return exact == null ? null : String(exact);
}

/**
 * Parse a whole bucketed-SOL filter box. Returns the canonical wire values plus a
 * message naming the first bad entry — surfaced inline and used to block Analyze,
 * so a typo can't read as "no filter" (silently returning every token) or as an
 * unsatisfiable filter (silently returning none). Blank ⇒ `{values: [], error: null}`.
 */
export function parseSolFilterList(text: string): { values: string[]; error: string | null } {
  const values: string[] = [];
  for (const raw of text.split(',')) {
    if (!raw.trim()) continue;
    const parsed = parseSolFilter(raw);
    if (parsed == null) {
      return {
        values: [],
        error: `"${raw.trim()}" is neither a SOL amount (1.515) nor a bucket range (1.5–1.6)`,
      };
    }
    if (!values.includes(parsed)) values.push(parsed);
  }
  return { values, error: null };
}

/** One entry of a built `field_filters` map. Bucketed SOL fields emit **strings**
 *  (a range can't be a number); discrete fields emit numbers; cashback a boolean.
 *  Every backend reader accepts all three (`parse_field_filters`,
 *  `matches_field_filter`). */
export type FieldFilterValue = string | number | boolean;

/**
 * Build the `field_filters` request map from the picker's raw text — **the one
 * builder** for every surface that sends it (grouped sweep, flow discovery,
 * metric discovery, the creation-stats dashboard).
 *
 * It exists because those surfaces each rolled their own loop over
 * `parseNumbers`, which silently dropped anything non-numeric — so when the
 * bucketed SOL fields gained range syntax, three call sites would have had to
 * learn it independently. That is the same one-field-many-readers drift that made
 * this control unsatisfiable in the first place; keep new callers on this builder.
 *
 * Returns `error` naming the first bad entry (callers block their run button and
 * show it) — never a silently narrowed or silently dropped filter.
 */
export function buildFieldFilters(
  fieldFiltersText: Record<string, string>,
  opts: {
    /** Field tags to read, in any order. `ix_labels`/`is_cashback_enabled` are skipped. */
    fields: readonly string[];
    /** Which of `fields` take the SOL amount/range syntax. */
    bucketed: ReadonlySet<string>;
    /** Tri-state cashback select; `'all'` ⇒ omitted. */
    cashback?: 'all' | 'true' | 'false';
    /** Human labels for the error message. */
    labels?: Record<string, string>;
  },
): { filters: Record<string, FieldFilterValue[]>; error: string | null } {
  const filters: Record<string, FieldFilterValue[]> = {};
  let error: string | null = null;
  for (const f of opts.fields) {
    if (f === 'ix_labels' || f === 'is_cashback_enabled') continue;
    const text = fieldFiltersText[f] ?? '';
    if (!text.trim()) continue;
    if (opts.bucketed.has(f)) {
      const parsed = parseSolFilterList(text);
      if (parsed.error) {
        error ??= `${opts.labels?.[f] ?? f}: ${parsed.error}`;
        continue;
      }
      if (parsed.values.length > 0) filters[f] = parsed.values;
    } else {
      const nums = parseNumbers(text);
      if (nums.length > 0) filters[f] = nums;
    }
  }
  if (opts.cashback && opts.cashback !== 'all') {
    filters.is_cashback_enabled = [opts.cashback === 'true'];
  }
  return { filters, error };
}

/** Thin wrap of the shared `parseIxLabelsText` SSOT (pretty JSON primary;
 *  newline/comma paste-compatible) so lab call sites keep a stable import path. */
export function parseIxLabelsFilter(text: string): {
  labels: string[] | null;
  error: string | null;
} {
  return parseIxLabelsText(text);
}
