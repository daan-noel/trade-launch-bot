// Compact auto-name for a fingerprint — a picker/log handle, not identity.
// Chip-aligned tokens, ix first, default 0.1 bucket omitted, no provenance
// prefix. Example: `3ix:Buy · max=1 · bkt=1`
//
// Rust SSOT: `Fingerprint::auto_name`. This file is the TS mirror; golden
// strings in the two test files stay byte-equal.
// Detail: hunter/docs/plans/strategies/fingerprint-auto-name.md

import { configuredIxLabels, ixLabelsCountTail } from 'lib/ixLabels';
import { formatCompact, formatDecimalTrim, tidySolDecimal } from 'utils/format';
import { fingerprintIdentityFromGroupKey, type FingerprintIdentity } from './matchGroupFingerprint';
import {
  DEFAULT_BUCKET_WIDTH_SOL,
  decimalsForBucketWidth,
  hasSolAxis,
  lamportsToSol,
  WILDCARD_NAME,
} from './types';

/** Axes the auto-name reads — identity fields only (`name` is the output). */
export type FingerprintAutoNameAxes = Pick<
  FingerprintIdentity,
  | 'wildcard'
  | 'cu_limit'
  | 'cu_price'
  | 'init_buy_lamports'
  | 'max_cost_lamports'
  | 'spendable_lamports_in'
  | 'first_slot_buy_lamports'
  | 'first_slot_sell_lamports'
  | 'bucket_size_amount'
  | 'ix_labels'
>;

/**
 * Build the auto-name from stored axes (lamports + SOL width).
 *
 * Order: `Nix:Tail`, then `cu_limit` / `cu_price` / `init` / `max` / `spend` /
 * `fs_buy` / `fs_sell`, then `bkt=exact` or `bkt={width}` when width ≠ 0.1.
 * Unset axes skipped. Empty → `ALL`.
 *
 * The `bkt=` part appears only when a SOL axis exists to spend the width on.
 *
 * A wildcard carries no axis and its bucket width is inert, so it names the token
 * set it matches — `ALL`.
 */
export function fingerprintAutoName(fp: FingerprintAutoNameAxes): string {
  if (fp.wildcard) return WILDCARD_NAME;
  const parts: string[] = [];
  const ix = configuredIxLabels(fp.ix_labels);
  if (ix) parts.push(ixLabelsCountTail(ix));
  if (fp.cu_limit != null) parts.push(`cu_limit=${formatCompact(fp.cu_limit, 1)}`);
  if (fp.cu_price != null) parts.push(`cu_price=${formatCompact(fp.cu_price, 1)}`);
  pushSol(parts, 'init', fp.init_buy_lamports);
  pushSol(parts, 'max', fp.max_cost_lamports);
  pushSol(parts, 'spend', fp.spendable_lamports_in);
  pushSol(parts, 'fs_buy', fp.first_slot_buy_lamports);
  pushSol(parts, 'fs_sell', fp.first_slot_sell_lamports);
  // With no SOL axis there is nothing to bucket, so the width reaches no match and
  // must reach no name — the same reason the wildcard above names none. Rust SSOT:
  // `Fingerprint::effective_bucket_size_amount`, which is also what every write edge
  // stores, so name and stored value stay in step.
  if (hasSolAxis(fp)) {
    if (fp.bucket_size_amount == null) {
      parts.push('bkt=exact');
    } else {
      const width = tidySolDecimal(fp.bucket_size_amount);
      if (width !== DEFAULT_BUCKET_WIDTH_SOL) {
        parts.push(`bkt=${formatDecimalTrim(width, decimalsForBucketWidth(width))}`);
      }
    }
  }
  return parts.length === 0 ? WILDCARD_NAME : parts.join(' · ');
}

function pushSol(parts: string[], label: string, lamports: number | null): void {
  const s = lamportsToSol(lamports);
  if (s == null) return;
  parts.push(`${label}=${formatDecimalTrim(s, 4)}`);
}

/**
 * Auto-name from a stored group key. SOL axes use the bucket lo-edge (same
 * representative `fingerprintIdentityFromGroupKey` feeds `find_or_create`).
 *
 * @param bucketWidthSol width used for SOL axes; `null` is exact mode and
 *        appends `bkt=exact`.
 */
export function fingerprintNameFromGroupKey(
  gk: Record<string, string>,
  bucketWidthSol: number | null = DEFAULT_BUCKET_WIDTH_SOL,
): string {
  return fingerprintAutoName(fingerprintIdentityFromGroupKey(gk, bucketWidthSol));
}

/**
 * Whether `name` is written in `fingerprintAutoName`'s own chip grammar: every
 * ` · `-separated part is a chip that function emits. Such a name was generated,
 * never typed, so {@link isStaleAutoName} may rewrite it once it stops matching
 * the axes. Mirrors Rust `is_generated_auto_name`.
 *
 * Deliberately strict — an unrecognised part makes the whole name a nickname. The
 * two mistakes do not cost the same: re-deriving a name it declined to touch is
 * free, while rewriting a real nickname destroys the only record of why that
 * fingerprint was created.
 */
export function isGeneratedAutoName(name: string): boolean {
  const n = name.trim();
  if (n === '') return false;
  if (n === WILDCARD_NAME) return true;
  return n.split(' · ').every(isAutoNameChip);
}

/** One chip of the {@link isGeneratedAutoName} grammar — kept beside
 *  `fingerprintAutoName` so a chip added there is added here in the same edit. */
function isAutoNameChip(part: string): boolean {
  // `3ix` / `3ix:BuyExactSolIn` — the count is what makes it a chip and not a word,
  // so a nickname prefix like `8dtx` is not `{digits}ix`.
  if (/^\d+ix(:.+)?$/.test(part)) return true;
  const eq = part.indexOf('=');
  if (eq < 0) return false;
  const label = part.slice(0, eq);
  const value = part.slice(eq + 1);
  // `formatDecimalTrim` output: optional sign, digits, at most one `.`.
  const dec = /^-?\d+(\.\d+)?$/;
  switch (label) {
    // `formatCompact` — a decimal with an optional K/M/G scale suffix.
    case 'cu_limit':
    case 'cu_price':
      return dec.test(value.replace(/[KMG]$/, ''));
    case 'init':
    case 'max':
    case 'spend':
    case 'fs_buy':
    case 'fs_sell':
      return dec.test(value);
    case 'bkt':
      return value === 'exact' || dec.test(value);
    default:
      return false;
  }
}

/**
 * Whether `name` is an auto-label that no longer says what the axes say — a
 * retired shape, or a current-grammar one that has drifted from `autoName`.
 * Mirrors Rust `Fingerprint::has_stale_auto_name`.
 */
export function isStaleAutoName(name: string, autoName: string): boolean {
  return isLegacyAutoName(name) || (isGeneratedAutoName(name) && name.trim() !== autoName);
}

/**
 * Retired generator shapes — `sweep {id} · group N`, C3 provenance prefixes,
 * the flow-discovery fallback, and a blank. Nicknames return false.
 * Mirrors Rust `is_legacy_auto_name`.
 */
export function isLegacyAutoName(name: string): boolean {
  const n = name.trim();
  if (n === '') return true;
  if (n.toLowerCase() === 'flow-discovery bind') return true;
  if (n.startsWith('sweep ') && n.includes(' · group ')) return true;
  return n.startsWith('c · ') || n.startsWith('f · ') || n.startsWith('s · ');
}
