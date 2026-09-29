/**
 * Entry Context client-side reads over the entries already in hand: each entry as
 * a pre-entry verdict (the Trader Analysis probe's own shape, so the same columns,
 * summary and Show control serve both pages), the per-token roll-up and the
 * structure board. Pure and DB-free — every number
 * re-reads server rows and never re-derives a share the server computed.
 */

import type { PreEntryVerdict } from '@lab/lib/preEntryProbeTypes';
import type { EntryRow } from './types';

// ── Verdict ─────────────────────────────────────────────────────────────────

/** The probe's thresholds: tagged transactions (buys and sells, on the tag's
 *  side) and the SOL they moved, within the probe's slot window. */
export interface ProbeThresholds {
  minHits: number;
  minSol: number;
}

const clears = (hits: number, sol: number, t: ProbeThresholds) =>
  hits >= Math.max(1, t.minHits) && sol >= t.minSol;

/** One entry as the probe's verdict: did the target land in the probe's slots
 *  before his buy, how near, and did the control window clear the same test.
 *  `unknown` when the tape cannot answer — never folded into `no-match`. */
export function entryVerdict(e: EntryRow, t: ProbeThresholds): PreEntryVerdict {
  const p = e.probe;
  const known = !e.unknown_reason;
  return {
    mint_address: e.mint_address,
    state: !known ? 'unknown' : clears(p.hits, p.sol, t) ? 'matched' : 'no-match',
    unknown_reason: e.unknown_reason,
    hits: p.hits,
    sol: p.sol,
    nearest_lag_slots: p.nearest?.lag_slots ?? null,
    nearest_lag_tx: p.nearest?.lag_tx ?? null,
    matched_unit: p.nearest?.key,
    matched_labels: p.nearest?.labels,
    control_hits: p.control_hits,
    control_sol: p.control_sol,
    control_matched: known && clears(p.control_hits, p.control_sol, t),
  };
}

// ── Token roll-up ───────────────────────────────────────────────────────────

export interface TokenRollup {
  /** His buys on this token in the range. */
  entries: number;
  /** Of those, the ones on screen (probe Show and table filters). */
  shown: number;
  /** Best tag tx share over this token's shown buys. */
  bestTxShare: number | null;
}

export function rollupByToken(
  entries: readonly EntryRow[],
  shown: ReadonlySet<EntryRow>,
): Map<string, TokenRollup> {
  const out = new Map<string, TokenRollup>();
  for (const e of entries) {
    let r = out.get(e.mint_address);
    if (!r) {
      r = { entries: 0, shown: 0, bestTxShare: null };
      out.set(e.mint_address, r);
    }
    r.entries += 1;
    if (!shown.has(e)) continue;
    r.shown += 1;
    const s = e.window.tx_share_pct;
    if (s != null && (r.bestTxShare == null || s > r.bestTxShare)) r.bestTxShare = s;
  }
  return out;
}

// ── Structure board ─────────────────────────────────────────────────────────

/** Middle value (the mean of the two middle ones for an even count); null when empty. */
export function median(xs: readonly number[]): number | null {
  if (xs.length === 0) return null;
  const v = [...xs].sort((a, b) => a - b);
  const m = v.length >> 1;
  return v.length % 2 ? v[m] : (v[m - 1] + v[m]) / 2;
}

export interface StructureStat {
  key: string;
  labels?: string[];
  /** Entries whose window holds at least one print of this structure. */
  present: number;
  /** Entries where it is the largest structure by buy transactions. */
  top: number;
  /** Mean buy-tx share over ALL the entries read (absent = 0 %). */
  meanTxShare: number;
  meanSolShare: number;
  /** Median buy-tx share over the entries where it traded: its typical share
   *  when it is there. Null when no window gave it a reading. */
  medianTxShare: number | null;
  medianSolShare: number | null;
  /** Entries where at least one of its buys carried the target tag. */
  tagged: number;
}

/** Every structure seen across the entries' windows, most present first. Only
 *  readable entries count, and a listed structure is only what the server kept
 *  (`groups_omitted` are the long tail). */
export function structureBoard(entries: readonly EntryRow[]): StructureStat[] {
  const readable = entries.filter((e) => !e.unknown_reason);
  const n = readable.length;
  const acc = new Map<string, StructureStat & { txSum: number; solSum: number; txs: number[]; sols: number[] }>();
  for (const e of readable) {
    e.groups.forEach((g, i) => {
      let s = acc.get(g.key);
      if (!s) {
        s = {
          key: g.key,
          labels: g.labels,
          present: 0,
          top: 0,
          meanTxShare: 0,
          meanSolShare: 0,
          medianTxShare: null,
          medianSolShare: null,
          tagged: 0,
          txSum: 0,
          solSum: 0,
          txs: [],
          sols: [],
        };
        acc.set(g.key, s);
      }
      s.present += 1;
      if (i === 0 && g.buy_tx > 0) s.top += 1;
      if (g.tag_buy_tx > 0) s.tagged += 1;
      s.txSum += g.buy_tx_share_pct ?? 0;
      s.solSum += g.buy_sol_share_pct ?? 0;
      if (g.buy_tx_share_pct != null) s.txs.push(g.buy_tx_share_pct);
      if (g.buy_sol_share_pct != null) s.sols.push(g.buy_sol_share_pct);
    });
  }
  return [...acc.values()]
    .map(({ txSum, solSum, txs, sols, ...s }) => ({
      ...s,
      meanTxShare: n > 0 ? txSum / n : 0,
      meanSolShare: n > 0 ? solSum / n : 0,
      medianTxShare: median(txs),
      medianSolShare: median(sols),
    }))
    .sort((a, b) => b.present - a.present || b.meanTxShare - a.meanTxShare || a.key.localeCompare(b.key));
}
