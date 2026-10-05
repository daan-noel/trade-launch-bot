import type { StrategyRule } from 'lib/strategy/types';
import { isCopyRule } from 'lib/strategy/copyRule';
import { copyHref, rulesHref } from 'lib/strategy/nav';

/**
 * A purpose a fingerprint can belong to. The Fingerprints page grows one tab
 * per entry here. A fingerprint belongs to a purpose when every rule that uses
 * it matches that purpose and no other. An unused fingerprint, one shared with
 * a rule of another purpose, or one that satisfies two purposes, stays on
 * General.
 *
 * Add a purpose by appending one entry. Matchers must be disjoint: a rule that
 * satisfies two purposes leaves its fingerprint on General.
 */
export const FINGERPRINT_PURPOSES = [
  {
    id: 'copy',
    label: 'Copy',
    empty:
      'No copy fingerprints. A fingerprint lands here when every rule that uses it is a copy rule.',
    matches: isCopyRule,
    ruleHref: copyHref,
  },
] as const;

export type FingerprintPurpose = (typeof FINGERPRINT_PURPOSES)[number];

/** `general` is the remainder: unused fingerprints and ones no single purpose owns. */
export const GENERAL_FINGERPRINT_PURPOSE = 'general' as const;

export type FingerprintPurposeId = typeof GENERAL_FINGERPRINT_PURPOSE | FingerprintPurpose['id'];

const PURPOSE_IDS = new Set<string>([
  GENERAL_FINGERPRINT_PURPOSE,
  ...FINGERPRINT_PURPOSES.map((p) => p.id),
]);

export function isFingerprintPurposeId(v: string | null): v is FingerprintPurposeId {
  return v != null && PURPOSE_IDS.has(v);
}

/**
 * Purpose of each fingerprint that at least one rule uses. A missing id is
 * General (the fingerprint is unused).
 */
export function fingerprintPurposes(
  rules: Pick<StrategyRule, 'fingerprint_id' | 'params'>[],
): Map<string, FingerprintPurposeId> {
  const hits = new Map<string, boolean[]>();
  for (const rule of rules) {
    const prev = hits.get(rule.fingerprint_id);
    const next = FINGERPRINT_PURPOSES.map((p, i) => p.matches(rule) && (prev?.[i] ?? true));
    hits.set(rule.fingerprint_id, next);
  }
  const out = new Map<string, FingerprintPurposeId>();
  for (const [id, flags] of hits) {
    const matched = FINGERPRINT_PURPOSES.filter((_, i) => flags[i]);
    out.set(id, matched.length === 1 ? matched[0].id : GENERAL_FINGERPRINT_PURPOSE);
  }
  return out;
}

export function purposeOf(
  fingerprintId: string,
  purposes: Map<string, FingerprintPurposeId>,
): FingerprintPurposeId {
  return purposes.get(fingerprintId) ?? GENERAL_FINGERPRINT_PURPOSE;
}

/** Fingerprints the metric-rule picker omits: a single purpose owns them. */
export function copyFingerprintIds(
  rules: Pick<StrategyRule, 'fingerprint_id' | 'params'>[],
): Set<string> {
  const ids = new Set<string>();
  for (const [id, purpose] of fingerprintPurposes(rules)) {
    if (purpose === 'copy') ids.add(id);
  }
  return ids;
}

/** Board a rule opens on. The first purpose whose matcher accepts the rule. */
export function fingerprintRuleHref(rule: Pick<StrategyRule, 'id' | 'params'>): string {
  for (const purpose of FINGERPRINT_PURPOSES) {
    if (purpose.matches(rule)) return purpose.ruleHref(rule.id);
  }
  return rulesHref(rule.id);
}

/** Board link for a Fingerprints tab. General opens Rules. */
export function fingerprintBoardHref(purpose: FingerprintPurposeId): string {
  const found = FINGERPRINT_PURPOSES.find((p) => p.id === purpose);
  return found ? found.ruleHref() : rulesHref();
}
