/**
 * Analysis-owned `ix_labels` pattern sets — the wire types of `ix_pattern_sets`
 * plus the paste parser that fills one.
 *
 * A fingerprint's `ix_patterns` is what the ENGINE classifies flow with;
 * a pattern set is the same fact owned by a study surface, for tokens that
 * belong to no cohort (Trader Analysis). Both feed the ONE classifier
 * (`lib/flow/classifyFlow`), so a pattern means exactly the same thing in each:
 * an EXACT ordered `ix_labels` sequence, `JSON.stringify`d as its key.
 *
 * `group` labels a subset (a launch client / aggregator name) so the lens can be
 * narrowed to one of them without re-pasting. It is never matched against.
 */

import { patternKeysFrom } from 'lib/flow/classifyFlow';

export interface IxPattern {
  /** Subset label; `null` ⇒ ungrouped. Display + narrowing only. */
  group: string | null;
  /** EXACT ordered instruction labels, verbatim from `trades.ix_labels`. */
  ix_labels: string[];
}

export interface IxPatternSet {
  id: string;
  name: string;
  wallet_address: string | null;
  patterns: IxPattern[];
  notes: string | null;
  created_at: string;
  updated_at: string;
}

/** Body of `POST /api/ix-pattern-sets` and its `PUT` twin (full replace). */
export interface IxPatternSetDraft {
  name: string;
  wallet_address?: string | null;
  patterns: IxPattern[];
  notes?: string | null;
}

/** Bucket name shown for `group: null`. Display only — never persisted. */
export const UNGROUPED = 'Ungrouped';

/** Identity of one ordered sequence — the same key `classifyFlow` matches on. */
export const patternKeyOf = (labels: readonly string[]): string => JSON.stringify(labels);

/** Group names in first-seen order, with {@link UNGROUPED} standing in for null. */
export function patternGroups(patterns: readonly IxPattern[]): string[] {
  const out: string[] = [];
  const seen = new Set<string>();
  for (const p of patterns) {
    const g = p.group ?? UNGROUPED;
    if (!seen.has(g)) {
      seen.add(g);
      out.push(g);
    }
  }
  return out;
}

/**
 * Classification keys for the enabled groups only — how the lens narrows to one
 * launch client without touching the stored set. `null` ⇒ every group.
 */
export function patternKeysForGroups(
  patterns: readonly IxPattern[],
  enabled: ReadonlySet<string> | null,
): ReadonlySet<string> | null {
  const picked = enabled
    ? patterns.filter((p) => enabled.has(p.group ?? UNGROUPED))
    : patterns;
  const keys = patternKeysFrom(picked.map((p) => p.ix_labels));
  return keys.size > 0 ? keys : null;
}

/** Add/remove one ordered sequence, keeping the rest in place. A re-added
 *  pattern lands at the end under `group` (membership is what matters). */
export function toggleIxPattern(
  patterns: readonly IxPattern[],
  labels: readonly string[],
  group: string | null,
): IxPattern[] {
  if (labels.length === 0) return patterns.map((p) => ({ ...p }));
  const key = patternKeyOf(labels);
  const kept = patterns.filter((p) => patternKeyOf(p.ix_labels) !== key);
  if (kept.length !== patterns.length) return kept.map((p) => ({ ...p }));
  return [...patterns.map((p) => ({ ...p })), { group, ix_labels: [...labels] }];
}

export interface PatternParseResult {
  patterns: IxPattern[];
  /** Sequences kept (post-dedupe). */
  accepted: number;
  /** Sequences dropped as an exact duplicate of an earlier one. */
  duplicates: number;
  /** Entries that carried no usable `ix_labels` array. */
  skipped: number;
  /** Fatal — nothing was parsed. `patterns` is then empty. */
  error: string | null;
}

/** One entry of a parsed payload, before dedupe. */
interface RawEntry {
  group: string | null;
  labels: string[];
}

const isStringArray = (v: unknown): v is string[] =>
  Array.isArray(v) && v.every((x) => typeof x === 'string');

/** Group name off an object entry — `group` is ours, `tool` is what a derived
 *  study file usually calls it, and `label`/`name` are the other two spellings
 *  seen in hand-written JSON. */
function groupOf(o: Record<string, unknown>): string | null {
  for (const k of ['group', 'tool', 'label', 'name']) {
    const v = o[k];
    if (typeof v === 'string' && v.trim()) return v.trim();
  }
  return null;
}

