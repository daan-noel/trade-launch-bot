import { describe, expect, it } from 'vitest';
import { patternKeysFrom } from './classifyFlow';
import { patternsFromKeys, togglePattern } from './volumePatterns';

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
