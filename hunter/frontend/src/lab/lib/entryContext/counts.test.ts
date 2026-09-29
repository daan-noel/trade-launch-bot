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

/** A buy. `hits` is the probe (the target signaled); `tag` of 10 is its share. */
const buy = (mint: string, tag: number, hits = 0): EntryRow => ({
  mint_address: mint,
  slot: 1,
  tx_index: 0,
  at: '2026-09-20T00:00:00Z',
  sol: 1,
  window: read(tag, 10),
  control: read(0, 10),
  groups: [],
  groups_omitted: 0,
  probe: {
    hits,
    sol: hits,
    control_hits: 0,
    control_sol: 0,
    nearest: hits > 0 ? { lag_slots: 0, lag_tx: null, lag_secs: 0, key: 'k' } : null,
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

  it('counts his buys and tokens, target signal against every structure', () => {
    const t = cohortTable(entries, logic, signaled, true);
    expect(t.target.buys).toEqual({ his: 2, matched: 1 });
    expect(t.target.tokens).toEqual({ his: 1, matched: 1 });
    expect(t.all.buys).toEqual({ his: 4, matched: 2 });
    expect(t.all.tokens).toEqual({ his: 3, matched: 2 });
  });

  it('a second filter narrows matched and leaves his', () => {
    const both = entryLogic({ tx_share: '>50', sol_share: '>80' }, cols, null, 'all');
    const t = cohortTable(entries, both, signaled, true);
    // A is 80/80, B is 90/90. sol share > 80 keeps B only, on the all row.
    expect(t.all.buys).toMatchObject({ his: 4, matched: 1 });
    expect(t.target.buys).toMatchObject({ his: 2, matched: 0 });
  });

  it('with the probe off the target row is empty', () => {
    const t = cohortTable(entries, logic, signaled, false);
    expect(t.target.buys.his).toBe(0);
    expect(t.all.buys.his).toBe(4);
  });
});
