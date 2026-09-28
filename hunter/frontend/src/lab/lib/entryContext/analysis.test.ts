import { describe, expect, it } from 'vitest';
import { AXIS_BY_KEY, ENTRY_AXES, formatAxis } from './axes';
import { compileFilter, rollupByToken, shareHistogram, structureBoard } from './analysis';
import { entryKey, type EntryGroupRow, type EntryRow, type EntryWindowRead } from './types';

const read = (tag: number, all: number): EntryWindowRead => ({
  buy_tx: all,
  tag_buy_tx: tag,
  buy_sol: all,
  tag_buy_sol: tag,
  sell_tx: 0,
  tag_sell_tx: 0,
  sell_sol: 0,
  tag_sell_sol: 0,
  tx_share_pct: all > 0 ? (100 * tag) / all : null,
  sol_share_pct: all > 0 ? (100 * tag) / all : null,
});

const group = (key: string, buyTx: number, all: number, tagged = 0): EntryGroupRow => ({
  key,
  buy_tx: buyTx,
  sell_tx: 0,
  buy_sol: buyTx,
  sell_sol: 0,
  tag_buy_tx: tagged,
  wallets: 1,
  buy_secs: 1,
  buy_tx_share_pct: all > 0 ? (100 * buyTx) / all : null,
  buy_sol_share_pct: all > 0 ? (100 * buyTx) / all : null,
});

const entry = (mint: string, slot: number, win: EntryWindowRead, ctl: EntryWindowRead, groups: EntryGroupRow[] = []): EntryRow => ({
  mint_address: mint,
  slot,
  tx_index: 0,
  at: '2026-09-20T00:00:00Z',
  sol: 1,
  window: win,
  control: ctl,
  groups,
  groups_omitted: 0,
});

// The finding: 6Vo made 20 of 22 buys in the 30s before he bought.
const hit = entry('A', 1, read(20, 22), read(1, 10), [group('6Vo', 20, 22, 20), group('pump', 2, 22)]);
const miss = entry('A', 2, read(1, 10), read(1, 10), [group('pump', 9, 10), group('6Vo', 1, 10, 1)]);
const empty = entry('B', 3, read(0, 0), read(0, 0));
const lost: EntryRow = { ...entry('B', 4, read(9, 9), read(9, 9)), unknown_reason: 'tape-truncated' };
const all = [hit, miss, empty, lost];

describe('entry context filter', () => {
  it('keeps the entries whose axis clears the condition', () => {
    const f = compileFilter([{ axis: 'tx_share', cond: '>50' }]);
    expect(all.filter(f.pass)).toEqual([hit]);
    expect(f.active).toBe(1);
  });

  it('never passes an empty window or an unknown entry, even with no condition', () => {
    const none = compileFilter([]);
    expect(none.pass(lost)).toBe(false);
    expect(none.pass(empty)).toBe(true);
    expect(compileFilter([{ axis: 'tx_share', cond: '<50' }]).pass(empty)).toBe(false);
  });

  it('reports a malformed line and lets it constrain nothing', () => {
    const f = compileFilter([{ axis: 'tx_share', cond: 'lots' }, { axis: 'nope', cond: '>1' }]);
    expect([...f.errors.keys()]).toEqual([0, 1]);
    expect(f.active).toBe(0);
  });

  it('ANDs lines and supports OR arms', () => {
    const f = compileFilter([
      { axis: 'tx_share', cond: '>50 | <5' },
      { axis: 'tx_share_lift', cond: '>=50' },
    ]);
    expect(all.filter(f.pass)).toEqual([hit]);
  });
});

describe('entry context summaries', () => {
  it('rolls entries up per token over the passing set', () => {
    const passing = new Set([entryKey(hit)]);
    const r = rollupByToken(all, passing);
    expect(r.get('A')).toEqual({ entries: 2, passing: 1, unknown: 0, bestTxShare: (100 * 20) / 22 });
    expect(r.get('B')).toMatchObject({ entries: 2, passing: 0, unknown: 1, bestTxShare: null });
  });

  it('buckets window vs control and counts no-reading apart', () => {
    const h = shareHistogram(all, AXIS_BY_KEY.get('tx_share')!, AXIS_BY_KEY.get('ctl_tx_share')!);
    expect(h.buckets[9].window).toBe(1);
    expect(h.buckets[1].window).toBe(1);
    expect(h.buckets[1].control).toBe(2);
    expect(h.noWindow).toBe(1);
    expect(h.noControl).toBe(1);
  });

  it('ranks structures by presence and counts who led', () => {
    const b = structureBoard(all);
    const six = b.find((s) => s.key === '6Vo')!;
    expect(six).toMatchObject({ present: 2, top: 1, tagged: 2 });
    // Mean over the three readable entries, absent counting 0 %.
    expect(six.meanTxShare).toBeCloseTo(((100 * 20) / 22 + 10) / 3);
  });

  it('every axis has a one-line definition and formats a missing value as -', () => {
    for (const a of ENTRY_AXES) {
      expect(a.definition(30).length).toBeGreaterThan(10);
      expect(formatAxis(a, null)).toBe('-');
    }
    expect(new Set(ENTRY_AXES.map((a) => a.key)).size).toBe(ENTRY_AXES.length);
  });
});
