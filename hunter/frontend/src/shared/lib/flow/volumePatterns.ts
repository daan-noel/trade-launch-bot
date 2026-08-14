/**
 * Edits to a `m_flow_split.volume_ix_patterns` set — the ONE toggler behind
 * every surface that stages patterns (the chart trades table, Flow Discovery's
 * structure checkboxes).
 *
 * A pattern is an EXACT ordered `ix_labels` sequence: never sorted, never
 * deduped, duplicates kept, same length. `JSON.stringify` of that array is its
 * identity — the same key `classifyFlow` matches trades on, so a pattern staged
 * here and a trade classified there can never disagree about what "the same
 * structure" means.
 */

export type VolumePattern = readonly string[];

/** Identity of one ordered label sequence. */
export function patternKey(labels: VolumePattern): string {
  return JSON.stringify(labels);
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

// Blank-label / empty-pattern sanitizing lives in `metricConfigWithVolumePatterns`
// (lib/strategy/registry), the one function that builds the persisted shape — a
// second copy here would be the same rule written twice, free to drift.
