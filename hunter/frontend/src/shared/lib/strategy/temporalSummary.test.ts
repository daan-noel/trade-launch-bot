import { describe, expect, it } from 'vitest';
import {
  avgPnlSol,
  bestPnlWallCell,
  buildTemporalSummary,
  floorToWallGrain,
  closedCount,
  formatWallClock,
  formatWallRange,
  formatWallSpan,
  holdBinsFor,
  isWallDayBreak,
  peakWallCell,
  pickHoldScheme,
  pickWallGrain,
  pnlHeatBackground,
  rebucketHold,
  rebucketWall,
  rowMatchesHoldBin,
  worstPnlWallCell,
  type TemporalRow,
} from './temporalSummary';

function row(partial: Partial<TemporalRow> & Pick<TemporalRow, 'mint_address' | 'exit'>): TemporalRow {
  return {
    fired: true,
    pnl_sol: 0,
    holding_secs: 0,
    entry_time: null,
    created_at: null,
    ...partial,
  };
}

const H = 3_600_000;
const D = 86_400_000;

describe('pnlHeatBackground', () => {
  it('uses a neutral wash for exact zero (not green)', () => {
    expect(pnlHeatBackground(0, 10, 3)).toBe('rgba(148,163,184,0.4)');
    expect(pnlHeatBackground(0, 10, 3)).not.toMatch(/34,197,94/);
  });

  it('keeps green for profit and red for loss', () => {
    expect(pnlHeatBackground(5, 10, 2)).toMatch(/^rgba\(34,197,94,/);
    expect(pnlHeatBackground(-5, 10, 2)).toMatch(/^rgba\(239,68,68,/);
  });
});

describe('pickWallGrain', () => {
  it('picks finer grains for short spans', () => {
    expect(pickWallGrain(2 * H)).toBe('30m');
    expect(pickWallGrain(12 * H)).toBe('1h');
    expect(pickWallGrain(2 * D)).toBe('2h');
    expect(pickWallGrain(5 * D)).toBe('4h');
    expect(pickWallGrain(14 * D)).toBe('day');
  });
});

describe('pickHoldScheme', () => {
  it('picks denser schemes for short closed holds', () => {
    expect(pickHoldScheme([5, 8, 12])).toBe('dense_15s');
    expect(pickHoldScheme([10, 20, 45])).toBe('dense_60s');
    expect(pickHoldScheme([60, 120, 240])).toBe('mid_5m');
    expect(pickHoldScheme([300, 600, 1200])).toBe('mid_30m');
    expect(pickHoldScheme([3600, 4000, 5000])).toBe('wide_2h');
    expect(pickHoldScheme([10_000, 20_000])).toBe('wide_day');
    expect(pickHoldScheme([])).toBe('mid_30m');
  });
});

describe('wall axis labels', () => {
  it('formats clock-first ticks and day breaks', () => {
    expect(formatWallClock('2026-07-15T14:30:00Z', '30m', 'UTC')).toMatch(/\d{2}:\d{2}/);
    // Noon UTC stays on the same local calendar day in common US/EU offsets.
    expect(isWallDayBreak('2026-07-16T12:00:00Z', '2026-07-15T12:00:00Z', 'UTC')).toBe(true);
    expect(isWallDayBreak('2026-07-15T16:00:00Z', '2026-07-15T14:00:00Z', 'UTC')).toBe(false);
  });

  it('builds a range caption from filled cells', () => {
    const rows: TemporalRow[] = [
      row({
        mint_address: 'a',
        exit: 'TakeProfit',
        holding_secs: 10,
        created_at: '2026-07-15T10:00:00Z',
      }),
      row({
        mint_address: 'b',
        exit: 'TakeProfit',
        holding_secs: 10,
        created_at: '2026-07-15T16:00:00Z',
      }),
    ];
    const t = buildTemporalSummary(rows, 'UTC', 'created_at', '1h');
    const range = formatWallRange(t.wall, t.wallGrain, 'UTC');
    expect(range).toContain('→');
  });
});

describe('buildTemporalSummary', () => {
  it('bins hold duration adaptively and stacks exits', () => {
    const rows: TemporalRow[] = [
      row({ mint_address: 'a', exit: 'TakeProfit', holding_secs: 10, pnl_sol: 1 }),
      row({ mint_address: 'b', exit: 'StopLoss', holding_secs: 20, pnl_sol: -0.5 }),
      row({ mint_address: 'c', exit: 'TakeProfit', holding_secs: 120, pnl_sol: 0.2 }),
      row({ mint_address: 'd', exit: 'Open', holding_secs: 0, pnl_sol: 0.1 }),
      row({ mint_address: 'e', exit: 'NoEntry', fired: false, holding_secs: 0, pnl_sol: 0 }),
    ];
    const t = buildTemporalSummary(rows, 'UTC');
    expect(t.nFired).toBe(4);
    // closed 10/20/120 → nearest-rank p90=120 → mid_5m
    expect(t.holdScheme).toBe('mid_5m');
    const bins = holdBinsFor('mid_5m');
    // 10s + 20s → <30s; 120s → 2–5m
    const under30 = t.hold.find((b) => b.id === 'hold_0_29')!;
    expect(under30.n).toBe(2);
    expect(under30.exits.n_exit_take_profit).toBe(1);
    expect(under30.exits.n_exit_stop_loss).toBe(1);
    expect(t.hold.find((b) => b.id === 'hold_120_299')!.n).toBe(1);
    const open = t.hold.find((b) => b.id === 'open')!;
    expect(open.n).toBe(1);
    expect(rowMatchesHoldBin(rows[0], 'hold_0_29', bins)).toBe(true);
  });

  it('uses dense_15s when all closed holds are sub-15s', () => {
    const rows: TemporalRow[] = [
      row({ mint_address: 'a', exit: 'TakeProfit', holding_secs: 2, pnl_sol: 1 }),
      row({ mint_address: 'b', exit: 'StopLoss', holding_secs: 7, pnl_sol: -0.5 }),
      row({ mint_address: 'c', exit: 'TakeProfit', holding_secs: 12, pnl_sol: 0.2 }),
    ];
    const t = buildTemporalSummary(rows, 'UTC');
    expect(t.holdScheme).toBe('dense_15s');
    expect(t.hold.find((b) => b.id === 'hold_0_2')!.n).toBe(1);
    expect(t.hold.find((b) => b.id === 'hold_6_9')!.n).toBe(1);
    expect(t.hold.find((b) => b.id === 'hold_10_14')!.n).toBe(1);
  });

  it('builds wall cells from created_at with adaptive 30m grain', () => {
    const rows: TemporalRow[] = [
      row({
        mint_address: 'a',
        exit: 'TakeProfit',
        holding_secs: 10,
        pnl_sol: 1,
        created_at: '2026-07-15T14:30:00Z',
      }),
      row({
        mint_address: 'b',
        exit: 'StopLoss',
        holding_secs: 20,
        pnl_sol: -0.5,
        created_at: '2026-07-15T14:45:00Z',
      }),
    ];
    const t = buildTemporalSummary(rows, 'UTC', 'created_at');
    expect(t.wallGrain).toBe('30m');
    expect(t.wallGrainAuto).toBe('30m');
    expect(t.wallSpanMs).toBe(15 * 60_000);
    const filled = t.wall.filter((c) => c.n > 0);
    expect(filled).toHaveLength(1);
    expect(filled[0].n).toBe(2);
    expect(filled[0].mints).toEqual(['a', 'b']);
    expect(peakWallCell(t.wall)?.n).toBe(2);
    expect(formatWallSpan(t.wallSpanMs)).toBe('15m');
  });

  it('picks best/worst PnL wall cells and avg helpers', () => {
    const rows: TemporalRow[] = [
      row({
        mint_address: 'a',
        exit: 'TakeProfit',
        holding_secs: 10,
        pnl_sol: 2,
        created_at: '2026-07-15T10:00:00Z',
      }),
      row({
        mint_address: 'b',
        exit: 'StopLoss',
        holding_secs: 20,
        pnl_sol: -1,
        created_at: '2026-07-15T16:00:00Z',
      }),
      row({
        mint_address: 'c',
        exit: 'Open',
        holding_secs: 0,
        pnl_sol: 0.25,
        created_at: '2026-07-15T16:30:00Z',
      }),
    ];
    const t = buildTemporalSummary(rows, 'UTC', 'created_at', '1h');
    const best = bestPnlWallCell(t.wall)!;
    const worst = worstPnlWallCell(t.wall)!;
    expect(best.pnl_sol).toBe(2);
    expect(worst.pnl_sol).toBe(-0.75);
    expect(avgPnlSol(best.pnl_sol, best.n)).toBe(2);
    expect(closedCount(worst.n, worst.exits)).toBe(1);
  });

  it('uses 1h grain when span is about a day', () => {
    const rows: TemporalRow[] = [
      row({
        mint_address: 'a',
        exit: 'TakeProfit',
        holding_secs: 10,
        created_at: '2026-07-15T01:00:00Z',
      }),
      row({
        mint_address: 'b',
        exit: 'TakeProfit',
        holding_secs: 10,
        created_at: '2026-07-15T20:00:00Z',
      }),
    ];
    const t = buildTemporalSummary(rows, 'UTC', 'created_at');
    expect(t.wallGrain).toBe('1h');
    expect(t.wall.filter((c) => c.n > 0)).toHaveLength(2);
  });

  it('honors manual grain override while keeping auto pick', () => {
    const rows: TemporalRow[] = [
      row({
        mint_address: 'a',
        exit: 'TakeProfit',
        holding_secs: 10,
        created_at: '2026-07-15T14:30:00Z',
      }),
      row({
        mint_address: 'b',
        exit: 'TakeProfit',
        holding_secs: 10,
        created_at: '2026-07-15T14:45:00Z',
      }),
    ];
    const t = buildTemporalSummary(rows, 'UTC', 'created_at', '1h');
    expect(t.wallGrain).toBe('1h');
    expect(t.wallGrainAuto).toBe('30m');
  });

  it('honors manual hold-scheme override while keeping auto pick', () => {
    const rows: TemporalRow[] = [
      row({ mint_address: 'a', exit: 'TakeProfit', holding_secs: 10 }),
      row({ mint_address: 'b', exit: 'StopLoss', holding_secs: 20 }),
    ];
    const t = buildTemporalSummary(rows, 'UTC', 'entry_time', 'auto', 'dense_15s');
    expect(t.holdScheme).toBe('dense_15s');
    expect(t.holdSchemeAuto).toBe('dense_60s');
    expect(t.hold.find((b) => b.id === 'hold_10_14')!.n).toBe(1);
  });

  it('rebuckets hold/wall for linked brush while keeping base edges', () => {
    const rows: TemporalRow[] = [
      row({
        mint_address: 'a',
        exit: 'TakeProfit',
        holding_secs: 5,
        pnl_sol: 1,
        created_at: '2026-07-15T10:00:00Z',
      }),
      row({
        mint_address: 'b',
        exit: 'StopLoss',
        holding_secs: 120,
        pnl_sol: -1,
        created_at: '2026-07-15T16:00:00Z',
      }),
    ];
    const base = buildTemporalSummary(rows, 'UTC', 'created_at', '1h', 'mid_5m');
    const holdOnlyA = rebucketHold(
      'mid_5m',
      rows.filter((r) => r.mint_address === 'a'),
    );
    expect(holdOnlyA.map((b) => b.id)).toEqual(base.hold.map((b) => b.id));
    expect(holdOnlyA.find((b) => b.id === 'hold_0_29')!.n).toBe(1);
    expect(holdOnlyA.find((b) => b.id === 'hold_120_299')!.n).toBe(0);

    const wallOnlyB = rebucketWall(
      base.wall,
      'created_at',
      '1h',
      rows.filter((r) => r.mint_address === 'b'),
      'UTC',
    );
    expect(wallOnlyB.map((c) => c.id)).toEqual(base.wall.map((c) => c.id));
    expect(wallOnlyB.reduce((s, c) => s + c.n, 0)).toBe(1);
  });
});

/**
 * Twin of the Rust guard `sim_query::wall_buckets_floor_in_the_requested_zone`
 * (same vectors, verbatim). Wall bins are CIVIL buckets: if these two folds ever
 * drift, the Wall clock card and the Timing calendar beside it silently disagree
 * about which day a position belongs to.
 */
describe('floorToWallGrain', () => {
  const at = (iso: string) => Date.parse(iso);

  it('floors wall buckets in the app timezone', () => {
    const ny = 'America/New_York';
    // A late-evening UTC instant is still the PREVIOUS civil day in New York.
    expect(floorToWallGrain(at('2026-01-15T02:30:00Z'), 'day', ny)).toBe(
      at('2026-01-14T05:00:00Z'),
    );
    // 4h grain aligns to LOCAL 00/04/08/…, not to the UTC epoch.
    expect(floorToWallGrain(at('2026-07-15T14:37:00Z'), '4h', ny)).toBe(
      at('2026-07-15T12:00:00Z'),
    );
    // Half-hour zone: an epoch-aligned floor is wrong at EVERY grain here.
    expect(floorToWallGrain(at('2026-07-15T14:20:00Z'), '1h', 'Asia/Kolkata')).toBe(
      at('2026-07-15T13:30:00Z'),
    );
    // DST fall-back day: the instant is EST (-5) but local midnight that day was
    // still EDT (-4) — the second pass is what gets this right.
    expect(floorToWallGrain(at('2026-11-01T12:00:00Z'), 'day', ny)).toBe(
      at('2026-11-01T04:00:00Z'),
    );
  });

  it('degrades to plain epoch alignment under UTC', () => {
    const t = at('2026-07-15T14:37:00Z');
    expect(floorToWallGrain(t, '4h', 'UTC')).toBe(t - (t % (4 * 3_600_000)));
  });

  it('drops no row across a DST transition', () => {
    // A DST day is 23h or 25h, so a `t += step` grid drifts off the boundaries.
    const stamps = [
      '2026-10-30T18:00:00Z',
      '2026-10-31T18:00:00Z',
      '2026-11-01T02:00:00Z', // before the 06:00Z fall-back
      '2026-11-01T18:00:00Z', // after it
      '2026-11-02T18:00:00Z',
      '2026-11-03T18:00:00Z',
    ];
    const rows = stamps.map((ts, i) =>
      row({ mint_address: `m${i}`, exit: 'TakeProfit', holding_secs: 30, created_at: ts }),
    );
    const t = buildTemporalSummary(rows, 'America/New_York', 'created_at', 'day');
    expect(t.wall.reduce((s, c) => s + c.n, 0)).toBe(stamps.length);
    // Cells stay contiguous: each end is the next start.
    for (let i = 0; i + 1 < t.wall.length; i++) {
      expect(t.wall[i]!.end).toBe(t.wall[i + 1]!.start);
    }
  });
});
