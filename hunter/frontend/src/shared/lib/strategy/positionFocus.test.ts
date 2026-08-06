import { describe, expect, it } from 'vitest';
import {
  buildFocusTableFilters,
  filterRowsByFocus,
  focusToStructuredFilters,
  positionFocusKey,
  rowMatchesLens,
  sameLens,
  togglePositionFocus,
  type FocusTablePoint,
  type PositionFocusRow,
} from './positionFocus';

const base: PositionFocusRow = {
  id: 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee',
  mint_address: 'Mint111',
  fired: true,
  isOpen: false,
  isClosed: true,
  exit_reason: 'TakeProfit',
  pnl_sol: 0.5,
  pnl_pct: 12,
  hold_secs: 45,
  is_migrated: true,
};

describe('togglePositionFocus', () => {
  it('stacks different kinds and toggles the same lens off', () => {
    let lenses = togglePositionFocus([], { kind: 'status', status: 'open' });
    lenses = togglePositionFocus(lenses, { kind: 'outcome', outcome: 'win' });
    expect(lenses).toHaveLength(2);
    lenses = togglePositionFocus(lenses, { kind: 'status', status: 'open' });
    expect(lenses).toEqual([{ kind: 'outcome', outcome: 'win' }]);
  });

  it('replaces the same kind', () => {
    const lenses = togglePositionFocus(
      [{ kind: 'status', status: 'open' }],
      { kind: 'status', status: 'closed' },
    );
    expect(lenses).toEqual([{ kind: 'status', status: 'closed' }]);
  });
});

describe('rowMatchesLens', () => {
  it('matches exit / outcome / migrated', () => {
    expect(rowMatchesLens(base, { kind: 'exit', reason: 'TakeProfit' })).toBe(true);
    expect(rowMatchesLens(base, { kind: 'outcome', outcome: 'win' })).toBe(true);
    expect(rowMatchesLens(base, { kind: 'migrated', migrated: true })).toBe(true);
    expect(rowMatchesLens(base, { kind: 'status', status: 'closed' })).toBe(true);
  });

  it('filters a stacked cohort', () => {
    const rows: PositionFocusRow[] = [
      base,
      { ...base, id: '2', pnl_sol: -0.1, pnl_pct: -5, exit_reason: 'StopLoss', is_migrated: false },
    ];
    const out = filterRowsByFocus(rows, [
      { kind: 'outcome', outcome: 'win' },
      { kind: 'migrated', migrated: true },
    ]);
    expect(out).toHaveLength(1);
    expect(out[0]!.id).toBe(base.id);
  });
});

describe('sameLens / key', () => {
  it('keys are stable', () => {
    const a = { kind: 'pct' as const, lo: 0, hi: 10 };
    const b = { kind: 'pct' as const, lo: 0, hi: 10 };
    expect(sameLens(a, b)).toBe(true);
    expect(positionFocusKey(a)).toBe('pct:0:10');
  });
});

describe('pct lens → structured filter', () => {
  it('maps open tails with lt/gte (not skipped as non-finite)', () => {
    expect(focusToStructuredFilters([{ kind: 'pct', lo: -Infinity, hi: -50 }])).toEqual({
      pnl_pct: { op: 'lt', val: -50 },
    });
    expect(focusToStructuredFilters([{ kind: 'pct', lo: 500, hi: Infinity }])).toEqual({
      pnl_pct: { op: 'gte', val: 500 },
    });
  });

  it('maps closed buckets half-open via between on pnl_pct (Evidence column key)', () => {
    const f = focusToStructuredFilters([{ kind: 'pct', lo: -10, hi: 0 }]);
    expect(f.pnl_pct?.op).toBe('between');
    if (f.pnl_pct?.op === 'between') {
      expect(f.pnl_pct.min).toBe(-10);
      expect(f.pnl_pct.max).toBeLessThan(0);
    }
  });

  it('matches open-tail rows client-side', () => {
    const deepLoss = { ...base, pnl_pct: -80 };
    const edge = { ...base, pnl_pct: -50 };
    expect(rowMatchesLens(deepLoss, { kind: 'pct', lo: -Infinity, hi: -50 })).toBe(true);
    expect(rowMatchesLens(edge, { kind: 'pct', lo: -Infinity, hi: -50 })).toBe(false);
  });
});

