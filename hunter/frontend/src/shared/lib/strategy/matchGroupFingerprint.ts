// Match a sweep/discovery `group_key` to a saved fingerprint using the same
// identity as promote/bind (`fingerprint_from_group_key` + `IDENTITY_WHERE`).
// `name` / `metric_config` are labels, not identity. Grouping-only axes
// (`is_cashback_enabled`, `token_program_id`) have no fingerprint field.

import { solToLamports, type Fingerprint } from './types';
import { tidySolDecimal } from 'utils/format';

/** Identity axes only — what `find_or_create` compares. */
export interface FingerprintIdentity {
  cu_limit: number | null;
  cu_price: number | null;
  init_buy_lamports: number | null;
  max_cost_lamports: number | null;
  spendable_lamports_in: number | null;
  first_slot_buy_lamports: number | null;
  first_slot_sell_lamports: number | null;
  bucket_size_amount: number;
  ix_labels: string[] | null;
}

/** Parse a `"lo–hi"` SOL bucket label's lower edge into lamports (plain numeric
 *  labels parse whole). Mirrors Rust `parse_lo_lamports` (en-dash separator). */
export function parseLoLamports(label: string): number | null {
  const lo = label.split('–')[0]?.trim();
  if (lo == null || lo === '') return null;
  const sol = Number(lo);
  if (!Number.isFinite(sol)) return null;
  return solToLamports(sol);
}

/**
 * Rebuild fingerprint identity from a stored group key at bucket `width`.
 * Continuous SOL fields use the bucket's lower edge; `∅` (missing) is skipped.
 */
export function fingerprintIdentityFromGroupKey(
  gk: Record<string, string>,
  bucketWidthSol: number,
): FingerprintIdentity {
  const id: FingerprintIdentity = {
    cu_limit: null,
    cu_price: null,
    init_buy_lamports: null,
    max_cost_lamports: null,
    spendable_lamports_in: null,
    first_slot_buy_lamports: null,
    first_slot_sell_lamports: null,
    bucket_size_amount: tidySolDecimal(bucketWidthSol),
    ix_labels: null,
  };
  for (const [tag, raw] of Object.entries(gk)) {
    if (raw === '∅') continue;
    switch (tag) {
      case 'cu_limit': {
        const n = Number(raw);
        if (Number.isFinite(n)) id.cu_limit = n;
        break;
      }
      case 'cu_price': {
        const n = Number(raw);
        if (Number.isFinite(n)) id.cu_price = n;
        break;
      }
      case 'initial_buy_sol':
        id.init_buy_lamports = parseLoLamports(raw);
        break;
      case 'max_cost_lamports':
        id.max_cost_lamports = parseLoLamports(raw);
        break;
      case 'spendable_lamports_in':
        id.spendable_lamports_in = parseLoLamports(raw);
        break;
      case 'first_slot_buy_sol':
        id.first_slot_buy_lamports = parseLoLamports(raw);
        break;
      case 'first_slot_sell_sol':
        id.first_slot_sell_lamports = parseLoLamports(raw);
        break;
      case 'ix_labels':
        id.ix_labels = raw.split(' | ');
        break;
      // Grouping-only — no fingerprint identity.
      case 'is_cashback_enabled':
      case 'token_program_id':
        break;
      default:
        break;
    }
  }
  return id;
}

function ixLabelsEqual(a: string[] | null, b: string[] | null): boolean {
  if (a == null && b == null) return true;
  if (a == null || b == null) return false;
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) {
    if (a[i] !== b[i]) return false;
  }
  return true;
}

/** True when every `IDENTITY_WHERE` axis matches (`NULL` is a value). */
export function fingerprintMatchesIdentity(fp: Fingerprint, id: FingerprintIdentity): boolean {
  return (
    fp.cu_limit === id.cu_limit &&
    fp.cu_price === id.cu_price &&
    fp.init_buy_lamports === id.init_buy_lamports &&
    fp.max_cost_lamports === id.max_cost_lamports &&
    fp.spendable_lamports_in === id.spendable_lamports_in &&
    fp.first_slot_buy_lamports === id.first_slot_buy_lamports &&
    fp.first_slot_sell_lamports === id.first_slot_sell_lamports &&
    tidySolDecimal(fp.bucket_size_amount) === id.bucket_size_amount &&
    ixLabelsEqual(fp.ix_labels, id.ix_labels)
  );
}

/** Find the saved fingerprint whose identity matches this group key, or null. */
export function findFingerprintForGroupKey(
  gk: Record<string, string>,
  fingerprints: Fingerprint[],
  bucketWidthSol: number,
): Fingerprint | null {
  const id = fingerprintIdentityFromGroupKey(gk, bucketWidthSol);
  return fingerprints.find((fp) => fingerprintMatchesIdentity(fp, id)) ?? null;
}
