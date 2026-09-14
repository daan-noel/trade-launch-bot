/**
 * Trader Analysis focus — maps per-mint `TraderTokenRow`s onto the shared
 * `PositionFocusLens` / `filterRowsByFocus` contract used by Evidence /
 * Simulate / Sweep. Independent of Console `hfocus` (no URL yet).
 *
 * Grain caveat: one row = one mint in the look-back window (not a per-episode
 * ledger). Timing lenses bucket on `wallet_last_trade_at`; hold/band use the
 * first→last trade span; outcome / pct use net-of-fee realized PnL only.
 */

import {
  filterRowsByFocus,
  type FocusMatchOpts,
  type PositionFocusLens,
  type PositionFocusRow,
} from 'lib/strategy/positionFocus';
import { isWalletTrade, walletHoldSeconds, walletNetPct } from './walletPnlStats';
import type { TraderTokenRow } from 'types';

export type { PositionFocusLens };

export type WalletFocusRow = PositionFocusRow & { _row: TraderTokenRow };

/** Project a trader mint-row into the shared focus predicate shape. */
export function traderRowToFocusRow(r: TraderTokenRow): WalletFocusRow {
  const lastMs = r.wallet_last_trade_at_ms ?? Date.parse(r.wallet_last_trade_at);
  // Win/loss / pct only on a trade (something sold against a cost basis), on
  // the page's net-of-fee basis — the same helpers `computeWalletSummary` counts
  // with, so a Winners tile and the rows it focuses agree.
  return {
    id: r.mint_address,
    mint_address: r.mint_address,
    fired: true,
    isOpen: r.wallet_is_open,
    isClosed: !r.wallet_is_open,
    exit_reason: null,
    pnl_sol: isWalletTrade(r) ? r.wallet_realized_pnl_sol_net_of_fee : null,
    pnl_pct: walletNetPct(r),
    hold_secs: walletHoldSeconds(r),
    is_migrated: r.is_migrated,
    timeMs: Number.isFinite(lastMs) ? lastMs : null,
    _row: r,
  };
}

/** Apply stacked focus lenses; returns the original trader rows. */
export function filterTraderRowsByFocus(
  rows: readonly TraderTokenRow[],
  lenses: readonly PositionFocusLens[],
  opts?: FocusMatchOpts,
): TraderTokenRow[] {
  if (lenses.length === 0) return rows as TraderTokenRow[];
  return filterRowsByFocus(rows.map(traderRowToFocusRow), lenses, opts).map((r) => r._row);
}
