import { describe, expect, it } from 'vitest';

import { FP_CONFIG_LISTS } from './FingerprintParamsSummary';

/**
 * **The fingerprint-config lock.**
 *
 * A group that declares `fingerprint_config` puts a list on the fingerprint, and
 * that list is part of the row's IDENTITY (`fingerprints_identity_uniq` is
 * criteria + wildcard + `metric_config`). Every surface that shows a fingerprint
 * therefore has to show it, or two rows that arm on different things render the
 * same.
 *
 * That is not hypothetical: `m_burst_slot` and `m_copy` were added to the engine
 * with a form control and a chip but no table column, no search text and no sort
 * key, so a copy fingerprint — which is a wildcard, and so has no axis chips at all
 * — displayed nothing whatsoever of the wallet it follows. This reads the Rust
 * registry directly so the next group fails HERE until it has an entry.
 */
describe('every fingerprint-scoped group is in FP_CONFIG_LISTS', () => {
  // Raw source of the Rust registry, via Vite's glob — the same mechanism
  // `strategyHelp.test.ts` uses to lock the help vocabulary.
  const rust = Object.values(
    (
      import.meta as unknown as {
        glob(
          pattern: string,
          opts: { eager: true; query: string; import: string },
        ): Record<string, string>;
      }
    ).glob('../../../../../engine/src/metrics/mod.rs', {
      eager: true,
      query: '?raw',
      import: 'default',
    }),
  )[0];

  /** Groups whose `fingerprint_config` is non-empty.
   *
   *  Read backwards from each populated `fingerprint_config:` to the `name:` above
   *  it, because a `GroupSpec` states its name before its config and the two are
   *  separated by a variable number of lines. `&[]` is skipped: that is a group with
   *  no fingerprint-side list, which owes this table nothing. */
  const configured = (() => {
    const out = new Set<string>();
    for (const m of rust.matchAll(/fingerprint_config:\s*&\[\s*(?!\])/g)) {
      const before = rust.slice(0, m.index);
      const names = [...before.matchAll(/^\s*name: "(m_[a-z_]+)",$/gm)];
      const last = names[names.length - 1]?.[1];
      if (last) out.add(last);
    }
    return out;
  })();

  it('reads the Rust registry — this guard is the lock', () => {
    expect(rust).toBeTruthy();
    // A regex that stops matching makes the assertion below vacuously pass, so the
    // lock asserts it actually harvested the groups it knows are there.
    expect(configured.has('m_flow_ix')).toBe(true);
    expect(configured.has('m_copy')).toBe(true);
    expect(configured.size).toBeGreaterThanOrEqual(4);
  });

  it('covers every group the registry gives a fingerprint_config', () => {
    const covered = new Set(FP_CONFIG_LISTS.map((s) => s.group));
    for (const group of configured) {
      expect(
        covered,
        `${group} declares fingerprint_config but has no FP_CONFIG_LISTS entry — its list would have no column, no search text and no sort key on the Fingerprints page`,
      ).toContain(group);
    }
  });

  it('names no group the registry does not declare', () => {
    // The other direction: a group renamed or deleted in Rust leaves an entry here
    // reading a `metric_config` key nothing ever writes, which renders as a column
    // of dashes rather than as an error.
    const groups = new Set([...rust.matchAll(/^\s*name: "(m_[a-z_]+)",$/gm)].map((m) => m[1]));
    for (const s of FP_CONFIG_LISTS) {
      expect(groups, `FP_CONFIG_LISTS names ${s.group}, which is not a registry group`).toContain(
        s.group,
      );
    }
  });

  it('gives each list its own stable column key and chip prefix', () => {
    // Column keys key saved sort/visibility prefs and the same-value tint map; a
    // collision silently merges two lists into one column.
    expect(new Set(FP_CONFIG_LISTS.map((s) => s.columnKey)).size).toBe(FP_CONFIG_LISTS.length);
    expect(new Set(FP_CONFIG_LISTS.map((s) => s.key)).size).toBe(FP_CONFIG_LISTS.length);
    expect(new Set(FP_CONFIG_LISTS.map((s) => s.group)).size).toBe(FP_CONFIG_LISTS.length);
  });
});
