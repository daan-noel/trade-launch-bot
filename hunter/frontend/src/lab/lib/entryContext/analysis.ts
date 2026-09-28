/**
 * Entry Context client-side reads over the entries already in hand: the axis
 * filter, the per-token roll-up and the summary (share histogram, structure
 * board). Pure and DB-free — every number here re-reads server rows and never
 * re-derives a share the server computed.
 */

import {
  conditionListPredicate,
  parseConditionList,
} from 'components/table/numericFilter';
import { AXIS_BY_KEY, type EntryAxis } from './axes';
import { entryKey, type EntryRow } from './types';

// ── Filter ──────────────────────────────────────────────────────────────────

/** One filter line: an axis and a condition in the table grammar (`>50`,
 *  `>=10, <=30`, `<5 | >90`). */
export interface AxisCondition {
  axis: string;
  cond: string;
}

export interface CompiledFilter {
  /** Entries with an unknown reason never pass: no reading is not a zero. */
  pass: (e: EntryRow) => boolean;
  /** Index of each malformed line → why. Malformed lines constrain nothing. */
  errors: ReadonlyMap<number, string>;
  /** Lines that constrain (parsed and non-empty). */
  active: number;
}

export function compileFilter(lines: readonly AxisCondition[]): CompiledFilter {
  const errors = new Map<number, string>();
  const tests: { axis: EntryAxis; ok: (n: number) => boolean }[] = [];
  lines.forEach((line, i) => {
    const axis = AXIS_BY_KEY.get(line.axis);
    if (!axis) {
      errors.set(i, `unknown axis ${line.axis}`);
      return;
    }
    const arms = parseConditionList(line.cond);
    if (arms == null) {
      errors.set(i, 'use >50, >=10, <=30 or <5 | >90');
      return;
    }
    if (arms.length > 0) tests.push({ axis, ok: conditionListPredicate(arms) });
  });
  return {
    pass: (e) =>
      !e.unknown_reason &&
      tests.every(({ axis, ok }) => {
        const v = axis.get(e);
        return v != null && Number.isFinite(v) && ok(v);
      }),
    errors,
    active: tests.length,
  };
}

// ── Token roll-up ───────────────────────────────────────────────────────────

export interface TokenRollup {
  entries: number;
  passing: number;
  unknown: number;
  /** Best tag tx share over this token's PASSING entries. */
  bestTxShare: number | null;
}

export function rollupByToken(
  entries: readonly EntryRow[],
  passing: ReadonlySet<string>,
): Map<string, TokenRollup> {
  const out = new Map<string, TokenRollup>();
  for (const e of entries) {
    let r = out.get(e.mint_address);
    if (!r) {
      r = { entries: 0, passing: 0, unknown: 0, bestTxShare: null };
      out.set(e.mint_address, r);
    }
    r.entries += 1;
    if (e.unknown_reason) r.unknown += 1;
    if (!passing.has(entryKey(e))) continue;
    r.passing += 1;
    const s = e.window.tx_share_pct;
    if (s != null && (r.bestTxShare == null || s > r.bestTxShare)) r.bestTxShare = s;
  }
  return out;
}

// ── Share histogram ─────────────────────────────────────────────────────────

export interface ShareBucket {
  lo: number;
  hi: number;
  window: number;
  control: number;
}

/** Ten 10-point buckets of one share axis, window vs control, over the readable
 *  entries. `100` lands in the last bucket. Entries with no reading are counted
 *  apart (`noWindow` / `noControl`), never as 0 %. */
export function shareHistogram(
  entries: readonly EntryRow[],
  windowAxis: EntryAxis,
  controlAxis: EntryAxis,
): { buckets: ShareBucket[]; noWindow: number; noControl: number } {
  const buckets: ShareBucket[] = Array.from({ length: 10 }, (_, i) => ({
    lo: i * 10,
    hi: (i + 1) * 10,
    window: 0,
    control: 0,
  }));
  const at = (v: number) => Math.min(9, Math.max(0, Math.floor(v / 10)));
  let noWindow = 0;
  let noControl = 0;
  for (const e of entries) {
    if (e.unknown_reason) continue;
    const w = windowAxis.get(e);
    const c = controlAxis.get(e);
    if (w == null) noWindow += 1;
    else buckets[at(w)].window += 1;
    if (c == null) noControl += 1;
    else buckets[at(c)].control += 1;
  }
  return { buckets, noWindow, noControl };
}

// ── Structure board ─────────────────────────────────────────────────────────

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
  /** Entries where at least one of its buys carried the target tag. */
  tagged: number;
}

/** Every structure seen across the entries' windows, most present first. Only
 *  readable entries count, and a listed structure is only what the server kept
 *  (`groups_omitted` are the long tail). */
export function structureBoard(entries: readonly EntryRow[]): StructureStat[] {
  const readable = entries.filter((e) => !e.unknown_reason);
  const n = readable.length;
  const acc = new Map<string, StructureStat & { txSum: number; solSum: number }>();
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
          tagged: 0,
          txSum: 0,
          solSum: 0,
        };
        acc.set(g.key, s);
      }
      s.present += 1;
      if (i === 0 && g.buy_tx > 0) s.top += 1;
      if (g.tag_buy_tx > 0) s.tagged += 1;
      s.txSum += g.buy_tx_share_pct ?? 0;
      s.solSum += g.buy_sol_share_pct ?? 0;
    });
  }
  return [...acc.values()]
    .map(({ txSum, solSum, ...s }) => ({
      ...s,
      meanTxShare: n > 0 ? txSum / n : 0,
      meanSolShare: n > 0 ? solSum / n : 0,
    }))
    .sort((a, b) => b.present - a.present || b.meanTxShare - a.meanTxShare || a.key.localeCompare(b.key));
}
