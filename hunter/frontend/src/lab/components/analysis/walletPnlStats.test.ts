import { describe, expect, it } from 'vitest';
import type { TraderTokenRow, WalletEpisode } from 'types';
import {
  buildHoldScatter,
  computeWalletSummary,
  dowHourInTz,
  rankedPnlBarRows,
  toPnlPoints,
  walletHoldSeconds,
  walletRowCounts,
  walletRowNetSol,
  walletRowOpenSol,
  walletRowPct,
  walletTrades,
} from './walletPnlStats';
import { rankByValue } from 'components/analytics/pnlSeries';

const T0 = Date.parse('2026-07-01T00:00:00Z');
let seq = 0;

/** One trade; each call gets its own entry slot so trade keys stay unique. */
function ep(overrides: Partial<WalletEpisode>): WalletEpisode {
  seq += 1;
  return {
    status: 'closed',
    missing_flow: false,
    unseen_buy: false,
    entry_slot: seq,
    entry_tx_index: 0,
    entry_ms: T0,
    exit_slot: 10_000 + seq,
    exit_tx_index: 0,
    exit_ms: T0 + 60_000,
    buy_count: 1,
    sell_count: 1,
    bought_tokens: 1_000,
    sold_tokens: 1_000,
    held_tokens: 0,
    sol_in: 1,
    sol_out: 1,
    net_sol: 0,
    pnl_pct: 0,
    mark_sol: null,
    open_pnl_sol: null,
    ...overrides,
  };
}

/** A closed trade: `net` ◎ on `cost` ◎ in, closing at tape slot `exitSlot`. */
function closed(net: number, cost = 1, overrides: Partial<WalletEpisode> = {}): WalletEpisode {
  return ep({ sol_in: cost, sol_out: cost + net, net_sol: net, pnl_pct: (net / cost) * 100, ...overrides });
}

function open(openPnl: number | null): WalletEpisode {
  return ep({
    status: 'open',
    exit_slot: null,
    exit_tx_index: null,
    exit_ms: null,
    net_sol: null,
    pnl_pct: null,
    held_tokens: 500,
    open_pnl_sol: openPnl,
  });
}

function incomplete(why: 'missing_flow' | 'unseen_buy'): WalletEpisode {
  const exact = why === 'unseen_buy';
  return ep({
    status: 'incomplete',
    [why]: true,
    sol_in: exact ? 1 : null,
    sol_out: exact ? 2 : null,
    net_sol: null,
    pnl_pct: null,
  });
}

/** Minimal valid `TraderTokenRow` holding `episodes`. */
function row(episodes: WalletEpisode[], overrides: Partial<TraderTokenRow> = {}): TraderTokenRow {
  const base: TraderTokenRow = {
    mint_address: overrides.mint_address ?? `Mint${seq}`,
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
    episodes,
    co_traders: [],
  };
  return { ...base, ...overrides };
}

const summarize = (rows: TraderTokenRow[]) => computeWalletSummary(rows, walletTrades(rows));

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

describe('walletTrades', () => {
  it('flattens every token row into trades keyed by mint and entry', () => {
    const a = closed(1, 1, { entry_slot: 5, entry_tx_index: 2 });
    const trades = walletTrades([row([a, open(0)], { mint_address: 'A' }), row([], { mint_address: 'B' })]);
    expect(trades).toHaveLength(2);
    expect(trades[0]!.key).toBe('A::5:2');
    expect(trades[0]!.row.mint_address).toBe('A');
  });
});

describe('per-token figures', () => {
  it('sums the closed trades only, money over the capital that earned it', () => {
    const r = row([closed(0.5, 1), closed(-0.25, 0.5), open(3), incomplete('unseen_buy')]);
    expect(walletRowNetSol(r)).toBeCloseTo(0.25, 9);
    expect(walletRowPct(r)).toBeCloseTo((0.25 / 1.5) * 100, 9);
    expect(walletRowOpenSol(r)).toBeCloseTo(3, 9);
    expect(walletRowCounts(r)).toEqual({ closed: 2, open: 1, incomplete: 1 });
  });

  it('is blank, never 0, without a closed trade', () => {
    const r = row([open(null), incomplete('missing_flow')]);
    expect(walletRowNetSol(r)).toBeNull();
    expect(walletRowPct(r)).toBeNull();
    expect(walletRowOpenSol(r)).toBeNull();
  });
});

