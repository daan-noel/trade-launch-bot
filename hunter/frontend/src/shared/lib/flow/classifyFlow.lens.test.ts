import { describe, expect, it } from 'vitest';

import { classifyFlowTrades, patternKeysFrom } from './classifyFlow';

const keys = patternKeysFrom([['A']]);

/** Two trades by the same wallet: the first matches structurally, the second
 *  does not — the whole difference contagion makes. */
const trades = [
  { wallet_address: 'w1', sol: 1, ix_labels: ['A'] },
  { wallet_address: 'w1', sol: 2, ix_labels: ['Z'] },
  { wallet_address: 'creator', sol: 3, ix_labels: ['Z'] },
];

describe('classifyFlowTrades contagion', () => {
  it('tags a wallet forward by default — the engine’s own rule', () => {
    const out = classifyFlowTrades(trades, { patternKeys: keys, creatorWallet: 'creator' });
    expect(out.map((t) => t.reason)).toEqual(['structural', 'wallet', 'creator']);
  });

  it('judges each trade by its own structure when contagion is off', () => {
    const out = classifyFlowTrades(trades, {
      patternKeys: keys,
      creatorWallet: 'creator',
      contagion: false,
    });
    expect(out.map((t) => t.isVol)).toEqual([true, false, false]);
    expect(out.map((t) => t.reason)).toEqual(['structural', null, null]);
  });

  it('never classifies an excluded wallet, structural match or not', () => {
    const out = classifyFlowTrades(trades, {
      patternKeys: keys,
      excludeWallets: new Set(['w1']),
      contagion: false,
    });
    expect(out.map((t) => t.isVol)).toEqual([false, false, false]);
    expect(out[0].nonVolSol).toBe(1);
  });

  it('an excluded wallet cannot seed contagion either', () => {
    const out = classifyFlowTrades(
      [...trades, { wallet_address: 'w2', sol: 4, ix_labels: ['Z'] }],
      { patternKeys: keys, excludeWallets: new Set(['w1']) },
    );
    expect(out.map((t) => t.isVol)).toEqual([false, false, false, false]);
  });
});
