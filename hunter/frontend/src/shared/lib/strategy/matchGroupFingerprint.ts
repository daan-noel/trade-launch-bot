// Match a sweep/discovery `group_key` to a saved fingerprint, using the same
// identity as promote/bind (`fingerprint_from_group_key` + `IDENTITY_WHERE`).
// `name` is a label, not identity; `metric_config` is (see `FingerprintIdentity`).
//
// **A group key carries predicates, not rendered labels.** A card's window IS the
// predicate a fingerprint stores, so identity here is a comparison of the same
// type on both sides — the retired form parsed a `"1.5–1.6"` string back into an
// anchor plus a row-wide width, a second lossy derivation that could not represent
// a `u64::MAX` ceiling at all.

import { configuredIxLabels } from 'lib/ixLabels';
import type { Fingerprint } from './types';
import {
  AXES,
  axisDef,
  compareBounds,
  lamportsToSolLabel,
  configuredAxes,
  isAxisId,
  type AxisId,
  type AxisPredicate,
  type Criteria,
} from './fingerprintAxes';

/**
 * Attach a run's applied exact-set `ix_labels` filter onto a card's `group_key`
 * when Instruction labels was **not** in group-by (so the key omitted that axis).
 *
 * **Every surface that turns a `group_key` into a fingerprint identity must call
 * this first.** The filter is part of what selected the run's corpus, but it lives
 * on the RUN, not the key — the group-by form disables the filter box when
 * `ix_labels` is grouped, so at most one of the two is ever set. The backend does
 * the same join (`promote_group` copies `run.ix_labels_filter` into the drafted
 * fingerprint), so a caller that skips it compares a key the backend would never
 * have produced: the identity branch of {@link findFingerprintForGroupKey} can't
 * hit, matching degrades to the ambiguous single-compatible fallback, and a
 * create/bind saves a fingerprint that dropped the label axis and therefore arms on
 * every token shape.
 *
 * Never overwrites an existing `ix_labels` key; a `null`/empty filter is a no-op
 * (the same "empty collection is the not-set sentinel" rule as
 * `configuredIxLabels`).
 */
export function withIxLabelsFilter(
  groupKey: Record<string, unknown>,
  ixLabelsFilter: readonly string[] | null | undefined,
): Record<string, unknown> {
  if (Object.prototype.hasOwnProperty.call(groupKey, 'ix_labels')) return groupKey;
  if (!ixLabelsFilter || ixLabelsFilter.length === 0) return groupKey;
  return { ...groupKey, ix_labels: { kind: 'labels', labels: [...ixLabelsFilter] } };
}

/** Identity axes only — what `find_or_create` compares. */
export interface FingerprintIdentity {
  criteria: Criteria;
  /** Matches every token, ignoring every axis. Part of identity (the backend
   *  `IDENTITY_WHERE` compares it), and never reconstructible from a group key — a
   *  group names axis VALUES, so {@link fingerprintIdentityFromGroupKey} always
   *  yields `false`. Carrying it anyway is what stops a saved wildcard row from
   *  keying identically to an axis-free card and badging it. */
  wildcard: boolean;
  /** Per-fingerprint metric config. Selects no token, so it is not MATCH identity —
   *  but it IS ROW identity, because it compiles into that row's live `m_flow_ix`
   *  patterns. Eleven `8dtx · <router>` rows share `{}` + `wildcard` and differ only
   *  here; without it {@link fingerprintIdentityKey} would badge an arbitrary one of
   *  them while `find_or_create` resolved to another.
   *
   *  **Optional, and absent means `{}`** — which is what a card creates with, so a
   *  group key and the backend's match-identity DTO both leave it unset and still key
   *  onto the right row. Only {@link fingerprintToIdentity} fills it in. */
  metric_config?: Record<string, unknown>;
}

/** One group-key value, as the backend serializes it. */
type GroupValue =
  | { kind: 'missing' }
  | { kind: 'text'; value: string }
  | { kind: 'flag'; value: boolean }
  | { kind: 'labels'; labels: string[] }
  | { kind: 'window'; min?: string; max?: string };

function asGroupValue(raw: unknown): GroupValue | null {
  if (raw == null || typeof raw !== 'object') return null;
  const v = raw as { kind?: unknown };
  switch (v.kind) {
    case 'missing':
    case 'text':
    case 'flag':
    case 'labels':
    case 'window':
      return raw as GroupValue;
    default:
      return null;
  }
}

