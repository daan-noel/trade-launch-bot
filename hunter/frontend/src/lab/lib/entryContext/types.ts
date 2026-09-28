/**
 * Wire types for **Entry Context** (`POST /api/wallets/{wallet}/entry-context`,
 * `lab/src/api/handlers/entry_context.rs`): the tape in the `W` seconds before each
 * of a wallet's buys, read under one tag by the engine's own tag state.
 *
 * Hand-written mirror of the Rust structs; the handler's wire-shape test pins the
 * keys on that side.
 */

import type { FlowSide, FlowTag } from 'lib/flow/classifyFlow';

export type EntryGroupBy = 'exact' | 'template' | 'program';

export interface EntryContextRequest {
  wallet: string;
  /** UTC RFC3339. Anchors are the wallet's buys with `from <= block_time <= to`. */
  from: string;
  to?: string | null;
  window_secs: number;
  /** ONE tag definition in the fingerprint `tags` shape. `side` / `sticky` are
   *  left out when unset: the engine's parser refuses a null. */
  tag: { match: FlowTag['match']; side?: FlowSide; sticky?: true };
  group_by: EntryGroupBy;
}

/** One window read on both halves of the tag. Counts are transactions (the
 *  engine's `leg_index = 0` rule); SOL sums every leg. */
export interface EntryWindowRead {
  buy_tx: number;
  tag_buy_tx: number;
  buy_sol: number;
  tag_buy_sol: number;
  sell_tx: number;
  tag_sell_tx: number;
  sell_sol: number;
  tag_sell_sol: number;
  /** `tag_buy_tx / buy_tx` %, null when the window holds no buy. */
  tx_share_pct: number | null;
  /** `tag_buy_sol / buy_sol` %, null when the window holds no buy SOL. */
  sol_share_pct: number | null;
}

/** One breakdown row: the window's prints of one structure. */
export interface EntryGroupRow {
  key: string;
  /** Ordered labels, `exact` grouping only. */
  labels?: string[];
  buy_tx: number;
  sell_tx: number;
  buy_sol: number;
  sell_sol: number;
  /** Buy transactions of this group that carried the target tag. */
  tag_buy_tx: number;
  wallets: number;
  /** Distinct one-second buckets holding a buy of this group. */
  buy_secs: number;
  buy_tx_share_pct: number | null;
  buy_sol_share_pct: number | null;
}

export type EntryUnknownReason = 'tape-truncated' | 'no-fee-readings';

export const UNKNOWN_REASON_TEXT: Record<EntryUnknownReason, string> = {
  'tape-truncated': 'the read reaches past the oldest tape the trades table still holds',
  'no-fee-readings': 'the tag pins fee fields and no print in the window carries a fee reading',
};

/** One anchor: the trader's buy transaction and the window before it. */
export interface EntryRow {
  mint_address: string;
  slot: number;
  tx_index: number;
  at: string;
  /** SOL his buy legs spent in this transaction. */
  sol: number;
  unknown_reason?: EntryUnknownReason;
  window: EntryWindowRead;
  control: EntryWindowRead;
  /** Largest first (buy tx, then buy SOL). */
  groups: EntryGroupRow[];
  groups_omitted: number;
}

export interface EntryContextResponse {
  entries: EntryRow[];
  truncated: boolean;
  max_entries: number;
  window_secs: number;
  group_by: EntryGroupBy;
  tape_floor?: string | null;
}

/** Stable identity of one anchor. */
export const entryKey = (e: Pick<EntryRow, 'mint_address' | 'slot' | 'tx_index'>): string =>
  `${e.mint_address}:${e.slot}:${e.tx_index}`;
