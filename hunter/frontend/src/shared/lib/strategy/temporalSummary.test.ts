import { describe, expect, it } from 'vitest';
import {
  buildTemporalSummary,
  formatWallSpan,
  peakWallCell,
  pickWallGrain,
  rowMatchesHoldBin,
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

describe('pickWallGrain', () => {
  it('picks finer grains for short spans', () => {
    expect(pickWallGrain(2 * H)).toBe('30m');
    expect(pickWallGrain(12 * H)).toBe('1h');
    expect(pickWallGrain(2 * D)).toBe('2h');
    expect(pickWallGrain(5 * D)).toBe('4h');
    expect(pickWallGrain(14 * D)).toBe('day');
  });
});

describe('buildTemporalSummary', () => {
  it('bins hold duration and stacks exits', () => {
    const rows: TemporalRow[] = [
      row({ mint_address: 'a', exit: 'TakeProfit', holding_secs: 10, pnl_sol: 1 }),
      row({ mint_address: 'b', exit: 'StopLoss', holding_secs: 20, pnl_sol: -0.5 }),
      row({ mint_address: 'c', exit: 'TakeProfit', holding_secs: 120, pnl_sol: 0.2 }),
      row({ mint_address: 'd', exit: 'Open', holding_secs: 0, pnl_sol: 0.1 }),
      row({ mint_address: 'e', exit: 'NoEntry', fired: false, holding_secs: 0, pnl_sol: 0 }),
    ];
    const t = buildTemporalSummary(rows);
    expect(t.nFired).toBe(4);
    const lt15 = t.hold.find((b) => b.id === 'lt15s')!;
    expect(lt15.n).toBe(1);
    expect(lt15.exits.n_exit_take_profit).toBe(1);
    const s15 = t.hold.find((b) => b.id === '15to60s')!;
    expect(s15.n).toBe(1);
    expect(s15.exits.n_exit_stop_loss).toBe(1);
    const open = t.hold.find((b) => b.id === 'open')!;
    expect(open.n).toBe(1);
    expect(rowMatchesHoldBin(rows[0], 'lt15s')).toBe(true);
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
    const t = buildTemporalSummary(rows, 'created_at');
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
    const t = buildTemporalSummary(rows, 'created_at');
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
    const t = buildTemporalSummary(rows, 'created_at', '1h');
    expect(t.wallGrain).toBe('1h');
    expect(t.wallGrainAuto).toBe('30m');
  });
});
