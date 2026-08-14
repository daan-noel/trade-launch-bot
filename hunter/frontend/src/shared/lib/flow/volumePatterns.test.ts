import { describe, expect, it } from 'vitest';
import { patternKeysFrom } from './classifyFlow';
import {
  formatVolumePatternsText,
  patternsFromKeys,
  togglePattern,
  volumePatternsActions,
  volumePatternsIdentity,
} from './volumePatterns';

const A = ['Compute Budget: SetComputeUnitLimit', 'Pump.Fun: Buy'];
const B = ['Compute Budget: SetComputeUnitLimit', 'Pump.Fun: Sell'];

describe('togglePattern', () => {
  it('adds an absent pattern and removes a present one', () => {
    const once = togglePattern([], A);
    expect(once).toEqual([A]);
    expect(togglePattern(once, A)).toEqual([]);
  });

  it('leaves the other patterns and their order alone', () => {
    expect(togglePattern([A, B], A)).toEqual([B]);
  });

  it('treats a re-ordered sequence as a DIFFERENT pattern (ix_labels are ordered)', () => {
    const reversed = [...A].reverse();
    expect(togglePattern([A], reversed)).toEqual([A, reversed]);
  });

  it('keeps duplicate labels inside a pattern — never dedupes the sequence', () => {
    const dupes = ['System Program: Transfer', 'Pump.Fun: Buy', 'System Program: Transfer'];
    const [staged] = togglePattern([], dupes);
    expect(staged).toEqual(dupes);
    expect(togglePattern([dupes], ['System Program: Transfer', 'Pump.Fun: Buy'])).toHaveLength(2);
  });

  it('ignores an empty sequence — an unlabelled trade has nothing to stage', () => {
    expect(togglePattern([A], [])).toEqual([A]);
  });

  it('copies rather than aliasing the input arrays', () => {
    const source = [...A];
    const [staged] = togglePattern([], source);
    source.push('mutated');
    expect(staged).toEqual(A);
  });
});

describe('volumePatternsActions / formatVolumePatternsText', () => {
  it('reads each pattern as its action sequence, one per line', () => {
    expect(volumePatternsActions([A, B])).toBe(
      'SetComputeUnitLimit > Buy\nSetComputeUnitLimit > Sell',
    );
  });

  it('is empty for an unconfigured set', () => {
    expect(volumePatternsActions([])).toBe('');
    expect(formatVolumePatternsText([])).toBe('');
    expect(formatVolumePatternsText([[]])).toBe('');
  });

  it('copies back as re-pastable JSON', () => {
    expect(JSON.parse(formatVolumePatternsText([A, B]))).toEqual([A, B]);
  });
});

describe('volumePatternsIdentity', () => {
  it('is order-insensitive — the set is matched by membership', () => {
    expect(volumePatternsIdentity([A, B])).toBe(volumePatternsIdentity([B, A]));
  });

  it('splits sets that differ only in a sequence, not in size', () => {
    const near = ['Compute Budget: SetComputeUnitLimit', 'Pump.Fun: BuyExactSolIn'];
    expect(volumePatternsIdentity([A])).not.toBe(volumePatternsIdentity([near]));
  });

  it('splits a re-ordered sequence — ix_labels are ordered', () => {
    expect(volumePatternsIdentity([A])).not.toBe(volumePatternsIdentity([[...A].reverse()]));
  });

  it('treats an empty pattern as absent', () => {
    expect(volumePatternsIdentity([[], A])).toBe(volumePatternsIdentity([A]));
    expect(volumePatternsIdentity([])).toBe('');
  });
});

describe('patternsFromKeys', () => {
  it('round-trips the keys a chart host hands down', () => {
    expect(patternsFromKeys(patternKeysFrom([A, B]))).toEqual(
      expect.arrayContaining([A, B]),
    );
  });

  it('is empty for a missing set', () => {
    expect(patternsFromKeys(null)).toEqual([]);
    expect(patternsFromKeys(undefined)).toEqual([]);
  });

  it('drops anything that is not a string array — a corrupted store never stages garbage', () => {
    expect(patternsFromKeys(new Set(['not json', '{"a":1}', '[1,2]', JSON.stringify(A)]))).toEqual([
      A,
    ]);
  });
});
