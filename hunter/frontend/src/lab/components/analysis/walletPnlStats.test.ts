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
  walletHoldSeconds,
  walletNetPct,
  walletTotalSol,
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
    wallet_matched_cost_sol: 1,
    wallet_realized_pnl_sol: 0.5,
    wallet_realized_pnl_sol_net_of_fee: 0.475,
    wallet_realized_pnl_pct: 50,
    wallet_unrealized_pnl_sol: null,
    wallet_total_pnl_sol: 0.5,
    wallet_is_open: false,
    wallet_partial_data: false,
    wallet_entry_at: null,
    wallet_exit_at: null,
    wallet_entry_curve_sol: null,
    wallet_entry_curve_pct: null,
    wallet_exit_curve_sol: null,
    wallet_exit_curve_pct: null,
    wallet_entry_slot: null,
    wallet_entry_tx_index: null,
    wallet_exit_slot: null,
    wallet_exit_tx_index: null,
    co_traders: [],
  };
  return { ...base, ...overrides };
}

/** A sold-against-cost row: `net` ◎ net of fee on `cost` ◎ of matched basis. */
function trade(net: number, overrides: Partial<TraderTokenRow> = {}, cost = 1): TraderTokenRow {
  return row({
    wallet_matched_cost_sol: cost,
    wallet_realized_pnl_sol_net_of_fee: net,
    ...overrides,
  });
}

