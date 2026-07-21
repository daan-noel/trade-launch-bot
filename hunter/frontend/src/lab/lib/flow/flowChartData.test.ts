import { describe, expect, it } from 'vitest';
import { buildFlowLines } from './flowChartData';
import { patternKeysFrom } from './classifyFlow';
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

describe('buildFlowLines', () => {
  const patternKeys = patternKeysFrom([['A']]);

  it('never decreases even when a late-arriving trade sorts slightly out of true time order', () => {
    // A live/very-recent token can have a trade whose tx_index hasn't been
    // backfilled yet (falls back to 0), landing it earlier in
    // `compareTradesChronologically`'s order than its real block_time implies.
    // The bucket-total + prefix-sum design must stay monotonic regardless.
    const trades: TradeRecord[] = [
      trade({ slot: 100, tx_index: 5, block_time: '2026-07-21T01:00:00Z', amount_sol: 10 }),
      trade({ slot: 100, tx_index: 6, block_time: '2026-07-21T01:00:30Z', amount_sol: 10 }),
      // Same slot, but tx_index unresolved (0) — sorts BEFORE the two above
      // even though its real time (bucket) is later.
      trade({ slot: 100, tx_index: 0, block_time: '2026-07-21T01:01:30Z', amount_sol: 50 }),
      trade({ slot: 101, tx_index: 1, block_time: '2026-07-21T01:02:00Z', amount_sol: 10 }),
    ];

    const lines = buildFlowLines(trades, 'time', 60, 'sol', { patternKeys, creatorWallet: null });

    for (const series of [lines.vol, lines.nonVol]) {
      for (let i = 1; i < series.length; i++) {
        expect(series[i].value).toBeGreaterThanOrEqual(series[i - 1].value);
      }
    }
    // Total across both series' final points must equal the full sum in.
    const totalIn = trades.reduce((s, t) => s + t.amount_sol, 0);
    const totalOut = (lines.vol.at(-1)?.value ?? 0) + (lines.nonVol.at(-1)?.value ?? 0);
    expect(totalOut).toBeCloseTo(totalIn, 6);
  });

  it('attributes each trade to its own time bucket regardless of processing order', () => {
    const early = trade({ slot: 1, tx_index: 0, block_time: '2026-07-21T01:00:00Z', amount_sol: 5 });
    const late = trade({ slot: 2, tx_index: 0, block_time: '2026-07-21T01:05:00Z', amount_sol: 7 });
    // Pass already out of chronological order — buildFlowLines re-sorts, but
    // the bucket totals themselves don't depend on array order at all.
    const lines = buildFlowLines([late, early], 'time', 60, 'sol', {
      patternKeys: new Set(),
      creatorWallet: null,
    });
    expect(lines.nonVol[0].value).toBeCloseTo(5, 6);
    expect(lines.nonVol.at(-1)!.value).toBeCloseTo(12, 6);
  });
});
