import { describe, expect, it } from 'vitest';
import {
  positionChartFactsFromEpisodes,
  positionChartFactsFromSim,
  type PositionChartEpisode,
} from './PositionChartCardExtra';
import type { SimulatedTokenResult } from 'types';

function episode(overrides: Partial<PositionChartEpisode> = {}): PositionChartEpisode {
  return {
    holdSecs: 30,
    pnlSol: 0.5,
    pnlPct: 20,
    entrySol: 1,
    entryPrice: 0.01,
    exitPrice: 0.012,
    exitReason: 'TakeProfit',
    isOpen: false,
    include: true,
    ...overrides,
  };
}

describe('positionChartFactsFromEpisodes', () => {
  it('passes through a single episode', () => {
    const facts = positionChartFactsFromEpisodes([episode()]);
    expect(facts.episodeCount).toBe(1);
    expect(facts.pnlSol).toBe(0.5);
    expect(facts.pnlPct).toBe(20);
    expect(facts.holdSecs).toBe(30);
    expect(facts.exitReason).toBe('TakeProfit');
  });

  it('sums PnL and takes max hold across episodes; drops pct', () => {
    const facts = positionChartFactsFromEpisodes([
      episode({ holdSecs: 10, pnlSol: 1, entrySol: 1 }),
      episode({ holdSecs: 45, pnlSol: -0.2, entrySol: 1, exitReason: 'StopLoss' }),
    ]);
    expect(facts.episodeCount).toBe(2);
    expect(facts.pnlSol).toBeCloseTo(0.8, 9);
    expect(facts.pnlPct).toBeNull();
    expect(facts.holdSecs).toBe(45);
    expect(facts.entrySol).toBe(2);
  });

  it('marks open when any episode is open', () => {
    const facts = positionChartFactsFromEpisodes([
      episode({ isOpen: false }),
      episode({ isOpen: true, exitReason: 'Open', exitPrice: null }),
    ]);
    expect(facts.isOpen).toBe(true);
    expect(facts.exitReason).toBeNull();
  });

  it('skips include:false episodes when any include:true exist', () => {
    const facts = positionChartFactsFromEpisodes([
      episode({ include: false, pnlSol: 99 }),
      episode({ include: true, pnlSol: 0.1 }),
    ]);
    expect(facts.episodeCount).toBe(1);
    expect(facts.pnlSol).toBe(0.1);
  });
});

describe('positionChartFactsFromSim', () => {
  it('maps a fired sim row', () => {
    const row = {
      mint_address: 'm',
      symbol: 'T',
      fired: true,
      entry_price: 0.01,
      entry_token_amount: 100,
      exit_price: 0.02,
      holding_secs: 12,
      pnl_percent: 100,
      pnl_sol: 1,
      exit_reason: 'TakeProfit',
      exit_time: '2026-07-01T00:00:12Z',
      entry_time: '2026-07-01T00:00:00Z',
    } as SimulatedTokenResult;
    const facts = positionChartFactsFromSim([row]);
    expect(facts.holdSecs).toBe(12);
    expect(facts.entrySol).toBeCloseTo(1, 9);
    expect(facts.pnlPct).toBe(100);
  });
});
