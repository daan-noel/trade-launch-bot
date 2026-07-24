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
 *
 * Axes **absent** from `gk` stay `null` here — the same shape `find_or_create`
 * stores — but {@link findFingerprintForGroupKey} treats absent keys as
 * unconstrained when falling back to compatible (superset) matching, so a
 * fingerprint later refined with extra axes (e.g. manual `ix_labels`) still
 * badges the card it was created from.
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

/** True when the identity carries at least one match criterion — mirrors the
 *  backend `Fingerprint::has_any_criterion` (`bucket_size_amount` alone doesn't
 *  count). A group with none (e.g. the ALL group, or grouping only by the
 *  grouping-only axes) can't be turned into a fingerprint — the create endpoints
 *  reject it. */
export function identityHasCriterion(id: FingerprintIdentity): boolean {
  return (
    id.cu_limit != null ||
    id.cu_price != null ||
    id.init_buy_lamports != null ||
    id.max_cost_lamports != null ||
    id.spendable_lamports_in != null ||
    id.first_slot_buy_lamports != null ||
    id.first_slot_sell_lamports != null ||
    id.ix_labels != null
  );
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

/** How many match axes are configured on a fingerprint (for “most specific” pick). */
function configuredAxisCount(fp: Fingerprint): number {
  let n = 0;
  if (fp.cu_limit != null) n += 1;
  if (fp.cu_price != null) n += 1;
  if (fp.init_buy_lamports != null) n += 1;
  if (fp.max_cost_lamports != null) n += 1;
  if (fp.spendable_lamports_in != null) n += 1;
  if (fp.first_slot_buy_lamports != null) n += 1;
  if (fp.first_slot_sell_lamports != null) n += 1;
  if (fp.ix_labels != null && fp.ix_labels.length > 0) n += 1;
  return n;
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

/**
 * Compatible (superset) match for the creation-stats / sweep “already a
 * fingerprint” badge: every axis **present** in `gk` must agree with `fp`
 * (`∅` ⇒ FP axis unset; a concrete value ⇒ exact equal). Axes only on the
 * fingerprint (e.g. manual `ix_labels` after a create that didn’t group by
 * labels) do not break the match — the FP is a refinement of the card.
 *
 * Grouping-only keys are ignored. Bucket width must still agree.
 */
export function fingerprintCompatibleWithGroupKey(
  fp: Fingerprint,
  gk: Record<string, string>,
  bucketWidthSol: number,
): boolean {
  if (tidySolDecimal(fp.bucket_size_amount) !== tidySolDecimal(bucketWidthSol)) {
    return false;
  }
  for (const [tag, raw] of Object.entries(gk)) {
    if (tag === 'is_cashback_enabled' || tag === 'token_program_id') continue;
    const missing = raw === '∅';
    switch (tag) {
      case 'cu_limit': {
        if (missing) {
          if (fp.cu_limit != null) return false;
        } else {
          const n = Number(raw);
          if (!Number.isFinite(n) || fp.cu_limit !== n) return false;
        }
        break;
      }
      case 'cu_price': {
        if (missing) {
          if (fp.cu_price != null) return false;
        } else {
          const n = Number(raw);
          if (!Number.isFinite(n) || fp.cu_price !== n) return false;
        }
        break;
      }
      case 'initial_buy_sol': {
        if (missing) {
          if (fp.init_buy_lamports != null) return false;
        } else if (fp.init_buy_lamports !== parseLoLamports(raw)) {
          return false;
        }
        break;
      }
      case 'max_cost_lamports': {
        if (missing) {
          if (fp.max_cost_lamports != null) return false;
        } else if (fp.max_cost_lamports !== parseLoLamports(raw)) {
          return false;
        }
        break;
      }
      case 'spendable_lamports_in': {
        if (missing) {
          if (fp.spendable_lamports_in != null) return false;
        } else if (fp.spendable_lamports_in !== parseLoLamports(raw)) {
          return false;
        }
        break;
      }
      case 'first_slot_buy_sol': {
        if (missing) {
          if (fp.first_slot_buy_lamports != null) return false;
        } else if (fp.first_slot_buy_lamports !== parseLoLamports(raw)) {
          return false;
        }
        break;
      }
      case 'first_slot_sell_sol': {
        if (missing) {
          if (fp.first_slot_sell_lamports != null) return false;
        } else if (fp.first_slot_sell_lamports !== parseLoLamports(raw)) {
          return false;
        }
        break;
      }
      case 'ix_labels': {
        if (missing) {
          if (fp.ix_labels != null && fp.ix_labels.length > 0) return false;
        } else if (!ixLabelsEqual(fp.ix_labels, raw.split(' | '))) {
          return false;
        }
        break;
      }
      default:
        break;
    }
  }
  return true;
}

/**
 * Find the saved fingerprint for this group key.
 *
 * Prefers the most-specific fingerprint {@link fingerprintCompatibleWithGroupKey}
 * with the group key. Exact `IDENTITY_WHERE` identity is included in that set;
 * picking most-specific means a card that didn’t group by `ix_labels` still
 * badges a fingerprint later refined with labels (and Create stays hidden
 * instead of minting a sparse duplicate of the pre-refinement identity).
 */
export function findFingerprintForGroupKey(
  gk: Record<string, string>,
  fingerprints: Fingerprint[],
  bucketWidthSol: number,
): Fingerprint | null {
  const id = fingerprintIdentityFromGroupKey(gk, bucketWidthSol);
  let best: Fingerprint | null = null;
  let bestN = -1;
  for (const fp of fingerprints) {
    if (!fingerprintCompatibleWithGroupKey(fp, gk, bucketWidthSol)) continue;
    const n = configuredAxisCount(fp);
    // Prefer more axes; on a tie prefer exact identity (same as find_or_create).
    if (
      n > bestN ||
      (n === bestN &&
        best != null &&
        fingerprintMatchesIdentity(fp, id) &&
        !fingerprintMatchesIdentity(best, id))
    ) {
      best = fp;
      bestN = n;
    }
  }
  return best;
}
