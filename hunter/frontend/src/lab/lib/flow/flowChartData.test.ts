import { describe, expect, it } from 'vitest';
import { buildFlowLines } from './flowChartData';
import type { TradeRecord } from 'types';

/** Minimal trade fixture — only the fields `buildFlowLines`/`classifyFlow`
 *  actually read. */
function trade(overrides: Partial<TradeRecord>): TradeRecord {
  return {
    id: overrides.id ?? Math.random().toString(36),
    mint_address: 'mint',
    wallet_address: 'w1',
    trade_type: 'buy',
    amount_sol: 1,
    token_amount: 1,
    price_per_token: 1,
    tx_signature: overrides.tx_signature ?? Math.random().toString(36),
    tx_index: 0,
    leg_index: 0,
    slot: 0,
    block_time: '2026-07-21T01:00:00Z',
    instruction_labels: null,
    ...overrides,
  };
}

const NO_PATTERNS = { patternKeys: new Set<string>(), creatorWallet: null };

describe('buildFlowLines', () => {
  it('nets buy − sell (net_sol) so the line drops when a cohort sells', () => {
    const trades: TradeRecord[] = [
      trade({ slot: 1, block_time: '2026-07-21T01:00:00Z', amount_sol: 10, trade_type: 'buy' }),
      trade({ slot: 2, block_time: '2026-07-21T01:01:00Z', amount_sol: 4, trade_type: 'sell' }),
    ];
    const lines = buildFlowLines(trades, 'time', 60, 'net_sol', NO_PATTERNS);
    expect(lines.nonVol[0].value).toBeCloseTo(10, 6);
    // A sell pulls the running net BACK down — impossible under the old gross flow.
    expect(lines.nonVol.at(-1)!.value).toBeCloseTo(6, 6);
  });

  it('attributes each trade to its own time bucket regardless of processing order', () => {
    // A live/very-recent token can have a trade whose tx_index hasn't been
    // backfilled yet (falls back to 0), landing it out of true time order in
    // `compareTradesChronologically`. The per-bucket total + prefix-sum design
    // must place each net delta in the right bucket regardless of array order.
    const early = trade({ slot: 1, tx_index: 0, block_time: '2026-07-21T01:00:00Z', amount_sol: 5 });
    const late = trade({ slot: 2, tx_index: 0, block_time: '2026-07-21T01:05:00Z', amount_sol: 7 });
    const lines = buildFlowLines([late, early], 'time', 60, 'net_sol', NO_PATTERNS);
    expect(lines.nonVol[0].value).toBeCloseTo(5, 6);
    expect(lines.nonVol.at(-1)!.value).toBeCloseTo(12, 6);
  });

  it('token basis tracks the net token balance', () => {
    const trades: TradeRecord[] = [
      trade({ slot: 1, block_time: '2026-07-21T01:00:00Z', token_amount: 1000, trade_type: 'buy' }),
      trade({ slot: 2, block_time: '2026-07-21T01:01:00Z', token_amount: 400, trade_type: 'sell' }),
    ];
    const lines = buildFlowLines(trades, 'time', 60, 'token', NO_PATTERNS);
    expect(lines.nonVol[0].value).toBeCloseTo(1000, 6);
    expect(lines.nonVol.at(-1)!.value).toBeCloseTo(600, 6);
  });

  it('real_sol marks the net token bag to the candle-close canonical spot, carrying it forward', () => {
    const trades: TradeRecord[] = [
      // Bucket A: net 1000 tokens, spot 20/1000 = 0.02 → 1000 × 0.02 = 20.
      trade({
        slot: 1,
        block_time: '2026-07-21T01:00:00Z',
        token_amount: 1000,
        trade_type: 'buy',
        reserve_sol: 20,
        reserve_token: 1000,
      }),
      // Bucket B: net 1100 tokens, spot 44/1100 = 0.04 → 1100 × 0.04 = 44
      // (price appreciated → the bag is worth more per token).
      trade({
        slot: 2,
        block_time: '2026-07-21T01:01:00Z',
        token_amount: 100,
        trade_type: 'buy',
        reserve_sol: 44,
        reserve_token: 1100,
      }),
      // Bucket C: a trade with NO reserve pair and price_per_token 0 → no
      // resolvable spot, so it carries forward (0.04); net 1200 → 1200 × 0.04 = 48.
      trade({
        slot: 3,
        block_time: '2026-07-21T01:02:00Z',
        token_amount: 100,
        trade_type: 'buy',
        price_per_token: 0,
      }),
    ];
    const lines = buildFlowLines(trades, 'time', 60, 'real_sol', NO_PATTERNS);
    expect(lines.nonVol[0].value).toBeCloseTo(20, 6);
    expect(lines.nonVol[1].value).toBeCloseTo(44, 6);
    expect(lines.nonVol.at(-1)!.value).toBeCloseTo(48, 6);
  });
});
