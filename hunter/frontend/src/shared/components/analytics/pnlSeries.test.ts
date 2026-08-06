import { describe, expect, it } from 'vitest';
import { monthAbbr, shiftDayKey, summarizeDailyPnl, type PnlDay } from './pnlSeries';

function day(d: string, pnlSol: number, count = 1): PnlDay {
  return { day: d, pnlSol, count, wins: pnlSol > 0 ? count : 0 };
}

describe('shiftDayKey', () => {
  it('walks civil dates in both directions', () => {
    expect(shiftDayKey('2026-08-05', 1)).toBe('2026-08-06');
    expect(shiftDayKey('2026-08-05', -1)).toBe('2026-08-04');
    expect(shiftDayKey('2026-08-31', 1)).toBe('2026-09-01');
    expect(shiftDayKey('2026-03-01', -1)).toBe('2026-02-28');
  });

  it('survives a DST transition (the UTC-noon dodge)', () => {
    // US spring-forward 2026-03-08: a naive local-midnight + 24 h lands at 23:00
    // the same day, which would repeat a date. Noon anchoring cannot.
    expect(shiftDayKey('2026-03-07', 1)).toBe('2026-03-08');
    expect(shiftDayKey('2026-03-08', 1)).toBe('2026-03-09');
    expect(shiftDayKey('2026-11-01', 1)).toBe('2026-11-02');
  });

  it('is exact over a long walk (no accumulated drift)', () => {
    let k = '2026-01-01';
    for (let i = 0; i < 365; i++) k = shiftDayKey(k, 1);
    expect(k).toBe('2027-01-01');
  });
});

describe('monthAbbr', () => {
  it('maps a day key to its month', () => {
    expect(monthAbbr('2026-01-31')).toBe('Jan');
    expect(monthAbbr('2026-08-05')).toBe('Aug');
    expect(monthAbbr('2026-12-01')).toBe('Dec');
  });
});

describe('summarizeDailyPnl', () => {
  it('counts green days and finds both extremes', () => {
    const s = summarizeDailyPnl([
      day('2026-08-01', 1.5, 4),
      day('2026-08-02', -0.5, 2),
      day('2026-08-03', 3.25, 9),
      day('2026-08-04', -2.75, 6),
    ]);
    expect(s.tradedDays).toBe(4);
    expect(s.greenDays).toBe(2);
    expect(s.redDays).toBe(2);
    expect(s.best?.day).toBe('2026-08-03');
    expect(s.worst?.day).toBe('2026-08-04');
  });

  it('ignores days with no closes — an absence is not a flat day', () => {
    const s = summarizeDailyPnl([day('2026-08-01', 0, 0), day('2026-08-02', 1, 3)]);
    expect(s.tradedDays).toBe(1);
    expect(s.greenDays).toBe(1);
    expect(s.best?.day).toBe('2026-08-02');
  });

  it('does not count a flat traded day as green or red', () => {
    const s = summarizeDailyPnl([day('2026-08-01', 0, 5)]);
    expect(s.tradedDays).toBe(1);
    expect(s.greenDays).toBe(0);
    expect(s.redDays).toBe(0);
  });

  it('takes the longest red run, and a green day breaks it', () => {
    const s = summarizeDailyPnl([
      day('2026-08-01', -1),
      day('2026-08-02', -1),
      day('2026-08-03', 1),
      day('2026-08-04', -1),
      day('2026-08-05', -1),
      day('2026-08-06', -1),
    ]);
    expect(s.longestRedStreak).toBe(3);
  });

  it('a no-trade gap does not break a red run', () => {
    // `buildDailyPnl` only emits days that traded, so a quiet stretch is simply
    // absent — it must not read as a recovery.
    const s = summarizeDailyPnl([
      day('2026-08-01', -1),
      day('2026-08-05', -1),
      day('2026-08-09', -1),
    ]);
    expect(s.longestRedStreak).toBe(3);
  });

  it('is empty-safe', () => {
    expect(summarizeDailyPnl([])).toEqual({
      tradedDays: 0,
      greenDays: 0,
      redDays: 0,
      best: null,
      worst: null,
      longestRedStreak: 0,
    });
  });
});
