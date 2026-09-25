import { describe, expect, it } from 'vitest';

import { classifyFlowTrades, type FlowTag } from './classifyFlow';

/** A flow lens reads an analysis-owned set as a tag: its sticky flag is the lens'
 *  wallet-rule switch and its side the lens' leg narrowing. */
const lensTag = (opts: Partial<FlowTag> = {}): FlowTag => ({
  name: 'lens',
  match: { ix_shape: [{ labels: ['A'] }] },
  side: null,
  sticky: false,
  exclude_creation_slot: false,
  ...opts,
});

const trades = [
  { wallet_address: 'w1', sol: 1, ix_labels: ['A'], side: 'buy' as const },
  { wallet_address: 'w1', sol: 2, ix_labels: ['Z'], side: 'buy' as const },
  { wallet_address: 'w2', sol: 4, ix_labels: ['A'], side: 'sell' as const },
];

describe('lens as a tag', () => {
  it('sticky carries a wallet forward; off, each trade is judged alone', () => {
    expect(classifyFlowTrades(trades, { tag: lensTag({ sticky: true }) }).map((t) => t.reason)).toEqual([
      'ix_shape',
      'sticky',
      'ix_shape',
    ]);
    expect(classifyFlowTrades(trades, { tag: lensTag() }).map((t) => t.isTagged)).toEqual([true, false, true]);
  });

  it('a side keeps only the asked leg, and an off-side match seeds nothing', () => {
    const out = classifyFlowTrades([...trades, { wallet_address: 'w2', sol: 8, ix_labels: ['Z'], side: 'buy' as const }], {
      tag: lensTag({ side: 'buy', sticky: true }),
    });
    expect(out.map((t) => t.isTagged)).toEqual([true, true, false, false]);
    expect(out[2].untaggedSol).toBe(4);
  });

  it('a trade with no side is off-side under a sided tag', () => {
    const out = classifyFlowTrades([{ wallet_address: 'w1', sol: 1, ix_labels: ['A'] }], {
      tag: lensTag({ side: 'buy' }),
    });
    expect(out[0].isTagged).toBe(false);
  });

  it('never classifies an excluded wallet, and it seeds nothing', () => {
    const out = classifyFlowTrades(trades, { tag: lensTag({ sticky: true }), excludeWallets: new Set(['w1']) });
    expect(out.map((t) => t.isTagged)).toEqual([false, false, true]);
    expect(out[0].untaggedSol).toBe(1);
  });
});
