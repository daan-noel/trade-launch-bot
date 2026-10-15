import { describe, expect, it } from 'vitest';

import type { TokenHighlightItem } from 'components/tokens/useTokenHighlight';
import type { FlowDiscoveryTokenGross } from 'types';
import { lensHitItems, lensRoster, tokenLensHits } from './tokenLensHits';

const tok = (mint_address: string): FlowDiscoveryTokenGross => ({ mint_address, gross_sol: 1, n_trades: 1 });
const W: TokenHighlightItem = { kind: 'wallet', key: 'w1', color: '#fde047', label: 'w1' };
const S: TokenHighlightItem = {
  kind: 'structure',
  key: 'k',
  color: '#22d3ee',
  label: 's',
  row: { labels: ['Pump.Fun: Buy'] },
};
const tokens = [tok('a'), tok('b'), tok('c')];
const hits = { b: { 'wallet:w1': 2, 'structure:k': 1 }, c: { 'structure:k': 3 } };

describe('lensHitItems', () => {
  it('keys each item by its lens id and sends a structure as its ix_shape entry', () => {
    expect(lensHitItems([W, S])).toEqual([
      { kind: 'wallet', id: 'wallet:w1', wallet: 'w1' },
      { kind: 'structure', id: 'structure:k', row: ['Pump.Fun: Buy'] },
    ]);
  });
});

describe('lensRoster', () => {
  it('is the list as given without hits', () => {
    expect(lensRoster(tokens, [W, S], null, new Set(), true)).toBe(tokens);
  });
  it('keeps coins carrying any item, most items first, when no chip is on', () => {
    expect(lensRoster(tokens, [W, S], hits, new Set(), true).map((t) => t.mint_address)).toEqual(['b', 'c']);
  });
  it('keeps only coins carrying every chip switched on', () => {
    const on = new Set(['wallet:w1', 'structure:k']);
    expect(lensRoster(tokens, [W, S], hits, on, true).map((t) => t.mint_address)).toEqual(['b']);
  });
  it('shows every coin under All, still ordered by items carried', () => {
    expect(lensRoster(tokens, [W, S], hits, new Set(['wallet:w1']), false).map((t) => t.mint_address)).toEqual([
      'b',
      'c',
      'a',
    ]);
  });
});

describe('tokenLensHits', () => {
  it('lists the items a coin carries in arming order', () => {
    expect(tokenLensHits('b', [W, S], hits).map((h) => [h.item.key, h.trades])).toEqual([
      ['w1', 2],
      ['k', 1],
    ]);
    expect(tokenLensHits('a', [W, S], hits)).toEqual([]);
  });
});
