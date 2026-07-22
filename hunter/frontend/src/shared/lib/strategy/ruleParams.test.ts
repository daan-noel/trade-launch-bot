import { describe, expect, it } from 'vitest';
import {
  emptyRuleParams,
  ruleParamsFromJson,
  ruleParamsJsonEqual,
  ruleParamsToJson,
} from './ruleParams';

describe('ruleParamsJsonEqual', () => {
  it('treats null / missing leaves as equal', () => {
    expect(ruleParamsJsonEqual({ take_profit: 50, stop_loss: null }, { take_profit: 50 })).toBe(
      true,
    );
  });

  it('ignores key order', () => {
    expect(
      ruleParamsJsonEqual(
        { stop_loss: 30, take_profit: 50 },
        { take_profit: 50, stop_loss: 30 },
      ),
    ).toBe(true);
  });

  it('detects different TP/SL', () => {
    expect(ruleParamsJsonEqual({ take_profit: 50 }, { take_profit: 40 })).toBe(false);
  });

  it('compares nested entry conditions', () => {
    const a = {
      take_profit: 50,
      entry: { m_snapshot: { time: [{ operator: '>', value: 10 }] } },
    };
    const b = {
      take_profit: 50,
      entry: { m_snapshot: { time: [{ operator: '>', value: 10 }] } },
    };
    const c = {
      take_profit: 50,
      entry: { m_snapshot: { time: [{ operator: '>', value: 20 }] } },
    };
    expect(ruleParamsJsonEqual(a, b)).toBe(true);
    expect(ruleParamsJsonEqual(a, c)).toBe(false);
  });

  it('distinguishes a re-entry block from a one-shot rule', () => {
    const oneShot = { take_profit: 50 };
    const reentry = {
      take_profit: 50,
      reentry: { cooldown_sec: 5, max_episodes_per_token: 10 },
    };
    expect(ruleParamsJsonEqual(oneShot, reentry)).toBe(false);
  });
});

describe('reentry round-trip (the silent-strip regression)', () => {
  it('carries reentry through toJson → fromJson unchanged', () => {
    const json = {
      take_profit: 50,
      reentry: { cooldown_sec: 5, max_episodes_per_token: 10 },
    };
    // fromJson → toJson must preserve reentry (before this field it was dropped).
    const form = ruleParamsFromJson(json, undefined);
    expect(form.reentry).toEqual({ cooldown_sec: 5, max_episodes_per_token: 10 });
    expect(ruleParamsJsonEqual(ruleParamsToJson(form), json)).toBe(true);
  });

  it('omits reentry when absent (one-shot stays one-shot)', () => {
    const form = emptyRuleParams();
    expect(form.reentry).toBeNull();
    expect('reentry' in ruleParamsToJson(form)).toBe(false);
  });

  it('drops a partial/malformed reentry object', () => {
    // Missing max_episodes_per_token ⇒ not a valid block ⇒ null (backend would reject).
    expect(ruleParamsFromJson({ reentry: { cooldown_sec: 5 } }, undefined).reentry).toBeNull();
    // Non-object ⇒ null.
    expect(ruleParamsFromJson({ reentry: 5 }, undefined).reentry).toBeNull();
  });

  it('does not serialize a reentry with non-finite fields', () => {
    // A field left blank in the editor becomes NaN — must not reach the wire.
    const json = ruleParamsToJson({
      ...emptyRuleParams(),
      reentry: { cooldown_sec: NaN, max_episodes_per_token: 10 },
    });
    expect('reentry' in json).toBe(false);
  });
});
