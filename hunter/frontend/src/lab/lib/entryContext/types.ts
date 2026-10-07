/**
 * Wire types for **Entry Context** (`POST /api/wallets/{wallet}/entry-context`,
 * `lab/src/api/handlers/entry_context.rs`): the tape in the `W` seconds before each
 * of a wallet's buys, read under one tag by the engine's own tag state.
 *
 * Hand-written mirror of the Rust structs; the handler's wire-shape test pins the
 * keys on that side.
 */

import type { FlowSide, FlowTag } from 'lib/flow/classifyFlow';
import type { PreEntryUnknownReason } from '@lab/lib/preEntryProbeTypes';

export interface EntryContextRequest {
  wallet: string;
  /** UTC RFC3339. Anchors are the wallet's buys with `from <= block_time <= to`. */
  from: string;
  to?: string | null;
  /** Analysis window, seconds: the shares, counts and structure breakdown. */
  window_secs: number;
  /** Probe window, slots: did the target land in the P slots before his buy. */
  probe_slots: number;
  /** Slots before his buy that reserve match reads. */
  slots_before?: number;
  /** Reserve-match slippages, in percent. `20` is 20%. Absent: derive the set
   *  from definite entries in this read. */
  slippage_pct?: number[];
  /** ONE tag definition in the fingerprint `tags` shape. `side` / `sticky` are
   *  left out when unset: the engine's parser refuses a null. Absent = no target:
   *  the windows are still read and every share is null. */
  tag?: EntryTargetTag;
}

/** ONE tag definition in the fingerprint `tags` shape. `side` / `sticky` are left
 *  out when unset: the engine's parser refuses a null. */
export interface EntryTargetTag {
  match: FlowTag['match'];
  side?: FlowSide;
  sticky?: true;
}

/** `POST /api/wallets/{wallet}/entry-context/range`: one picked range of one token,
 *  read exactly as an entry's window (the range is the window, the same length
 *  before it the control). His own trades are left out. */
export interface EntryRangeRequest {
  wallet: string;
  mint: string;
  /** UTC RFC3339, closed bounds. */
  from: string;
  to: string;
  /** The last slot the range holds. */
  end_slot: number;
  probe_slots: number;
  slots_before?: number;
  slippage_pct?: number[];
  tag?: EntryTargetTag;
}

export interface EntryRangeResponse {
  /** The range as one entry's read: `window` = the range, `control` = before it. */
  read: EntryRow;
  window_secs: number;
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
  /** The reserve-match signal's structure: the only transaction in the 2 slots
   *  before him. Kept past the cap. Absent when that window is empty or crowded. */
  reserve?: true;
}

/** The only transaction in the 2 slots before his buy. */
export interface ReservePrint {
  slot: number;
  tx_index: number;
}

/** The probe's own reasons (`UNKNOWN_HINT` explains each); an anchor here is a buy
 *  he made, so `no-entry` never occurs. */
export type EntryUnknownReason = Exclude<PreEntryUnknownReason, 'no-entry'>;

/** The tagged print nearest ahead of his buy inside the probe window. */
export interface NearestTag {
  /** Slots back from his. `0` = his own slot: co-arrival, not a readable trigger. */
  lag_slots: number;
  /** Transactions ahead of his in the same slot; null a slot or more away. */
  lag_tx: number | null;
  /** Seconds back by block time (second precision). */
  lag_secs: number;
  /** Its own tape position. */
  slot: number;
  tx_index: number;
  /** Its template grain (`program|CU|ATA|N|S|F`). */
  key: string;
  /** Its exact ordered ix labels (absent when the print has none). */
  labels?: string[];
}

/** The pre-entry probe's read over its slot window: tagged transactions (either
 *  side, on the tag's side) and the SOL they moved, the same one window earlier, and
 *  the nearest one. The page applies min hits / min SOL. */
export interface ProbeRead {
  hits: number;
  sol: number;
  control_hits: number;
  control_sol: number;
  nearest: NearestTag | null;
}

/** The price drop, in percent, if each group of the coin's biggest holders sold
 *  its whole bags at once, read at a seat (`entry_holders.rs`). */
