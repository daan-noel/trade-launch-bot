import { describe, expect, it } from 'vitest';
import {
  alignFlowToBars,
  buildFlowLines,
  cohortCurvePriceSol,
  tradeFlowReasons,
} from './flowChartData';
import { shapeTag } from './tapeClassify';
import {
  PUMP_INITIAL_VIRTUAL_SOL,
  PUMP_INITIAL_VIRTUAL_TOKEN,
} from 'components/token-price-chart/constants';
import type { OhlcBar } from 'components/token-price-chart/types';
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

/** A tag no trade carries: every trade is the rest. */
const NO_PATTERNS = { tag: shapeTag('t', []) };

describe('buildFlowLines', () => {
  it('nets buy − sell (cost_sol) so the line drops when a cohort sells', () => {
    const trades: TradeRecord[] = [
      trade({ slot: 1, block_time: '2026-07-21T01:00:00Z', amount_sol: 10, trade_type: 'buy' }),
      trade({ slot: 2, block_time: '2026-07-21T01:01:00Z', amount_sol: 4, trade_type: 'sell' }),
    ];
    const lines = buildFlowLines(trades, 'time', 60, 'cost_sol', NO_PATTERNS);
    expect(lines.untagged[0].value).toBeCloseTo(10, 6);
    // A sell pulls the running net BACK down — impossible under the old gross flow.
    expect(lines.untagged.at(-1)!.value).toBeCloseTo(6, 6);
  });

  it('attributes each trade to its own time bucket regardless of processing order', () => {
    // A live/very-recent token can have a trade whose tx_index hasn't been
    // backfilled yet (falls back to 0), landing it out of true time order in
    // `compareTradesChronologically`. The per-bucket total + prefix-sum design
    // must place each net delta in the right bucket regardless of array order.
    const early = trade({ slot: 1, tx_index: 0, block_time: '2026-07-21T01:00:00Z', amount_sol: 5 });
    const late = trade({ slot: 2, tx_index: 0, block_time: '2026-07-21T01:05:00Z', amount_sol: 7 });
    const lines = buildFlowLines([late, early], 'time', 60, 'cost_sol', NO_PATTERNS);
    expect(lines.untagged[0].value).toBeCloseTo(5, 6);
    expect(lines.untagged.at(-1)!.value).toBeCloseTo(12, 6);
  });

  it('token basis tracks the net token balance', () => {
    const trades: TradeRecord[] = [
      trade({ slot: 1, block_time: '2026-07-21T01:00:00Z', token_amount: 1000, trade_type: 'buy' }),
      trade({ slot: 2, block_time: '2026-07-21T01:01:00Z', token_amount: 400, trade_type: 'sell' }),
    ];
    const lines = buildFlowLines(trades, 'time', 60, 'token', NO_PATTERNS);
    expect(lines.untagged[0].value).toBeCloseTo(1000, 6);
    expect(lines.untagged.at(-1)!.value).toBeCloseTo(600, 6);
  });

  it('value_sol marks the net token bag to the candle-close canonical spot, carrying it forward', () => {
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
    const lines = buildFlowLines(trades, 'time', 60, 'value_sol', NO_PATTERNS);
    expect(lines.untagged[0].value).toBeCloseTo(20, 6);
    expect(lines.untagged[1].value).toBeCloseTo(44, 6);
    expect(lines.untagged.at(-1)!.value).toBeCloseTo(48, 6);
  });
});

