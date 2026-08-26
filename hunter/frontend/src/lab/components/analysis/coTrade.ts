import type { BadgeVariant } from 'components/ui/Badge';
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
 *
 * A row can carry several comparison wallets, so every single-answer derivation
 * here takes an optional FOCUS wallet: unfocused it reports the row's tightest
 * coupling (the headline), focused it reports that one wallet even when another
 * sat closer. Without the focus the second and third wallet on a shared row are
 * unsortable and unfilterable — visible only as chips.
 */

/**
 * Comparison wallets one co-trade read may carry — the frontend copy of the
 * backend's `MAX_COMPARISON_WALLETS` (`hunter/lab/src/api/handlers/wallets.rs`),
 * which silently drops anything past it. The picker enforces the SAME number so
 * a wallet can never sit in the "Comparing" legend having never been queried.
 * `coTrade.test.ts` reads the Rust constant and fails if the two drift, and
 * asserts the marker palette has a distinct hue per slot up to it.
 */
export const MAX_COMPARE_WALLETS = 8;

/** Rank a bucket for sorting: tightest coupling first. */
const BUCKET_RANK: Record<CoTradeBucket, number> = {
  'co-slot': 0,
  leads: 1,
  follows: 2,
  independent: 3,
};

/** Every bucket in display / sort order — the summary strip's column order. */
export const CO_TRADE_BUCKETS: CoTradeBucket[] = ['co-slot', 'leads', 'follows', 'independent'];

/**
 * A bucket, or the fifth outcome a row can be counted under: shared, but with an
 * entry that cannot be ordered against the primary's (one of the two has no entry
 * leg inside the window).
 *
 * `unordered` is a real answer, not a missing one, so it is selectable like the
 * rest — a wallet whose entries all predate the window is a finding about the
 * window, and hiding it inside "no bucket" makes those rows unreachable.
 */
export type CoBucketKey = CoTradeBucket | 'unordered';

/** Every selectable bucket key, in strip order. */
export const CO_BUCKET_KEYS: CoBucketKey[] = [...CO_TRADE_BUCKETS, 'unordered'];

/** Badge variant per bucket — ONE mapping, so the Coupling column and the summary
 *  strip's selectable badges can never color the same bucket differently. */
export const CO_BUCKET_VARIANT: Record<CoBucketKey, BadgeVariant> = {
  // Same slot is the finding, so it gets the loudest chip.
  'co-slot': 'accent',
  leads: 'success',
  follows: 'info',
  independent: 'neutral',
  unordered: 'neutral',
};

/** Human gloss, used as the tooltip wherever a bucket is shown as a chip. */
export const CO_TRADE_BUCKET_HINT: Record<CoTradeBucket, string> = {
  'co-slot':
    'Same slot — both wallets landed in the same block, so neither could have seen the other. They reacted to the same event on the tape: a shared trigger, not a copy.',
  leads: 'The other wallet entered 1-3 slots BEFORE this one.',
  follows: 'The other wallet entered 1-3 slots AFTER this one.',
  independent:
    'More than 3 slots apart — too far for one tape event to have driven both. Most likely an independent visit to the same token.',
};

/** The same gloss over every SELECTABLE key, `unordered` included. */
export const CO_BUCKET_HINT: Record<CoBucketKey, string> = {
  ...CO_TRADE_BUCKET_HINT,
  unordered:
    'Shared the token, but one of the two wallets has no entry leg inside the window, so the two entries cannot be ordered. A real answer about the window, not a gap in the data.',
};

/** How many comparison wallets were also on this mint. Always the full count —
 *  a focused wallet narrows what the single-answer columns SAY, never how many
 *  wallets were actually here. */
export const coCount = (r: TraderTokenRow): number => r.co_traders.length;

/**
 * The comparison wallet a row's single-answer columns (lag, bucket) speak for.
 *
 * With a wallet FOCUSED — the summary strip's per-wallet chips — that wallet is
 * the answer, present or absent. Focusing exists precisely so one wallet's
 * coupling can be read, sorted and filtered on without the tightest of the
 * others standing in for it: on a row three wallets share, the other two are
 * otherwise visible only as chips.
 *
 * Unfocused it falls back to [`tightestCoTrader`] — the row's headline coupling.
 */
export function pickCoTrader(r: TraderTokenRow, focus: string | null = null): CoTrader | null {
  if (focus) return r.co_traders.find((c) => c.wallet === focus) ?? null;
  return tightestCoTrader(r);
}

/** Did this row's cohort include a co-trader at all — with a wallet focused,
 *  THAT wallet. */
export const hasCoTrader = (r: TraderTokenRow, focus: string | null = null): boolean =>
  focus ? r.co_traders.some((c) => c.wallet === focus) : r.co_traders.length > 0;

/**
 * The "co-traded only" predicate: the row carries at least `minWallets` of the
 * comparison wallets, and — under a focus — that wallet among them.
 *
 * `minWallets` is the whole point of the control. At 1 this is the UNION: any one
 * of them was also here, which two busy wallets satisfy by coincidence alone. At
 * the size of the comparison set it is the INTERSECTION: only the tokens the
 * primary and EVERY named wallet touched, which is the set worth reading when the
 * question is whether these wallets are one operation. The values in between
 * ("3 of 5") matter because a family rarely turns out in full on any one token —
 * demanding all of them can empty the table while a real core of four is sitting
 * right underneath it.
 *
 * Depth is a count of DISTINCT wallets, never of trades: `co_traders` holds one
 * entry per wallet per mint.
 */