export interface HolderRead {
  /** Wallets holding more than zero tokens. */
  holders: number;
  top1_drop_pct: number;
  top10_drop_pct: number;
  top1pct_drop_pct: number;
  top10pct_drop_pct: number;
}

/** The window, its control and its breakdown, read from one seat. */
export interface SeatRead {
  window: EntryWindowRead;
  control: EntryWindowRead;
  groups: EntryGroupRow[];
  groups_omitted: number;
  /** Absent when the coin's history is not all on the tape. */
  holders?: HolderRead;
}

/** One anchor: the trader's buy transaction and the window before it. */
export interface EntryRow {
  /** The same read from the seat right behind the signal (the probe's nearest
   *  target print): what a bot firing on that print reads, and what the scan reads
   *  for that print. Absent with no signal. */
  at_signal?: SeatRead;
  /** The top-holder read at his seat; absent when the coin's history is not all on
   *  the tape. */
  holders?: HolderRead;
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
  probe: ProbeRead;
  /** The reserve-match signal. Absent when that window is empty, or crowded and
   *  his ceiling is missing or matches no print. */
  reserve?: ReservePrint;
  /** Which reserve outcome this entry is. Absent on a response from before the field. */
  reserve_call?: ReserveCall;
}

/** How reserve match read one entry. `definite` is the loud case. */
export type ReserveKind = 'definite' | 'single' | 'crowded' | 'no_ceiling' | 'no_match' | 'several' | 'empty';

/** Reserve match, in the words both pages show. */
export interface ReserveCall {
  kind: ReserveKind;
  /** Percent. Present on `definite` (read here) and `crowded` (matched). */
  slippage_pct?: number;
  /** Seconds of silence before the one print. */
  quiet_secs?: number;
  /** The named print's quote, in SOL. */
  quote_sol?: number;
}

/** One slippage the definite entries of one buy family agree on, and how many read it. */
export interface SlippageReading {
  pct: number;
  entries: number;
  /** `ceiling` is `Buy` / `BuyV2`. `floor` is `BuyExactSolIn` and the exact-quote buys. */
  family?: 'ceiling' | 'floor';
}

export interface EntryContextResponse {
  entries: EntryRow[];
  truncated: boolean;
  max_entries: number;
  window_secs: number;
  probe_slots: number;
  /** Slippage percents the reserve match used. Derived from definite entries when
   *  the request leaves `slippage_pct` out. */
  slippage_pct: number[];
  /** The set the definite entries produced, with a count on each setting. */
  slippage_readings?: SlippageReading[];
  tape_floor?: string | null;
}

/** Stable identity of one anchor. */
export const entryKey = (e: Pick<EntryRow, 'mint_address' | 'slot' | 'tx_index'>): string =>
  `${e.mint_address}:${e.slot}:${e.tx_index}`;

/** `POST /api/wallets/{wallet}/entry-context/scan`: every token traded in the range.
 *  A point is a buy of the target. The body is the buys read's. */
export interface EntryScanRequest extends EntryContextRequest {
  /** Read the market again and replace the stored result of this request. */
  refresh?: boolean;
}

/** One target buy, read as a buy landing right behind it is (`at` is that buy, `sol`
 *  0, the window is the W seconds up to and with it, the breakdown cut to its top
 *  row), plus what came after. */
export interface ScanMoment extends EntryRow {
  /** The last trade's price at the moment, SOL per raw token (his trades left out). */
  price?: number;
  /** Price change from `price` to the last trade price `after_secs` later, percent.
   *  No trade in between = 0. Null with no price at the moment. */
  ret_pct: [number | null, number | null];
  /** Seconds from the moment to his next buy on the token; absent when he bought no more. */
  next_buy_secs?: number;
}

export interface EntryScanResponse {
  moments: ScanMoment[];
  /** The moment cap cut the scan short (most recent tokens kept). */
  truncated: boolean;
  /** Tokens checked. */
  mints: number;
  window_secs: number;
  probe_slots: number;
  /** The two horizons `ret_pct` is read at, seconds. */
  after_secs: [number, number];
  /** When the market was read, UTC ISO. A stored result keeps the time of its read. */
  scanned_at: string;
}
