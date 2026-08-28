import { describe, expect, it } from 'vitest';

import {
  findRuleFireMarkers,
  metricConditionStatesAt,
  metricThresholdsFor,
} from './metricPanes';
import type { StrategyRegistry } from './registry';
import type { RuleParams } from './ruleParams';
import type { MetricSeriesResponse } from './types';

/**
 * The chart's metric-fire markers. Entry is a CONJUNCTION, so the trap is a
 * condition that has been holding for minutes taking credit for the instant two
 * trailing windows crossed — the more so when it is a monotone lifetime metric that
 * shares its name with a windowed twin.
 *
 * Shaped after `rule-search · champion 2`: `untagged_buy` (lifetime, monotone) crosses
 * 5.5 long before `tagged_buy` at 2 s and `unique_wallets` at 3 s land together.
 */

const REGISTRY: StrategyRegistry = {
  operators: ['>', '>=', '<', '<=', '=', '!='],
  groups: [
    {
      name: 'm_flow_ix',
      kind: 'static',
      strict_params: [],
      metrics: [{ name: 'untagged_buy', unit: 'sol', eq_tolerance: 0, monotonic: true, hue: 200 }],
    },
    {
      name: 'm_flow_ix_window',
      kind: 'dynamic',
      strict_params: [],
      metrics: [
        { name: 'tagged_buy', unit: 'sol', eq_tolerance: 0, monotonic: false, hue: 210 },
        { name: 'untagged_buy', unit: 'sol', eq_tolerance: 0, monotonic: false, hue: 200 },
      ],
    },
    {
      name: 'm_flow_window',
      kind: 'dynamic',
      strict_params: [],
      metrics: [
        { name: 'unique_wallets', unit: 'count', eq_tolerance: 0, monotonic: false, hue: 40 },
      ],
    },
    {
      name: 'm_state',
      kind: 'static',
      strict_params: [],
      metrics: [{ name: 'liquidity', unit: 'sol', eq_tolerance: 0, monotonic: false, hue: 90 }],
    },
  ],
};

const PARAMS: RuleParams = {
  entry: {
    m_flow_ix: [{ strict: {}, metrics: { untagged_buy: [[{ operator: '>=', value: 5.5 }]] } }],
    m_flow_ix_window: [
      { strict: { window_size_sec: 2 }, metrics: { tagged_buy: [[{ operator: '>=', value: 0.85 }]] } },
    ],
    m_flow_window: [
      {
        strict: { window_size_sec: 3 },
        metrics: { unique_wallets: [[{ operator: '>=', value: 3 }]] },
      },
    ],
  },
  exit: {
    m_state: [{ strict: {}, metrics: { liquidity: [[{ operator: '>=', value: 40 }]] } }],
  },
} as unknown as RuleParams;

const at = (sec: number) => `2026-08-14T00:00:0${sec}Z`;

/** Row 1: the lifetime metric crosses alone. Row 3: both windows land ⇒ entry.
 *  Row 4: liquidity crosses ⇒ exit. */
const DATA: MetricSeriesResponse = {
  mint_address: 'MINT',
  at: [at(0), at(1), at(2), at(3), at(4)],
  price: [1, 1.1, 1.2, 1.3, 1.4],
  series: [
    {
      metric: 'untagged_buy',
      group: 'm_flow_ix',
      unit: 'sol',
      window_size_sec: null,
      values: [1.0, 6.1, 9.1, 9.1, 9.1],
    },
    {
      metric: 'tagged_buy',
      group: 'm_flow_ix_window',
      unit: 'sol',
      window_size_sec: 2,
      values: [0, 0, 0.74, 0.95, 0.2],
    },
    {
      metric: 'unique_wallets',
      group: 'm_flow_window',
      unit: 'count',
      window_size_sec: 3,
      values: [1, 1, 2, 3, 1],
    },
    {
      metric: 'liquidity',
      group: 'm_state',
      unit: 'sol',
      window_size_sec: null,
      values: [30, 31, 32, 33, 41],
    },
  ],
} as unknown as MetricSeriesResponse;

