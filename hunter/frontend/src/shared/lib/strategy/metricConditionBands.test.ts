import { describe, expect, it } from 'vitest';

import { metricConditionBands } from './metricPanes';
import type { StrategyRegistry } from './registry';
import type { RuleParams } from './ruleParams';
import type { MetricSeriesResponse } from './types';

/**
 * The lab's chart timeline: one lane per authored condition, filled over the
 * stretches it held. The traps are the same ones the live twin guards — an
 * unreadable row must break a span rather than extend it, a condition that never
 * crosses must keep its (empty) lane, and the value line must land on the condition
 * the exit reason NAMES, not on its identically-named lifetime twin.
 */

const REGISTRY: StrategyRegistry = {
  operators: ['>', '>=', '<', '<=', '=', '!='],
  groups: [
    {
      name: 'm_flow_split_window',
      kind: 'dynamic',
      strict_params: [],
      metrics: [{ name: 'nonvol_buy', unit: 'sol', eq_tolerance: 0, monotonic: false, hue: 200 }],
    },
    {
      name: 'm_flow_split_lifetime',
      kind: 'static',
      strict_params: [],
      metrics: [{ name: 'nonvol_buy', unit: 'sol', eq_tolerance: 0, monotonic: true, hue: 200 }],
    },
    {
      name: 'm_snapshot',
      kind: 'static',
      strict_params: [],
      metrics: [{ name: 'liquidity', unit: 'sol', eq_tolerance: 0, monotonic: false, hue: 90 }],
    },
  ],
};

/** `liquidity >= 20` to enter; either `nonvol_buy` — windowed or lifetime — to leave. */
const PARAMS: RuleParams = {
  entry: { m_snapshot: [{ strict: {}, metrics: { liquidity: [[{ operator: '>=', value: 20 }]] } }] },
  exit: {
    m_flow_split_window: [
      { strict: { window_size_sec: 2 }, metrics: { nonvol_buy: [[{ operator: '>=', value: 0.9 }]] } },
    ],
    m_flow_split_lifetime: [
      { strict: {}, metrics: { nonvol_buy: [[{ operator: '>=', value: 5 }]] } },
    ],
  },
} as unknown as RuleParams;

const DATA: MetricSeriesResponse = {
  mint_address: 'MINT',
  at: [
    '2026-08-14T00:00:00Z',
    '2026-08-14T00:00:01Z',
    '2026-08-14T00:00:02Z',
    '2026-08-14T00:00:03Z',
  ],
  series: [
    {
      metric: 'liquidity',
      group: 'm_snapshot',
      unit: 'sol',
      window_size_sec: null,
      // Held, dropped out, then a gap — the gap must NOT bridge the two stretches.
      values: [25, 10, null, 30],
    },
    {
      metric: 'nonvol_buy',
      group: 'm_flow_split_window',
      unit: 'sol',
      window_size_sec: 2,
      values: [0, 0, 1.2, 1.4],
    },
    {
      metric: 'nonvol_buy',
      group: 'm_flow_split_lifetime',
      unit: 'sol',
      window_size_sec: null,
      values: [0, 0, 0, 0],
    },
  ],
};

const t = (sec: number) => Date.parse(`2026-08-14T00:00:0${sec}Z`) / 1000;

