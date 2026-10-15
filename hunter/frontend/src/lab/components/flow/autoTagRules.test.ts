import { describe, expect, it } from 'vitest';

import { patternRowKey, type IxPatternRow } from 'lib/strategy/ixPatternRows';
import type { OwnerStage } from 'types';

import {
  AUTO_TAG_RULES,
  EMPTY_AUTO_STAGE,
  levelRow,
  MATCH_LEVELS,
  pickAutoRow,
  pickAutoWallet,
  stagedPicks,
  syncAutoStage,
} from './autoTagRules';

const A: IxPatternRow = { labels: ['Private: ix#01', 'Pump.Fun: BUY'], level: 'core', side: 'buy' };
const B: IxPatternRow = { labels: ['Axiom Trade: ix#05', 'Pump.Fun: BUY'], level: 'core', side: 'buy' };
const MANUAL: IxPatternRow = { labels: ['Pump.Fun: Sell'] };

describe('auto-tag rules', () => {
  it('names every backend rule once', () => {
    const ids = AUTO_TAG_RULES.map((r) => r.id);
    expect(new Set(ids).size).toBe(7);
  });
});

describe('stagedPicks', () => {
  it('keeps passing wallets only', () => {
    const stage = {
      rows: [{ row: A, level: 1, trades: 20, coins: 2, sol: 5, owner_sol: 5, owner_pct: 100, reason: null }],
      wallets: [
        { wallet: 'in', rules: ['below_zero'], coins: 2, days_since: 0, reason: null },
        { wallet: 'old', rules: ['below_zero'], coins: 2, days_since: 6, reason: 'too old' },
      ],
    } as unknown as OwnerStage;
    const picks = stagedPicks(stage);
    expect(picks.rows.map(patternRowKey)).toEqual([patternRowKey(A)]);
    expect(picks.wallets).toEqual(['in']);
  });
});

describe('syncAutoStage', () => {
  it('adds what the stage picks and keeps what the user added by hand', () => {
    const out = syncAutoStage({ rows: [MANUAL], wallets: ['mine'] }, EMPTY_AUTO_STAGE, { rows: [A], wallets: ['w'] });
    expect(out.rows.map(patternRowKey)).toEqual([MANUAL, A].map(patternRowKey));
    expect(out.wallets).toEqual(['mine', 'w']);
  });

  it('takes out what a toggle no longer supports, never a hand-added item', () => {
    const first = syncAutoStage({ rows: [MANUAL], wallets: [] }, EMPTY_AUTO_STAGE, { rows: [A, B], wallets: ['w'] });
    const second = syncAutoStage(first, first.auto, { rows: [B], wallets: [] });
    expect(second.rows.map(patternRowKey)).toEqual([MANUAL, B].map(patternRowKey));
    expect(second.wallets).toEqual([]);
  });

  it('never re-adds an item the user took out', () => {
    const first = syncAutoStage({ rows: [], wallets: [] }, EMPTY_AUTO_STAGE, { rows: [A], wallets: ['w'] });
    const un = pickAutoRow(first.rows, first.auto, A, false);
    const unW = pickAutoWallet(first.wallets, un.auto, 'w', false);
    const again = syncAutoStage({ rows: un.rows, wallets: unW.wallets }, unW.auto, { rows: [A], wallets: ['w'] });
    expect(again.rows).toEqual([]);
    expect(again.wallets).toEqual([]);
    // Ticking it back makes it auto-tag's again.
    const back = pickAutoRow(again.rows, again.auto, A, true);
    expect(back.rows.map(patternRowKey)).toEqual([patternRowKey(A)]);
    expect(syncAutoStage({ rows: back.rows, wallets: [] }, back.auto, { rows: [], wallets: [] }).rows).toEqual([]);
  });

  it('leaves an item the user already had when the stage drops it', () => {
    const first = syncAutoStage({ rows: [A], wallets: [] }, EMPTY_AUTO_STAGE, { rows: [A], wallets: [] });
    const second = syncAutoStage(first, first.auto, { rows: [], wallets: [] });
    expect(second.rows.map(patternRowKey)).toEqual([patternRowKey(A)]);
  });
});

describe('levelRow', () => {
  const LABELS = [
    'Compute Budget: SetComputeUnitLimit',
    'Compute Budget: SetComputeUnitPrice',
    'Associated Token: CreateIdempotent',
    'Pump.Fun: BuyExactSolIn',
    'System Program: Transfer',
  ];
  const t = { labels: LABELS, side: 'buy' as const, cu_limit: 200_000, cu_price: 20_000, tip_lamports: 5_000 };
  const core = { labels: ['Pump.Fun: BUY'], level: 'core' as const, side: 'buy' as const };

  it('writes the row owner_stage stores at each level', () => {
    expect(MATCH_LEVELS).toHaveLength(7);
    expect(levelRow(1, t)).toEqual(core);
    expect(levelRow(2, t)).toEqual({ ...core, marks: 'CL CP T1 A1' });
    expect(levelRow(3, t)).toEqual({ ...core, cu_price: 20_000 });
    expect(levelRow(4, t)).toEqual({ ...core, tip_lamports: 5_000 });
    expect(levelRow(5, t)).toEqual({ ...core, cu_price: 20_000, tip_lamports: 5_000 });
    expect(levelRow(6, t)).toEqual({ ...core, marks: 'CL CP T1 A1', cu_price: 20_000, tip_lamports: 5_000 });
    expect(levelRow(7, t)).toEqual({ labels: LABELS, cu_limit: 200_000, cu_price: 20_000, tip_lamports: 5_000 });
  });

  it('skips a level pinning a fee the trade has no reading for', () => {
    const noPrice = { ...t, cu_price: null };
    expect(levelRow(3, noPrice)).toBeNull();
    expect(levelRow(5, noPrice)).toBeNull();
    expect(levelRow(7, noPrice)).toBeNull();
    expect(levelRow(4, noPrice)).not.toBeNull();
    expect(levelRow(8, t)).toBeNull();
  });
});
