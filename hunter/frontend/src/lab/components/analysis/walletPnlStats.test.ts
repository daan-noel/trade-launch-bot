import { describe, expect, it } from 'vitest';
import type { TraderTokenRow } from 'types';
import {
  buildEquityCurve,
  buildHoldScatter,
  buildPnlHeatCells,
  computeWalletSummary,
  dowHourInTz,
  pnlDistributionBuckets,
  rankedPnlBarRows,
} from './walletPnlStats';
import { rankByValue } from 'components/analytics/pnlSeries';

/** Minimal valid `TraderTokenRow` with sane token-record defaults; each test
 *  overrides only the wallet_* fields it cares about. */
function row(overrides: Partial<TraderTokenRow>): TraderTokenRow {
  const base: TraderTokenRow = {
    mint_address: overrides.mint_address ?? 'MintAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA',
    name: 'Test Token',
    symbol: 'TEST',
    creator_wallet: 'CreatorAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA',
    trade_count: 10,
    current_price: 0.01,
    volume_sol_total: 100,
    first_slot_buy_sol: null,
    first_slot_sell_sol: null,
    ath_price: null,
    ath_timestamp: null,
    market_cap: null,
    initial_buy_sol: null,
    initial_supply_token: null,
    token_amount: null,
    max_cost_lamports: null,
    spendable_lamports_in: null,
    min_tokens_out: null,
    cu_limit: null,
    cu_price: null,
    ix_labels_count: 0,
    instruction_labels: [],
    is_migrated: false,
    is_dead: false,
    is_mayhem_mode: false,
    is_cashback_enabled: false,
    created_at: '2026-07-01T00:00:00Z',
    creation_tx_signature: 'sig',
    last_trade_at: '2026-07-01T00:10:00Z',
    lifetime_secs: null,
    last_synced_at: null,
    wallet_first_trade_at: '2026-07-01T00:00:00Z',
    wallet_last_trade_at: '2026-07-01T00:10:00Z',
    wallet_buy_count: 1,
    wallet_sell_count: 1,
    wallet_buy_sol: 1,
    wallet_sell_sol: 1.5,
    wallet_avg_buy_price: 0.01,
    wallet_avg_sell_price: 0.015,
    wallet_net_token_amount: 0,
    wallet_realized_pnl_sol: 0.5,
    wallet_realized_pnl_sol_net_of_fee: 0.475,
    wallet_realized_pnl_pct: 50,
    wallet_unrealized_pnl_sol: null,
    wallet_total_pnl_sol: 0.5,
    wallet_is_open: false,
    wallet_partial_data: false,
  };
  return { ...base, ...overrides };
}

describe('dowHourInTz', () => {
  it('reads dow/hour in UTC', () => {
    // 2026-07-27 is a Monday. 14:30 UTC → hour 14, Mon = dow 1.
    const ms = Date.parse('2026-07-27T14:30:00Z');
    expect(dowHourInTz(ms, 'UTC')).toEqual({ dow: 1, hour: 14 });
  });

  it('shifts across the day boundary for a non-UTC zone', () => {
    // 2026-07-27T23:30Z is 2026-07-28T08:30 in UTC+9 (Tokyo) — Tuesday, hour 8.
    const ms = Date.parse('2026-07-27T23:30:00Z');
    expect(dowHourInTz(ms, 'Asia/Tokyo')).toEqual({ dow: 2, hour: 8 });
  });
});

