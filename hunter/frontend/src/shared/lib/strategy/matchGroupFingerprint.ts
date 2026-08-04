// Match a sweep/discovery `group_key` to a saved fingerprint using the same
// identity as promote/bind (`fingerprint_from_group_key` + `IDENTITY_WHERE`).
// `name` / `metric_config` are labels, not identity. Grouping-only axes
// (`is_cashback_enabled`, `token_program_id`) have no fingerprint field.

import { solToLamports, type Fingerprint } from './types';
import { configuredIxLabels } from 'lib/ixLabels';
import { tidySolDecimal } from 'utils/format';

/**
 * Attach a run's applied exact-set `ix_labels` filter onto a card's `group_key`
 * when Instruction labels was **not** in group-by (so the key omitted that axis).
 *
 * **Every surface that turns a `group_key` into a fingerprint identity must call
 * this first.** The filter is part of what selected the run's corpus, but it lives
 * on the RUN, not the key — the group-by form disables the filter box when
 * `ix_labels` is grouped, so at most one of the two is ever set. The backend
 * already does the same join (`promote_group` copies `run.ix_labels_filter` into
 * the drafted fingerprint), so a caller that skips it compares a key the backend
 * would never have produced: the identity branch of
 * {@link findFingerprintForGroupKey} can't hit, matching silently degrades to the
 * ambiguous single-compatible fallback, and a create/bind saves a fingerprint that
 * dropped the label axis and therefore arms on every token shape.
 *
 * Lives here, next to the identity functions it feeds, rather than in a page's
 * display helpers — that separation is what let the sweep page omit it.
 *
 * Never overwrites an existing `ix_labels` key; a `null`/empty filter is a no-op
 * (the same "empty collection is the not-set sentinel" rule as
 * `configuredIxLabels`).
 */
export function withIxLabelsFilter(
  groupKey: Record<string, string>,
  ixLabelsFilter: readonly string[] | null | undefined,
): Record<string, string> {
  if (Object.prototype.hasOwnProperty.call(groupKey, 'ix_labels')) return groupKey;
  if (!ixLabelsFilter || ixLabelsFilter.length === 0) return groupKey;
  return { ...groupKey, ix_labels: ixLabelsFilter.join(' | ') };
}

/** Identity axes only — what `find_or_create` compares. */
export interface FingerprintIdentity {
  cu_limit: number | null;
  cu_price: number | null;
  init_buy_lamports: number | null;
  max_cost_lamports: number | null;
  spendable_lamports_in: number | null;
  first_slot_buy_lamports: number | null;
  first_slot_sell_lamports: number | null;
  /** SOL bucket width, or `null` when the fingerprint matches exact amounts. */
  bucket_size_amount: number | null;
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
 * badges the card it was created from — but only while that refinement is
 * unambiguous (see {@link findFingerprintForGroupKey}).
 */
export function fingerprintIdentityFromGroupKey(
  gk: Record<string, string>,
  /** The width the card was grouped at, or `null` for an exact-grouped run — the
   *  precision must agree for a card and a fingerprint to be the same thing. */
  bucketWidthSol: number | null,
): FingerprintIdentity {
  const id: FingerprintIdentity = {
    cu_limit: null,
    cu_price: null,
    init_buy_lamports: null,
    max_cost_lamports: null,
    spendable_lamports_in: null,
    first_slot_buy_lamports: null,
    first_slot_sell_lamports: null,
    bucket_size_amount: bucketWidthSol == null ? null : tidySolDecimal(bucketWidthSol),
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
    // An empty label list is "not set" — same verdict the backend reaches via
    // `configured_labels`. A bare `!= null` here would offer Create on a group
    // the server then rejects as criterion-less.
    configuredIxLabels(id.ix_labels) != null
  );
}

/** Label-axis equality with `[]` normalized to "not set" first, so the two
 *  spellings of absent can never read as different identities. */
function ixLabelsEqual(rawA: string[] | null, rawB: string[] | null): boolean {
  const a = configuredIxLabels(rawA);
  const b = configuredIxLabels(rawB);
  if (a == null && b == null) return true;
  if (a == null || b == null) return false;
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) {
    if (a[i] !== b[i]) return false;
  }
  return true;
}

/** Whether two bucket widths denote the same precision. `null` (exact) only ever
 *  equals `null` — an exact fingerprint and a bucketed one are different rules
 *  even when every axis value agrees, because they arm on different token sets. */
