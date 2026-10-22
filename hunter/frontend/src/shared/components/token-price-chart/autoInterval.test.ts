import { describe, expect, it } from 'vitest';
import { pickAutoInterval } from './autoInterval';

describe('pickAutoInterval', () => {
  it('keeps 1s while the life fits the width', () => {
    expect(pickAutoInterval(300, 1200)).toBe('1s');
  });
  it('coarsens as the life outgrows the width', () => {
    expect(pickAutoInterval(3600, 1200)).toBe('30s');
    expect(pickAutoInterval(6 * 3600, 1200)).toBe('1m');
    expect(pickAutoInterval(24 * 3600, 1200)).toBe('5m');
  });
  it('caps at the coarsest interval', () => {
    expect(pickAutoInterval(30 * 86400, 600)).toBe('5m');
  });
  it('keeps the finest interval without a span or width', () => {
    expect(pickAutoInterval(0, 1200)).toBe('1s');
    expect(pickAutoInterval(3600, 0)).toBe('1s');
  });
});

import { framedEndSec, lastBarIndexAtOrBefore } from './autoInterval';

describe('framedEndSec', () => {
  it('drops a sparse tail past the 99% rank', () => {
    const secs = [...Array.from({ length: 990 }, (_, i) => i), ...Array.from({ length: 10 }, (_, i) => 5000 + i)];
    expect(framedEndSec(secs)).toBe(989);
  });
  it('is the last print for a short tape', () => {
    expect(framedEndSec([1, 2, 3])).toBe(3);
    expect(framedEndSec([])).toBe(0);
  });
});

describe('lastBarIndexAtOrBefore', () => {
  const bars = [{ time: 10 }, { time: 20 }, { time: 30 }];
  it('finds the bar holding the time', () => {
    expect(lastBarIndexAtOrBefore(bars, 25)).toBe(1);
    expect(lastBarIndexAtOrBefore(bars, 30)).toBe(2);
    expect(lastBarIndexAtOrBefore(bars, 5)).toBe(-1);
  });
});
