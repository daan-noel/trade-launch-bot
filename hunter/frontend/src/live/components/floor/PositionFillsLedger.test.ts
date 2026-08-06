import { describe, expect, it } from 'vitest';
import { fillsFromPositionFacts } from './PositionFillsLedger';

describe('fillsFromPositionFacts', () => {
  it('builds buy + sell from position entry/exit when ledger is empty', () => {
    const rows = fillsFromPositionFacts({
      positionId: 'pos-1',
      entrySol: 0.5,
      entryTokenAmount: 1_000_000,
      exitSol: 0.6,
      exitTokenAmount: 1_000_000,
      exitReason: 'TakeProfit',
      pnlSol: 0.1,
      inspect: {
        mint_address: 'Mint111',
        entryTime: '2026-08-01T12:00:00Z',
        entryPrice: 0.000001,
        exitTime: '2026-08-01T12:01:00Z',
        exitPrice: 0.0000012,
        exitLabel: 'TakeProfit',
      },
    });
    expect(rows).toHaveLength(2);
    expect(rows[0]?.side).toBe('buy');
    expect(rows[0]?.sol_lamports).toBe(500_000_000);
    expect(rows[1]?.side).toBe('sell');
    expect(rows[1]?.reason).toBe('TakeProfit');
  });

  it('builds buy-only for an open position', () => {
    const rows = fillsFromPositionFacts({
      positionId: 'pos-2',
      entrySol: 0.25,
      inspect: {
        mint_address: 'Mint222',
        entryTime: '2026-08-01T12:00:00Z',
        entryPrice: 0.000002,
        exitTime: null,
        exitPrice: null,
      },
    });
    expect(rows).toHaveLength(1);
    expect(rows[0]?.side).toBe('buy');
  });

  it('returns empty when there is no entry snapshot', () => {
    expect(
      fillsFromPositionFacts({
        positionId: 'pos-3',
        inspect: {
          mint_address: 'Mint333',
          entryTime: null,
          entryPrice: null,
          exitTime: null,
          exitPrice: null,
        },
      }),
    ).toEqual([]);
  });
});