/** A never-sold bag: no matched cost, so no realized verdict. */
function openBag(mark: number, overrides: Partial<TraderTokenRow> = {}): TraderTokenRow {
  return row({
    wallet_matched_cost_sol: 0,
    wallet_realized_pnl_sol: 0,
    wallet_realized_pnl_sol_net_of_fee: 0,
    wallet_realized_pnl_pct: null,
    wallet_unrealized_pnl_sol: mark,
    wallet_is_open: true,
    ...overrides,
  });
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

describe('per-token net figures', () => {
  it('net % is net realized over the matched cost', () => {
    expect(walletNetPct(trade(0.25, {}, 0.5))).toBeCloseTo(50, 9);
    expect(walletNetPct(openBag(1))).toBeNull();
  });

  it('total is net realized plus the open mark', () => {
    expect(walletTotalSol(trade(0.2, { wallet_unrealized_pnl_sol: 0.3 }))).toBeCloseTo(0.5, 9);
    expect(walletTotalSol(openBag(-0.4))).toBeCloseTo(-0.4, 9);
  });
});

describe('computeWalletSummary', () => {
  it('is all-zero/null for an empty row set', () => {
    const s = computeWalletSummary([]);
    expect(s.tokenCount).toBe(0);
    expect(s.tradeCount).toBe(0);
    expect(s.winRate).toBeNull();
    expect(s.returnPct).toBeNull();
    expect(s.medianPct).toBeNull();
    expect(s.payoffRatio).toBeNull();
    expect(s.profitFactor).toBeNull();
  });

  it('separates open bags from win/loss verdicts, on the net basis', () => {
    const rows = [
      trade(1.0, { wallet_realized_pnl_sol: 1.05 }, 2),
      trade(-0.5, { wallet_realized_pnl_sol: -0.45 }, 2),
      // Pure open bag: never sold, so no realized verdict — excluded from win/loss.
      openBag(2.0),
    ];
    const s = computeWalletSummary(rows);
    expect(s.tokenCount).toBe(3);
    expect(s.openCount).toBe(1);
    expect(s.tradeCount).toBe(2);
    expect(s.winCount).toBe(1);
    expect(s.lossCount).toBe(1);
    expect(s.winRate).toBeCloseTo(50, 9);
    expect(s.grossRealizedSol).toBeCloseTo(0.6, 9);
    expect(s.netRealizedSol).toBeCloseTo(0.5, 9);
    expect(s.openMarkSol).toBeCloseTo(2.0, 9);
    expect(s.totalSol).toBeCloseTo(2.5, 9);
    // Σ net / Σ matched cost: 0.5 / 4.
    expect(s.returnPct).toBeCloseTo(12.5, 9);
    expect(s.expectancySol).toBeCloseTo(0.25, 9);
    expect(s.avgWinSol).toBeCloseTo(1.0, 9);
    expect(s.avgLossSol).toBeCloseTo(-0.5, 9);
    expect(s.payoffRatio).toBeCloseTo(2.0, 9);
    expect(s.profitFactor).toBeCloseTo(2.0, 9);
  });

  it('a gross winner the fee turns red is a loss', () => {
    const s = computeWalletSummary([trade(-0.01, { wallet_realized_pnl_sol: 0.01 })]);
    expect(s.winCount).toBe(0);
    expect(s.lossCount).toBe(1);
    expect(s.worstTradeSol).toBeCloseTo(-0.01, 9);
  });

  it('reports the per-trade % distribution by nearest rank', () => {
    // Net % of -50, -10, 0, 20, 40, 60, 100, 150, 200, 400 on 1 ◎ each.
    const nets = [-0.5, -0.1, 0, 0.2, 0.4, 0.6, 1, 1.5, 2, 4];
    const s = computeWalletSummary(nets.map((n) => trade(n)));
    expect(s.tradeCount).toBe(10);
    expect(s.meanPct).toBeCloseTo(91, 9);
    // round(9 × 0.5) = 5 → 60; round(0.9) = 1 → -10; round(8.1) = 8 → 200.
    expect(s.medianPct).toBeCloseTo(60, 9);
    expect(s.p10Pct).toBeCloseTo(-10, 9);
    expect(s.p90Pct).toBeCloseTo(200, 9);
    expect(s.bestPct).toBeCloseTo(400, 9);
    expect(s.worstPct).toBeCloseTo(-50, 9);
    expect(s.worstTradeSol).toBeCloseTo(-0.5, 9);
    expect(s.worstTradePct).toBeCloseTo(-50, 9);
  });

  it('return % weights by capital while mean % weights every trade the same', () => {
    // +100 % on 0.1 ◎ and -50 % on 10 ◎: the mean is positive, the money is not.
    const s = computeWalletSummary([trade(0.1, {}, 0.1), trade(-5, {}, 10)]);
    expect(s.meanPct).toBeCloseTo(25, 9);
    expect(s.returnPct).toBeLessThan(0);
    expect(Math.sign(s.returnPct!)).toBe(Math.sign(s.netRealizedSol));
  });

  it('counts the longest losing run in last-trade order, not row order', () => {
    const at = (min: number) => Date.parse('2026-07-01T00:00:00Z') + min * 60_000;
    const rows = [
      trade(-1, { wallet_last_trade_at_ms: at(4) }),
      trade(1, { wallet_last_trade_at_ms: at(1) }),
      trade(-1, { wallet_last_trade_at_ms: at(3) }),
      trade(-1, { wallet_last_trade_at_ms: at(2) }),
      trade(1, { wallet_last_trade_at_ms: at(5) }),
    ];
    expect(computeWalletSummary(rows).longestLossStreak).toBe(3);
  });

  it('max drawdown is the equity curve drop on the total ◎', () => {
    const at = (min: number) => Date.parse('2026-07-01T00:00:00Z') + min * 60_000;
    const rows = [
      trade(2, { wallet_last_trade_at_ms: at(1) }),
      trade(-1.5, { wallet_last_trade_at_ms: at(2) }),
      // An open mark counts: the curve sums the Total, not realized alone.
      openBag(-1, { wallet_last_trade_at_ms: at(3) }),
      trade(3, { wallet_last_trade_at_ms: at(4) }),
    ];
    expect(computeWalletSummary(rows).maxDrawdownSol).toBeCloseTo(2.5, 9);
  });

  it('reads the behavior medians and sums the capital', () => {
    const t0 = Date.parse('2026-07-01T00:00:00Z');
    const rows = [
      trade(0.1, {
        wallet_buy_sol: 1,
        wallet_sell_sol: 1.1,
        wallet_first_trade_at_ms: t0,
        wallet_last_trade_at_ms: t0 + 60_000,
        wallet_entry_curve_pct: 10,
      }),
      trade(0.1, {
        wallet_buy_sol: 3,
        wallet_sell_sol: 3.1,
        wallet_first_trade_at_ms: t0,
        wallet_last_trade_at_ms: t0 + 600_000,
        wallet_entry_curve_pct: 30,
      }),
      openBag(0, { wallet_buy_sol: 2, wallet_sell_sol: 0, wallet_entry_curve_pct: 20 }),
    ];
    const s = computeWalletSummary(rows);
    expect(s.capitalInSol).toBeCloseTo(6, 9);
    expect(s.volumeSol).toBeCloseTo(10.2, 9);
    expect(s.medianBuySol).toBeCloseTo(2, 9);
    expect(s.medianEntryCurvePct).toBeCloseTo(20, 9);
    // Hold is read over trades only: [60, 600] → round(0.5) = 1 → 600.
    expect(s.medianHoldSecs).toBeCloseTo(600, 9);
  });

  it('reports null payoff/profit-factor with no losses', () => {
    const s = computeWalletSummary([trade(1.0)]);
    expect(s.payoffRatio).toBeNull();
    expect(s.profitFactor).toBeNull();
    expect(s.worstTradeSol).toBeNull();
  });

  it('flags partial data', () => {
    const rows = [row({ wallet_partial_data: true }), row({ wallet_partial_data: false })];
    expect(computeWalletSummary(rows).partialDataCount).toBe(1);
  });
});

describe('buildPnlHeatCells', () => {
  it('always returns all 168 day×hour cells', () => {
    const cells = buildPnlHeatCells([], 'UTC');
    expect(cells).toHaveLength(7 * 24);
    expect(cells.every((c) => c.count === 0 && c.pnl_sol === 0)).toBe(true);
  });

  it('buckets a row into its last-trade dow/hour and sums the net total', () => {
    const ms = Date.parse('2026-07-27T14:30:00Z'); // Monday 14:00 UTC bucket
    const rows = [
      trade(1.5, { wallet_last_trade_at_ms: ms }),
      openBag(-0.5, { wallet_last_trade_at_ms: ms }),
    ];
    const cells = buildPnlHeatCells(rows, 'UTC');
    const cell = cells.find((c) => c.dow === 1 && c.hour === 14);
    expect(cell).toBeDefined();
    expect(cell!.count).toBe(2);
    expect(cell!.pnl_sol).toBeCloseTo(1.0, 9);
  });

  it('falls back to parsing wallet_last_trade_at when the _ms field is absent', () => {
    const rows = [
      trade(2.0, {
        wallet_last_trade_at: '2026-07-27T14:30:00Z',
        wallet_last_trade_at_ms: undefined,
      }),
    ];
    const cells = buildPnlHeatCells(rows, 'UTC');
    const cell = cells.find((c) => c.dow === 1 && c.hour === 14);
    expect(cell!.pnl_sol).toBeCloseTo(2.0, 9);
  });
});

describe('rankedPnlBarRows', () => {
  it('sorts descending by net total without mutating the input', () => {
    const rows = [
      trade(-1, { mint_address: 'a' }),
      trade(5, { mint_address: 'b' }),
      trade(2, { mint_address: 'c' }),
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
  it('buckets net % and excludes rows with no matched cost basis', () => {
    const rows = [
      trade(-0.6), // < -50
      trade(-0.05), // -10..0
      trade(0), // 0..10 (half-open [0,10))
      trade(0.15), // 10..20
      trade(2.5), // 200…500
      openBag(1), // excluded — pure open bag
    ];
    const buckets = pnlDistributionBuckets(rows);
    const total = buckets.reduce((s, b) => s + b.count, 0);
    expect(total).toBe(5);
    expect(buckets.find((b) => b.label === '< -50%')!.count).toBe(1);
    expect(buckets.find((b) => b.label === '-10…0%')!.count).toBe(1);
    expect(buckets.find((b) => b.label === '0…10%')!.count).toBe(1);
    expect(buckets.find((b) => b.label === '10…20%')!.count).toBe(1);
    expect(buckets.find((b) => b.label === '200…500%')!.count).toBe(1);
    expect(buckets.some((b) => b.label === '≥ 500%')).toBe(true);
  });

  it('sparse density collapses the near-zero zone', () => {
    const rows = [trade(-0.05), trade(0.15), trade(1.2)];
    const buckets = pnlDistributionBuckets(rows, 'sparse');
    expect(buckets.map((b) => b.label)).toEqual([
      '< -50%',
      '-50…0%',
      '0…50%',
      '50…100%',
      '100…200%',
      '200…500%',
      '≥ 500%',
    ]);
    expect(buckets.find((b) => b.label === '-50…0%')!.count).toBe(1);
    expect(buckets.find((b) => b.label === '0…50%')!.count).toBe(1);
    expect(buckets.find((b) => b.label === '100…200%')!.count).toBe(1);
  });

  it('every bucket has a stable win/loss/breakeven sign', () => {
    const buckets = pnlDistributionBuckets([]);
    expect(buckets.filter((b) => b.sign === -1).length).toBeGreaterThan(0);
    expect(buckets.filter((b) => b.sign === 1).length).toBeGreaterThan(0);
  });
});

describe('buildEquityCurve', () => {
  it('accumulates the net total in ascending time order regardless of input order', () => {
    const t1 = Date.parse('2026-07-01T00:00:00Z');
    const t2 = Date.parse('2026-07-02T00:00:00Z');
    const t3 = Date.parse('2026-07-03T00:00:00Z');
    const rows = [
      trade(1, { wallet_last_trade_at_ms: t3 }),
      trade(2, { wallet_last_trade_at_ms: t1 }),
      trade(-1, { wallet_last_trade_at_ms: t2 }),
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
      trade(1, { wallet_last_trade_at_ms: t }),
      trade(1, { wallet_last_trade_at_ms: t2 }),
    ];
    const curve = buildEquityCurve(rows);
    expect(curve).toHaveLength(1);
    expect(curve[0]!.cumPnlSol).toBe(2);
  });
});

describe('walletHoldSeconds', () => {
  it('returns first→last span in seconds', () => {
    const t0 = Date.parse('2026-07-01T00:00:00Z');
    const t1 = Date.parse('2026-07-01T00:05:00Z');
    expect(
      walletHoldSeconds(
        row({ wallet_first_trade_at_ms: t0, wallet_last_trade_at_ms: t1 }),
      ),
    ).toBeCloseTo(300, 9);
  });
});

describe('buildHoldScatter', () => {
  it('excludes rows with no matched verdict or non-positive hold', () => {
    const t0 = Date.parse('2026-07-01T00:00:00Z');
    const t1 = Date.parse('2026-07-01T00:05:00Z');
    const rows = [
      trade(0.2, { wallet_first_trade_at_ms: t0, wallet_last_trade_at_ms: t1 }),
      openBag(1), // no verdict
      trade(0.05, { wallet_first_trade_at_ms: t1, wallet_last_trade_at_ms: t1 }), // zero hold
    ];
    const points = buildHoldScatter(rows);
    expect(points).toHaveLength(1);
    expect(points[0]!.holdSeconds).toBeCloseTo(300, 9);
    expect(points[0]!.pnlPct).toBeCloseTo(20, 9);
    expect(points[0]!.isWin).toBe(true);
  });
});