describe('computeWalletSummary', () => {
  it('is all-zero/null for an empty row set', () => {
    const s = computeWalletSummary([]);
    expect(s.tokenCount).toBe(0);
    expect(s.winRate).toBeNull();
    expect(s.payoffRatio).toBeNull();
    expect(s.profitFactor).toBeNull();
  });

  it('separates open bags from win/loss verdicts', () => {
    const rows = [
      row({ wallet_realized_pnl_sol: 1.0, wallet_realized_pnl_pct: 50, wallet_is_open: false }),
      row({ wallet_realized_pnl_sol: -0.5, wallet_realized_pnl_pct: -25, wallet_is_open: false }),
      // Pure open bag: never sold, so no realized verdict — excluded from win/loss.
      row({
        wallet_realized_pnl_sol: 0,
        wallet_realized_pnl_pct: null,
        wallet_unrealized_pnl_sol: 2.0,
        wallet_is_open: true,
      }),
    ];
    const s = computeWalletSummary(rows);
    expect(s.tokenCount).toBe(3);
    expect(s.openCount).toBe(1);
    expect(s.winCount).toBe(1);
    expect(s.lossCount).toBe(1);
    expect(s.winRate).toBeCloseTo(50, 9);
    expect(s.totalRealizedPnlSol).toBeCloseTo(0.5, 9);
    expect(s.totalUnrealizedPnlSol).toBeCloseTo(2.0, 9);
    expect(s.totalPnlSol).toBeCloseTo(2.5, 9);
    expect(s.avgWinSol).toBeCloseTo(1.0, 9);
    expect(s.avgLossSol).toBeCloseTo(-0.5, 9);
    expect(s.payoffRatio).toBeCloseTo(2.0, 9);
    expect(s.profitFactor).toBeCloseTo(2.0, 9);
  });

  it('reports null payoff/profit-factor with no losses', () => {
    const rows = [row({ wallet_realized_pnl_sol: 1.0, wallet_realized_pnl_pct: 10 })];
    const s = computeWalletSummary(rows);
    expect(s.payoffRatio).toBeNull();
    expect(s.profitFactor).toBeNull();
  });

  it('counts totalVolumeSol as buy+sell and flags partial data', () => {
    const rows = [
      row({ wallet_buy_sol: 2, wallet_sell_sol: 3, wallet_partial_data: true }),
      row({ wallet_buy_sol: 1, wallet_sell_sol: 1, wallet_partial_data: false }),
    ];
    const s = computeWalletSummary(rows);
    expect(s.totalVolumeSol).toBeCloseTo(7, 9);
    expect(s.partialDataCount).toBe(1);
  });
});

describe('buildPnlHeatCells', () => {
  it('always returns all 168 day×hour cells', () => {
    const cells = buildPnlHeatCells([], 'UTC');
    expect(cells).toHaveLength(7 * 24);
    expect(cells.every((c) => c.count === 0 && c.pnl_sol === 0)).toBe(true);
  });

  it('buckets a row into its last-trade dow/hour and sums pnl', () => {
    const ms = Date.parse('2026-07-27T14:30:00Z'); // Monday 14:00 UTC bucket
    const rows = [
      row({ wallet_last_trade_at_ms: ms, wallet_total_pnl_sol: 1.5 }),
      row({ wallet_last_trade_at_ms: ms, wallet_total_pnl_sol: -0.5 }),
    ];
    const cells = buildPnlHeatCells(rows, 'UTC');
    const cell = cells.find((c) => c.dow === 1 && c.hour === 14);
    expect(cell).toBeDefined();
    expect(cell!.count).toBe(2);
    expect(cell!.pnl_sol).toBeCloseTo(1.0, 9);
  });

  it('falls back to parsing wallet_last_trade_at when the _ms field is absent', () => {
    const rows = [
      row({
        wallet_last_trade_at: '2026-07-27T14:30:00Z',
        wallet_last_trade_at_ms: undefined,
        wallet_total_pnl_sol: 2.0,
      }),
    ];
    const cells = buildPnlHeatCells(rows, 'UTC');
    const cell = cells.find((c) => c.dow === 1 && c.hour === 14);
    expect(cell!.pnl_sol).toBeCloseTo(2.0, 9);
  });
});

describe('rankedPnlBarRows', () => {
  it('sorts descending by total pnl without mutating the input', () => {
    const rows = [
      row({ mint_address: 'a', wallet_total_pnl_sol: -1 }),
      row({ mint_address: 'b', wallet_total_pnl_sol: 5 }),
      row({ mint_address: 'c', wallet_total_pnl_sol: 2 }),
    ];
    const ranked = rankByValue(rankedPnlBarRows(rows));
    expect(ranked.map((r) => r.key)).toEqual(['b', 'c', 'a']);
    expect(rows.map((r) => r.mint_address)).toEqual(['a', 'b', 'c']);
  });

  it('tags an open bag so the bar can mark it', () => {
    const bars = rankedPnlBarRows([
      row({ mint_address: 'open', wallet_is_open: true }),
      row({ mint_address: 'closed', wallet_is_open: false }),
    ]);
    expect(bars.find((b) => b.key === 'open')!.tag).toBe('open');
    expect(bars.find((b) => b.key === 'closed')!.tag).toBeNull();
  });
});