describe('exit Metric± → structured filter', () => {
  it('keeps the win/loss cut on the wire', () => {
    expect(focusToStructuredFilters([{ kind: 'exit', reason: 'Metric+' }])).toEqual({
      exit_reason: { op: 'contains', val: 'Metrics' },
      pnl_sol: { op: 'gt', val: 0 },
    });
    expect(focusToStructuredFilters([{ kind: 'exit', reason: 'Metric-' }])).toEqual({
      exit_reason: { op: 'contains', val: 'Metrics' },
      pnl_sol: { op: 'lte', val: 0 },
    });
  });
});

describe('status / pos / band structured filters', () => {
  it('maps Evidence open/closed/fired', () => {
    expect(focusToStructuredFilters([{ kind: 'status', status: 'open' }])).toEqual({
      status: { op: 'neq', val: 'End' },
    });
    expect(focusToStructuredFilters([{ kind: 'status', status: 'closed' }])).toEqual({
      status: { op: 'eq', val: 'End' },
    });
    expect(focusToStructuredFilters([{ kind: 'status', status: 'fired' }])).toEqual({
      exit_reason: { op: 'neq', val: 'NoEntry' },
    });
  });

  it('maps Simulate open/fired via exit_reason; closed is key-resolved', () => {
    expect(
      focusToStructuredFilters([{ kind: 'status', status: 'open' }], { mode: 'mint' }),
    ).toEqual({ exit_reason: { op: 'eq', val: 'Open' } });
    expect(
      focusToStructuredFilters([{ kind: 'status', status: 'fired' }], { mode: 'mint' }),
    ).toEqual({ exit_reason: { op: 'neq', val: 'NoEntry' } });
    expect(
      focusToStructuredFilters([{ kind: 'status', status: 'closed' }], { mode: 'mint' }),
    ).toEqual({});
  });

  it('does not let fired overwrite an exit needle', () => {
    expect(
      focusToStructuredFilters([
        { kind: 'exit', reason: 'TakeProfit' },
        { kind: 'status', status: 'fired' },
      ]),
    ).toEqual({ exit_reason: { op: 'contains', val: 'TakeProfit' } });
  });

  it('maps Evidence pos to id and Simulate pos to mint+entry_time', () => {
    expect(
      focusToStructuredFilters([{ kind: 'pos', positionId: 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee' }]),
    ).toEqual({ id: { op: 'eq', val: 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee' } });
    expect(
      focusToStructuredFilters(
        [{ kind: 'pos', positionId: 'MintAAA::2026-07-20T22:39:35Z' }],
        { mode: 'mint' },
      ),
    ).toEqual({
      mint_address: { op: 'eq', val: 'MintAAA' },
      entry_time: { op: 'eq', val: '2026-07-20T22:39:35Z' },
    });
  });

  it('maps Simulate band to holding + pnl_pct; Evidence band is key-resolved', () => {
    const band = {
      kind: 'band' as const,
      holdLo: 10,
      holdHi: 60,
      pctLo: -5,
      pctHi: 25,
    };
    expect(focusToStructuredFilters([band], { mode: 'mint' })).toEqual({
      holding: { op: 'between', min: 10, max: 60 },
      pnl_pct: { op: 'between', min: -5, max: 25 },
    });
    expect(focusToStructuredFilters([band], { mode: 'positionId' })).toEqual({});
  });
});

describe('buildFocusTableFilters key resolution', () => {
  const points: FocusTablePoint[] = [
    {
      key: 'id-tp',
      mint_address: 'M1',
      timeMs: Date.parse('2024-01-01T15:30:00.000Z'),
      pnlPct: 12,
      holdSeconds: 45,
      pnlSol: 0.5,
      exit_reason: 'TakeProfit',
      isOpen: false,
    },
    {
      key: 'id-other',
      mint_address: 'M2',
      timeMs: Date.parse('2024-01-01T16:30:00.000Z'),
      pnlPct: -8,
      holdSeconds: 20,
      pnlSol: -0.1,
      exit_reason: 'Migrated',
      isOpen: false,
    },
    {
      key: 'id-open',
      mint_address: 'M3',
      timeMs: Date.parse('2024-01-01T17:30:00.000Z'),
      pnlPct: 3,
      holdSeconds: 5,
      pnlSol: 0.05,
      exit_reason: null,
      isOpen: true,
    },
  ];

  it('resolves exit:Other to matching ids on Evidence', () => {
    const f = buildFocusTableFilters(
      [{ kind: 'exit', reason: 'Other' }],
      points,
      'America/New_York',
      'positionId',
    );
    expect(f.id).toEqual({ op: 'in', val: ['id-other'] });
  });

  it('resolves Evidence band via id in', () => {
    const f = buildFocusTableFilters(
      [{ kind: 'band', holdLo: 10, holdHi: 60, pctLo: 0, pctHi: 20 }],
      points,
      'UTC',
      'positionId',
    );
    expect(f.id).toEqual({ op: 'in', val: ['id-tp'] });
  });

  it('resolves Simulate closed via mint in', () => {
    const f = buildFocusTableFilters(
      [{ kind: 'status', status: 'closed' }],
      points,
      'UTC',
      'mint',
    );
    expect(f.mint_address?.op).toBe('in');
    if (f.mint_address?.op === 'in') {
      expect(f.mint_address.val).toEqual(expect.arrayContaining(['M1', 'M2']));
      expect(f.mint_address.val).not.toContain('M3');
    }
  });
});

describe('heat lens', () => {
  it('matches dow×hour in the given timezone', () => {
    // 2024-01-01 15:30 UTC = Mon 10:30 America/New_York (EST, UTC-5)
    const row: PositionFocusRow = {
      ...base,
      timeMs: Date.parse('2024-01-01T15:30:00.000Z'),
    };
    expect(
      rowMatchesLens(row, { kind: 'heat', dow: 1, hour: 10 }, { timeZone: 'America/New_York' }),
    ).toBe(true);
    expect(
      rowMatchesLens(row, { kind: 'heat', dow: 1, hour: 11 }, { timeZone: 'America/New_York' }),
    ).toBe(false);
  });
});

describe('calendar lenses', () => {
  const ny = { timeZone: 'America/New_York' };
  /** 2024-01-01 02:30 UTC is still Sun Dec 31 in New York (EST, UTC-5) — the
   *  calendar cell that produced the lens must win, not the UTC date. */
  const lateNight: PositionFocusRow = {
    ...base,
    timeMs: Date.parse('2024-01-01T02:30:00.000Z'),
  };

  it('day matches the civil day in the caller timezone', () => {
    expect(rowMatchesLens(lateNight, { kind: 'day', day: '2023-12-31' }, ny)).toBe(true);
    expect(rowMatchesLens(lateNight, { kind: 'day', day: '2024-01-01' }, ny)).toBe(false);
    expect(rowMatchesLens(lateNight, { kind: 'day', day: '2024-01-01' }, { timeZone: 'UTC' })).toBe(
      true,
    );
  });

  it('week spans its Sunday through Saturday, half-open', () => {
    // 2023-12-31 is a Sunday; the week covers 12-31 … 01-06.
    const week = { kind: 'week' as const, weekStart: '2023-12-31' };
    expect(rowMatchesLens(lateNight, week, ny)).toBe(true);
    const sat: PositionFocusRow = { ...base, timeMs: Date.parse('2024-01-06T18:00:00.000Z') };
    expect(rowMatchesLens(sat, week, ny)).toBe(true);
    const nextSun: PositionFocusRow = { ...base, timeMs: Date.parse('2024-01-07T18:00:00.000Z') };
    expect(rowMatchesLens(nextSun, week, ny)).toBe(false);
    const prevSat: PositionFocusRow = { ...base, timeMs: Date.parse('2023-12-30T18:00:00.000Z') };
    expect(rowMatchesLens(prevSat, week, ny)).toBe(false);
  });

  it('needs a timezone and a decision instant', () => {
    expect(rowMatchesLens(lateNight, { kind: 'day', day: '2023-12-31' })).toBe(false);
    expect(rowMatchesLens(base, { kind: 'day', day: '2023-12-31' }, ny)).toBe(false);
  });

  it('keys the calendar lenses stably', () => {
    expect(positionFocusKey({ kind: 'day', day: '2024-01-01' })).toBe('day:2024-01-01');
    expect(positionFocusKey({ kind: 'week', weekStart: '2023-12-31' })).toBe('week:2023-12-31');
    expect(
      sameLens({ kind: 'week', weekStart: '2023-12-31' }, { kind: 'week', weekStart: '2024-01-07' }),
    ).toBe(false);
  });
});
