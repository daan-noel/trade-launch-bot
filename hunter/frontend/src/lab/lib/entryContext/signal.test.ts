import { describe, expect, it } from 'vitest';
import { matchesSignal, matchesSlippage, parseSlippagePct, signalCounts, slippageText } from './signal';
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

describe('slippage text', () => {
  it('reads a comma-separated box', () => {
    expect(parseSlippagePct('10, 20, 30')).toEqual([10, 20, 30]);
    expect(parseSlippagePct('')).toEqual([]);
  });

  it('shows the derived options once each', () => {
    expect(
      slippageText([
        { pct: 10, entries: 4, family: 'ceiling' },
        { pct: 20, entries: 133, family: 'ceiling' },
        { pct: 30, entries: 2, family: 'floor' },
        { pct: 20, entries: 1, family: 'floor' },
      ]),
    ).toBe('10, 20, 30');
  });

  it('keeps the buys at one derived slippage', () => {
    const at = (pct: number | undefined): EntryRow =>
      row({ kind: 'definite', slippage_pct: pct });
    const entries = [at(20), at(15), at(20.04), at(undefined)];
    expect(entries.filter((e) => matchesSlippage(e, null))).toHaveLength(4);
    expect(entries.filter((e) => matchesSlippage(e, 20)).map((e) => e.reserve_call?.slippage_pct)).toEqual([
      20, 20.04,
    ]);
    expect(entries.filter((e) => matchesSlippage(e, 15))).toHaveLength(1);
  });
});
