import { describe, expect, it } from 'vitest';
import { classifyFlowTrades, patternKeysFrom } from './flow/classifyFlow';
import { applyTokenLiveStats, liveTradeToTradeRecord, tradeDedupeKey } from './liveTrade';
import type { LiveTrade, TokenDetailRecord, TokenLiveStats, TradeRecord } from 'types';

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
    instruction_labels: ['Compute Budget: SetComputeUnitLimit', 'Pump.Fun: Buy'],
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

  it('carries ix labels, so a live-appended row classifies as vol like a refetched one', () => {
    const row = liveTradeToTradeRecord(sample);
    expect(row.instruction_labels).toEqual(sample.instruction_labels);

    const keys = patternKeysFrom([sample.instruction_labels!]);
    const [classified] = classifyFlowTrades(
      [{ wallet_address: row.wallet_address, sol: row.amount_sol, ix_labels: row.instruction_labels }],
      { patternKeys: keys },
    );
    expect(classified.isVol).toBe(true);
  });

  it('a frame without labels carries no labels, never an empty match', () => {
    const { instruction_labels: _omitted, ...noLabels } = sample;
    const row = liveTradeToTradeRecord(noLabels);
    expect(row.instruction_labels).toEqual([]);

    // Empty is the missing sentinel, not a pattern: it must never match, not even
    // against a staged empty pattern (mirrors Rust `ix_hash_opt` ⇒ `None`).
    const [classified] = classifyFlowTrades(
      [{ wallet_address: row.wallet_address, sol: row.amount_sol, ix_labels: row.instruction_labels }],
      { patternKeys: patternKeysFrom([[]]) },
    );
    expect(classified.isVol).toBe(false);
  });

  it('normalizes the object-wrapper ix_labels shape, so it classifies like the bare array', () => {
    // `trades.ix_labels` is persisted in either shape and the SSE frame relays the
    // column verbatim; an un-normalized wrapper reads as "no labels" and silently
    // books the trade organic.
    const wrapped = {
      ...sample,
      instruction_labels: { instructions: sample.instruction_labels } as never,
    };
    const row = liveTradeToTradeRecord(wrapped);
    expect(row.instruction_labels).toEqual(sample.instruction_labels);

    const [classified] = classifyFlowTrades(
      [{ wallet_address: row.wallet_address, sol: row.amount_sol, ix_labels: row.instruction_labels }],
      { patternKeys: patternKeysFrom([sample.instruction_labels!]) },
    );
    expect(classified.isVol).toBe(true);
  });
});

describe('applyTokenLiveStats', () => {
  const stats: TokenLiveStats = {
    current_price: 4e-7,
    volume_sol_total: 812.5,
    market_cap: 400,
    trade_count: 1_204,
    ath_price: 9e-7,
    ath_timestamp: '2026-07-20T12:00:05Z',
    last_trade_at: '2026-07-20T12:00:09Z',
  };

  /** Only the fields the snapshot owns — everything else on the row is REST-owned. */
  const stale = (): TokenDetailRecord =>
    ({
      mint_address: 'Mint111',
      symbol: 'TEST',
      current_price: 1e-7,
      volume_sol_total: 10,
      market_cap: 100,
      trade_count: 3,
      ath_price: 2e-7,
      ath_timestamp: '2026-07-20T11:00:00Z',
      last_trade_at: '2026-07-20T11:00:00Z',
    }) as TokenDetailRecord;

  it('moves the ATH the chart line reads', () => {
    // The whole point of patching `getTokenDetail`: the query never polls and has
    // no invalidating tag, so without this the ATH line stays at mount-time value
    // while the bars keep printing new highs above it.
    const row = stale();
    applyTokenLiveStats(row, stats);
    expect(row.ath_price).toBe(9e-7);
    expect(row.ath_timestamp).toBe('2026-07-20T12:00:05Z');
  });

  it('writes every field the snapshot carries (SSOT guard vs TokenLiveStats)', () => {
    const row = stale();
    applyTokenLiveStats(row, stats);
    for (const key of Object.keys(stats) as (keyof TokenLiveStats)[]) {
      expect(row[key], key).toEqual(stats[key]);
    }
  });

  it('leaves REST-owned fields alone', () => {
    const row = stale();
    applyTokenLiveStats(row, stats);
    expect(row.mint_address).toBe('Mint111');
    expect(row.symbol).toBe('TEST');
  });
});
