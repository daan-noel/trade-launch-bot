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
    expect(out.map((t) => t.isTagged)).toEqual([true, false, false]);
    expect(out.map((t) => t.reason)).toEqual(['structural', null, null]);
  });

  it('never classifies an excluded wallet, structural match or not', () => {
    const out = classifyFlowTrades(trades, {
      patternKeys: keys,
      excludeWallets: new Set(['w1']),
      contagion: false,
    });
    expect(out.map((t) => t.isTagged)).toEqual([false, false, false]);
    expect(out[0].untaggedSol).toBe(1);
  });

  it('an excluded wallet cannot seed contagion either', () => {
    const out = classifyFlowTrades(
      [...trades, { wallet_address: 'w2', sol: 4, ix_labels: ['Z'] }],
      { patternKeys: keys, excludeWallets: new Set(['w1']) },
    );
    expect(out.map((t) => t.isTagged)).toEqual([false, false, false, false]);
  });
});

/** The same structure `A` on both legs — what a real aggregator pattern looks
 *  like, since `ix_labels` carry no direction. */
const bothLegs = [
  { wallet_address: 'w1', sol: 1, ix_labels: ['A'], side: 'buy' as const },
  { wallet_address: 'w2', sol: 2, ix_labels: ['A'], side: 'sell' as const },
  { wallet_address: 'w3', sol: 4, ix_labels: ['Z'], side: 'buy' as const },
];

describe('classifyFlowTrades side narrowing', () => {
  it('counts both legs of one pattern when unnarrowed', () => {
    const out = classifyFlowTrades(bothLegs, { patternKeys: keys, contagion: false });
    expect(out.map((t) => t.isTagged)).toEqual([true, true, false]);
  });

  it('keeps only the asked leg, and books the other as non-volume', () => {
    const buys = classifyFlowTrades(bothLegs, {
      patternKeys: keys,
      contagion: false,
      side: 'buy',
    });
    expect(buys.map((t) => t.isTagged)).toEqual([true, false, false]);
    expect(buys[1].untaggedSol).toBe(2);

    const sells = classifyFlowTrades(bothLegs, {
      patternKeys: keys,
      contagion: false,
      side: 'sell',
    });
    expect(sells.map((t) => t.isTagged)).toEqual([false, true, false]);
  });

  it('an off-side trade cannot seed contagion', () => {
    const out = classifyFlowTrades(
      [...bothLegs, { wallet_address: 'w2', sol: 8, ix_labels: ['Z'], side: 'buy' as const }],
      { patternKeys: keys, side: 'buy' },
    );
    // w2's only match is its SELL, which the lens is not asking about — so its
    // later buy stays organic.
    expect(out.map((t) => t.isTagged)).toEqual([true, false, false, false]);
  });

  it('a trade with no side is off-side under any narrowing', () => {
    const out = classifyFlowTrades([{ wallet_address: 'w1', sol: 1, ix_labels: ['A'] }], {
      patternKeys: keys,
      side: 'buy',
    });
    expect(out[0].isTagged).toBe(false);
  });
});
