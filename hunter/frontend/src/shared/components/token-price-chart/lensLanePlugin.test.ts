import { describe, expect, it } from 'vitest';

import { laneRowAt, laneText, lensLaneHeight } from './lensLanePlugin';
import type { ChartTrade } from './types';

const t = (trade_type: 'buy' | 'sell', amount_sol: number): ChartTrade => ({
  block_time: '2026-01-01T00:00:00Z',
  price_per_token: 1e-7,
  trade_type,
  amount_sol,
});

describe('laneText', () => {
  it('prints one buy as its size and several as sum plus count', () => {
    expect(laneText([t('buy', 0.5)], 'buys')).toBe('0.500');
    expect(laneText([t('buy', 0.5), t('buy', 0.2)], 'buys')).toBe('0.700 (2)');
  });

  it('prints nothing for a sells-only bar unless every side is on', () => {
    expect(laneText([t('sell', 0.3)], 'buys')).toBeNull();
    expect(laneText([t('buy', 0.5), t('sell', 0.3)], 'all')).toBe('+0.500 −0.300');
  });

  it('prints nothing with sizes off', () => {
    expect(laneText([t('buy', 0.5)], 'off')).toBeNull();
  });
});

describe('lensLaneHeight', () => {
  it('is zero with no rows and grows with rows', () => {
    expect(lensLaneHeight(0, 400)).toBe(0);
    expect(lensLaneHeight(2, 400)).toBeGreaterThan(lensLaneHeight(1, 400));
  });

  it('never takes more than the lane share of the pane', () => {
    // 8 rows in a short pane squeeze instead of covering the candles.
    expect(lensLaneHeight(8, 200)).toBeLessThanOrEqual(200 * 0.4);
  });
});

describe('laneRowAt', () => {
  // 2 rows in a 400 px pane: 20 px rows, 2 px apart, the first at y = 355.
  it('finds the row under the pointer, top row first', () => {
    expect(laneRowAt(356, 2, 400)).toBe(0);
    expect(laneRowAt(386, 2, 400)).toBe(1);
  });

  it('is null over the candles and below the last row', () => {
    expect(laneRowAt(340, 2, 400)).toBeNull();
    expect(laneRowAt(399, 2, 400)).toBeNull();
    expect(laneRowAt(356, 0, 400)).toBeNull();
  });
});
