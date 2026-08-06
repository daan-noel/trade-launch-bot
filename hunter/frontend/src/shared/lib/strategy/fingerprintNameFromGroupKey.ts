// Compact auto-name for a fingerprint created/bound from a group_key (C3).
// Name is a label only — match identity lives on the axes. Skip unset/`∅`,
// glue short axis keys to compact values, collapse ix to `Nix:Tail`, append
// `bW` only when bucket width ≠ default 0.1.
//
// Example: `c · fsb19.5 · 3ix:Buy · b0.5`
//
// The ix part carries the trailing action, not just the count: a bare `3ix`
// named `[…, Pump.Fun: Buy]` and `[…, Pump.Fun: BuyExactSolIn]` identically,
// so two fingerprints that arm on different tokens were indistinguishable in
// every picker and table that shows a name. A name has no color to carry the
// chip's ribbon, so it needs the text discriminator.

import { ixLabelsCountTail } from 'lib/ixLabels';
import { formatDecimalTrim, tidySolDecimal } from 'utils/format';

/** Provenance tag — creation-stats / flow-discovery / sweep. */
export type FingerprintNameSource = 'c' | 'f' | 's';

/** Default SOL bucket width — mirrors `grouping::SOL_BUCKET_WIDTH` / lab
 *  `SOL_BUCKET_WIDTH`. Kept local so this shared helper never imports `@lab`. */
const DEFAULT_BUCKET_WIDTH = 0.1;

/** Group-key field → short name token (C3). */
const AXIS_SHORT: Record<string, string> = {
  cu_limit: 'cu',
  cu_price: 'cup',
  initial_buy_sol: 'init',
  max_cost_lamports: 'max',
  spendable_lamports_in: 'spend',
  first_slot_buy_sol: 'fsb',
  first_slot_sell_sol: 'fss',
};

/** Backend grouping sentinel for a missing axis value. */
const MISSING_VALUE = '∅';

/** Lo-edge of a `"lo–hi"` bucket label, trailing zeros trimmed. */
function compactBucketLo(label: string): string {
  const lo = label.split('–')[0]?.trim() ?? label;
  const n = Number(lo);
  if (!Number.isFinite(n)) return lo;
  return formatDecimalTrim(n, 6);
}

/**
 * Build a C3 fingerprint name from a stored group key.
 *
 * @param gk group_key map (may include `∅` and grouping-only axes)
 * @param source one-letter provenance (`c` / `f` / `s`)
 * @param bucketWidthSol width used for SOL axes; `b{width}` appended when ≠ 0.1.
 *        `null` is the **exact**-amount mode (`SolPrecision::Exact` / NULL stored
 *        width) and appends `bexact` — never a width. Two fingerprints with the
 *        same axes but different precision are different rules that arm on
 *        different token sets, so the name has to tell them apart; `exact` is the
 *        same word `formatBucketWidth` and `BucketChip` use for this mode.
 */
export function fingerprintNameFromGroupKey(
  gk: Record<string, string>,
  source: FingerprintNameSource,
  bucketWidthSol: number | null = DEFAULT_BUCKET_WIDTH,
): string {
  const parts: string[] = [source];
  for (const [k, v] of Object.entries(gk)) {
    if (v === MISSING_VALUE) continue;
    if (k === 'is_cashback_enabled' || k === 'token_program_id') continue;
    if (k === 'ix_labels') {
      parts.push(ixLabelsCountTail(v.split(' | ').filter((s) => s.length > 0)));
      continue;
    }
    const short = AXIS_SHORT[k] ?? k;
    const val = v.includes('–') ? compactBucketLo(v) : v;
    parts.push(`${short}${val}`);
  }
  if (bucketWidthSol == null) {
    parts.push('bexact');
  } else {
    const width = tidySolDecimal(bucketWidthSol);
    if (width !== DEFAULT_BUCKET_WIDTH) {
      parts.push(`b${formatDecimalTrim(width, 4)}`);
    }
  }
  if (parts.length === 1) parts.push('ALL');
  return parts.join(' · ');
}
