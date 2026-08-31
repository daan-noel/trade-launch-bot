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
      entry: { m_state: { time: [{ operator: '>', value: 10 }] } },
    };
    const b = {
      take_profit: 50,
      entry: { m_state: { time: [{ operator: '>', value: 10 }] } },
    };
    const c = {
      take_profit: 50,
      entry: { m_state: { time: [{ operator: '>', value: 20 }] } },
    };
    expect(ruleParamsJsonEqual(a, b)).toBe(true);
    expect(ruleParamsJsonEqual(a, c)).toBe(false);
  });

  // The "best" badge on the sweep's Used-by column compares a saved rule's params
  // to the group's `best_params`. An editor round-trip re-emits a group's window
  // instances sorted by window, so a positional array compare dropped the badge on
  // every multi-window winner even though the rule was that exact combo.
  it('ignores the order of a group\'s window instances', () => {
    const swept = {
      exit: {
        m_flow_window: [
          { buy: [{ operator: '<', value: 3 }], window_size_sec: 25 },
          { sell: [{ operator: '>=', value: 20 }], window_size_sec: 5 },
        ],
      },
    };
    const promoted = {
      exit: {
        m_flow_window: [
          { sell: [{ operator: '>=', value: 20 }], window_size_sec: 5 },
          { buy: [{ operator: '<', value: 3 }], window_size_sec: 25 },
        ],
      },
    };
    expect(ruleParamsJsonEqual(swept, promoted)).toBe(true);
  });

  it('ignores the order of AND atoms within one metric', () => {
    const a = { entry: { m_state: { time: [{ operator: '>', value: 10 }, { operator: '<', value: 45 }] } } };
    const b = { entry: { m_state: { time: [{ operator: '<', value: 45 }, { operator: '>', value: 10 }] } } };
    expect(ruleParamsJsonEqual(a, b)).toBe(true);
  });

  it('KEEPS DNF exit clause order — first fully-true clause labels the exit', () => {
    const a = {
      exit: [
        { m_state: { time: [{ operator: '>', value: 10 }] } },
        { m_state: { liquidity: [{ operator: '<', value: 20 }] } },
      ],
    };
    const b = {
      exit: [
        { m_state: { liquidity: [{ operator: '<', value: 20 }] } },
        { m_state: { time: [{ operator: '>', value: 10 }] } },
      ],
    };
    expect(ruleParamsJsonEqual(a, b)).toBe(false);
  });

  it('KEEPS scale_out positional — the ladder executes in authored order', () => {
    const a = { scale_out: [{ sell_bps: 7000, take_profit: 50 }, { sell_bps: null, take_profit: 10 }] };
    const b = { scale_out: [{ sell_bps: null, take_profit: 10 }, { sell_bps: 7000, take_profit: 50 }] };
    expect(ruleParamsJsonEqual(a, b)).toBe(false);
  });

  it('still normalizes a scale_out stage\'s own conditions order-free', () => {
    const stage = (windows: number[]) => ({
      scale_out: [
        {
          sell_bps: 5000,
          conditions: {
            m_flow_window: windows.map((w) => ({
              buy: [{ operator: '<', value: 3 }],
              window_size_sec: w,
            })),
          },
        },
      ],
    });
    expect(ruleParamsJsonEqual(stage([25, 50]), stage([50, 25]))).toBe(true);
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
    // fromJson → toJson must preserve reentry — a round-trip that drops it is
    // the regression this pins.
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

describe('disabled (parked conditions) round-trip', () => {
  // A parked gate on the SAME group/window/metric as a live one — the shape the
  // toggle exists to produce, and the reason the two live in separate bags.
  const json = {
    entry: { m_price_window: { window_size_sec: 30, trail: [{ operator: '>=', value: 20 }] } },
    disabled: {
      entry: { m_price_window: { window_size_sec: 30, trail: [{ operator: '>=', value: 12 }] } },
    },
  };

  it('carries the parked bag through fromJson → toJson unchanged', () => {
    const form = ruleParamsFromJson(json, REG);
    expect(form.entry?.m_price_window[0].metrics.trail[0][0].value).toBe(20);
    expect(form.disabled?.entry?.m_price_window[0].metrics.trail[0][0].value).toBe(12);
    expect(ruleParamsJsonEqual(ruleParamsToJson(form), json)).toBe(true);
  });

  it('omits the key entirely when nothing is parked (no migration)', () => {
    expect('disabled' in ruleParamsToJson(emptyRuleParams())).toBe(false);
    const live = ruleParamsFromJson({ entry: json.entry }, REG);
    expect(live.disabled).toBeNull();
    expect('disabled' in ruleParamsToJson(live)).toBe(false);
  });

  it('folds an empty / all-empty bag to null (the backend sentinel)', () => {
    for (const empty of [
      { disabled: {} },
      { disabled: { entry: {}, exit: {} } },
      { disabled: { scale_out: [] } },
    ]) {
      const form = ruleParamsFromJson(empty, REG);
      expect(form.disabled).toBeNull();
      expect('disabled' in ruleParamsToJson(form)).toBe(false);
    }
  });

  it('round-trips parked scale-out stages beside the live ladder', () => {
    // Parked stages that could never coexist in ONE ladder (a second remainder, a
    // 99% tranche on top of a live 70%) — the bag is a shelf, not a ladder.
    const withStages = {
      scale_out: [{ sell_bps: 7000, take_profit: 50 }],
      disabled: {
        scale_out: [{ sell_bps: 9900, take_profit: 40 }, { take_profit: 120 }],
      },
    };
    const form = ruleParamsFromJson(withStages, REG);
    expect(form.scale_out).toHaveLength(1);
    expect(form.disabled?.scale_out).toHaveLength(2);
    expect(form.disabled?.scale_out?.[1].take_profit).toBe(120);
    expect(ruleParamsJsonEqual(ruleParamsToJson(form), withStages)).toBe(true);
  });

  it('keeps a parked bag alive when ONLY stages are parked', () => {
    const form = ruleParamsFromJson({ disabled: { scale_out: [{ take_profit: 40 }] } }, REG);
    expect(form.disabled?.scale_out).toHaveLength(1);
    expect(ruleParamsToJson(form)).toEqual({ disabled: { scale_out: [{ take_profit: 40 }] } });
  });
});

describe('array-form DNF exit', () => {
  const REG2: StrategyRegistry = {
    operators: ['>', '>=', '<', '<=', '=', '!='],
    groups: [
      {
        name: 'm_price_window',
        kind: 'dynamic',
        strict_params: [{ name: 'window_size_sec', required: true }],
        metrics: [{ name: 'trail', unit: 'percent', eq_tolerance: 0.1, monotonic: false, hue: 45 }],
      },
      {
        name: 'm_state',
        kind: 'static',
        strict_params: [],
        metrics: [{ name: 'time', unit: 'seconds', eq_tolerance: 0.5, monotonic: true, hue: 200 }],
      },
    ],
  };

  it('round-trips a two-metric way as an array', () => {
    const raw = {
      exit: [
        {
          m_state: { time: [{ operator: '>', value: 10 }] },
          m_price_window: { window_size_sec: 30, trail: [{ operator: '>=', value: 12 }] },
        },
      ],
    };
    const form = ruleParamsFromJson(raw, REG2);
    expect(form.exitClauses).toHaveLength(1);
    expect(Object.keys(form.exit ?? {})).toHaveLength(0);
    const json = ruleParamsToJson(form);
    expect(Array.isArray(json.exit)).toBe(true);
    expect(ruleParamsJsonEqual(json, raw)).toBe(true);
  });

  it('parses parked array-form exit instead of dropping it', () => {
    const raw = {
      disabled: {
        exit: [{ m_state: { time: [{ operator: '>', value: 5 }] } }],
      },
    };
    const form = ruleParamsFromJson(raw, REG2);
    expect(form.disabled?.exitClauses).toHaveLength(1);
    expect(ruleParamsToJson(form).disabled).toEqual(raw.disabled);
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

describe('scale_out round-trip', () => {
  it('carries stages through toJson → fromJson unchanged', () => {
    const json = {
      scale_out: [
        { sell_bps: 7000, take_profit: 50 },
        {
          sell_bps: 2000,
          conditions: {
            m_price_window: {
              window_size_sec: 10,
              trail: [{ operator: '>=', value: 8 }],
            },
          },
        },
        { take_profit: 10 },
      ],
    };
    const form = ruleParamsFromJson(json, REG);
    expect(form.scale_out).toHaveLength(3);
    expect(form.scale_out![0]).toMatchObject({ sell_bps: 7000, take_profit: 50 });
    expect(form.scale_out![1].sell_bps).toBe(2000);
    expect(form.scale_out![2].sell_bps).toBeNull();
    expect(form.scale_out![2].take_profit).toBe(10);
    expect(ruleParamsJsonEqual(ruleParamsToJson(form), json)).toBe(true);
  });

  it('folds empty scale_out to null (configured_labels sentinel)', () => {
    expect(ruleParamsFromJson({ scale_out: [] }, undefined).scale_out).toBeNull();
    expect('scale_out' in ruleParamsToJson(emptyRuleParams())).toBe(false);
  });
});