/** The fingerprint predicate a group value asserts, or `null` when it names
 *  nothing a rule can match on (`missing`, or a grouping-only field). */
export function predicateFromGroupValue(raw: unknown): AxisPredicate | null {
  const v = asGroupValue(raw);
  if (!v) return null;
  if (v.kind === 'window') {
    if (v.min == null && v.max == null) return null;
    return { kind: 'range', ...(v.min != null && { min: v.min }), ...(v.max != null && { max: v.max }) };
  }
  if (v.kind === 'labels') {
    return v.labels.length > 0 ? { kind: 'sequence', labels: v.labels } : null;
  }
  return null;
}

/**
 * Rebuild fingerprint identity from a stored group key — a **copy** of its
 * predicates.
 *
 * Axes **absent** from `gk` stay absent here — the same shape `find_or_create`
 * stores — but {@link findFingerprintForGroupKey} treats absent keys as
 * unconstrained when falling back to compatible (superset) matching, so a
 * fingerprint later refined with extra axes still badges the card it was created
 * from, while that refinement is unambiguous.
 */
export function fingerprintIdentityFromGroupKey(
  gk: Record<string, unknown>,
): FingerprintIdentity {
  const criteria: Criteria = {};
  for (const [tag, raw] of Object.entries(gk)) {
    if (!isAxisId(tag)) continue; // grouping-only fields have no fingerprint axis
    const pred = predicateFromGroupValue(raw);
    if (pred) criteria[tag] = pred;
  }
  // A group key names axis VALUES, so it can never describe "every token", and a
  // card is created with no metric config (absent = `{}`).
  return { criteria, wildcard: false };
}

/** True when the identity carries at least one match criterion — mirrors the
 *  backend `Fingerprint::has_any_criterion`. A group with none (the ALL group, or
 *  one grouped only by the grouping-only fields) cannot become a fingerprint, and
 *  the create endpoints reject it. */
export function identityHasCriterion(id: FingerprintIdentity): boolean {
  // The explicit "every token" criterion — counted here exactly as the backend
  // counts it, or a wildcard row reads as criterion-less on this side.
  return id.wildcard || configuredAxes(id.criteria).length > 0;
}

/** Whether two predicates name the same set. Bounds compare as decimal STRINGS —
 *  `Number()` would round a `u64::MAX` ceiling and call two distinct amounts
 *  equal, which is exactly how a ceiling used to be unrepresentable here. */
export function predicatesEqual(a: AxisPredicate | undefined, b: AxisPredicate | undefined): boolean {
  if (a == null || b == null) return a == null && b == null;
  if (a.kind !== b.kind) return false;
  if (a.kind === 'sequence' && b.kind === 'sequence') {
    const x = configuredIxLabels(a.labels);
    const y = configuredIxLabels(b.labels);
    if (x == null || y == null) return x == null && y == null;
    return x.length === y.length && x.every((v, i) => v === y[i]);
  }
  if (a.kind === 'range' && b.kind === 'range') {
    const bound = (p?: string, q?: string) =>
      p == null || q == null ? p == null && q == null : compareBounds(p, q) === 0;
    return bound(a.min, b.min) && bound(a.max, b.max);
  }
  return false;
}

/**
 * True when every match axis agrees — the same token set.
 *
 * MATCH identity only: two rows differing solely in `metric_config` both return
 * true. For the row `find_or_create` resolves to, key with
 * {@link fingerprintIdentityKey}.
 *
 * This is the compare to reach for wherever the backend hands you an identity it
 * authored — the `identity` block of a resolved `GroupSelection`
 * (`lab/src/sweep/selection.rs`), which is literally the row
 * `FingerprintRepo::find_or_create` keys on. Prefer that over
 * {@link findFingerprintForGroupKey}, which must reconstruct an identity from a
 * group key: a key is a lossy view of what selected a group (the run's
 * `ix_labels_filter` / `field_filters` live on the run row and never appear in it),
 * so the reconstruction runs wide and needs an ambiguous single-compatible fallback
 * to paper over the gap.
 */
export function fingerprintMatchesIdentity(fp: Fingerprint, id: FingerprintIdentity): boolean {
  if (fp.wildcard !== id.wildcard) return false;
  const a = fp.criteria ?? {};
  const b = id.criteria ?? {};
  return AXES.every((def) => predicatesEqual(a[def.id], b[def.id]));
}

