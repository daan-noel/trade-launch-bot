import { describe, expect, it } from 'vitest';
import { entryColumns } from '@lab/components/entry-context/entryColumns';
import { entryVerdict } from './analysis';
import { entryLogic } from './logic';
import type { EntryRow, EntryWindowRead } from './types';

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

const entry = (win: EntryWindowRead, ctl: EntryWindowRead, lag: number | null, sol = 1): EntryRow => ({
  mint_address: 'M',
  slot: 1,
  tx_index: 0,
  at: '2026-09-20T00:00:00Z',
  sol,
  window: win,
  control: ctl,
  groups: [],
  groups_omitted: 0,
  probe: {
    hits: win.tag_buy_tx,
    sol: win.tag_buy_sol,
    control_hits: ctl.tag_buy_tx,
    control_sol: ctl.tag_buy_sol,
    nearest: lag == null ? null : { lag_slots: lag, lag_tx: null, lag_secs: 1, key: 'k', slot: 0, tx_index: 0 },
  },
});

const cols = entryColumns(30, (m) => m, null, 25);

describe('entryLogic', () => {
  // Three buys: the target dominates right before the first two, and only the
  // second one was already dominated by it earlier.
  const a = entry(read(8, 10), read(0, 10), 2);
  const b = entry(read(9, 10), read(9, 10), 1);
  const c = entry(read(1, 10), read(0, 10), null, 5);

  it('checks a target filter on both stretches through its twin', () => {
    const logic = entryLogic({ tx_share: '>50' }, cols, null, 'all');
    expect([a, b, c].map(logic.last)).toEqual([true, true, false]);
    expect([a, b, c].map(logic.earlier)).toEqual([false, true, false]);
  });

  it('applies a scope filter the same on both stretches', () => {
    const logic = entryLogic({ entry_sol: '>2', tx_share: '>=0' }, cols, null, 'all');
    expect(logic.conditions.map((x) => x.kind)).toEqual(['scope', 'signal']);
    expect([a, b, c].map(logic.inScope)).toEqual([false, false, true]);
  });

  it('leaves a signal with no earlier value out of Earlier, and names it', () => {
    const logic = entryLogic({ tx_share_lift: '>=50' }, cols, null, 'all');
    expect(logic.conditions[0]).toMatchObject({ kind: 'signal', earlierChecked: false });
    expect([a, b].map(logic.last)).toEqual([true, false]);
    // Nothing twinned: every buy passes Earlier, so the link errs low.
    expect([a, b].map(logic.earlier)).toEqual([true, true]);
  });

  it("checks the probe's Show on the earlier stretch through control_matched", () => {
    const t = { minHits: 1, minSol: 0 };
    const verdictOf = (e: EntryRow) => entryVerdict(e, t);
    const withProbe = entryColumns(30, (m) => m, verdictOf, 25);
    const logic = entryLogic({}, withProbe, verdictOf, 'matched');
    expect([a, b, c].map(logic.last)).toEqual([true, true, true]);
    expect([a, b, c].map(logic.earlier)).toEqual([false, true, false]);
  });

  it('ignores a filter on a column that is not on screen', () => {
    expect(entryLogic({ pe_hits: '>0' }, cols, null, 'all').signals).toBe(0);
  });

  it('keeps the probe out of the idea, so another filter joins the idea', () => {
    const t = { minHits: 1, minSol: 0 };
    const verdictOf = (e: EntryRow) => entryVerdict(e, t);
    const withProbe = entryColumns(30, (m) => m, verdictOf, 25);
    const logic = entryLogic({ tx_share: '>50', pe_hits: '>=1' }, withProbe, verdictOf, 'matched');
    expect([a, b, c].map(logic.idea)).toEqual([true, true, false]);
    // c's probe still matches (1 hit, Show = matched). Only the idea rejects it.
    expect([a, b, c].map(logic.probe)).toEqual([true, true, true]);
    expect(logic.last(c)).toBe(false);
  });

  it('marks the filters that need the selected IXs, and leaves them out of anyIx', () => {
    const t = { minHits: 1, minSol: 0 };
    const verdictOf = (e: EntryRow) => entryVerdict(e, t);
    const withProbe = entryColumns(30, (m) => m, verdictOf, 25);
    const filters = { entry_sol: '>2', pe_hits: '>=1', tx_share: '>50', sig_tx_share: '>0', buy_tx: '>=10' };
    const logic = entryLogic(filters, withProbe, verdictOf, 'all');
    expect(Object.fromEntries(logic.conditions.map((x) => [x.key, x.needsIxs]))).toEqual({
      entry_sol: false,
      pe_hits: true,
      tx_share: true,
      sig_tx_share: false,
      buy_tx: false,
    });
    // Neither has an ix signal (no breakdown rows), so IX tx % fails both, and that
    // filter applies to every row: it does not depend on the selected IXs.
    expect([a, c].map(logic.idea)).toEqual([false, false]);
    expect([a, c].map(logic.anyIx)).toEqual([false, false]);
  });
});
