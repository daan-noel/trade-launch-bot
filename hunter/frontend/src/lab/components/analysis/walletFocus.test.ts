import { describe, expect, it } from 'vitest';
import { togglePositionFocus } from 'lib/strategy/positionFocus';
import type { TraderTokenRow, WalletEpisode } from 'types';
import { filterTradesByFocus, filterTraderRowsByFocus, tradeToFocusRow } from './walletFocus';
import { walletTrades } from './walletPnlStats';

const MON_14 = Date.parse('2026-07-27T14:30:00Z'); // Mon 14:00 UTC
let seq = 0;

/** One trade closing at `exitMs`; `net` ◎ on 1 ◎ in. */
function closed(net: number, exitMs = MON_14): WalletEpisode {
  seq += 1;
  return {
    status: 'closed',
    missing_flow: false,
    unseen_buy: false,
    entry_slot: seq,
    entry_tx_index: 0,
    entry_ms: exitMs - 60_000,
    exit_slot: 10_000 + seq,
    exit_tx_index: 0,
    exit_ms: exitMs,
    buy_count: 1,
    sell_count: 1,
    bought_tokens: 1_000,
    sold_tokens: 1_000,
    held_tokens: 0,
    sol_in: 1,
    sol_out: 1 + net,
    net_sol: net,
    pnl_pct: net * 100,
    mark_sol: null,
    open_pnl_sol: null,
  };
}

function open(entryMs: number): WalletEpisode {
  return {
    ...closed(0),
    status: 'open',
    entry_ms: entryMs,
    exit_slot: null,
    exit_tx_index: null,
    exit_ms: null,
    net_sol: null,
    pnl_pct: null,
    held_tokens: 500,
    open_pnl_sol: 0.3,
  };
}

/** Minimal valid `TraderTokenRow` holding `episodes`. */
function row(mint: string, episodes: WalletEpisode[]): TraderTokenRow {
  return {
    mint_address: mint,
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
}

const mints = (rows: TraderTokenRow[]) => rows.map((r) => r.mint_address);

describe('tradeToFocusRow', () => {
  it('maps a closed trade to its own verdict, hold and closing instant', () => {
    const [t] = walletTrades([row('M', [closed(0.2)])]);
    const f = tradeToFocusRow(t!);
    expect(f.isClosed).toBe(true);
    expect(f.pnl_sol).toBeCloseTo(0.2, 9);
    expect(f.pnl_pct).toBeCloseTo(20, 9);
    expect(f.hold_secs).toBe(60);
    expect(f.timeMs).toBe(MON_14);
    expect(f.id).toBe(t!.key);
  });

  it('gives an open trade no verdict and its entry as the instant', () => {
    const [t] = walletTrades([row('M', [open(MON_14)])]);
    const f = tradeToFocusRow(t!);
    expect(f.isOpen).toBe(true);
    expect(f.isClosed).toBe(false);
    expect(f.pnl_sol).toBeNull();
    expect(f.hold_secs).toBeNull();
    expect(f.timeMs).toBe(MON_14);
  });
});

describe('filterTraderRowsByFocus', () => {
  // `Mixed` re-entered: one winning and one losing trade.
  const rows = [
    row('WinMint', [closed(1)]),
    row('LossMint', [closed(-0.5)]),
    row('Mixed', [closed(0.3), closed(-0.2)]),
    row('OpenMint', [open(Date.parse('2026-07-28T08:00:00Z'))]),
  ];

  it('shows a token when any of its trades matches', () => {
    expect(mints(filterTraderRowsByFocus(rows, [{ kind: 'status', status: 'open' }]))).toEqual(['OpenMint']);
    expect(mints(filterTraderRowsByFocus(rows, [{ kind: 'status', status: 'closed' }]))).toEqual([
      'WinMint',
      'LossMint',
      'Mixed',
    ]);
    expect(mints(filterTraderRowsByFocus(rows, [{ kind: 'outcome', outcome: 'win' }]))).toEqual([
      'WinMint',
      'Mixed',
    ]);
    // An open trade has no verdict: neither win nor loss.
    expect(mints(filterTraderRowsByFocus(rows, [{ kind: 'outcome', outcome: 'loss' }]))).toEqual([
      'LossMint',
      'Mixed',
    ]);
  });

  it('keeps only the matching trades for the summary', () => {
    const losers = filterTradesByFocus(walletTrades(rows), [{ kind: 'outcome', outcome: 'loss' }]);
    expect(losers.map((t) => t.ep.net_sol)).toEqual([-0.5, -0.2]);
  });

  it('stacks heat + outcome and toggles off', () => {
    let lenses = togglePositionFocus([], { kind: 'heat', dow: 1, hour: 14 });
    lenses = togglePositionFocus(lenses, { kind: 'outcome', outcome: 'win' });
    expect(mints(filterTraderRowsByFocus(rows, lenses, { timeZone: 'UTC' }))).toEqual(['WinMint', 'Mixed']);

    lenses = togglePositionFocus(lenses, { kind: 'heat', dow: 1, hour: 14 });
    expect(lenses).toEqual([{ kind: 'outcome', outcome: 'win' }]);
  });

  it('focuses a single trade via pos', () => {
    const loss = walletTrades(rows).find((t) => t.ep.net_sol === -0.2)!;
    const out = filterTraderRowsByFocus(rows, [{ kind: 'pos', positionId: loss.key }]);
    expect(mints(out)).toEqual(['Mixed']);
  });
});
