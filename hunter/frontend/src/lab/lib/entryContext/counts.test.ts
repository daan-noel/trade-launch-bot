import { describe, expect, it } from 'vitest';
import { entryColumns } from '@lab/components/entry-context/entryColumns';
import { entryVerdict } from './analysis';
import { cohortTable } from './counts';
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

/** A buy. `hits` is the probe (the target signaled); `tag` of `all` is its share. */
const buy = (mint: string, tag: number, hits = 0, all = 10): EntryRow => ({
  mint_address: mint,
  slot: 1,
  tx_index: 0,
  at: '2026-09-20T00:00:00Z',
  sol: 1,
  window: read(tag, all),
  control: read(0, all),
  groups: [],
  groups_omitted: 0,
  probe: {
    hits,
    sol: hits,
    control_hits: 0,
    control_sol: 0,
    nearest: hits > 0 ? { lag_slots: 0, lag_tx: null, lag_secs: 0, key: 'k', slot: 0, tx_index: 0 } : null,
  },
});

const cols = entryColumns(30, (m) => m, null, 25);
const logic = entryLogic({ tx_share: '>50' }, cols, null, 'all');
const signaled = (e: EntryRow) => entryVerdict(e, { minHits: 1, minSol: 0 }).state === 'matched';

describe('cohortTable', () => {
  const entries = [
    buy('A', 8, 1), // target signal, idea holds
    buy('A', 2, 1), // target signal, idea does not
    buy('B', 9, 0), // no signal, idea holds
    buy('C', 1, 0), // neither
    { ...buy('D', 9, 1), unknown_reason: 'tape-truncated' as const },
  ];

  it('counts his buys and tokens; the all row skips a filter that needs the selected IXs', () => {
    const t = cohortTable(entries, logic, signaled, true);
    expect(t.target.buys).toEqual({ his: 2, matched: 1 });
    expect(t.target.tokens).toEqual({ his: 1, matched: 1 });
    // Target tx % is the only filter: the all row has none to ask, so every buy passes.
    expect(t.all.buys).toEqual({ his: 4, matched: 4 });
    expect(t.all.tokens).toEqual({ his: 3, matched: 3 });
  });

  it('a second target filter narrows the pool row only', () => {
    const both = entryLogic({ tx_share: '>50', sol_share: '>80' }, cols, null, 'all');
    const t = cohortTable(entries, both, signaled, true);
    // A is 80/80: sol share > 80 drops it from the pool row.
    expect(t.target.buys).toMatchObject({ his: 2, matched: 0 });
    expect(t.all.buys).toMatchObject({ his: 4, matched: 4 });
  });

  it('a filter that needs no IXs narrows both rows', () => {
    const mixed = entryLogic({ tx_share: '>50', buy_tx: '>10' }, cols, null, 'all');
    const busy = [
      buy('A', 16, 1, 20), // pool, 80%, 20 buys: passes both
      buy('A', 4, 1, 5), // pool, 80%, 5 buys: fails All buys
      buy('B', 18, 0, 20), // not pool, 20 buys
      buy('C', 1, 0, 5), // not pool, 5 buys
    ];
    const t = cohortTable(busy, mixed, signaled, true);
    expect(t.target.buys).toEqual({ his: 2, matched: 1 });
    // The all row asks All buys > 10 only: A's first buy and B's.
    expect(t.all.buys).toEqual({ his: 4, matched: 2 });
    expect(t.all.tokens).toEqual({ his: 3, matched: 2 });
  });

  it('with the probe off the target row is empty', () => {
    const t = cohortTable(entries, logic, signaled, false);
    expect(t.target.buys.his).toBe(0);
    expect(t.all.buys.his).toBe(4);
  });
});
