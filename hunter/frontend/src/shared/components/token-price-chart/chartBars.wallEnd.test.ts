import { describe, expect, it } from 'vitest';

import { buildBarWallEndSec } from './chartBars';
import type { ChartTrade, OhlcBar } from './types';

/**
 * `buildBarWallEndSec` is what lets an off-chart surface answer "what was true at
 * this candle". The case that matters is a bar holding NO trade: those used to
 * resolve to `null`, which downstream is indistinguishable from "the pointer is not
 * on the plot", so the rule-condition strip fell back to its pinned exit readout and
 * every gap bar reported the same borrowed numbers.
 */

function bar(time: number): OhlcBar {
  return {
    time: time as OhlcBar['time'],
    open: 1,
    high: 1,
    low: 1,
    close: 1,
    volume: 0,
    inflow: 0,
    outflow: 0,
    liquiditySol: null,
  };
}

const ISO = (sec: string) => `2026-08-13T11:59:${sec}Z`;
const T = (sec: string) => Date.parse(ISO(sec)) / 1000;

function trade(slot: number, sec: string): ChartTrade {
  return { block_time: ISO(sec), price_per_token: 1, trade_type: 'buy', slot };
}

describe('buildBarWallEndSec', () => {
  it('gives a time bar the last instant it covers, not its start', () => {
    const bars = [bar(1000), bar(1001)];
    const map = buildBarWallEndSec(bars, [], 'time', 1);
    // The bar's start is a moment its own trades had not happened yet.
    expect(map.get(1000)).toBeGreaterThan(1000);
    expect(map.get(1000)).toBeLessThan(1001);
  });

  it('resolves EVERY slot bar, including the ones holding no trade', () => {
    const trades = [
      trade(100, '48.240'),
      trade(104, '50.078'),
      trade(108, '53.723'),
    ];
    const bars = [100, 101, 102, 103, 104, 105, 106, 107, 108].map(bar);
    const map = buildBarWallEndSec(bars, trades, 'slot', 1);

    for (const b of bars) {
      expect(map.get(b.time as number), `slot ${b.time}`).toBeDefined();
    }
    // Anchors keep their real trade time.
    expect(map.get(100)).toBeCloseTo(T('48.240'), 3);
    expect(map.get(104)).toBeCloseTo(T('50.078'), 3);
    expect(map.get(108)).toBeCloseTo(T('53.723'), 3);
  });

  it('keeps empty slots strictly between their neighbours, in order', () => {
    const trades = [trade(100, '48.240'), trade(108, '53.723')];
    const bars = [100, 101, 102, 103, 104, 105, 106, 107, 108].map(bar);
    const map = buildBarWallEndSec(bars, trades, 'slot', 1);

    const seq = bars.map((b) => map.get(b.time as number) as number);
    for (let i = 1; i < seq.length; i++) {
      expect(seq[i], `slot ${100 + i} after ${99 + i}`).toBeGreaterThan(seq[i - 1]);
    }
    // Never past the next bar that does have a trade — a nominal slot duration is
    // not a promise, and overshooting would read a later state than the bar shows.
    for (const v of seq) expect(v).toBeLessThanOrEqual(T('53.723'));
  });

  it('places bars that precede the first anchor', () => {
    const trades = [trade(105, '50.000')];
    const bars = [102, 103, 104, 105].map(bar);
    const map = buildBarWallEndSec(bars, trades, 'slot', 1);
    expect(map.get(102)).toBeCloseTo(T('50.000') - 1.2, 3);
    expect(map.get(104)).toBeCloseTo(T('50.000') - 0.4, 3);
    expect(map.get(105)).toBeCloseTo(T('50.000'), 3);
  });
});
