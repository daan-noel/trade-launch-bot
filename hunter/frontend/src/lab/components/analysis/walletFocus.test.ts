import { describe, expect, it } from 'vitest';
import { togglePositionFocus } from 'lib/strategy/positionFocus';
import type { TraderTokenRow } from 'types';
import { filterTraderRowsByFocus, traderRowToFocusRow } from './walletFocus';

/** Minimal valid `TraderTokenRow`; overrides only what the case needs. */
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

describe('traderRowToFocusRow', () => {
  it('maps open / closed and hold span from first→last trade', () => {
    const open = traderRowToFocusRow(
      row({
        mint_address: 'OpenMint',
        wallet_is_open: true,
        wallet_realized_pnl_pct: null,
        wallet_realized_pnl_sol: 0,
        wallet_unrealized_pnl_sol: 0.2,
        wallet_total_pnl_sol: 0.2,
        wallet_first_trade_at: '2026-07-01T00:00:00Z',
        wallet_last_trade_at: '2026-07-01T00:01:00Z',
      }),
    );
    expect(open.isOpen).toBe(true);
    expect(open.isClosed).toBe(false);
    expect(open.pnl_sol).toBeNull(); // no realized verdict
    expect(open.hold_secs).toBe(60);
    expect(open.id).toBe('OpenMint');
  });
});

describe('filterTraderRowsByFocus', () => {
  const win = row({
    mint_address: 'WinMint',
    wallet_realized_pnl_sol: 1,
    wallet_realized_pnl_pct: 20,
    wallet_is_open: false,
    wallet_last_trade_at: '2026-07-27T14:30:00Z', // Mon 14:00 UTC
  });
  const loss = row({
    mint_address: 'LossMint',
    wallet_realized_pnl_sol: -0.5,
    wallet_realized_pnl_pct: -10,
    wallet_is_open: false,
    wallet_last_trade_at: '2026-07-27T14:30:00Z',
  });
  const openBag = row({
    mint_address: 'OpenMint',
    wallet_is_open: true,
    wallet_realized_pnl_pct: null,
    wallet_realized_pnl_sol: 0,
    wallet_unrealized_pnl_sol: 0.3,
    wallet_total_pnl_sol: 0.3,
    wallet_last_trade_at: '2026-07-28T08:00:00Z',
  });
  const rows = [win, loss, openBag];

  it('filters open / closed / outcome', () => {
    expect(filterTraderRowsByFocus(rows, [{ kind: 'status', status: 'open' }]).map((r) => r.mint_address)).toEqual([
      'OpenMint',
    ]);
    expect(
      filterTraderRowsByFocus(rows, [{ kind: 'status', status: 'closed' }]).map((r) => r.mint_address),
    ).toEqual(['WinMint', 'LossMint']);
    expect(
      filterTraderRowsByFocus(rows, [{ kind: 'outcome', outcome: 'win' }]).map((r) => r.mint_address),
    ).toEqual(['WinMint']);
    // Open bag with no realized % is neither win nor loss.
    expect(filterTraderRowsByFocus(rows, [{ kind: 'outcome', outcome: 'loss' }]).map((r) => r.mint_address)).toEqual([
      'LossMint',
    ]);
  });

  it('stacks heat + outcome and toggles off', () => {
    let lenses = togglePositionFocus([], { kind: 'heat', dow: 1, hour: 14 });
    lenses = togglePositionFocus(lenses, { kind: 'outcome', outcome: 'win' });
    const out = filterTraderRowsByFocus(rows, lenses, { timeZone: 'UTC' });
    expect(out.map((r) => r.mint_address)).toEqual(['WinMint']);

    lenses = togglePositionFocus(lenses, { kind: 'heat', dow: 1, hour: 14 });
    expect(lenses).toEqual([{ kind: 'outcome', outcome: 'win' }]);
  });

  it('focuses a single mint via pos', () => {
    expect(
      filterTraderRowsByFocus(rows, [{ kind: 'pos', positionId: 'LossMint' }]).map((r) => r.mint_address),
    ).toEqual(['LossMint']);
  });
});
