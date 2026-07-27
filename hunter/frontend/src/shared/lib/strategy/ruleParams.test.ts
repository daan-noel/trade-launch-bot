import { describe, expect, it } from 'vitest';
import {
  emptyRuleParams,
  ruleParamsFromJson,
  ruleParamsJsonEqual,
  ruleParamsToJson,
  sideInstances,
} from './ruleParams';
import type { StrategyRegistry } from './registry';

// Minimal registry: a dynamic price-window group (so `window_size_sec` is a strict
// param and `trail` is a known metric with a tolerance).
const REG: StrategyRegistry = {
  operators: ['>', '>=', '<', '<=', '=', '!='],
  groups: [
    {
      name: 'm_price_window',
      kind: 'dynamic',
      strict_params: [{ name: 'window_size_sec', required: true }],
      metrics: [{ name: 'trail', unit: 'percent', eq_tolerance: 0.1, monotonic: false, hue: 45 }],
    },
  ],
};

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

describe('exclusivity round-trip', () => {
  it('carries exclusive + priority through toJson → fromJson unchanged', () => {
    const json = { take_profit: 50, exclusive: true, priority: -3 };
    const form = ruleParamsFromJson(json, undefined);
    expect(form.exclusive).toBe(true);
    expect(form.priority).toBe(-3);
    expect(ruleParamsJsonEqual(ruleParamsToJson(form), json)).toBe(true);
  });

  it('omits the defaults so existing rules round-trip unchanged', () => {
    const form = emptyRuleParams();
    expect(form.exclusive).toBe(false);
    expect(form.priority).toBe(0);
    const json = ruleParamsToJson(form);
    expect('exclusive' in json).toBe(false);
    expect('priority' in json).toBe(false);
  });
});

describe('multi-window per group (the array-form round-trip)', () => {
  it('serializes one instance as an object, two as an array (mirrors the backend)', () => {
    const one = ruleParamsFromJson(
      { entry: { m_price_window: { window_size_sec: 30, trail: [{ operator: '>=', value: 8 }] } } },
      REG,
    );
    expect((ruleParamsToJson(one).entry as Record<string, unknown>).m_price_window).not.toBeInstanceOf(
      Array,
    );

    const two = ruleParamsFromJson(
      {
        entry: {
          m_price_window: [
            { window_size_sec: 5, trail: [{ operator: '>=', value: 8 }] },
            { window_size_sec: 30, trail: [{ operator: '>=', value: 15 }] },
          ],
        },
      },
      REG,
    );
    const grp = (ruleParamsToJson(two).entry as Record<string, unknown>).m_price_window;
    expect(Array.isArray(grp)).toBe(true);
    expect((grp as unknown[]).length).toBe(2);
  });

  it('loads the two-window array form without dropping conditions (the silent-drop bug)', () => {
    const json = {
      entry: {
        m_price_window: [
          { window_size_sec: 5, trail: [{ operator: '>=', value: 8 }] },
          { window_size_sec: 30, trail: [{ operator: '>=', value: 15 }] },
        ],
      },
    };
    const form = ruleParamsFromJson(json, REG);
    // Both instances survive — the old Record<group,…> model collapsed these to nothing.
    expect(form.entry?.m_price_window).toHaveLength(2);
    const windows = sideInstances(form.entry)
      .map(([, g]) => g.strict.window_size_sec)
      .sort((a, b) => a - b);
    expect(windows).toEqual([5, 30]);
    // Full round-trip is identity.
    expect(ruleParamsJsonEqual(ruleParamsToJson(form), json)).toBe(true);
  });
});
