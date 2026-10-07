import { describe, expect, it } from 'vitest';
import { matchesSignal, signalCounts } from './signal';
import type { EntryRow, ReserveCall } from './types';

const call = (kind: ReserveCall['kind']): ReserveCall => ({ kind });

const row = (reserve_call: ReserveCall): EntryRow => ({ reserve_call }) as EntryRow;

describe('signal counts', () => {
  const entries = [
    row(call('definite')),
    row(call('definite')),
    row(call('crowded')),
    row(call('no_ceiling')),
    row(call('no_match')),
    row(call('several')),
    row(call('single')),
    row(call('empty')),
  ];

  it('counts each reserve outcome', () => {
    expect(signalCounts(entries)).toMatchObject({
      definite: 2,
      single: 1,
      crowded: 1,
      noCeiling: 1,
      noMatch: 1,
      several: 1,
      empty: 1,
    });
  });

  it('keeps a blank click on the crowded failures', () => {
    const blank = entries.filter((e) => matchesSignal(e, 'blank'));
    expect(blank.map((e) => e.reserve_call?.kind)).toEqual(['no_ceiling', 'no_match', 'several']);
  });

  it('keeps a definite click on the gold rows only', () => {
    expect(entries.filter((e) => matchesSignal(e, 'definite'))).toHaveLength(2);
  });
});