describe('metricConditionBands', () => {
  it('fills one lane per authored condition, tagged by side', () => {
    const bands = metricConditionBands(PARAMS, DATA, REGISTRY, null)!;
    expect(bands.lanes.map((l) => l.label)).toEqual([
      'IN liquidity >= 20',
      'OUT nonvol_buy@2s >= 0.9',
      'OUT nonvol_buy >= 5',
    ]);
    expect(bands.coverage).toEqual({ from: t(0), to: t(3) });
  });

  it('breaks a span on an unreadable row instead of bridging it', () => {
    const bands = metricConditionBands(PARAMS, DATA, REGISTRY, null)!;
    expect(bands.lanes[0].spans).toEqual([
      { from: t(0), to: t(0) },
      { from: t(3), to: t(3) },
    ]);
  });

  it('keeps an empty lane for a condition that never crossed', () => {
    const bands = metricConditionBands(PARAMS, DATA, REGISTRY, null)!;
    expect(bands.lanes[2].spans).toEqual([]);
    expect(bands.lanes[1].spans).toEqual([{ from: t(2), to: t(3) }]);
  });

  it('draws the condition the exit reason names, with its threshold', () => {
    // The window qualifier is the whole point: both exits are `nonvol_buy`, and a
    // name-only match is free to draw the lifetime twin of a windowed fire.
    const windowed = metricConditionBands(PARAMS, DATA, REGISTRY, 'nonvol_buy(2s) >= 0.9')!;
    expect(windowed.valueLane?.label).toBe('nonvol_buy@2s >= 0.9');
    expect(windowed.valueLane?.thresholds).toEqual([0.9]);
    expect(windowed.valueLane?.points.at(-1)).toEqual({ timeSec: t(3), value: 1.4 });
  });

  it('resolves a bare (unqualified) reason only when the rule leaves it unambiguous', () => {
    // Two `nonvol_buy` exits and no window qualifier: nothing in the label picks
    // one, so it falls back rather than guessing — same rule as the live strip.
    const ambiguous = metricConditionBands(PARAMS, DATA, REGISTRY, 'nonvol_buy >= 5')!;
    expect(ambiguous.valueLane?.label).toBe('nonvol_buy@2s >= 0.9');

    const oneExit = {
      ...PARAMS,
      exit: { m_flow_split_lifetime: PARAMS.exit!.m_flow_split_lifetime },
    } as RuleParams;
    const resolved = metricConditionBands(oneExit, DATA, REGISTRY, 'nonvol_buy >= 5')!;
    expect(resolved.valueLane?.label).toBe('nonvol_buy >= 5');
    expect(resolved.valueLane?.thresholds).toEqual([5]);
  });

  // A two-sided condition is the common shape of an entry band, and drawing only one
  // of its edges (or, as before, neither) leaves no way to see where it stopped
  // holding.
  it('draws BOTH edges of a two-sided condition', () => {
    const banded = {
      ...PARAMS,
      exit: {
        m_flow_split_window: [
          {
            strict: { window_size_sec: 2 },
            metrics: { nonvol_buy: [[{ operator: '>', value: 0.2 }, { operator: '<', value: 0.9 }]] },
          },
        ],
      },
    } as unknown as RuleParams;
    const bands = metricConditionBands(banded, DATA, REGISTRY, null)!;
    expect(bands.valueLane?.thresholds).toEqual([0.2, 0.9]);
  });

  // Two OR arms disagree about where the line sits, so there is no honest set.
  it('draws no line when the condition ORs several arms', () => {
    const ored = {
      ...PARAMS,
      exit: {
        m_flow_split_window: [
          {
            strict: { window_size_sec: 2 },
            metrics: { nonvol_buy: [[{ operator: '>=', value: 0.9 }], [{ operator: '<', value: 0.1 }]] },
          },
        ],
      },
    } as unknown as RuleParams;
    const bands = metricConditionBands(ored, DATA, REGISTRY, null)!;
    expect(bands.valueLane?.thresholds).toEqual([]);
  });

  it('falls back to the first exit condition for a non-metric exit reason', () => {
    for (const reason of ['TakeProfit', null, undefined]) {
      const bands = metricConditionBands(PARAMS, DATA, REGISTRY, reason)!;
      expect(bands.valueLane?.label, `reason ${reason}`).toBe('nonvol_buy@2s >= 0.9');
    }
  });

  it('has nothing to draw without rows or without conditions', () => {
    expect(metricConditionBands(PARAMS, { ...DATA, at: [] }, REGISTRY, null)).toBeNull();
    expect(metricConditionBands({} as RuleParams, DATA, REGISTRY, null)).toBeNull();
  });
});
