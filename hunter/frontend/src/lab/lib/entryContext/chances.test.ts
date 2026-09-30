import { describe, expect, it } from 'vitest';
import { findChances, gapStats, pointPauses, suggestPause } from './chances';
import type { ScanMoment } from './types';

const T0 = Date.parse('2026-09-20T00:00:00Z');

/** A buy on `mint` at `sec` seconds; `ok` says whether it passes the filters. */
const pt = (mint: string, sec: number, ok: boolean, unknown = false, txIndex = 0): ScanMoment & { ok: boolean } => ({
  mint_address: mint,
  slot: Math.floor(sec / 0.4),
  tx_index: txIndex,
  at: new Date(T0 + sec * 1000).toISOString(),
  sol: 0,
  ...(unknown ? { unknown_reason: 'tape-truncated' as const } : {}),
  window: undefined as never,
  control: undefined as never,
  groups: [],
  groups_omitted: 0,
  probe: undefined as never,
  ret_pct: [null, null],
  ok,
});

const isPoint = (m: ScanMoment) => (m as ScanMoment & { ok: boolean }).ok;
const sizes = (ms: ScanMoment[], pause: number) => findChances(ms, isPoint, pause).map((c) => c.points.length);

describe('findChances', () => {
  // The worked example: points at 10 / 18 / 25, a fail at 40, points at 55 / 70, a fail at 90.
  const coin = [pt('A', 10, true), pt('A', 18, true), pt('A', 25, true), pt('A', 40, false),
    pt('A', 55, true), pt('A', 70, true), pt('A', 90, false)];

  it('a run of points is one chance, and a fail ends it', () => {
    expect(sizes(coin, 300)).toEqual([3, 2]);
  });

  it('says what ended each chance', () => {
    expect(findChances(coin, isPoint, 300).map((c) => c.end)).toEqual(['fail', 'fail']);
    expect(findChances(coin, isPoint, 10).map((c) => c.end)).toEqual(['fail', 'pause', 'fail']);
    expect(findChances(coin.slice(0, 3), isPoint, 300).map((c) => c.end)).toEqual(['last']);
  });

  it('with fails off, only the pause ends a chance', () => {
    const f = (pause: number) => findChances(coin, isPoint, pause, false);
    expect(f(300).map((c) => c.points.length)).toEqual([5]);
    expect(f(20).map((c) => [c.points.length, c.end])).toEqual([[3, 'pause'], [2, 'last']]);
    expect(pointPauses(coin, isPoint, false)).toEqual([8, 7, 30, 15]);
  });

  it('a pause longer than the limit starts a new chance', () => {
    // 10 -> 18 -> 25 are 8 s and 7 s apart; 55 -> 70 is 15 s.
    expect(sizes(coin, 10)).toEqual([3, 1, 1]);
    expect(sizes(coin, 5)).toEqual([1, 1, 1, 1, 1]);
  });

  it('keeps points of one slot in one chance: their times are milliseconds apart', () => {
    // Five buys in one slot, 3-12 ms apart, as the tape stamps them.
    const ms = [0, 0.003, 0.007, 0.01, 0.012].map((d, i) => pt('A', 10 + d, true, false, i));
    expect(sizes(ms, 1)).toEqual([5]);
  });

  it('reads each token in tape order, whatever order the points arrive in', () => {
    expect(sizes([...coin].reverse(), 300)).toEqual([3, 2]);
  });

  it('keeps tokens apart', () => {
    const ms = [pt('A', 10, true), pt('B', 12, true), pt('A', 14, true)];
    const cs = findChances(ms, isPoint, 300);
    expect(cs.map((c) => [c.mint_address, c.points.length])).toEqual([['A', 2], ['B', 1]]);
  });

  it('skips a buy the scan could not read', () => {
    const ms = [pt('A', 10, true), pt('A', 12, false, true), pt('A', 14, true)];
    expect(sizes(ms, 300)).toEqual([2]);
  });
});

describe('pauses', () => {
  it('are the gaps between points in a row, never across a fail', () => {
    const ms = [pt('A', 10, true), pt('A', 18, true), pt('A', 40, false), pt('A', 55, true), pt('A', 70, true)];
    expect(pointPauses(ms, isPoint)).toEqual([8, 15]);
    expect(gapStats([8, 15, 2])).toEqual({ n: 3, min: 2, median: 8, max: 15 });
    expect(gapStats([])).toBeNull();
  });
});

describe('suggestPause', () => {
  it('is the first step where the count stops moving', () => {
    // The 8dtx 6Vo3 hour (Target tx % > 50): 248 at 20 s, 234 from 30 s on.
    const counts = [[1, 717], [2, 552], [5, 382], [10, 294], [20, 248], [30, 234], [60, 234]] as const;
    expect(suggestPause(counts)).toBe(30);
  });

  it('is null when the count never settles or there are no chances', () => {
    expect(suggestPause([[1, 100], [2, 50], [5, 20]])).toBeNull();
    expect(suggestPause([[1, 0], [2, 0]])).toBeNull();
  });
});
