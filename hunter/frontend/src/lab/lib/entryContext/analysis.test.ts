import { describe, expect, it } from 'vitest';
import { probeSummary } from '@lab/lib/preEntryProbeTypes';
import { ENTRY_AXES, formatAxis } from './axes';
import { atDecision, entryVerdict, rollupByToken, structureBoard } from './analysis';
import type { EntryGroupRow, EntryRow, EntryWindowRead } from './types';

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

const entry = (
  mint: string,
  slot: number,
  win: EntryWindowRead,
  ctl: EntryWindowRead,
  groups: EntryGroupRow[] = [],
): EntryRow => ({
  mint_address: mint,
  slot,
  tx_index: 0,
  at: '2026-09-20T00:00:00Z',
  sol: 1,
  window: win,
  control: ctl,
  groups,
  groups_omitted: 0,
  // The probe's slots see the same tagged buys as the window, in these fixtures.
  probe: {
    hits: win.tag_buy_tx,
    sol: win.tag_buy_sol,
    control_hits: ctl.tag_buy_tx,
    control_sol: ctl.tag_buy_sol,
    nearest: win.tag_buy_tx > 0 ? { lag_slots: 3, lag_tx: null, lag_secs: 1, key: '6Vo', slot: 0, tx_index: 0 } : null,
  },
});

// The finding: 6Vo made 20 of 22 buys in the 30s before he bought.
const hit = entry('A', 1, read(20, 22), read(0, 10), [group('6Vo', 20, 22, 20), group('pump', 2, 22)]);
const miss = entry('A', 2, read(1, 10), read(1, 10), [group('pump', 9, 10), group('6Vo', 1, 10, 1)]);
const empty = entry('B', 3, read(0, 0), read(0, 0));
const lost: EntryRow = { ...entry('B', 4, read(9, 9), read(9, 9)), unknown_reason: 'tape-truncated' };
const all = [hit, miss, empty, lost];
const PRESENCE = { minHits: 1, minSol: 0 };

describe('the seat a buy is read from', () => {
  // Behind the signal the target made 2 of 2 buys; by his own buy a third buy landed.
  const seat = { window: read(2, 2), control: read(0, 1), groups: [], groups_omitted: 0 };
  const pooled: EntryRow = { ...entry('A', 1, read(2, 3), read(0, 1)), at_signal: seat };
  const outside: EntryRow = { ...entry('A', 2, read(0, 3), read(0, 1)) };

  it('reads a pool buy behind its signal, and any other buy at his own', () => {
    const [a, b] = atDecision([pooled, outside], PRESENCE, true);
    expect(a.window).toEqual(read(2, 2));
    expect(a.probe).toBe(pooled.probe);
    expect(b).toBe(outside);
  });

  it('keeps his own seat when the probe is off or its thresholds leave the buy out', () => {
    expect(atDecision([pooled], PRESENCE, false)[0]).toBe(pooled);
    expect(atDecision([pooled], { minHits: 5, minSol: 0 }, true)[0]).toBe(pooled);
  });
});

describe('entry context verdict', () => {
  it('is the probe verdict: matched / no-match / unknown on min hits and SOL', () => {
    expect(all.map((e) => entryVerdict(e, PRESENCE).state)).toEqual([
      'matched',
      'matched',
      'no-match',
      'unknown',
    ]);
    expect(entryVerdict(miss, { minHits: 5, minSol: 0 }).state).toBe('no-match');
    expect(entryVerdict(hit, { minHits: 1, minSol: 25 }).state).toBe('no-match');
  });

  it('carries the nearest target and the control test', () => {
    const v = entryVerdict(hit, PRESENCE);
    expect(v).toMatchObject({ hits: 20, sol: 20, nearest_lag_slots: 3, matched_unit: '6Vo' });
    expect(v.control_matched).toBe(false);
    expect(entryVerdict(miss, PRESENCE).control_matched).toBe(true);
    // An unknown entry never claims its control matched.
    expect(entryVerdict(lost, PRESENCE).control_matched).toBe(false);
  });

  it('reads through the probe summary Trader Analysis prints', () => {
    const s = probeSummary(all.map((e) => entryVerdict(e, PRESENCE)), 0);
    expect(s).toBe('before 2/4 · earlier 1/4 · median lag 3.0 slots · 1 unknown');
  });
});

describe('entry context summaries', () => {
  it('rolls buys up per token over the shown set', () => {
    const r = rollupByToken(all, new Set([hit]));
    expect(r.get('A')).toEqual({ entries: 2, shown: 1, bestTxShare: (100 * 20) / 22 });
    expect(r.get('B')).toEqual({ entries: 2, shown: 0, bestTxShare: null });
  });

  it('ranks structures by presence and counts who led', () => {
    const b = structureBoard(all);
    const six = b.find((s) => s.key === '6Vo')!;
    expect(six).toMatchObject({ present: 2, top: 1, tagged: 2 });
    // Mean over the three readable buys, absent counting 0 %.
    expect(six.meanTxShare).toBeCloseTo(((100 * 20) / 22 + 10) / 3);
  });

  it('every axis has a one-line definition and formats a missing value as -', () => {
    for (const a of ENTRY_AXES) {
      expect(a.definition(30).length).toBeGreaterThan(10);
      expect(formatAxis(a, null)).toBe('-');
      if (a.unit === 'sol') expect(formatAxis(a, 1.5)).toBe(`◎${(1.5).toFixed(a.digits)}`);
    }
    expect(new Set(ENTRY_AXES.map((a) => a.key)).size).toBe(ENTRY_AXES.length);
  });
});
