import { describe, expect, it } from 'vitest';
import { liveTradeToTradeRecord, tradeDedupeKey } from './liveTrade';
import type { LiveTrade, TradeRecord } from 'types';

/** Fields the chart sort / OHLC path requires — must stay on the SSE→REST adapter. */
const CHART_ORDER_KEYS: (keyof TradeRecord)[] = [
  'slot',
  'tx_index',
  'leg_index',
  'block_time',
  'wallet_address',
  'price_per_token',
  'amount_sol',
  'token_amount',
  'trade_type',
  'tx_signature',
  'mint_address',
];

describe('liveTradeToTradeRecord', () => {
  const sample: LiveTrade = {
    mint_address: 'Mint111',
    wallet: 'Wallet222',
    trade_type: 'buy',
    amount_sol: 1.5,
    token_amount: 1_000_000,
    price_per_token: 0.0000015,
    tx_signature: 'SigAbc',
    tx_index: 42,
    leg_index: 1,
    reserve_sol: 30,
    reserve_token: 20_000_000,
    venue: 'curve',
    slot: 99,
    timestamp: '2026-07-20T12:00:00Z',
  };

  it('maps wallet → wallet_address and timestamp → block_time', () => {
    const row = liveTradeToTradeRecord(sample);
    expect(row.wallet_address).toBe('Wallet222');
    expect(row.block_time).toBe('2026-07-20T12:00:00Z');
    expect(row.trade_type).toBe('buy');
    expect(row.tx_index).toBe(42);
    expect(row.leg_index).toBe(1);
    expect(row.venue).toBe('curve');
  });

  it('exposes every chart-order field (SSOT guard vs TradeRecord)', () => {
    const row = liveTradeToTradeRecord(sample);
    for (const key of CHART_ORDER_KEYS) {
      expect(row[key], key).toBeDefined();
    }
  });

  it('dedupe key is signature + leg', () => {
    const row = liveTradeToTradeRecord(sample);
    expect(tradeDedupeKey(row)).toBe('SigAbc:1');
  });
});