describe('findRuleFireMarkers — entry', () => {
  it('marks the instant the WHOLE conjunction holds, not the first condition to cross', () => {
    const entry = findRuleFireMarkers(PARAMS, DATA, REGISTRY).find((m) => m.kind === 'entry')!;
    expect(entry.time).toBe(at(3));
    expect(entry.role).toBe('signal');
  });

  it('labels the conditions that FLIPPED, never the one already holding', () => {
    const entry = findRuleFireMarkers(PARAMS, DATA, REGISTRY).find((m) => m.kind === 'entry')!;
    // `untagged_buy >= 5.5` was true from row 1 and is monotone — it decided nothing
    // about the timing, and naming it sends the reader to a line that crossed two
    // rows earlier.
    expect(entry.label).not.toContain('untagged_buy');
    expect(entry.label).toBe('tagged_buy@2s >= 0.85 + unique_wallets@3s >= 3');
  });

  it('qualifies a windowed metric so it cannot read as its lifetime twin', () => {
    const twins = {
      ...PARAMS,
      entry: {
        m_flow_ix: PARAMS.entry!.m_flow_ix,
        m_flow_ix_window: [
          {
            strict: { window_size_sec: 2 },
            metrics: { untagged_buy: [[{ operator: '>=', value: 0.9 }]] },
          },
        ],
      },
    } as unknown as RuleParams;
    const data = {
      ...DATA,
      series: [
        DATA.series[0],
        {
          metric: 'untagged_buy',
          group: 'm_flow_ix_window',
          unit: 'sol',
          window_size_sec: 2,
          values: [0, 0, 0, 1.2, 1.2],
        },
        DATA.series[3],
      ],
    } as unknown as MetricSeriesResponse;
    const entry = findRuleFireMarkers(twins, data, REGISTRY).find((m) => m.kind === 'entry')!;
    expect(entry.label).toBe('untagged_buy@2s >= 0.9');
  });

  it('names the whole conjunction when the exit veto — not a condition — cleared', () => {
    // Entry conditions all hold from row 0; `liquidity >= 40` vetoes until row 2.
    // Nothing on the entry side flipped, so there is no binding condition to name.
    const vetoed = {
      ...DATA,
      series: [
        { ...DATA.series[0], values: [9.1, 9.1, 9.1, 9.1, 9.1] },
        { ...DATA.series[1], values: [0.95, 0.95, 0.95, 0.95, 0.95] },
        { ...DATA.series[2], values: [3, 3, 3, 3, 3] },
        { ...DATA.series[3], values: [41, 41, 30, 30, 30] },
      ],
    } as unknown as MetricSeriesResponse;
    const entry = findRuleFireMarkers(PARAMS, vetoed, REGISTRY).find((m) => m.kind === 'entry')!;
    expect(entry.time).toBe(at(2));
    expect(entry.label).toBe('untagged_buy >= 5.5 + tagged_buy@2s >= 0.85 +1');
  });

  it('refuses entry while an exit metric already holds', () => {
    const always = {
      ...DATA,
      series: [...DATA.series.slice(0, 3), { ...DATA.series[3], values: [41, 41, 41, 41, 41] }],
    } as unknown as MetricSeriesResponse;
    expect(findRuleFireMarkers(PARAMS, always, REGISTRY).some((m) => m.kind === 'entry')).toBe(
      false,
    );
  });
});

describe('findRuleFireMarkers — exit', () => {
  it('names the satisfied exit metric, window-qualified, after the entry', () => {
    const exit = findRuleFireMarkers(PARAMS, DATA, REGISTRY).find((m) => m.kind === 'exit')!;
    expect(exit.time).toBe(at(4));
    expect(exit.label).toBe('liquidity >= 40');
  });
});

describe('metricConditionStatesAt', () => {
  it('keys a lifetime metric apart from its windowed twin', () => {
    const twins = {
      entry: {
        m_flow_ix: PARAMS.entry!.m_flow_ix,
        m_flow_ix_window: [
          {
            strict: { window_size_sec: 2 },
            metrics: { untagged_buy: [[{ operator: '>=', value: 0.9 }]] },
          },
        ],
      },
    } as unknown as RuleParams;
    const data = {
      ...DATA,
      series: [
        DATA.series[0],
        {
          metric: 'untagged_buy',
          group: 'm_flow_ix_window',
          unit: 'sol',
          window_size_sec: 2,
          values: [0, 0, 0, 0, 0],
        },
      ],
    } as unknown as MetricSeriesResponse;
    // Row 2: lifetime 9.1 (satisfied), windowed 0 (not). One key each, one verdict
    // each — collapsing them onto `untagged_buy` paints the windowed pane green.
    const states = metricConditionStatesAt(twins, 2, data, REGISTRY);
    expect(states.map((s) => [s.key, s.ok])).toEqual([
      ['untagged_buy', true],
      ['untagged_buy@2', false],
    ]);
  });
});

describe('metricThresholdsFor', () => {
  it('scopes a threshold to the column the rule authored it on', () => {
    expect(metricThresholdsFor(PARAMS, 'tagged_buy', 2, REGISTRY)).toEqual([
      { side: 'entry', value: 0.85 },
    ]);
    // The same metric at a window the rule never authored draws nothing.
    expect(metricThresholdsFor(PARAMS, 'tagged_buy', 10, REGISTRY)).toEqual([]);
    // A lifetime condition never leaks onto a windowed pane.
    expect(metricThresholdsFor(PARAMS, 'untagged_buy', null, REGISTRY)).toEqual([
      { side: 'entry', value: 5.5 },
    ]);
    expect(metricThresholdsFor(PARAMS, 'untagged_buy', 2, REGISTRY)).toEqual([]);
  });
});