/**
 * Compatible (superset) match for the "already a fingerprint" badge: every axis
 * **present** in `gk` must agree with `fp` (`missing` ⇒ the FP axis is unset too;
 * a value ⇒ the same predicate). Axes only on the fingerprint (e.g. manual
 * `ix_labels` added after a create that didn't group by labels) do not break the
 * match — the FP is a *candidate* refinement of the card, and
 * {@link findFingerprintForGroupKey} decides whether that candidacy is unambiguous
 * enough to badge.
 *
 * Grouping-only keys are ignored.
 */
export function fingerprintCompatibleWithGroupKey(
  fp: Fingerprint,
  gk: Record<string, unknown>,
): boolean {
  // A wildcard row is not a refinement of anything — it drops every axis the card
  // is made of, so badging a card with it would claim the card's group is what the
  // rule arms on when the rule arms on every token.
  if (fp.wildcard) return false;
  const criteria = fp.criteria ?? {};
  for (const [tag, raw] of Object.entries(gk)) {
    if (!isAxisId(tag)) continue;
    const pred = predicateFromGroupValue(raw);
    if (pred == null) {
      // The card's tokens have NO value on this axis. A fingerprint that configures
      // it matches tokens that DO — the opposite population, not a refinement.
      if (criteria[tag] != null) return false;
      continue;
    }
    if (!predicatesEqual(criteria[tag], pred)) return false;
  }
  return true;
}

/** Canonical `IDENTITY_WHERE` key — a stable string for Map lookups. */
export function fingerprintIdentityKey(id: FingerprintIdentity): string {
  const parts = AXES.map((def) => {
    const p = (id.criteria ?? {})[def.id];
    if (p == null) return '';
    if (p.kind === 'sequence') {
      const labels = configuredIxLabels(p.labels);
      return labels == null ? '' : `seq:${labels.join('\0')}`;
    }
    return `range:${p.min ?? ''}:${p.max ?? ''}`;
  });
  parts.push(id.wildcard ? 'any' : '');
  parts.push(canonicalJson(id.metric_config ?? {}));
  return parts.join('|');
}

/** Stable JSON with object keys sorted, so two configs that Postgres `jsonb` calls
 *  equal (it normalises key order) produce ONE string here. `JSON.stringify` alone
 *  keeps insertion order, which would fork the key on a re-serialized row. */
function canonicalJson(v: unknown): string {
  if (v === null || typeof v !== 'object') return JSON.stringify(v) ?? 'null';
  if (Array.isArray(v)) return `[${v.map(canonicalJson).join(',')}]`;
  const entries = Object.entries(v as Record<string, unknown>)
    .filter(([, val]) => val !== undefined)
    .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0));
  return `{${entries.map(([k, val]) => `${JSON.stringify(k)}:${canonicalJson(val)}`).join(',')}}`;
}

/** Identity from a saved fingerprint row (same shape as the group-key rebuild). */
export function fingerprintToIdentity(fp: Fingerprint): FingerprintIdentity {
  return {
    criteria: fp.criteria ?? {},
    wildcard: fp.wildcard,
    metric_config: fp.metric_config ?? {},
  };
}

/** Exact-identity index for O(1) group→fingerprint hits. First write wins on dupes. */
export function indexFingerprintsByIdentity(
  fingerprints: readonly Fingerprint[],
): Map<string, Fingerprint> {
  const map = new Map<string, Fingerprint>();
  for (const fp of fingerprints) {
    const key = fingerprintIdentityKey(fingerprintToIdentity(fp));
    if (!map.has(key)) map.set(key, fp);
  }
  return map;
}

export interface FindFingerprintOptions {
  /** Prebuilt {@link indexFingerprintsByIdentity} — skips the exact-scan pass. */
  byIdentity?: Map<string, Fingerprint>;
}

