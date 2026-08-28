/**
 * Edits to a `m_flow_ix.ix_patterns` set — the ONE toggler behind
 * every surface that stages patterns (the chart trades table, Flow Discovery's
 * structure checkboxes).
 *
 * A pattern is an EXACT ordered `ix_labels` sequence: never sorted, never
 * deduped, duplicates kept, same length. `JSON.stringify` of that array is its
 * identity — the same key `classifyFlow` matches trades on, so a pattern staged
 * here and a trade classified there can never disagree about what "the same
 * structure" means.
 */

import { ixLabelsActions } from 'lib/ixLabels';

export type VolumePattern = readonly string[];

/** Identity of one ordered label sequence. */
export function patternKey(labels: VolumePattern): string {
  return JSON.stringify(labels);
}

/** The set as re-pastable JSON — `[["A","B"],["C"]]`. Empty ⇒ `""`. The twin of
 *  `formatIxLabelsText` one level up: that one serializes a single sequence,
 *  this one the set of sequences a fingerprint matches. */
export function formatVolumePatternsText(patterns: readonly VolumePattern[]): string {
  const list = patterns.filter((p) => p.length > 0);
  return list.length === 0 ? '' : JSON.stringify(list, null, 2);
}

/**
 * Readable action sequences, one pattern per line —
 * `"Create_v2 > Create > Buy"` / `"CreateIdempotent > Buy > Transfer"`. For
 * tooltips and filter text, where the JSON is too wide to read.
 *
 * **A pattern set's identity is its sequences, never its size** — the same rule
 * `ixLabelsActions` carries, one level up: `[Create, Buy]` and
 * `[CreateIdempotent, Buy, Transfer]` both count as one pattern while
 * classifying different trades as volume, so any surface collapsing a set to a
 * bare count renders two distinct fingerprints identically.
 */
export function ixPatternsActions(patterns: readonly VolumePattern[]): string {
  return patterns
    .filter((p) => p.length > 0)
    .map((p) => ixLabelsActions([...p]))
    .join('\n');
}

/**
 * Stable identity string for a whole set. Sorted by {@link patternKey}, because
 * position carries no meaning here (`togglePattern` appends a re-added pattern
 * at the end) — two fingerprints matching the same structures in a different
 * order are the same match criterion and must key the same.
 */
export function ixPatternsIdentity(patterns: readonly VolumePattern[]): string {
  return patterns
    .filter((p) => p.length > 0)
    .map(patternKey)
    .sort()
    .join('');
}

/**
 * Remove `labels` if the set already has it, else append it. Surviving patterns
 * keep their order; a re-added pattern lands at the end (position carries no
 * meaning — the set is matched by membership).
 */
export function togglePattern(
  patterns: readonly VolumePattern[],
  labels: VolumePattern,
): string[][] {
  if (labels.length === 0) return patterns.map((p) => [...p]);
  const key = patternKey(labels);
  const kept = patterns.filter((p) => patternKey(p) !== key);
  if (kept.length !== patterns.length) return kept.map((p) => [...p]);
  return [...patterns.map((p) => [...p]), [...labels]];
}

/**
 * Rebuild pattern arrays from a `flowPatternKeys` set. Chart hosts hand their
 * saved patterns down as keys, not arrays, so a draft seeded from a host has to
 * round-trip through this. A key that doesn't parse back to a string array can
 * only come from a corrupted store — drop it rather than staging garbage.
 */
export function patternsFromKeys(
  keys: ReadonlySet<string> | null | undefined,
): string[][] {
  if (!keys) return [];
  const out: string[][] = [];
  for (const key of keys) {
    try {
      const parsed: unknown = JSON.parse(key);
      if (Array.isArray(parsed) && parsed.every((x) => typeof x === 'string')) {
        out.push(parsed as string[]);
      }
    } catch {
      /* not a pattern key */
    }
  }
  return out;
}

// Blank-label / empty-pattern sanitizing lives in `metricConfigWithIxPatterns`
// (lib/strategy/registry), the one function that builds the persisted shape — a
// second copy here would be the same rule written twice, free to drift.