describe('computeWalletSummary', () => {
  it('is all-zero/null for an empty row set', () => {
    const s = summarize([]);
    expect(s.tokenCount).toBe(0);
    expect(s.tradeCount).toBe(0);
    expect(s.winRate).toBeNull();
    expect(s.returnPct).toBeNull();
    expect(s.medianPct).toBeNull();
    expect(s.payoffRatio).toBeNull();
    expect(s.profitFactor).toBeNull();
    expect(s.openPnlSol).toBeNull();
  });

  it('sums closed trades and keeps open and incomplete ones apart', () => {
    const rows = [
      row([closed(1.0, 2), closed(-0.5, 2)]),
      row([open(2.0), incomplete('missing_flow'), incomplete('unseen_buy')]),
    ];
    const s = summarize(rows);
    expect(s.tokenCount).toBe(2);
    expect(s.tradeCount).toBe(2);
    expect(s.openCount).toBe(1);
    expect(s.incompleteCount).toBe(2);
    expect(s.missingFlowCount).toBe(1);
    expect(s.unseenBuyCount).toBe(1);
    expect(s.winCount).toBe(1);
    expect(s.lossCount).toBe(1);
    expect(s.winRate).toBeCloseTo(50, 9);
    expect(s.netSol).toBeCloseTo(0.5, 9);
    expect(s.openPnlSol).toBeCloseTo(2.0, 9);
    // Σ net / Σ SOL in of the closed trades: 0.5 / 4. The incomplete trade's
    // exact-looking SOL is not in it.
    expect(s.returnPct).toBeCloseTo(12.5, 9);
    expect(s.capitalInSol).toBeCloseTo(4, 9);
    expect(s.expectancySol).toBeCloseTo(0.25, 9);
    expect(s.avgWinSol).toBeCloseTo(1.0, 9);
    expect(s.avgLossSol).toBeCloseTo(-0.5, 9);
    expect(s.payoffRatio).toBeCloseTo(2.0, 9);
    expect(s.profitFactor).toBeCloseTo(2.0, 9);
  });

  it('counts a re-entry as its own trade', () => {
    const s = summarize([row([closed(1), closed(-1), closed(1)])]);
    expect(s.tokenCount).toBe(1);
    expect(s.tradeCount).toBe(3);
  });

  it('reports the per-trade % distribution by nearest rank', () => {
    // PnL % of -50, -10, 0, 20, 40, 60, 100, 150, 200, 400 on 1 ◎ each.
    const nets = [-0.5, -0.1, 0, 0.2, 0.4, 0.6, 1, 1.5, 2, 4];
    const s = summarize([row(nets.map((n) => closed(n)))]);
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
    const s = summarize([row([closed(0.1, 0.1), closed(-5, 10)])]);
    expect(s.meanPct).toBeCloseTo(25, 9);
    expect(s.returnPct).toBeLessThan(0);
    expect(Math.sign(s.returnPct!)).toBe(Math.sign(s.netSol));
  });

  it('counts the longest losing run in closing-sell tape order, not input order', () => {
    const at = (slot: number) => ({ exit_slot: slot, exit_ms: T0 });
    const trades = [
      closed(-1, 1, at(4)),
      closed(1, 1, at(1)),
      closed(-1, 1, at(3)),
      closed(-1, 1, at(2)),
      closed(1, 1, at(5)),
    ];
    expect(summarize([row(trades)]).longestLossStreak).toBe(3);
  });

  it('max drawdown is the most lost in one stretch, in tape order, open trades out', () => {
    // +1, +2 (peak 3), -2, -3 (now -2) → 5. All in one second, so only the tape
    // order separates them; an open trade's estimate never enters.
    const at = (slot: number) => ({ exit_slot: slot, exit_ms: T0 });
    const trades = [closed(-3, 3, at(4)), closed(2, 1, at(2)), closed(1, 1, at(1)), closed(-2, 2, at(3)), open(-10)];
    expect(summarize([row(trades)]).maxDrawdownSol).toBeCloseTo(5, 9);
  });

  it('max drawdown folds by closing-sell time before tape order', () => {
    // Time order +2, -1, -1 → 2. Tape order alone (-1, +2, -1) would read 1.
    const trades = [
      closed(-1, 1, { exit_slot: 1, exit_ms: T0 + 2_000 }),
      closed(2, 1, { exit_slot: 2, exit_ms: T0 + 1_000 }),
      closed(-1, 1, { exit_slot: 3, exit_ms: T0 + 3_000 }),
    ];
    expect(summarize([row(trades)]).maxDrawdownSol).toBeCloseTo(2, 9);
  });

  it('reads the behavior medians and sums the capital', () => {
    const rows = [
      row([closed(0.1, 1, { entry_ms: T0, exit_ms: T0 + 60_000 })], { wallet_entry_curve_pct: 10 }),
      row([closed(0.1, 3, { entry_ms: T0, exit_ms: T0 + 600_000 })], { wallet_entry_curve_pct: 30 }),
      row([open(0)], { wallet_entry_curve_pct: 20 }),
    ];
    const s = summarize(rows);
    expect(s.capitalInSol).toBeCloseTo(4, 9);
    expect(s.capitalOutSol).toBeCloseTo(4.2, 9);
    // Closed trades only: [1, 3] → round(0.5) = 1 → 3.
    expect(s.medianBuySol).toBeCloseTo(3, 9);
    expect(s.medianEntryCurvePct).toBeCloseTo(20, 9);
    // One round trip each: [60, 600] → 600.
    expect(s.medianHoldSecs).toBeCloseTo(600, 9);
  });

  it('reports null payoff/profit-factor with no losses', () => {
    const s = summarize([row([closed(1.0)])]);
    expect(s.payoffRatio).toBeNull();
    expect(s.profitFactor).toBeNull();
    expect(s.worstTradeSol).toBeNull();
  });
});

