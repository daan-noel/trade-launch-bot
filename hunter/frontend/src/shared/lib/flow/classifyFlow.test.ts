import { describe, expect, it } from 'vitest';
import { classifyFlowTrades, flowReasonsById, patternKeysFrom } from './classifyFlow';

/** Fixture mirrors `hunter_engine::metrics::flow_ix` tests (Rust SSOT:
 *  hunter/engine/src/metrics/flow_ix.rs) — the decision order and
 *  forward-tagging behavior asserted here must stay in lockstep with
 *  `FlowState::classify`/`on_trade`. A change to one side without the other
 *  is the SSOT drift this test exists to catch. */
describe('classifyFlowTrades', () => {
  const A = ['Compute Budget: SetComputeUnitLimit', 'Pump.Fun: Buy'];
  const B = ['Compute Budget: SetComputeUnitLimit', 'Pump.Fun: Sell'];
  const patternKeys = patternKeysFrom([A]);

  it('classifies structural matches as volume, others as organic', () => {
    const trades = [
      { wallet_address: 'w1', sol: 1, ix_labels: A },
      { wallet_address: 'w2', sol: 2, ix_labels: B },
    ];
    const out = classifyFlowTrades(trades, { patternKeys });
    expect(out[0]).toMatchObject({ isTagged: true, taggedSol: 1, untaggedSol: 0 });
    expect(out[1]).toMatchObject({ isTagged: false, taggedSol: 0, untaggedSol: 2 });
  });

  it('forward-tags a wallet after its first volume trade — later non-matching trades from the same wallet still count as volume', () => {
    const trades = [
      { wallet_address: 'w1', sol: 1, ix_labels: A },
      { wallet_address: 'w1', sol: 3, ix_labels: B },
    ];
    const out = classifyFlowTrades(trades, { patternKeys });
    expect(out[0].isTagged).toBe(true);
    expect(out[1]).toMatchObject({ isTagged: true, taggedSol: 3, untaggedSol: 0 });
  });

  it('never retroactively reclassifies an earlier trade (forward-only contagion)', () => {
    const trades = [
      { wallet_address: 'w1', sol: 5, ix_labels: B },
      { wallet_address: 'w1', sol: 1, ix_labels: A },
    ];
    const out = classifyFlowTrades(trades, { patternKeys });
    expect(out[0]).toMatchObject({ isTagged: false, untaggedSol: 5 });
    expect(out[1]).toMatchObject({ isTagged: true, taggedSol: 1 });
  });

  it('creator wallet is always volume and seeds contagion', () => {
    const trades = [{ wallet_address: 'creator', sol: 4, ix_labels: B }];
    const out = classifyFlowTrades(trades, { patternKeys, creatorWallet: 'creator' });
    expect(out[0]).toMatchObject({ isTagged: true, taggedSol: 4 });
  });

  it('missing/empty ix_labels never structurally match — organic unless wallet already tagged', () => {
    const trades = [{ wallet_address: 'w9', sol: 2, ix_labels: null }];
    const out = classifyFlowTrades(trades, { patternKeys });
    expect(out[0]).toMatchObject({ isTagged: false, untaggedSol: 2 });
  });
});

/** The badge in the trades table tests structure alone; these reasons are how a
 *  row that the LINES count as volume explains itself. `isTagged` must stay exactly
 *  what it was — the reason is additive. */
describe('classifyFlowTrades reasons', () => {
  const A = ['Compute Budget: SetComputeUnitLimit', 'Pump.Fun: Buy'];
  const B = ['Compute Budget: SetComputeUnitLimit', 'Pump.Fun: Sell'];
  const patternKeys = patternKeysFrom([A]);

  it('names the structural match, then contagion on the same wallet', () => {
    const trades = [
      { wallet_address: 'w1', sol: 1, ix_labels: A },
      { wallet_address: 'w1', sol: 3, ix_labels: B },
      { wallet_address: 'w2', sol: 2, ix_labels: B },
    ];
    const out = classifyFlowTrades(trades, { patternKeys });
    expect(out.map((t) => t.reason)).toEqual(['structural', 'wallet', null]);
  });

  it('separates the creator from ordinary contagion', () => {
    const trades = [
      { wallet_address: 'creator', sol: 4, ix_labels: B },
      { wallet_address: 'w1', sol: 1, ix_labels: A },
    ];
    const out = classifyFlowTrades(trades, { patternKeys, creatorWallet: 'creator' });
    expect(out.map((t) => t.reason)).toEqual(['creator', 'structural']);
  });

  it('a structural match on an already-tagged wallet still reads as contagion', () => {
    const trades = [
      { wallet_address: 'w1', sol: 1, ix_labels: A },
      { wallet_address: 'w1', sol: 1, ix_labels: A },
    ];
    const out = classifyFlowTrades(trades, { patternKeys });
    expect(out.map((t) => t.reason)).toEqual(['structural', 'wallet']);
  });

  it('reason is null exactly when the trade is organic', () => {
    const trades = [
      { wallet_address: 'w1', sol: 1, ix_labels: A },
      { wallet_address: 'w2', sol: 2, ix_labels: B },
    ];
    for (const t of classifyFlowTrades(trades, { patternKeys })) {
      expect(t.reason == null).toBe(!t.isTagged);
    }
  });
});

