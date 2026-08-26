import type { CoTradeBucket, CoTrader, TraderTokenRow } from 'types';

/**
 * Co-trade derivations for Trader Analysis — reading a row's `co_traders` (the
 * comparison wallets that were also on that mint) down to the few numbers the
 * table sorts and filters on.
 *
 * The question these answer is never "did two wallets both buy this token" —
 * over a week of memecoins that is mostly coincidence. It is **where their
 * entries sit relative to each other on the tape**, which is why every function
 * here keys off `entry_lag_slots` rather than off overlap counts.
 *
 * `entry_lag_slots` is signed against the PRIMARY wallet: negative = the
 * comparison wallet got in first. `null` means one of the two has no entry leg
 * in the window, and an unknown ordering must stay unknown — never fall back to
 * a timestamp, which is second-precision and ties across a whole slot.
 */

/** Rank a bucket for sorting: tightest coupling first. */
const BUCKET_RANK: Record<CoTradeBucket, number> = {
  'co-slot': 0,
  leads: 1,
  follows: 2,
  independent: 3,
};

/** Every bucket in display / sort order — the summary strip's column order. */
export const CO_TRADE_BUCKETS: CoTradeBucket[] = ['co-slot', 'leads', 'follows', 'independent'];

/** Human gloss, used as the tooltip wherever a bucket is shown as a chip. */
export const CO_TRADE_BUCKET_HINT: Record<CoTradeBucket, string> = {
  'co-slot':
    'Same slot — both wallets landed in the same block, so neither could have seen the other. They reacted to the same event on the tape: a shared trigger, not a copy.',
  leads: 'The other wallet entered 1-3 slots BEFORE this one.',
  follows: 'The other wallet entered 1-3 slots AFTER this one.',
  independent:
    'More than 3 slots apart — too far for one tape event to have driven both. Most likely an independent visit to the same token.',
};

/** How many comparison wallets were also on this mint. */
export const coCount = (r: TraderTokenRow): number => r.co_traders.length;

/**
 * The comparison wallet whose entry sits CLOSEST to the primary's, by absolute
 * slot distance — the row's headline coupling. Ties (the common case: several
 * wallets in the same slot) break toward the one that entered EARLIEST, since
 * the interesting reading of a tie is who was ahead.
 *
 * Wallets with an unknown lag are skipped; `null` when none has a lag at all.
 */
export function tightestCoTrader(r: TraderTokenRow): CoTrader | null {
  let best: CoTrader | null = null;
  for (const c of r.co_traders) {
    if (c.entry_lag_slots == null) continue;
    if (best == null) {
      best = c;
      continue;
    }
    const bestLag = best.entry_lag_slots!;
    const d = Math.abs(c.entry_lag_slots) - Math.abs(bestLag);
    if (d < 0 || (d === 0 && c.entry_lag_slots < bestLag)) best = c;
  }
  return best;
}

/** Signed slot lag of [`tightestCoTrader`]; negative = that wallet was ahead. */
export const coLagSlots = (r: TraderTokenRow): number | null =>
  tightestCoTrader(r)?.entry_lag_slots ?? null;

/** Bucket of the tightest coupling on this row. */
export const coBucket = (r: TraderTokenRow): CoTradeBucket | null =>
  tightestCoTrader(r)?.bucket ?? null;

/** Sort key for the bucket column — tightest first, unknown last. */
export const coBucketRank = (r: TraderTokenRow): number | null => {
  const b = coBucket(r);
  return b ? BUCKET_RANK[b] : null;
};

/**
 * Who entered this mint first, across the primary and every comparison wallet
 * that has a tape position: `null` when nobody's ordering is known, `''` when
 * the primary did, else the winning wallet's address.
 *
 * A negative lag is by definition ahead of the primary, so the answer is just
 * the most-negative lag — no re-derivation from timestamps.
 */
export function firstMover(r: TraderTokenRow): string | null {
  // Every lag is measured FROM the primary, so the primary itself sits at the
  // origin. With no entry leg of its own there is no origin and every lag is
  // null, which is an unknown ordering rather than a win for anyone.
  if (r.wallet_entry_slot == null) return null;
  let wallet = '';
  let slot = 0;
  let tx = 0;
  for (const c of r.co_traders) {
    if (c.entry_lag_slots == null) continue;
    // Ties inside a slot fall to `entry_lag_tx`: one transaction of position in
    // a slot is a real advantage, not a rounding error.
    const cTx = c.entry_lag_tx ?? 0;
    if (c.entry_lag_slots < slot || (c.entry_lag_slots === slot && cTx < tx)) {
      slot = c.entry_lag_slots;
      tx = cTx;
      wallet = c.wallet;
    }
  }
  return wallet;
}

/** Count of rows per bucket across a cohort — the summary strip's whole content.
 *  A row with co-traders but no resolvable lag counts as `unknown`. */
export interface CoTradeMix {
  /** Rows where at least one comparison wallet was present. */
  overlap: number;
  /** Rows in the cohort at all — the denominator for the overlap share. */
  total: number;
  byBucket: Record<CoTradeBucket, number>;
  unknown: number;
}

/**
 * The bucket mix over a cohort.
 *
 * This is the page's stand-in for a base-rate test, and it is the number to read
 * before believing in a "family": coincidental overlap lands in `independent`
 * (two wallets that happened to visit the same token hours apart), while wallets
 * driven by the same tape event pile into `co-slot` / `leads` / `follows`. The
 * raw overlap count on its own says almost nothing.
 */
export function coTradeMix(rows: TraderTokenRow[]): CoTradeMix {
  const mix: CoTradeMix = {
    overlap: 0,
    total: rows.length,
    byBucket: { 'co-slot': 0, leads: 0, follows: 0, independent: 0 },
    unknown: 0,
  };
  for (const r of rows) {
    if (r.co_traders.length === 0) continue;
    mix.overlap++;
    const b = coBucket(r);
    if (b) mix.byBucket[b]++;
    else mix.unknown++;
  }
  return mix;
}

/** A signed slot lag as text: `same slot` at 0, else `-2` / `+3` slots. */
export function formatLagSlots(lag: number | null): string {
  if (lag == null) return '-';
  if (lag === 0) return 'same slot';
  return `${lag > 0 ? '+' : ''}${lag}`;
}
