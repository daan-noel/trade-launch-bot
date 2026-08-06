import { describe, expect, it } from 'vitest';
import {
  dayKeyInTz,
  foldPnlDeck,
  matchesPctFocus,
  monthAbbr,
  pctFocusFilter,
  pnlDistributionBuckets,
  shiftDayKey,
  summarizeDailyPnl,
  type PnlDay,
  type PnlPoint,
} from './pnlSeries';

function pt(partial: Partial<PnlPoint> & Pick<PnlPoint, 'key' | 'timeMs' | 'pnlSol'>): PnlPoint {
  return {
    pnlPct: null,
    label: partial.label ?? partial.key,
    ...partial,
  };
}

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

describe('dayKeyInTz', () => {
  it('returns a stable ISO day key', () => {
    // 2026-08-05 18:00 UTC → still 2026-08-05 in America/New_York (UTC-4).
    expect(dayKeyInTz(Date.parse('2026-08-05T18:00:00Z'), 'America/New_York')).toBe('2026-08-05');
    // Same instant is already the next civil day in Tokyo.
    expect(dayKeyInTz(Date.parse('2026-08-05T18:00:00Z'), 'Asia/Tokyo')).toBe('2026-08-06');
  });
});

describe('pnlDistributionBuckets', () => {
  it('bins via the edge grid (incl. open tails)', () => {
    const points = [
      pt({ key: 'a', timeMs: 1, pnlSol: -1, pnlPct: -80 }),
      pt({ key: 'b', timeMs: 2, pnlSol: -0.1, pnlPct: -5 }),
      pt({ key: 'c', timeMs: 3, pnlSol: 1, pnlPct: 15 }),
      pt({ key: 'd', timeMs: 4, pnlSol: 2, pnlPct: 600 }),
      pt({ key: 'e', timeMs: 5, pnlSol: 0, pnlPct: null }),
    ];
    const buckets = pnlDistributionBuckets(points, 'default');
    const byLabel = Object.fromEntries(buckets.map((b) => [b.label, b.count]));
    expect(byLabel['< -50%']).toBe(1);
    expect(byLabel['-10…0%']).toBe(1);
    expect(byLabel['10…20%']).toBe(1);
    expect(byLabel['≥ 500%']).toBe(1);
    expect(buckets.reduce((s, b) => s + b.count, 0)).toBe(4);
  });
});

describe('pctFocusFilter / matchesPctFocus', () => {
  it('maps open and closed edges', () => {
    expect(pctFocusFilter(-Infinity, -50)).toEqual({ op: 'lt', val: -50 });
    expect(pctFocusFilter(500, Infinity)).toEqual({ op: 'gte', val: 500 });
    const mid = pctFocusFilter(-10, 0);
    expect(mid.op).toBe('between');
    if (mid.op === 'between') {
      expect(mid.min).toBe(-10);
      expect(mid.max).toBeLessThan(0);
    }
  });

  it('mirrors half-open client edges', () => {
    expect(matchesPctFocus(-60, -Infinity, -50)).toBe(true);
    expect(matchesPctFocus(-50, -Infinity, -50)).toBe(false);
    expect(matchesPctFocus(500, 500, Infinity)).toBe(true);
    expect(matchesPctFocus(499, 500, Infinity)).toBe(false);
    expect(matchesPctFocus(-10, -10, 0)).toBe(true);
    expect(matchesPctFocus(0, -10, 0)).toBe(false);
  });
});

describe('foldPnlDeck', () => {
  it('builds curve, heat, daily, sparklines, and trends from one cohort', () => {
    const t0 = Date.parse('2026-08-01T15:00:00Z');
    const hour = 3_600_000;
    const points: PnlPoint[] = [];
    for (let i = 0; i < 40; i++) {
      points.push(
        pt({
          key: `r1-${i}`,
          timeMs: t0 + i * hour,
          pnlSol: i < 20 ? 0.1 : -0.05,
          pnlPct: i < 20 ? 10 : -5,
          groupId: 'rule-a',
          label: 'A',
        }),
      );
    }
    for (let i = 0; i < 5; i++) {
      points.push(
        pt({
          key: `r2-${i}`,
          timeMs: t0 + i * hour,
          pnlSol: 1,
          pnlPct: 50,
          groupId: 'rule-b',
          label: 'B',
        }),
      );
    }

    const fold = foldPnlDeck(points, {
      timeZone: 'UTC',
      density: 'sparse',
      labelOf: (id) => (id === 'rule-a' ? 'Alpha' : 'Beta'),
      window: 20,
    });

    expect(fold.curve.length).toBeGreaterThan(0);
    expect(fold.curve[fold.curve.length - 1]!.cumPnlSol).toBeCloseTo(20 * 0.1 + 20 * -0.05 + 5, 6);
    expect(fold.heatCells).toHaveLength(168);
    expect(fold.days.length).toBeGreaterThan(0);
    expect(fold.sparkByGroup.get('rule-a')?.length).toBeGreaterThan(0);
    expect(fold.trends.map((t) => t.groupId).sort()).toEqual(['rule-a', 'rule-b']);
    const alpha = fold.trends.find((t) => t.groupId === 'rule-a')!;
    expect(alpha.label).toBe('Alpha');
    expect(alpha.decaying).toBe(true);
  });
});