describe('flow lines on the price axis', () => {
  const V0 = PUMP_INITIAL_VIRTUAL_SOL;
  const X0 = PUMP_INITIAL_VIRTUAL_TOKEN;
  const K = V0 * X0;
  const genesis = { sol: V0, token: X0 };
  const startPrice = V0 / X0;

  it('draws net 0 at the anchor spot, and a SOL net through the curve', () => {
    expect(cohortCurvePriceSol(genesis, 'cost_sol', 0, 0)).toBeCloseTo(startPrice, 20);
    expect(cohortCurvePriceSol(genesis, 'token', 0, 0)).toBeCloseTo(startPrice, 20);
    // +10 SOL from 30: (40/30)^2 = 1.78x the start price; -10: (20/30)^2.
    expect(cohortCurvePriceSol(genesis, 'cost_sol', 10, 0) / startPrice).toBeCloseTo(16 / 9, 9);
    expect(cohortCurvePriceSol(genesis, 'cost_sol', -10, 0) / startPrice).toBeCloseTo(4 / 9, 9);
  });

  it('token basis lands where the SOL basis does on the same one-way path', () => {
    const netToken = X0 - K / (V0 + 10);
    expect(cohortCurvePriceSol(genesis, 'token', 0, netToken)).toBeCloseTo(
      cohortCurvePriceSol(genesis, 'cost_sol', 10, 0),
      20,
    );
  });

  it('a sole cohort from genesis ends on the candle close (the post-trade spot)', () => {
    const netToken = X0 - K / (V0 + 10);
    const buy = trade({
      amount_sol: 10,
      token_amount: netToken,
      reserve_sol: V0 + 10,
      reserve_token: K / (V0 + 10),
      venue: 'curve',
    });
    const lines = buildFlowLines([buy], 'time', 60, 'cost_sol', NO_PATTERNS);
    expect(lines.anchor.sol).toBeCloseTo(V0, 9);
    expect(lines.anchor.token).toBeCloseTo(X0, 0);
    const spot = (V0 + 10) / (K / (V0 + 10));
    expect(lines.untagged[0].priceSol / spot).toBeCloseTo(1, 12);
    // The other cohort traded nothing: it stays on the start price.
    expect(lines.tagged[0].priceSol / startPrice).toBeCloseTo(1, 12);
  });

  it('ends both lines at the first AMM trade, and fills earlier bars at the start price', () => {
    const trades: TradeRecord[] = [
      trade({ slot: 1, block_time: '2026-07-21T01:01:00Z', amount_sol: 5, venue: 'curve' }),
      trade({ slot: 2, block_time: '2026-07-21T01:02:00Z', amount_sol: 3, venue: 'amm' }),
      trade({ slot: 3, block_time: '2026-07-21T01:03:00Z', amount_sol: 3, venue: 'curve' }),
    ];
    const lines = buildFlowLines(trades, 'time', 60, 'cost_sol', NO_PATTERNS);
    expect(lines.untagged).toHaveLength(1);
    expect(lines.untagged[0].value).toBeCloseTo(5, 9);
    const t0 = Date.parse('2026-07-21T01:00:00Z') / 1000;
    const bars = [0, 60, 120, 180].map((d) => ({ time: t0 + d }) as unknown as OhlcBar);
    const aligned = alignFlowToBars(lines, bars);
    expect(aligned.untagged.map((p) => p.time as number)).toEqual([t0, t0 + 60]);
    expect(aligned.untagged[0].value).toBe(0);
    expect(aligned.untagged[0].priceSol).toBeCloseTo(startPrice, 20);
  });

  it('draws nothing on an AMM-only history', () => {
    const lines = buildFlowLines(
      [trade({ block_time: '2026-07-21T01:00:00Z', venue: 'amm' })],
      'time',
      60,
      'cost_sol',
      NO_PATTERNS,
    );
    const t0 = Date.parse('2026-07-21T01:00:00Z') / 1000;
    const aligned = alignFlowToBars(lines, [{ time: t0 } as unknown as OhlcBar]);
    expect(aligned.untagged).toHaveLength(0);
  });
});

describe('tradeFlowReasons', () => {
  it('classifies in canonical order whatever order the rows arrive in', () => {
    const tag = { ...shapeTag('t', [{ labels: ['A'] }]), sticky: true };
    const first = trade({ id: 'a', slot: 1, instruction_labels: ['A'] });
    const later = trade({ id: 'b', slot: 2, instruction_labels: ['Z'] });
    const map = tradeFlowReasons([later, first], { tag });
    expect(map?.get('a')).toBe('ix_shape');
    expect(map?.get('b')).toBe('sticky');
  });

  it('puts a creation-slot buyer on the tagged line', () => {
    const base = shapeTag('t', [{ labels: ['A'] }]);
    const tag = { ...base, match: { ...base.match, creation_slot: true } };
    const trades: TradeRecord[] = [
      trade({ slot: 1, amount_sol: 1, instruction_labels: ['Pump.Fun: Create', 'Pump.Fun: Buy'] }),
      trade({ slot: 2, wallet_address: 'w2', block_time: '2026-07-21T01:01:00Z', amount_sol: 5 }),
    ];
    const lines = buildFlowLines(trades, 'time', 60, 'cost_sol', { tag });
    expect(lines.tagged.at(-1)!.value).toBeCloseTo(1, 6);
    expect(lines.untagged.at(-1)!.value).toBeCloseTo(5, 6);
  });
});