function sameWidth(a: number | null, b: number | null): boolean {
  if (a == null || b == null) return a == null && b == null;
  return tidySolDecimal(a) === tidySolDecimal(b);
}

/**
 * True when every `IDENTITY_WHERE` axis matches (`NULL` is a value).
 *
 * This is the compare to reach for wherever the backend hands you an identity it
 * authored — the `identity` block of a resolved `GroupSelection`
 * (`lab/src/sweep/selection.rs`), which is literally the row
 * `FingerprintRepo::find_or_create` keys on. Prefer that over
 * {@link findFingerprintForGroupKey}, which must *reconstruct* an identity from a
 * group key: a group key is a lossy view of what selected a group (the run's
 * `ix_labels_filter` / `field_filters` live on the run row and never appear in
 * it), so the reconstruction runs wide and needs an ambiguous
 * single-compatible fallback to paper over the gap.
 */
export function fingerprintMatchesIdentity(fp: Fingerprint, id: FingerprintIdentity): boolean {
  return (
    fp.cu_limit === id.cu_limit &&
    fp.cu_price === id.cu_price &&
    fp.init_buy_lamports === id.init_buy_lamports &&
    fp.max_cost_lamports === id.max_cost_lamports &&
    fp.spendable_lamports_in === id.spendable_lamports_in &&
    fp.first_slot_buy_lamports === id.first_slot_buy_lamports &&
    fp.first_slot_sell_lamports === id.first_slot_sell_lamports &&
    sameWidth(fp.bucket_size_amount, id.bucket_size_amount) &&
    ixLabelsEqual(fp.ix_labels, id.ix_labels)
  );
}

/**
 * Compatible (superset) match for the creation-stats / sweep “already a
 * fingerprint” badge: every axis **present** in `gk` must agree with `fp`
 * (`∅` ⇒ FP axis unset; a concrete value ⇒ exact equal). Axes only on the
 * fingerprint (e.g. manual `ix_labels` after a create that didn’t group by
 * labels) do not break the match — the FP is a *candidate* refinement of the
 * card; {@link findFingerprintForGroupKey} decides whether that candidacy is
 * unambiguous enough to badge.
 *
 * Grouping-only keys are ignored. Bucket width must still agree.
 */
export function fingerprintCompatibleWithGroupKey(
  fp: Fingerprint,
  gk: Record<string, string>,
  /** The width the card was grouped at, or `null` for an exact-grouped run — the
   *  precision must agree for a card and a fingerprint to be the same thing. */
  bucketWidthSol: number | null,
): boolean {
  if (!sameWidth(fp.bucket_size_amount, bucketWidthSol)) {
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
          if (configuredIxLabels(fp.ix_labels) != null) return false;
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
 * 1. **Exact `IDENTITY_WHERE` identity wins** — the same comparison
 *    `find_or_create` runs, so a card always badges the fingerprint it would
 *    resolve to on create.
 * 2. No exact hit ⇒ fall back to {@link fingerprintCompatibleWithGroupKey}
 *    (superset) matching, but **only when exactly one** fingerprint is
 *    compatible — that keeps a card badging the fingerprint it was created
 *    from after a manual refinement (e.g. `ix_labels` added later).
 * 3. Two or more compatible refinements ⇒ `null`: axes the card didn’t group
 *    by (typically `ix_labels`) distinguish those fingerprints, so badging any
 *    one of them would conflate genuinely different identities. The caller
 *    then shows Create — safe, because the exact identity demonstrably isn’t
 *    saved and `find_or_create` compares exact identity (no dupe risk).
 */
export function findFingerprintForGroupKey(
  gk: Record<string, string>,
  fingerprints: Fingerprint[],
  /** The width the card was grouped at, or `null` for an exact-grouped run — the
   *  precision must agree for a card and a fingerprint to be the same thing. */
  bucketWidthSol: number | null,
): Fingerprint | null {
  const id = fingerprintIdentityFromGroupKey(gk, bucketWidthSol);
  let compatible: Fingerprint | null = null;
  let compatibleN = 0;
  for (const fp of fingerprints) {
    if (!fingerprintCompatibleWithGroupKey(fp, gk, bucketWidthSol)) continue;
    if (fingerprintMatchesIdentity(fp, id)) return fp;
    compatible ??= fp;
    compatibleN += 1;
  }
  return compatibleN === 1 ? compatible : null;
}