describe('pnlDistributionBuckets', () => {
  it('buckets realized pct and excludes rows with no matched cost basis', () => {
    const rows = [
      row({ wallet_realized_pnl_pct: -60 }), // < -50
      row({ wallet_realized_pnl_pct: -5 }), // -10..0
      row({ wallet_realized_pnl_pct: 0 }), // 0..10 (half-open [0,10))
      row({ wallet_realized_pnl_pct: 15 }), // 10..20
      row({ wallet_realized_pnl_pct: null }), // excluded — pure open bag
    ];
    const buckets = pnlDistributionBuckets(rows);
    const total = buckets.reduce((s, b) => s + b.count, 0);
    expect(total).toBe(4);
    expect(buckets.find((b) => b.label === '< -50%')!.count).toBe(1);
    expect(buckets.find((b) => b.label === '-10…0%')!.count).toBe(1);
    expect(buckets.find((b) => b.label === '0…10%')!.count).toBe(1);
    expect(buckets.find((b) => b.label === '10…20%')!.count).toBe(1);
  });

  it('every bucket has a stable win/loss/breakeven sign', () => {
    const buckets = pnlDistributionBuckets([]);
    expect(buckets.filter((b) => b.sign === -1).length).toBeGreaterThan(0);
    expect(buckets.filter((b) => b.sign === 1).length).toBeGreaterThan(0);
  });
});

describe('buildEquityCurve', () => {
  it('accumulates total pnl in ascending time order regardless of input order', () => {
    const t1 = Date.parse('2026-07-01T00:00:00Z');
    const t2 = Date.parse('2026-07-02T00:00:00Z');
    const t3 = Date.parse('2026-07-03T00:00:00Z');
    const rows = [
      row({ wallet_last_trade_at_ms: t3, wallet_total_pnl_sol: 1 }),
      row({ wallet_last_trade_at_ms: t1, wallet_total_pnl_sol: 2 }),
      row({ wallet_last_trade_at_ms: t2, wallet_total_pnl_sol: -1 }),
    ];
    const curve = buildEquityCurve(rows);
    expect(curve.map((p) => p.cumPnlSol)).toEqual([2, 1, 2]);
    expect(curve[0]!.time).toBeLessThan(curve[1]!.time);
    expect(curve[1]!.time).toBeLessThan(curve[2]!.time);
  });

  it('collapses same-second ties into one point', () => {
    const t = Date.parse('2026-07-01T00:00:00.100Z');
    const t2 = Date.parse('2026-07-01T00:00:00.900Z'); // same second
    const rows = [
      row({ wallet_last_trade_at_ms: t, wallet_total_pnl_sol: 1 }),
      row({ wallet_last_trade_at_ms: t2, wallet_total_pnl_sol: 1 }),
    ];
    const curve = buildEquityCurve(rows);
    expect(curve).toHaveLength(1);
    expect(curve[0]!.cumPnlSol).toBe(2);
  });
});

describe('buildHoldScatter', () => {
  it('excludes rows with no matched verdict or non-positive hold', () => {
    const t0 = Date.parse('2026-07-01T00:00:00Z');
    const t1 = Date.parse('2026-07-01T00:05:00Z');
    const rows = [
      row({
        wallet_first_trade_at_ms: t0,
        wallet_last_trade_at_ms: t1,
        wallet_realized_pnl_pct: 20,
        wallet_realized_pnl_sol: 0.2,
      }),
      row({ wallet_realized_pnl_pct: null }), // no verdict
      row({
        wallet_first_trade_at_ms: t1,
        wallet_last_trade_at_ms: t1,
        wallet_realized_pnl_pct: 5,
      }), // zero hold
    ];
    const points = buildHoldScatter(rows);
    expect(points).toHaveLength(1);
    expect(points[0]!.holdSeconds).toBeCloseTo(300, 9);
    expect(points[0]!.isWin).toBe(true);
  });
});
