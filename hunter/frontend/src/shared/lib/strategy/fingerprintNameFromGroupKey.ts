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
import { DEFAULT_BUCKET_WIDTH_SOL, lamportsToSol } from './types';

/** Axes the auto-name reads — identity fields only (`name` is the output). */
export type FingerprintAutoNameAxes = Pick<
  FingerprintIdentity,
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
 */
export function fingerprintAutoName(fp: FingerprintAutoNameAxes): string {
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
  if (fp.bucket_size_amount == null) {
    parts.push('bkt=exact');
  } else {
    const width = tidySolDecimal(fp.bucket_size_amount);
    if (width !== DEFAULT_BUCKET_WIDTH_SOL) {
      parts.push(`bkt=${formatDecimalTrim(width, 4)}`);
    }
  }
  return parts.length === 0 ? 'ALL' : parts.join(' · ');
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
