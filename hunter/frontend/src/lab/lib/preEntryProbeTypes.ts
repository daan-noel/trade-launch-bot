/**
 * Wire types for the **pre-entry ix probe** (`POST /api/wallets/{wallet}/pre-entry-ix`)
 * — the Trader Analysis flow lens asked across every token at once: did a
 * structure from this set land on the tape BEFORE the trader entered, and how
 * often does that happen anyway?
 *
 * Matching happens in Rust on the engine's own classifiers, so the filter can
 * never fire on a set the charts classify differently. Nothing here re-derives a
 * verdict client-side; this module only shapes the request and reads the answer.
 */

import type { IxPattern, IxPatternSetKind } from 'lib/flow/ixPatternSets';
import type { FlowSide } from 'lib/flow/classifyFlow';

/** One row's anchor: the trader's FIRST buy on the mint, as the token table
 *  already carries it. The tape position is `(slot, tx_index)` — `block_time`
 *  ties across a whole slot and cannot order two prints. All three absent ⇒ the
 *  window caught no buy leg, and the backend answers `unknown`. */
export interface EntryAnchor {
  mint: string;
  entry_slot: number | null;
  entry_tx_index: number | null;
  entry_at: string | null;
}

export interface PreEntryProbeRequest {
  wallet: string;
  anchors: EntryAnchor[];
  /** Window `W` in SLOTS. The probe reads `[entry - 2W, entry]`: `[entry - W,
   *  entry]` is the window, `[entry - 2W, entry - W)` the control. */
  window_slots: number;
  min_hits: number;
  min_sol: number;
  /** Absent ⇒ both legs (the lens' own default). */
  side?: FlowSide | null;
  kind: IxPatternSetKind;
  /** The NARROWED set — what the charts classify with, so the chips move the
   *  filter and the overlay together. */
  patterns: IxPattern[];
  templates: string[];
}

/** Why a row could not be answered. Never folded into `no-match`: retention and
 *  a missing fee column would then shape the hit rate silently. */
export type PreEntryUnknownReason = 'no-entry' | 'tape-truncated' | 'no-fee-readings';

export type PreEntryState = 'matched' | 'no-match' | 'unknown';

export interface PreEntryVerdict {
  mint_address: string;
  state: PreEntryState;
  unknown_reason?: PreEntryUnknownReason;
  /** Matching TRANSACTIONS (legs collapsed on `(slot, tx_index)`), not legs. */
  hits: number;
  sol: number;
  /** Nearest match's distance back from the anchor. `0` = the trader's own slot,
   *  where `nearest_lag_tx` is the entire distance. */
  nearest_lag_slots: number | null;
  nearest_lag_tx: number | null;
  matched_unit?: string;
  control_hits: number;
  control_sol: number;
  control_matched: boolean;
}

export interface PreEntryProbeResponse {
  verdicts: PreEntryVerdict[];
  /** Anchors past the backend's ceiling. Non-zero ⇒ the page is answering for a
   *  prefix of its rows and has to say so. */
  skipped: number;
  /** Oldest instant `trades` can still answer for — what a `tape-truncated` row
   *  ran into. */
  tape_floor?: string;
}

export const UNKNOWN_HINT: Record<PreEntryUnknownReason, string> = {
  'no-entry': 'the window caught no buy leg, so there is no entry to sit before',
  'tape-truncated': 'the probe window reaches past the oldest tape still stored',
  'no-fee-readings':
    'the set pins fee fields and no print in the window carries a fee reading',
};

/**
 * The nearest match as a lag label.
 *
 * `same slot +N` is deliberately NOT spelled as "0 slots": a print in the
 * trader's own slot is co-arrival, and a trigger inside our own reaction time
 * (p50 +1 slot) is not one we could act on. The number that says so has to be
 * legible at a glance, or the column reads as a clean hit.
 */
export function formatLag(v: PreEntryVerdict): string {
  const { nearest_lag_slots: slots, nearest_lag_tx: tx } = v;
  if (slots == null) return '-';
  if (slots === 0) return `same slot${tx != null ? ` −${tx} tx` : ''}`;
  return `${slots} slot${slots === 1 ? '' : 's'}`;
}

/** Sort key for the lag column: slots, with the intra-slot distance as a
 *  fraction so same-slot rows order among themselves and still sort ahead of
 *  everything a slot away. Unmatched rows sort last.
 *
 *  The tx term applies at lag 0 only — the backend reports one there and nowhere
 *  else, because a slot away it is the gap between two unrelated block positions,
 *  not a finer lag. Folding it in anyway would reorder rows on noise and drag the
 *  bar's median lag by up to a whole slot. */
export function lagSortValue(v: PreEntryVerdict | undefined): number | null {
  if (!v || v.nearest_lag_slots == null) return null;
  if (v.nearest_lag_slots !== 0) return v.nearest_lag_slots;
  return Math.min(Math.abs(v.nearest_lag_tx ?? 0), 999) / 1000;
}

/** What the bar says the filter was read over. Presence with no denominator is
 *  not evidence, so the control count travels with the match count and the
 *  unknowns are named rather than folded into the misses. */
export function probeSummary(
  verdicts: readonly PreEntryVerdict[],
  skipped: number,
): string {
  const total = verdicts.length;
  if (total === 0) return 'no rows probed';
  const matched = verdicts.filter((v) => v.state === 'matched').length;
  const unknown = verdicts.filter((v) => v.state === 'unknown').length;
  const control = verdicts.filter((v) => v.control_matched).length;
  const lags = verdicts
    .filter((v) => v.state === 'matched')
    .map((v) => lagSortValue(v))
    .filter((n): n is number => n != null)
    .sort((a, b) => a - b);
  const median = lags.length ? lags[Math.floor(lags.length / 2)] : null;
  const parts = [`matched ${matched}/${total}`, `control ${control}/${total}`];
  if (median != null) parts.push(`median lag ${median.toFixed(1)} slots`);
  if (unknown > 0) parts.push(`${unknown} unknown`);
  if (skipped > 0) parts.push(`${skipped} not probed`);
  return parts.join(' · ');
}
