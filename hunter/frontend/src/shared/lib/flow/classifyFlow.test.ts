import { describe, expect, it } from 'vitest';
import { classifyFlowTrades, patternKeysFrom } from './classifyFlow';

const A = ['buy', 'sell'];

describe('classifyFlowTrades', () => {
  const patternKeys = patternKeysFrom([A]);

  it('marks structural matches as volume', () => {
    const trades = [
      { wallet_address: 'w1', sol: 1, ix_labels: A },
      { wallet_address: 'w2', sol: 2, ix_labels: ['other'] },
    ];
    const out = classifyFlowTrades(trades, { patternKeys });
    expect(out[0].isVol).toBe(true);
    expect(out[1].isVol).toBe(false);
  });

  it('contagion tags later trades from a volume wallet', () => {
    const trades = [
      { wallet_address: 'w1', sol: 1, ix_labels: A },
      { wallet_address: 'w1', sol: 2, ix_labels: ['organic'] },
    ];
    const out = classifyFlowTrades(trades, { patternKeys });
    expect(out[0].isVol).toBe(true);
    expect(out[1].isVol).toBe(true);
  });

  it('seeds creator as volume', () => {
    const trades = [{ wallet_address: 'creator', sol: 1, ix_labels: ['organic'] }];
    const out = classifyFlowTrades(trades, { patternKeys, creatorWallet: 'creator' });
    expect(out[0].isVol).toBe(true);
  });

  it('leaves unmatched non-creator organic', () => {
    const trades = [{ wallet_address: 'w2', sol: 1, ix_labels: ['organic'] }];
    const out = classifyFlowTrades(trades, { patternKeys });
    expect(out[0].isVol).toBe(false);
  });
});

describe('patternKeysFrom', () => {
  it('dedupes and drops empty', () => {
    const keys = patternKeysFrom([['a', 'b'], [], ['a', 'b']]);
    expect(keys.size).toBe(1);
    expect(keys.has(JSON.stringify(['a', 'b']))).toBe(true);
  });
});
