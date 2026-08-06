import { describe, expect, it } from 'vitest';
import { classifyFlowTrades, patternKeysFrom } from './classifyFlow';

/** Fixture mirrors `hunter_engine::metrics::flow_split` tests (Rust SSOT:
 *  hunter/engine/src/metrics/flow_split.rs) — the decision order and
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
    expect(out[0]).toMatchObject({ isVol: true, volSol: 1, nonVolSol: 0 });
    expect(out[1]).toMatchObject({ isVol: false, volSol: 0, nonVolSol: 2 });
  });

  it('forward-tags a wallet after its first volume trade — later non-matching trades from the same wallet still count as volume', () => {
    const trades = [
      { wallet_address: 'w1', sol: 1, ix_labels: A },
      { wallet_address: 'w1', sol: 3, ix_labels: B },
    ];
    const out = classifyFlowTrades(trades, { patternKeys });
    expect(out[0].isVol).toBe(true);
    expect(out[1]).toMatchObject({ isVol: true, volSol: 3, nonVolSol: 0 });
  });

  it('never retroactively reclassifies an earlier trade (forward-only contagion)', () => {
    const trades = [
      { wallet_address: 'w1', sol: 5, ix_labels: B },
      { wallet_address: 'w1', sol: 1, ix_labels: A },
    ];
    const out = classifyFlowTrades(trades, { patternKeys });
    expect(out[0]).toMatchObject({ isVol: false, nonVolSol: 5 });
    expect(out[1]).toMatchObject({ isVol: true, volSol: 1 });
  });

  it('creator wallet is always volume and seeds contagion', () => {
    const trades = [{ wallet_address: 'creator', sol: 4, ix_labels: B }];
    const out = classifyFlowTrades(trades, { patternKeys, creatorWallet: 'creator' });
    expect(out[0]).toMatchObject({ isVol: true, volSol: 4 });
  });

  it('missing/empty ix_labels never structurally match — organic unless wallet already tagged', () => {
    const trades = [{ wallet_address: 'w9', sol: 2, ix_labels: null }];
    const out = classifyFlowTrades(trades, { patternKeys });
    expect(out[0]).toMatchObject({ isVol: false, nonVolSol: 2 });
  });
});

describe('patternKeysFrom', () => {
  it('drops empty pattern arrays and de-dupes by content', () => {
    const keys = patternKeysFrom([['a', 'b'], [], ['a', 'b']]);
    expect(keys.size).toBe(1);
    expect(keys.has(JSON.stringify(['a', 'b']))).toBe(true);
  });
});
