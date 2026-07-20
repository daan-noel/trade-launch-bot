import { describe, expect, it } from 'vitest';
import {
  liveTradeSpotSolPerRaw,
  spotSolPerRawToUsd,
  valueSolAtSpot,
} from './liveMark';
import type { LiveTrade } from 'types';

function trade(partial: Partial<LiveTrade>): LiveTrade {
  return {
    mint_address: 'Mint111',
    wallet: 'Wal111',
    trade_type: 'buy',
    amount_sol: 1,
    token_amount: 1_000_000,
    price_per_token: 0.000001,
    tx_signature: 'Sig111',
    tx_index: 0,
    leg_index: 0,
    slot: 1,
    timestamp: '2026-01-01T00:00:00Z',
    ...partial,
  };
}

describe('liveTradeSpotSolPerRaw', () => {
  it('prefers reserves over execution price', () => {
    expect(
      liveTradeSpotSolPerRaw(
        trade({ price_per_token: 0.001, reserve_sol: 30, reserve_token: 1_000_000 }),
      ),
    ).toBeCloseTo(30 / 1_000_000);
  });

  it('falls back to price_per_token', () => {
    expect(liveTradeSpotSolPerRaw(trade({ price_per_token: 0.000002 }))).toBe(0.000002);
  });

  it('returns null when no usable spot', () => {
    expect(liveTradeSpotSolPerRaw(trade({ price_per_token: 0, reserve_sol: null }))).toBeNull();
  });
});

describe('spotSolPerRawToUsd', () => {
  it('scales raw→UI then × SOL/USD', () => {
    // 1e-9 SOL/raw × 1e6 decimals = 0.001 SOL/UI × $150 = $0.15
    expect(spotSolPerRawToUsd(1e-9, 6, 150)).toBeCloseTo(0.15);
  });
});

describe('valueSolAtSpot', () => {
  it('multiplies spot × raw amount', () => {
    expect(valueSolAtSpot(1e-6, 2_000_000)).toBeCloseTo(2);
  });
});
