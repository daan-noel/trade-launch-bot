import { describe, expect, it } from 'vitest';
import type { UTCTimestamp } from 'lightweight-charts';

import { buildWalletMarkerDefs } from './TokenPriceChart';
import { COMPARE_MARKER_COLORS, compareWalletColor } from './constants';
import type { ChartTrade, OhlcBar, ProfileWalletInfo } from './types';

/**
 * The three marker tiers on Trader Analysis: focus > comparison set > crowd.
 * These assert the tier a wallet lands in and the silhouette/flags that carry it
 * — the part a chart screenshot can't regress-test.
 */

const BAR_TIME = 1000 as UTCTimestamp;

const bar: OhlcBar = {
  time: BAR_TIME,
  open: 1,
  high: 2,
  low: 0.5,
  close: 1.5,
  volume: 10,
  inflow: 6,
  outflow: 4,
  liquiditySol: 40,
};

const trade = (wallet: string, type: 'buy' | 'sell'): ChartTrade => ({
  block_time: new Date(BAR_TIME * 1000).toISOString(),
  price_per_token: 1,
  trade_type: type,
  token_amount: 100,
  wallet_address: wallet,
});

const wallet = (address: string, extra: Partial<ProfileWalletInfo> = {}): ProfileWalletInfo => ({
  address,
  label: address,
  color: '#111111',
  ...extra,
});

const defsFor = (trades: ChartTrade[], wallets: ProfileWalletInfo[]) =>
  buildWalletMarkerDefs(trades, wallets, [bar], 'time', 1);

describe('compareWalletColor', () => {
  it('keys the hue off the comparison SLOT, and cycles', () => {
    expect(compareWalletColor(0)).toBe(COMPARE_MARKER_COLORS[0]);
    expect(compareWalletColor(1)).toBe(COMPARE_MARKER_COLORS[1]);
    expect(compareWalletColor(COMPARE_MARKER_COLORS.length)).toBe(COMPARE_MARKER_COLORS[0]);
  });

  it('leaves a `mine` wallet on its own fixed color', () => {
    expect(compareWalletColor(0, { color: '#fbbf24', isMine: true })).toBe('#fbbf24');
    expect(compareWalletColor(0, { color: '#fbbf24' })).toBe(COMPARE_MARKER_COLORS[0]);
  });
});

describe('buildWalletMarkerDefs — comparison tier', () => {
  it('gives a compared wallet a square silhouette and the compared flag', () => {
    const [def] = defsFor([trade('C', 'buy')], [wallet('C', { isCompared: true })]);
    expect(def.shape).toBe('square');
    expect(def.compared).toBe(true);
    expect(def.highlighted).toBeFalsy();
  });

  it('keeps the class silhouette when a compared wallet is also mine or dev', () => {
    const defs = defsFor(
      [trade('M', 'buy'), trade('D', 'buy')],
      [wallet('M', { isCompared: true, isMine: true }), wallet('D', { isCompared: true, isDev: true })],
    );
    const byLetter = new Map(defs.map((d) => [d.letter, d]));
    expect(byLetter.get('★')!.shape).toBe('diamond');
    expect(byLetter.get('D')!.shape).toBe('triangle');
    // Class wins the shape, but both still carry the comparison ring/size tier.
    expect(defs.every((d) => d.compared)).toBe(true);
  });

  it('lets focus outrank comparison on the silhouette', () => {
    const [def] = defsFor(
      [trade('F', 'buy')],
      [wallet('F', { isHighlighted: true, isCompared: true })],
    );
    expect(def.shape).toBe('hexagon');
    expect(def.highlighted).toBe(true);
  });

  it('carries `dimmed` through to the marker def', () => {
    const [def] = defsFor([trade('X', 'buy')], [wallet('X', { dimmed: true })]);
    expect(def.dimmed).toBe(true);
    expect(def.compared).toBeFalsy();
  });

  it('stacks focus nearest the bar, then the comparison set, then the crowd', () => {
    const defs = defsFor(
      [trade('X', 'buy'), trade('C', 'buy'), trade('F', 'buy')],
      [
        wallet('X', { dimmed: true }),
        wallet('C', { isCompared: true }),
        wallet('F', { isHighlighted: true }),
      ],
    );
    const order = [...defs].sort((a, b) => a.stackIndex - b.stackIndex).map((d) => d.letter);
    expect(order).toEqual(['F', 'C', 'X']);
  });

  it('stacks each direction independently', () => {
    const defs = defsFor(
      [trade('C', 'sell'), trade('F', 'sell')],
      [wallet('C', { isCompared: true }), wallet('F', { isHighlighted: true })],
    );
    expect(defs.every((d) => d.type === 'sell')).toBe(true);
    expect([...defs].sort((a, b) => a.stackIndex - b.stackIndex).map((d) => d.letter)).toEqual([
      'F',
      'C',
    ]);
  });
});
