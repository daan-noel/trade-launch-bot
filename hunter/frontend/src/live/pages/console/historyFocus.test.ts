import { describe, expect, it } from 'vitest';
import {
  dayBoundsUtcIso,
  historyFocusLabel,
  intersectUtcWindow,
  matchesHeatFocus,
  matchesHoldBandFocus,
  parseHistoryFocus,
  pctFocusFilter,
  serializeHistoryFocus,
  toggleHistoryFocus,
  weekBoundsUtcIso,
} from './historyFocus';

describe('parseHistoryFocus / serializeHistoryFocus', () => {
  it('round-trips each kind', () => {
    const cases = [
      { kind: 'day' as const, day: '2026-08-05' },
      { kind: 'heat' as const, dow: 2, hour: 14 },
      { kind: 'pct' as const, lo: -50, hi: -20 },
      { kind: 'week' as const, weekStart: '2026-08-02' },
      { kind: 'pct' as const, lo: -Infinity, hi: -50 },
      { kind: 'pct' as const, lo: 50, hi: 100 },
      { kind: 'pct' as const, lo: 500, hi: Infinity },
      { kind: 'rule' as const, ruleId: '01234567-89ab-cdef-0123-456789abcdef' },
      { kind: 'pos' as const, positionId: '01234567-89ab-cdef-0123-456789abcdef' },
      {
        kind: 'holdBand' as const,
        holdLo: 5,
        holdHi: 120,
        pctLo: -20,
        pctHi: 50,
      },
    ];
    for (const f of cases) {
      expect(parseHistoryFocus(serializeHistoryFocus(f))).toEqual(f);
    }
  });

  it('rejects unknown / malformed wire', () => {
    expect(parseHistoryFocus(null)).toBeNull();
    expect(parseHistoryFocus('day:nope')).toBeNull();
    expect(parseHistoryFocus('week:2026-8-2')).toBeNull();
    expect(parseHistoryFocus('heat:9:14')).toBeNull();
    expect(parseHistoryFocus('pct:1:2')).toBeNull(); // not a histogram edge pair
    expect(parseHistoryFocus('pct:50:inf')).toBeNull(); // old open-tail; not adjacent now
    expect(parseHistoryFocus('rule:not-a-uuid')).toBeNull();
    expect(parseHistoryFocus('pos:not-a-uuid')).toBeNull();
  });
});

describe('toggleHistoryFocus', () => {
  it('sets on empty, clears on same, replaces on different', () => {
    const a = { kind: 'day' as const, day: '2026-08-05' };
    const b = { kind: 'day' as const, day: '2026-08-06' };
    expect(toggleHistoryFocus(null, a)).toEqual(a);
    expect(toggleHistoryFocus(a, a)).toBeNull();
    expect(toggleHistoryFocus(a, b)).toEqual(b);
  });
});

describe('historyFocusLabel', () => {
  it('formats each kind', () => {
    expect(historyFocusLabel({ kind: 'day', day: '2026-08-05' })).toBe('2026-08-05');
    expect(historyFocusLabel({ kind: 'heat', dow: 1, hour: 9 })).toBe('Mon 09:00');
    expect(historyFocusLabel({ kind: 'pct', lo: -Infinity, hi: -50 })).toBe('< -50%');
    expect(historyFocusLabel({ kind: 'pct', lo: 10, hi: 20 })).toBe('10…20%');
    expect(historyFocusLabel({ kind: 'pct', lo: 500, hi: Infinity })).toBe('≥ 500%');
    expect(
      historyFocusLabel(
        { kind: 'rule', ruleId: '01234567-89ab-cdef-0123-456789abcdef' },
        () => 'scalper',
      ),
    ).toBe('scalper');
  });
});

describe('dayBoundsUtcIso', () => {
  it('returns a half-open UTC window covering the civil day in the zone', () => {
    const { fromIso, toIso } = dayBoundsUtcIso('2026-08-05', 'America/New_York');
    const from = Date.parse(fromIso);
    const to = Date.parse(toIso);
    expect(to - from).toBe(86_400_000);
    // 2026-08-05 00:00 EDT = 04:00 UTC
    expect(fromIso.startsWith('2026-08-05T04:00:00')).toBe(true);
  });
});

describe('weekBoundsUtcIso', () => {
  it('covers exactly the 7 days from the given Sunday', () => {
    const { fromIso, toIso } = weekBoundsUtcIso('2026-08-02', 'America/New_York');
    expect(fromIso.startsWith('2026-08-02T04:00:00')).toBe(true);
    expect(Date.parse(toIso) - Date.parse(fromIso)).toBe(7 * 86_400_000);
  });

  it('starts where the first day starts and ends where the last day ends', () => {
    // The week lens must be the union of its 7 day lenses, never an off-by-one.
    const week = weekBoundsUtcIso('2026-08-02', 'UTC');
    expect(week.fromIso).toBe(dayBoundsUtcIso('2026-08-02', 'UTC').fromIso);
    expect(week.toIso).toBe(dayBoundsUtcIso('2026-08-08', 'UTC').toIso);
  });
});

describe('intersectUtcWindow', () => {
  it('intersects and detects empty', () => {
    const hit = intersectUtcWindow(
      '2026-08-01T00:00:00.000Z',
      '2026-08-10T00:00:00.000Z',
      '2026-08-05T00:00:00.000Z',
      '2026-08-06T00:00:00.000Z',
    );
    expect(hit).toEqual({
      fromIso: '2026-08-05T00:00:00.000Z',
      toIso: '2026-08-06T00:00:00.000Z',
    });
    expect(
      intersectUtcWindow(
        '2026-08-01T00:00:00.000Z',
        '2026-08-02T00:00:00.000Z',
        '2026-08-05T00:00:00.000Z',
        '2026-08-06T00:00:00.000Z',
      ),
    ).toBeNull();
  });
});

describe('pctFocusFilter', () => {
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
});

describe('matchesHeatFocus', () => {
  it('matches dow×hour in the given timezone', () => {
    // 2026-08-03 is a Monday; 18:30 UTC = 14:30 America/New_York (EDT).
    const focus = { kind: 'heat' as const, dow: 1, hour: 14 };
    expect(matchesHeatFocus('2026-08-03T18:30:00.000Z', focus, 'America/New_York')).toBe(true);
    expect(matchesHeatFocus('2026-08-03T18:30:00.000Z', focus, 'UTC')).toBe(false);
  });
});

describe('matchesHoldBandFocus', () => {
  it('matches hold seconds and pnl% inside the brush', () => {
    const focus = { kind: 'holdBand' as const, holdLo: 10, holdHi: 60, pctLo: -5, pctHi: 25 };
    expect(
      matchesHoldBandFocus(
        '2026-08-03T12:00:00.000Z',
        '2026-08-03T12:00:30.000Z', // 30s
        10,
        focus,
      ),
    ).toBe(true);
    expect(
      matchesHoldBandFocus(
        '2026-08-03T12:00:00.000Z',
        '2026-08-03T12:02:00.000Z', // 120s
        10,
        focus,
      ),
    ).toBe(false);
  });
});