/** Pull entries out of any of the accepted JSON shapes; `null` ⇒ not one. */
function entriesFromJson(value: unknown): RawEntry[] | null {
  // `{ "patterns": [...] }` — a derived study file, notes and counts included.
  if (value && typeof value === 'object' && !Array.isArray(value)) {
    const inner = (value as Record<string, unknown>).patterns;
    return inner === undefined ? null : entriesFromJson(inner);
  }
  if (!Array.isArray(value)) return null;
  // A single sequence pasted bare.
  if (isStringArray(value)) return [{ group: null, labels: value }];
  const out: RawEntry[] = [];
  for (const item of value) {
    if (isStringArray(item)) {
      out.push({ group: null, labels: item });
      continue;
    }
    if (item && typeof item === 'object') {
      const o = item as Record<string, unknown>;
      const labels = o.ix_labels ?? o.instruction_labels ?? o.labels;
      if (isStringArray(labels)) {
        out.push({ group: groupOf(o), labels });
        continue;
      }
    }
    // Anything else is counted as skipped by the caller (length difference).
    out.push({ group: null, labels: [] });
  }
  return out;
}

/**
 * Parse a pasted pattern payload.
 *
 * Accepts, in order: a `{ "patterns": [...] }` wrapper, an array of
 * `{ ix_labels, group|tool }` objects, an array of label arrays, one bare label
 * array, or one JSON array per line. Deliberately does NOT accept the
 * `"A > B > C"` display form: those are shortened ACTION names, and a pattern
 * built from them would match no trade while looking correct.
 */
export function parsePastedPatterns(text: string): PatternParseResult {
  const trimmed = text.trim();
  const empty: PatternParseResult = {
    patterns: [],
    accepted: 0,
    duplicates: 0,
    skipped: 0,
    error: null,
  };
  if (!trimmed) return empty;

  let entries: RawEntry[] | null = null;
  try {
    entries = entriesFromJson(JSON.parse(trimmed) as unknown);
  } catch {
    // Not one JSON document — try one array per line.
    const lines = trimmed.split(/\r?\n/).map((l) => l.trim().replace(/,$/, ''));
    const perLine: RawEntry[] = [];
    for (const line of lines) {
      if (!line) continue;
      try {
        const parsed: unknown = JSON.parse(line);
        const one = entriesFromJson(parsed);
        if (one) perLine.push(...one);
        else perLine.push({ group: null, labels: [] });
      } catch {
        return {
          ...empty,
          error:
            'Not JSON. Paste [["Label A","Label B"], …], a [{ "tool": …, "ix_labels": […] }] list, or one JSON array per line — the "A > B" display form is lossy and cannot be matched.',
        };
      }
    }
    entries = perLine;
  }

  if (!entries) {
    return {
      ...empty,
      error: 'JSON parsed, but carried no ix_labels — expected arrays of label strings.',
    };
  }

  const seen = new Set<string>();
  const patterns: IxPattern[] = [];
  let duplicates = 0;
  let skipped = 0;
  for (const e of entries) {
    const labels = e.labels.map((l) => l.trim()).filter(Boolean);
    if (labels.length === 0) {
      skipped += 1;
      continue;
    }
    const key = patternKeyOf(labels);
    if (seen.has(key)) {
      duplicates += 1;
      continue;
    }
    seen.add(key);
    patterns.push({ group: e.group, ix_labels: labels });
  }

  if (patterns.length === 0 && skipped === 0 && duplicates === 0) {
    return { ...empty, error: 'No patterns found in that paste.' };
  }
  return { patterns, accepted: patterns.length, duplicates, skipped, error: null };
}

/** The set as re-pastable JSON, groups preserved. */
export function formatPatternsJson(patterns: readonly IxPattern[]): string {
  return JSON.stringify(
    patterns.map((p) => (p.group ? { tool: p.group, ix_labels: p.ix_labels } : p.ix_labels)),
    null,
    2,
  );
}

/** Bare label arrays — the shape a fingerprint's `ix_patterns` stores,
 *  for promoting a lens into one. Group labels have no home there and drop. */
export function toIxPatterns(patterns: readonly IxPattern[]): string[][] {
  return patterns.map((p) => [...p.ix_labels]);
}