describe('flowReasonsById', () => {
  const A = ['Pump.Fun: Buy'];
  const patternKeys = patternKeysFrom([A]);

  it('keys volume trades by id and omits organic ones', () => {
    const map = flowReasonsById(
      [
        { id: 't1', wallet_address: 'w1', sol: 1, ix_labels: A },
        { id: 't2', wallet_address: 'w2', sol: 2, ix_labels: ['Pump.Fun: Sell'] },
        { id: 't3', wallet_address: 'w1', sol: 3, ix_labels: ['Pump.Fun: Sell'] },
      ],
      { patternKeys },
    );
    expect(map.get('t1')).toBe('structural');
    expect(map.has('t2')).toBe(false);
    expect(map.get('t3')).toBe('wallet');
  });
});

describe('patternKeysFrom', () => {
  it('drops empty pattern arrays and de-dupes by content', () => {
    const keys = patternKeysFrom([['a', 'b'], [], ['a', 'b']]);
    expect(keys.size).toBe(1);
    expect(keys.has(JSON.stringify(['a', 'b']))).toBe(true);
  });
});

describe('classifyFlowTrades dump / working / fee rows', () => {
  const A = ['Compute Budget: SetComputeUnitLimit', 'Pump.Fun: Buy'];
  const A2 = ['Compute Budget: SetComputeUnitLimit', 'Pump.Fun: Sell'];

  it('contagion off does not forward-tag later trades of the same wallet', () => {
    const patternKeys = patternKeysFrom([A]);
    const out = classifyFlowTrades(
      [
        { wallet_address: 'w1', sol: 1, ix_labels: A },
        { wallet_address: 'w1', sol: 3, ix_labels: A2 },
      ],
      { patternKeys, contagion: false },
    );
    expect(out.map((t) => t.isTagged)).toEqual([true, false]);
  });

  it('a grain match hits every structure that hashes to that grain, not the exact sequence', () => {
    // Both A and A2 share program Pump.Fun + CU flag → `Pump.Fun|CU`.
    const out = classifyFlowTrades(
      [
        { wallet_address: 'w1', sol: 1, ix_labels: A },
        { wallet_address: 'w2', sol: 2, ix_labels: A2 },
        { wallet_address: 'w3', sol: 3, ix_labels: ['Axiom Trade: buy'] },
      ],
      { patternKeys: new Set(['Pump.Fun|CU']), match: 'grain', contagion: false },
    );
    expect(out.map((t) => t.isTagged)).toEqual([true, true, false]);
  });

  it('a bare program name hits every grain of that program', () => {
    const out = classifyFlowTrades(
      [
        { wallet_address: 'w1', sol: 1, ix_labels: A },
        { wallet_address: 'w2', sol: 2, ix_labels: ['Axiom Trade: buy'] },
      ],
      { patternKeys: new Set(['Pump.Fun']), match: 'grain', contagion: false },
    );
    expect(out.map((t) => t.isTagged)).toEqual([true, false]);
  });

  it('a pinned row matches only the tx that carries that budget', () => {
    const rows = [{ labels: [...A], cu_limit: 300_000 }];
    const patternKeys = patternKeysFrom([A]);
    const out = classifyFlowTrades(
      [
        { wallet_address: 'w1', sol: 1, ix_labels: A, cu_limit: 300_000 },
        { wallet_address: 'w2', sol: 2, ix_labels: A, cu_limit: 200_000 },
        { wallet_address: 'w3', sol: 3, ix_labels: A },
      ],
      { patternKeys, patternRows: rows, contagion: false },
    );
    expect(out.map((t) => t.isTagged)).toEqual([true, false, false]);
  });

  it('an unpinned row is a fee wildcard — every budget of that shape matches', () => {
    const rows = [{ labels: [...A] }];
    const patternKeys = patternKeysFrom([A]);
    const out = classifyFlowTrades(
      [
        { wallet_address: 'w1', sol: 1, ix_labels: A, cu_limit: 300_000 },
        { wallet_address: 'w2', sol: 2, ix_labels: A },
      ],
      { patternKeys, patternRows: rows, contagion: false },
    );
    expect(out.map((t) => t.isTagged)).toEqual([true, true]);
  });
});
