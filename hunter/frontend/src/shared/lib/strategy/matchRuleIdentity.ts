// Match a rule draft to an existing `strategy_rules` row using the same identity
// as the backend Duplicate gate (`RuleRepo::find_identical`): fingerprint + trade
// knobs + params. `rule_name` / `is_active` / `is_enabled` are labels/lifecycle,
// not identity
// (mirrors fingerprint `find_or_create`, which ignores `name`).

import type { CreateRuleBody, StrategyRule, TradeMode } from './types';

/** Trading identity — what create/save refuse to duplicate. */
export interface RuleIdentity {
  fingerprint_id: string;
  trade_mode: TradeMode | string;
  buy_amount_lamports: number;
  max_concurrent_tokens: number;
  max_total_tokens: number;
  params: unknown;
}

/** Stable deep equality for JSON-shaped params (key-order independent). */
export function jsonValuesEqual(a: unknown, b: unknown): boolean {
  if (a === b) return true;
  if (a == null || b == null) return a === b;
  if (typeof a !== typeof b) return false;
  if (Array.isArray(a)) {
    if (!Array.isArray(b) || a.length !== b.length) return false;
    for (let i = 0; i < a.length; i++) {
      if (!jsonValuesEqual(a[i], b[i])) return false;
    }
    return true;
  }
  if (typeof a === 'object') {
    const ao = a as Record<string, unknown>;
    const bo = b as Record<string, unknown>;
    const keys = Object.keys(ao).sort();
    const bKeys = Object.keys(bo).sort();
    if (keys.length !== bKeys.length) return false;
    for (let i = 0; i < keys.length; i++) {
      if (keys[i] !== bKeys[i]) return false;
      if (!jsonValuesEqual(ao[keys[i]!], bo[bKeys[i]!])) return false;
    }
    return true;
  }
  // Numbers: treat NaN as unequal to everything (incl. NaN).
  return Object.is(a, b);
}

/** True when every identity axis matches (`excludeId` skips that row on edit). */
export function ruleMatchesIdentity(
  rule: StrategyRule,
  id: RuleIdentity,
  excludeId?: string,
): boolean {
  if (excludeId && rule.id === excludeId) return false;
  return (
    rule.fingerprint_id === id.fingerprint_id &&
    rule.trade_mode === id.trade_mode &&
    rule.buy_amount_lamports === id.buy_amount_lamports &&
    rule.max_concurrent_tokens === id.max_concurrent_tokens &&
    rule.max_total_tokens === id.max_total_tokens &&
    jsonValuesEqual(rule.params, id.params)
  );
}

/** First saved rule with the same trading identity, or null. */
export function findIdenticalRule(
  rules: StrategyRule[],
  id: RuleIdentity,
  excludeId?: string,
): StrategyRule | null {
  return rules.find((r) => ruleMatchesIdentity(r, id, excludeId)) ?? null;
}

/** Identity slice of a create/promote draft (or full rule). */
export function ruleIdentityOf(
  draft: Pick<
    CreateRuleBody,
    | 'fingerprint_id'
    | 'trade_mode'
    | 'buy_amount_lamports'
    | 'max_concurrent_tokens'
    | 'max_total_tokens'
    | 'params'
  >,
): RuleIdentity {
  return {
    fingerprint_id: draft.fingerprint_id,
    trade_mode: draft.trade_mode,
    buy_amount_lamports: draft.buy_amount_lamports,
    max_concurrent_tokens: draft.max_concurrent_tokens,
    max_total_tokens: draft.max_total_tokens,
    params: draft.params,
  };
}

/** User-facing message matching the backend 409 body shape. */
export function duplicateRuleMessage(existing: StrategyRule): string {
  return `identical rule already exists: "${existing.rule_name}" (${existing.id})`;
}