/**
 * Find the saved fingerprint for this group key.
 *
 * 1. **Exact `IDENTITY_WHERE` identity wins** — the same comparison
 *    `find_or_create` runs, so a card always badges the fingerprint it would
 *    resolve to on create. Prefer an {@link indexFingerprintsByIdentity} map for
 *    O(1) here when matching many groups against one library.
 * 2. No exact hit ⇒ fall back to {@link fingerprintCompatibleWithGroupKey}
 *    (superset) matching, but **only when exactly one** fingerprint is compatible —
 *    that keeps a card badging the fingerprint it was created from after a manual
 *    refinement.
 * 3. Two or more compatible refinements ⇒ `null`: axes the card didn't group by
 *    distinguish those fingerprints, so badging any one of them would conflate
 *    genuinely different identities. The caller then shows Create — safe, because
 *    the exact identity demonstrably isn't saved and `find_or_create` compares
 *    exact identity (no dupe risk).
 */
export function findFingerprintForGroupKey(
  gk: Record<string, unknown>,
  fingerprints: readonly Fingerprint[],
  opts?: FindFingerprintOptions,
): Fingerprint | null {
  const id = fingerprintIdentityFromGroupKey(gk);
  const byIdentity = opts?.byIdentity;
  if (byIdentity) {
    const exact = byIdentity.get(fingerprintIdentityKey(id));
    if (exact) return exact;
  }
  let compatible: Fingerprint | null = null;
  let compatibleN = 0;
  for (const fp of fingerprints) {
    if (!fingerprintCompatibleWithGroupKey(fp, gk)) continue;
    // Without an index, exact identity still wins inside the compatible scan.
    if (!byIdentity && fingerprintMatchesIdentity(fp, id)) return fp;
    compatible ??= fp;
    compatibleN += 1;
  }
  return compatibleN === 1 ? compatible : null;
}

/** Per-group badge/create verdict for creation-stats / discovery cards. */
export interface GroupFingerprintMatch {
  matched: Fingerprint | null;
  identity: FingerprintIdentity;
  canCreate: boolean;
}

/**
 * Resolve every group's fingerprint badge in one pass: build the identity index
 * once, attach the run-level `ix_labels` filter, and avoid rebuilding identity
 * twice (match + canCreate).
 *
 * There is no "overflow" verdict any more: a bound is a decimal string over the
 * full `u64` domain, so a `max_sol_cost = u64::MAX` ceiling — which used to be
 * unstorable, and so uncreatable from a card — is an ordinary value.
 */
export function matchFingerprintsForGroups(
  groups: readonly { g: number; group_key: Record<string, unknown> }[],
  fingerprints: readonly Fingerprint[],
  ixLabelsFilter: readonly string[] | null | undefined,
  /** When the whole run is scoped to one fingerprint, that id wins for every card. */
  scoped: Fingerprint | null,
): Map<number, GroupFingerprintMatch> {
  const map = new Map<number, GroupFingerprintMatch>();
  const byIdentity = scoped ? null : indexFingerprintsByIdentity(fingerprints);
  for (const g of groups) {
    const gk = withIxLabelsFilter(g.group_key, ixLabelsFilter);
    const identity = fingerprintIdentityFromGroupKey(gk);
    const matched =
      scoped ??
      findFingerprintForGroupKey(gk, fingerprints, { byIdentity: byIdentity ?? undefined });
    map.set(g.g, { matched, identity, canCreate: matched == null && identityHasCriterion(identity) });
  }
  return map;
}

/** A card's group key as display chips, in registry order. */
export function renderGroupKey(gk: Record<string, unknown>): [string, string][] {
  return Object.entries(gk).map(([tag, raw]) => {
    const v = asGroupValue(raw);
    if (!v) return [tag, String(raw)];
    switch (v.kind) {
      case 'missing':
        return [tag, '∅'];
      case 'text':
        return [tag, v.value];
      case 'flag':
        return [tag, String(v.value)];
      case 'labels':
        return [tag, v.labels.join(' | ')];
      case 'window': {
        const unit = isAxisId(tag) ? axisDef(tag as AxisId).unit : 'count';
        const pred = predicateFromGroupValue(raw);
        if (!pred || pred.kind !== 'range') return [tag, 'any'];
        const fmt = (s: string) => (unit === 'lamports' ? lamportsToSolLabel(s) : s);
        if (pred.min != null && pred.max != null) {
          return [tag, pred.min === pred.max ? fmt(pred.min) : `${fmt(pred.min)}–${fmt(pred.max)}`];
        }
        if (pred.min != null) return [tag, `≥${fmt(pred.min)}`];
        if (pred.max != null) return [tag, `≤${fmt(pred.max)}`];
        return [tag, 'any'];
      }
    }
  });
}
