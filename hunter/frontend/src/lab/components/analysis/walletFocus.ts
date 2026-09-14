/**
 * Trader Analysis focus — maps the wallet's trades onto the shared
 * `PositionFocusLens` / `filterRowsByFocus` contract used by Evidence /
 * Simulate / Sweep. Independent of Console `hfocus` (no URL yet).
 *
 * A lens matches TRADES (round trips, `walletTrades`): outcome / pct / band on a
 * closed trade's exact figures, hold on its round trip, timing on its closing
 * sell, `pos` on its key. The table stays one row per token, so a token shows
 * when any of its trades matches, and the summary folds only the matching trades.
 */

import {
  filterRowsByFocus,
  type FocusMatchOpts,
  type PositionFocusLens,
  type PositionFocusRow,
} from 'lib/strategy/positionFocus';
import { tradeHoldSeconds, tradeNetSol, tradeTimeMs, walletTrades, type WalletTrade } from './walletPnlStats';
import type { TraderTokenRow } from 'types';

export type { PositionFocusLens };

export type WalletFocusRow = PositionFocusRow & { _trade: WalletTrade };

/** Project one trade into the shared focus predicate shape. Win / loss and % only
 *  on a closed trade — the same `tradeNetSol` the summary counts with, so a
 *  Winners tile and the trades it focuses agree. */
export function tradeToFocusRow(t: WalletTrade): WalletFocusRow {
  const net = tradeNetSol(t.ep);
  return {
    id: t.key,
    mint_address: t.row.mint_address,
    fired: true,
    isOpen: t.ep.status === 'open',
    isClosed: t.ep.status === 'closed',
    exit_reason: null,
    pnl_sol: net,
    pnl_pct: net == null ? null : t.ep.pnl_pct,
    hold_secs: tradeHoldSeconds(t.ep),
    is_migrated: t.row.is_migrated,
    timeMs: tradeTimeMs(t.ep),
    _trade: t,
  };
}

/** The trades every stacked lens matches. */
export function filterTradesByFocus(
  trades: readonly WalletTrade[],
  lenses: readonly PositionFocusLens[],
  opts?: FocusMatchOpts,
): WalletTrade[] {
  if (lenses.length === 0) return trades as WalletTrade[];
  return filterRowsByFocus(trades.map(tradeToFocusRow), lenses, opts).map((r) => r._trade);
}

/** The token rows with at least one matching trade, in their original order. */
export function filterTraderRowsByFocus(
  rows: readonly TraderTokenRow[],
  lenses: readonly PositionFocusLens[],
  opts?: FocusMatchOpts,
): TraderTokenRow[] {
  if (lenses.length === 0) return rows as TraderTokenRow[];
  const hit = new Set(filterTradesByFocus(walletTrades(rows), lenses, opts).map((t) => t.row));
  return rows.filter((r) => hit.has(r));
}
