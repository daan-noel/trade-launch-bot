import { describe, expect, it } from 'vitest';
import type { StrategyRule } from './types';
import {
  findIdenticalRule,
  jsonValuesEqual,
  ruleIdentityOf,
  ruleMatchesIdentity,
} from './matchRuleIdentity';

function rule(partial: Partial<StrategyRule> & Pick<StrategyRule, 'id'>): StrategyRule {
  return {
    rule_name: 'r',
    fingerprint_id: 'fp-1',
    trade_mode: 'paper',
    is_active: false,
    buy_amount_lamports: 1_000_000_000,
    max_concurrent_tokens: 1,
    max_total_tokens: 0,
    params: { take_profit: 100, stop_loss: 30 },
    created_at: '',
    updated_at: '',
    ...partial,
  };
}

describe('jsonValuesEqual', () => {
  it('ignores object key order', () => {
    expect(jsonValuesEqual({ a: 1, b: 2 }, { b: 2, a: 1 })).toBe(true);
  });
  it('compares nested arrays', () => {
    expect(jsonValuesEqual({ x: [1, { y: 2 }] }, { x: [1, { y: 2 }] })).toBe(true);
    expect(jsonValuesEqual({ x: [1, 2] }, { x: [1, 3] })).toBe(false);
  });
});

describe('ruleMatchesIdentity', () => {
  it('matches when trading knobs + params match, ignoring name/active', () => {
    const a = rule({ id: '1', rule_name: 'promoted g1 c2', is_active: true });
    const b = rule({ id: '2', rule_name: 'other', is_active: false });
    expect(ruleMatchesIdentity(a, ruleIdentityOf(b))).toBe(true);
  });

  it('rejects when fingerprint or buy size differs', () => {
    const base = rule({ id: '1' });
    expect(
      ruleMatchesIdentity(base, ruleIdentityOf(rule({ id: '2', fingerprint_id: 'fp-other' }))),
    ).toBe(false);
    expect(
      ruleMatchesIdentity(base, ruleIdentityOf(rule({ id: '2', buy_amount_lamports: 2e9 }))),
    ).toBe(false);
  });

  it('excludeId skips self on edit', () => {
    const a = rule({ id: '1' });
    expect(ruleMatchesIdentity(a, ruleIdentityOf(a), '1')).toBe(false);
    expect(findIdenticalRule([a], ruleIdentityOf(a), '1')).toBeNull();
    expect(findIdenticalRule([a], ruleIdentityOf(a))?.id).toBe('1');
  });
});