describe('toPnlPoints', () => {
  it('makes one point per closed trade, in closing-sell tape order', () => {
    const trades = walletTrades([
      row([closed(1, 1, { exit_slot: 30 }), open(1), incomplete('missing_flow')]),
      row([closed(-1, 1, { exit_slot: 20 })]),
    ]);
    const points = toPnlPoints(trades);
    expect(points.map((p) => p.pnlSol)).toEqual([-1, 1]);
  });
});

describe('rankedPnlBarRows', () => {
  it('ranks closed trades by net ◎ without mutating the input', () => {
    const rows = [row([closed(-1), closed(5)], { mint_address: 'a' }), row([closed(2), open(9)], { mint_address: 'b' })];
    const trades = walletTrades(rows);
    const ranked = rankByValue(rankedPnlBarRows(trades));
    expect(ranked.map((r) => r.value)).toEqual([5, 2, -1]);
    expect(trades).toHaveLength(4);
  });
});

describe('walletHoldSeconds', () => {
  it('returns first→last span in seconds', () => {
    const t1 = T0 + 300_000;
    expect(walletHoldSeconds(row([], { wallet_first_trade_at_ms: T0, wallet_last_trade_at_ms: t1 }))).toBeCloseTo(300, 9);
  });
});

describe('buildHoldScatter', () => {
  it('plots closed trades with a positive hold, one point per round trip', () => {
    const trades = walletTrades([
      row([
        closed(0.2, 1, { entry_ms: T0, exit_ms: T0 + 300_000 }),
        closed(0.05, 1, { entry_ms: T0, exit_ms: T0 }), // zero hold
        open(1), // no verdict
      ]),
    ]);
    const points = buildHoldScatter(trades);
    expect(points).toHaveLength(1);
    expect(points[0]!.holdSeconds).toBeCloseTo(300, 9);
    expect(points[0]!.pnlPct).toBeCloseTo(20, 9);
    expect(points[0]!.isWin).toBe(true);
  });
});
