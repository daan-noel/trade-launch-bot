import { describe, expect, it } from 'vitest';
import { legPnlPctFromSol, pnlPctFromSol } from './pnlPct';

describe('pnlPctFromSol', () => {
  it('is pnl over the SOL paid', () => {
    expect(pnlPctFromSol(0.1, 0.5)).toBeCloseTo(20, 9);
  });

  it('is null without a positive stake', () => {
    expect(pnlPctFromSol(0.1, 0)).toBeNull();
    expect(pnlPctFromSol(0.1, null)).toBeNull();
    expect(pnlPctFromSol(null, 0.5)).toBeNull();
  });
});

describe('legPnlPctFromSol', () => {
  it('charges the leg its token share of the SOL paid', () => {
    // Paid 1 SOL for 1000 tokens; half the bag sells for 0.495 SOL.
    expect(legPnlPctFromSol(0.495, 500, 1, 1000)).toBeCloseTo(-1, 9);
  });

  it('legs of a fully sold bag sum to the position PnL', () => {
    const paid = 0.5;
    const legs: Array<[number, number]> = [
      [0.3, 400],
      [0.26, 600],
    ];
    const pnl = legs.reduce((acc, [sol, tok]) => {
      const cost = paid * (tok / 1000);
      return acc + ((legPnlPctFromSol(sol, tok, paid, 1000) ?? 0) / 100) * cost;
    }, 0);
    expect(pnl).toBeCloseTo(0.56 - paid, 9);
  });

  it('is null when a basis is missing', () => {
    expect(legPnlPctFromSol(0.5, 500, null, 1000)).toBeNull();
    expect(legPnlPctFromSol(0.5, 500, 1, 0)).toBeNull();
    expect(legPnlPctFromSol(0.5, 0, 1, 1000)).toBeNull();
    expect(legPnlPctFromSol(null, 500, 1, 1000)).toBeNull();
  });
});