export function passesCoFilter(
  r: TraderTokenRow,
  focus: string | null,
  minWallets: number,
): boolean {
  return r.co_traders.length >= Math.max(1, minWallets) && hasCoTrader(r, focus);
}

/**
 * The bucket a row is COUNTED under, or `null` when it carries no comparison
 * wallet at all (with a focus, none from that wallet).
 *
 * One derivation behind the strip's badge counts AND the badge filter, so a badge
 * that reads `co-slot 40` selects exactly those 40 rows. Two copies of this rule
 * would let a count and its own filter disagree, which is the one thing a
 * clickable count must never do.
 */
export function coBucketKey(r: TraderTokenRow, focus: string | null = null): CoBucketKey | null {
  if (!hasCoTrader(r, focus)) return null;
  return coBucket(r, focus) ?? 'unordered';
}

/** Does this row fall in one of the SELECTED buckets? An empty selection is no
 *  narrowing at all — never "nothing matches". */
export function matchesCoBuckets(
  r: TraderTokenRow,
  focus: string | null,
  selected: readonly CoBucketKey[],
): boolean {
  if (selected.length === 0) return true;
  const key = coBucketKey(r, focus);
  return key != null && selected.includes(key);
}

/**
 * How many rows of the cohort carry AT LEAST n comparison wallets, for
 * n = 1..`size` (index 0 is n = 1).
 *
 * The intersection's size is the number the strip has to show BEFORE the filter
 * is applied: "all 5" landing on zero rows and "all 5" landing on eleven are the
 * same empty table once you toggle blind, and only one of them means the family
 * is not real. Reading the whole ladder also shows where it falls off — a set
 * that holds 200 rows at 2 and 3 at 3 has one pair in it, not a family.
 */
export function coDepthCounts(rows: TraderTokenRow[], size: number): number[] {
  const out = new Array<number>(Math.max(0, size)).fill(0);
  for (const r of rows) {
    // A row can only be counted up to the set size — a stale row fetched under a
    // larger comparison set must not write past the ladder.
    const depth = Math.min(r.co_traders.length, out.length);
    for (let i = 0; i < depth; i++) out[i]++;
  }
  return out;
}

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

/** Signed slot lag of [`pickCoTrader`]; negative = that wallet was ahead. */
export const coLagSlots = (r: TraderTokenRow, focus: string | null = null): number | null =>
  pickCoTrader(r, focus)?.entry_lag_slots ?? null;

/** Bucket of the row's answering co-trader. */
export const coBucket = (r: TraderTokenRow, focus: string | null = null): CoTradeBucket | null =>
  pickCoTrader(r, focus)?.bucket ?? null;

/** Sort key for the bucket column — tightest first, unknown last. */
export const coBucketRank = (r: TraderTokenRow, focus: string | null = null): number | null => {
  const b = coBucket(r, focus);
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
 * The bucket mix over a cohort — across ALL comparison wallets by default (each
 * row counted once, on its tightest coupling), or for ONE focused wallet.
 *
 * This is the page's stand-in for a base-rate test, and it is the number to read
 * before believing in a "family": coincidental overlap lands in `independent`
 * (two wallets that happened to visit the same token hours apart), while wallets
 * driven by the same tape event pile into `co-slot` / `leads` / `follows`. The
 * raw overlap count on its own says almost nothing.
 *
 * Read the unfocused mix as the family's ceiling, never as any one wallet's
 * evidence: a row three wallets share contributes a single bucket, so one busy
 * wallet's coupling can carry a strip that a second wallet had no part in. The
 * per-wallet split ([`coTradePerWallet`]) is what separates them.
 */
export function coTradeMix(rows: TraderTokenRow[], focus: string | null = null): CoTradeMix {
  const mix: CoTradeMix = {
    overlap: 0,
    total: rows.length,
    byBucket: { 'co-slot': 0, leads: 0, follows: 0, independent: 0 },
    unknown: 0,
  };
  for (const r of rows) {
    if (!hasCoTrader(r, focus)) continue;
    mix.overlap++;
    const b = coBucket(r, focus);
    if (b) mix.byBucket[b]++;
    else mix.unknown++;
  }
  return mix;
}

/** Overlaps close enough that ONE tape event could have driven both wallets —
 *  `co-slot` + `leads` + `follows`. The SSOT for the strip's "coupled" share, at
 *  both the headline and the per-wallet grain. */
export const coupledCount = (mix: CoTradeMix): number =>
  mix.byBucket['co-slot'] + mix.byBucket.leads + mix.byBucket.follows;

/** One comparison wallet's own mix over the cohort. */
export interface CoWalletMix extends CoTradeMix {
  wallet: string;
  /** [`coupledCount`] of this wallet's own mix. */
  coupled: number;
}

/**
 * The mix PER comparison wallet, in the caller's slot order.
 *
 * The strip's headline number cannot answer "which of these wallets is the
 * family": a wallet that shared 900 tokens and one that shared 2 both vanish
 * into a single overlap count, and every single-answer column reports whichever
 * of them happened to sit closest on each row. Splitting per wallet is the whole
 * multi-wallet read — and each entry doubles as a focus target.
 */
export function coTradePerWallet(rows: TraderTokenRow[], comparison: string[]): CoWalletMix[] {
  return comparison.map((wallet) => {
    const mix = coTradeMix(rows, wallet);
    return { wallet, ...mix, coupled: coupledCount(mix) };
  });
}

/** A signed slot lag as text: `same slot` at 0, else `-2` / `+3` slots. */
export function formatLagSlots(lag: number | null): string {
  if (lag == null) return '-';
  if (lag === 0) return 'same slot';
  return `${lag > 0 ? '+' : ''}${lag}`;
}
