/**
 * Trader Analysis focus — maps per-mint `TraderTokenRow`s onto the shared
 * `PositionFocusLens` / `filterRowsByFocus` contract used by Evidence /
 * Simulate / Sweep. Independent of Console `hfocus` (no URL yet).
 *
 * Grain caveat: one row = one mint in the look-back window (not a per-episode
 * ledger). Timing lenses bucket on `wallet_last_trade_at`; hold/band use the
 * first→last trade span; outcome / pct use realized PnL only.
 */

import {
  filterRowsByFocus,
  type FocusMatchOpts,
  type PositionFocusLens,
  type PositionFocusRow,
} from 'lib/strategy/positionFocus';
import { walletHoldSeconds } from './walletPnlStats';
import type { TraderTokenRow } from 'types';

export type { PositionFocusLens };

export type WalletFocusRow = PositionFocusRow & { _row: TraderTokenRow };

/** Project a trader mint-row into the shared focus predicate shape. */
export function traderRowToFocusRow(r: TraderTokenRow): WalletFocusRow {
  const lastMs = r.wallet_last_trade_at_ms ?? Date.parse(r.wallet_last_trade_at);
  // Win/loss / pct only when there's a matched cost basis — matches
  // `computeWalletSummary` (open-only bags are neither winners nor losers).
  const hasRealized = r.wallet_realized_pnl_pct != null;
  return {
    id: r.mint_address,
    mint_address: r.mint_address,
    fired: true,
    isOpen: r.wallet_is_open,
    isClosed: !r.wallet_is_open,
    exit_reason: null,
    pnl_sol: hasRealized ? r.wallet_realized_pnl_sol : null,
    pnl_pct: r.wallet_realized_pnl_pct,
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
