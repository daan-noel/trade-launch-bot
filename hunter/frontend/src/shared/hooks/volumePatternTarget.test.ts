import { describe, expect, it } from 'vitest';
import { resolveIxPatternTarget } from './useIxPatternTarget';

/**
 * The precedence rule behind every Tagged-badge write. The cases that matter are the
 * ones where a pattern-set match is available AND wrong — an id the host already
 * resolved has to win, or the badge either goes dead or edits someone else's row.
 */
describe('resolveIxPatternTarget', () => {
  it('takes the host fingerprint over a single pattern-set match', () => {
    const r = resolveIxPatternTarget({
      pickedId: null,
      hostFingerprintId: 'host-fp',
      matchIds: ['other-fp'],
    });
    expect(r.targetId).toBe('host-fp');
    expect(r.inferred).toBe(false);
    expect(r.offHost).toBe(false);
  });

  it('targets the host fingerprint even when nothing carries its set', () => {
    // Authoring's starting state: the row exists, its patterns do not yet. This is
    // the case that left the picker empty and the badge uneditable.
    const r = resolveIxPatternTarget({
      pickedId: null,
      hostFingerprintId: 'host-fp',
      matchIds: [],
    });
    expect(r.targetId).toBe('host-fp');
    expect(r.inferred).toBe(false);
  });

  it('infers a lone match only when the host has no fingerprint, and flags it', () => {
    const r = resolveIxPatternTarget({
      pickedId: null,
      hostFingerprintId: null,
      matchIds: ['only-fp'],
    });
    expect(r.targetId).toBe('only-fp');
    expect(r.inferred).toBe(true);
  });

  it('refuses to guess between several matching fingerprints', () => {
    // What an empty pattern set looks like across a corpus: every unconfigured row
    // matches, so there is no answer to infer.
    const r = resolveIxPatternTarget({
      pickedId: null,
      hostFingerprintId: null,
      matchIds: ['a', 'b'],
    });
    expect(r.targetId).toBeNull();
    expect(r.inferred).toBe(false);
    expect(r.offHost).toBe(false);
  });

  it('lets an explicit pick outrank both, and reports it as off-host', () => {
    const r = resolveIxPatternTarget({
      pickedId: 'picked-fp',
      hostFingerprintId: 'host-fp',
      matchIds: ['host-fp'],
    });
    expect(r.targetId).toBe('picked-fp');
    expect(r.inferred).toBe(false);
    expect(r.offHost).toBe(true);
  });

  it('does not call a pick off-host when it re-picks the host row', () => {
    const r = resolveIxPatternTarget({
      pickedId: 'host-fp',
      hostFingerprintId: 'host-fp',
      matchIds: [],
    });
    expect(r.offHost).toBe(false);
  });

  it('is never off-host when the host has no fingerprint to be off', () => {
    const r = resolveIxPatternTarget({
      pickedId: 'picked-fp',
      hostFingerprintId: null,
      matchIds: ['a', 'b'],
    });
    expect(r.targetId).toBe('picked-fp');
    expect(r.inferred).toBe(false);
    expect(r.offHost).toBe(false);
  });
});
