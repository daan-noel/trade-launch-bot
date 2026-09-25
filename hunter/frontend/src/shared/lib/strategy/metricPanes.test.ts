import { describe, expect, it } from 'vitest';

import {
  findRuleFireMarkers,
  metricClockHorizons,
  metricConditionBands,
  metricConditionStatesAt,
  metricThresholdsFor,
  paneRuleFromJson,
  rulePaneKeys,
  ruleWindows,
  type MetricSeriesColumn,
  type MetricSeriesResponse,
  type PaneRule,
} from './metricPanes';
import type { MetricSpec, MetricUnit, StrategyRegistry } from './registry';

/**
 * The lab's metric-pane overlay over v2 rules: a pane is a read label, and the rule's
 * verdicts over a fetched series follow the engine's `can_enter` / `held_line`.
 */

function spec(path: string, unit: MetricUnit, eq_tolerance = 0): MetricSpec {
  const name = path.slice(path.indexOf('.') + 1);
  return {
    name,
    path,
    phrase: name,
    unit,
    summary: `${name} summary`,
    example: `${name} example`,
    note: '',
    tags: 'optional',
    tag_level: 'trade',
    spans: { life: true, window: true, since_age: false, slice: false },
    monotonic: false,
    eq_tolerance,
    hue: 200,
    position: path.startsWith('m_position.'),
  };
}

const SPECS = [
  spec('m_state.age_sec', 'seconds', 0.5),
  spec('m_state.liquidity_sol', 'sol'),
  spec('m_price.stall_sec', 'seconds', 0.5),
  spec('m_flow.buy_sol', 'sol'),
  spec('m_flow.slice_sol_share_pct', 'percent'),
  spec('m_crowd.unique_wallets', 'count'),
  spec('m_position.pnl_pct', 'percent'),
  spec('m_position.stage_sec', 'seconds'),
];

const REGISTRY: StrategyRegistry = {
  operators: ['>', '>=', '<', '<=', '=', '!='],
  families: [...new Set(SPECS.map((s) => s.path.split('.')[0]))].map((name) => ({
    name,
    title: name,
    summary: '',
    example: '',
    metrics: SPECS.filter((s) => s.path.startsWith(`${name}.`)),
  })),
  spans: [],
  tags: { summary: '', example: '', fields: [], markers: [], builtin: [] },
  rule_parts: [],
};

/** A one-atom condition. */
const c = (metric: string, operator: string, value: number, extra: Record<string, string> = {}) => ({
  metric,
  ...extra,
  is: [{ operator, value }],
});

function rule(params: unknown): PaneRule {
  const { rule: r, error } = paneRuleFromJson(params);
  if (!r) throw new Error(error ?? 'no rule');
  return r;
}

const at = (sec: number) => `2026-08-14T00:00:0${sec}Z`;
const t = (sec: number) => Date.parse(at(sec)) / 1000;

function col(label: string, values: Array<number | null>): MetricSeriesColumn {
  const metric = label.split(' ')[0];
  return { metric, family: metric.split('.')[0], unit: 'sol', label, values };
}

function data(series: MetricSeriesColumn[]): MetricSeriesResponse {
  const n = series[0].values.length;
  return {
    mint_address: 'MINT',
    at: Array.from({ length: n }, (_, i) => at(i)),
    price: Array.from({ length: n }, (_, i) => 1 + i / 10),
    series,
  };
}

/** A monotone life read crosses first; the two windowed reads land together at row 3;
 *  liquidity crosses at row 4. */
const RULE = rule({
  enter: {
    event: [
      c('m_flow.buy_sol', '>=', 5.5),
      c('m_flow.buy_sol', '>=', 0.85, { tag: 'volume', span: '2s' }),
      c('m_crowd.unique_wallets', '>=', 3, { span: '3s' }),
    ],
  },
  always: [{ if: [c('m_state.liquidity_sol', '>=', 40)], sell: true }],
});

const SERIES = [
  col('m_flow.buy_sol', [1, 6.1, 9.1, 9.1, 9.1]),
  col('m_flow.buy_sol @volume [2s]', [0, 0, 0.74, 0.95, 0.2]),
  col('m_crowd.unique_wallets [3s]', [1, 1, 2, 3, 1]),
  col('m_state.liquidity_sol', [30, 31, 32, 33, 41]),
];
const DATA = data(SERIES);

describe('the reads a rule makes', () => {
  it('lists each condition read once, in rule order, the shortcut reads with the exits', () => {
    const r = rule({
      enter: { event: [c('m_flow.buy_sol', '>=', 1, { span: '10s' }), c('m_flow.buy_sol', '>=', 2, { span: '10s' })] },
      take_profit: 50,
      always: [{ if: [c('m_state.liquidity_sol', '>=', 40)], sell: true }],
    });
    expect(rulePaneKeys(r)).toEqual(['m_flow.buy_sol [10s]', 'm_position.pnl_pct', 'm_state.liquidity_sol']);
  });

  it('asks for every window and every slice, whole', () => {
    const r = rule({
      enter: {
        event: [
          c('m_flow.buy_sol', '>=', 1, { span: '30sl@1' }),
          c('m_flow.slice_sol_share_pct', '>=', 50, { span: '30s@2', slice: '2s' }),
        ],
      },
    });
    expect(ruleWindows(r)).toEqual([
      { size: 2, lag: 2, unit: 'sec' },
      { size: 30, lag: 2, unit: 'sec' },
      { size: 30, lag: 1, unit: 'slot' },
    ]);
    expect(ruleWindows(rule({ enter: { event: [c('m_flow.buy_sol', '>=', 1)] } }))).toHaveLength(3);
  });

  it('declares the clock horizons with the = band and the age deadline', () => {
    const r = rule({
      enter: { filters: [c('m_state.age_sec', '<', 20), c('m_price.stall_sec', '>=', 60)] },
      stages: [{ name: 'a', ends: { age_sec: 30 } }],
    });
    expect(metricClockHorizons(r, REGISTRY)).toEqual({ timeHorizonSec: 30.5, stallHorizonSec: 60.5 });
  });

  it('names a stored format-1 rule instead of reading it', () => {
    expect(paneRuleFromJson({ entry: {} }).error).toMatch(/format-1/);
  });
});

describe('findRuleFireMarkers: entry', () => {
  it('marks the row the whole conjunction holds and names what flipped', () => {
    const entry = findRuleFireMarkers(RULE, DATA, REGISTRY).find((m) => m.kind === 'entry')!;
    expect(entry.time).toBe(at(3));
    expect(entry.role).toBe('signal');
    // The monotone life read held since row 1 and decided nothing about the timing.
    expect(entry.label).toBe('m_flow.buy_sol @volume [2s] >= 0.85 + m_crowd.unique_wallets [3s] >= 3');
  });

  it('waits out a sell line that already holds, then names the whole conjunction', () => {
    const vetoed = data([
      col('m_flow.buy_sol', [9, 9, 9, 9, 9]),
      col('m_flow.buy_sol @volume [2s]', [1, 1, 1, 1, 1]),
      col('m_crowd.unique_wallets [3s]', [3, 3, 3, 3, 3]),
      col('m_state.liquidity_sol', [41, 41, 30, 30, 30]),
    ]);
    const entry = findRuleFireMarkers(RULE, vetoed, REGISTRY).find((m) => m.kind === 'entry')!;
    expect(entry.time).toBe(at(2));
    expect(entry.label).toBe('m_flow.buy_sol >= 5.5 + m_flow.buy_sol @volume [2s] >= 0.85 +1');
  });

  it('never lets a line on our own position veto the buy', () => {
    const r = rule({ enter: { event: [c('m_flow.buy_sol', '>=', 5)] }, take_profit: 50 });
    const d = data([col('m_flow.buy_sol', [6, 6, 6]), col('m_position.pnl_pct', [60, 60, 60])]);
    const markers = findRuleFireMarkers(r, d, REGISTRY);
    expect(markers.map((m) => [m.kind, m.time, m.label])).toEqual([
      ['entry', at(0), 'm_flow.buy_sol >= 5'],
      ['exit', at(1), 'TakeProfit'],
    ]);
  });
});

describe('findRuleFireMarkers: held side', () => {
  it('labels the exit with the reason the engine books', () => {
    const exit = findRuleFireMarkers(RULE, DATA, REGISTRY).find((m) => m.kind === 'exit')!;
    expect(exit.time).toBe(at(4));
    expect(exit.label).toBe('m_state.liquidity_sol >= 40');
  });

  it('moves to the next stage at the deadline and reads the stage clock from there', () => {
    const r = rule({
      enter: { event: [c('m_flow.buy_sol', '>=', 5)] },
      stages: [
        { name: 'early', ends: { stage_sec: 2 }, on: [{ if: [c('m_state.liquidity_sol', '>=', 100)], sell: 'top' }] },
        { name: 'late', on: [{ if: [c('m_position.stage_sec', '>=', 1)], sell: 'late_out' }] },
      ],
    });
    const d = data([col('m_flow.buy_sol', [6, 6, 6, 6, 6, 6]), col('m_state.liquidity_sol', [1, 1, 1, 1, 1, 1])]);
    const exit = findRuleFireMarkers(r, d, REGISTRY).find((m) => m.kind === 'exit')!;
    expect(exit.time).toBe(at(3));
    expect(exit.label).toBe('late_out');
  });

  it('follows a go line to its stage', () => {
    const r = rule({
      enter: { event: [c('m_flow.buy_sol', '>=', 5)] },
      stages: [
        { name: 'early', on: [{ if: [c('m_flow.buy_sol', '>=', 9)], go: 'late' }] },
        { name: 'late', on: [{ if: [c('m_position.stage_sec', '>=', 1)], sell: 'late_out' }] },
      ],
    });
    const d = data([col('m_flow.buy_sol', [6, 6, 9, 9, 9, 9])]);
    const exit = findRuleFireMarkers(r, d, REGISTRY).find((m) => m.kind === 'exit')!;
    expect(exit.time).toBe(at(3));
  });
});

describe('metricConditionStatesAt / metricThresholdsFor', () => {
  const twins = rule({
    enter: { event: [c('m_flow.buy_sol', '>=', 5.5), c('m_flow.buy_sol', '>=', 0.9, { span: '2s' })] },
  });
  const d = data([col('m_flow.buy_sol', [9, 9, 9]), col('m_flow.buy_sol [2s]', [0, 0, 0])]);

  it('keys a life read apart from its windowed twin', () => {
    expect(metricConditionStatesAt(twins, 2, d, REGISTRY).map((s) => [s.key, s.ok])).toEqual([
      ['m_flow.buy_sol', true],
      ['m_flow.buy_sol [2s]', false],
    ]);
  });

  it('draws a threshold only on the read the rule authored it on', () => {
    expect(metricThresholdsFor(twins, 'm_flow.buy_sol [2s]')).toEqual([{ side: 'entry', value: 0.9 }]);
    expect(metricThresholdsFor(twins, 'm_flow.buy_sol [10s]')).toEqual([]);
  });
});

describe('metricConditionBands', () => {
  const BANDS = rule({
    enter: { event: [c('m_state.liquidity_sol', '>=', 20)] },
    always: [
      { if: [c('m_flow.buy_sol', '>=', 0.9, { span: '2s' })], sell: 'burst' },
      { if: [c('m_flow.buy_sol', '>=', 5)], sell: true },
    ],
  });
  const D = data([
    // Held, dropped out, then a gap: the gap must NOT bridge the two stretches.
    col('m_state.liquidity_sol', [25, 10, null, 30]),
    col('m_flow.buy_sol [2s]', [0, 0, 1.2, 1.4]),
    col('m_flow.buy_sol', [0, 0, 0, 0]),
  ]);

  it('fills one lane per condition, tagged by side, and keeps an empty one', () => {
    const b = metricConditionBands(BANDS, D, REGISTRY, null)!;
    expect(b.lanes.map((l) => l.label)).toEqual([
      'IN m_state.liquidity_sol >= 20',
      'OUT m_flow.buy_sol [2s] >= 0.9',
      'OUT m_flow.buy_sol >= 5',
    ]);
    expect(b.lanes[0].spans).toEqual([
      { from: t(0), to: t(0) },
      { from: t(3), to: t(3) },
    ]);
    expect(b.lanes[1].spans).toEqual([{ from: t(2), to: t(3) }]);
    expect(b.lanes[2].spans).toEqual([]);
    expect(b.coverage).toEqual({ from: t(0), to: t(3) });
  });

  it('draws the condition of the line that booked the exit reason', () => {
    expect(metricConditionBands(BANDS, D, REGISTRY, 'm_flow.buy_sol >= 5')!.valueLane?.label).toBe(
      'm_flow.buy_sol >= 5',
    );
    const burst = metricConditionBands(BANDS, D, REGISTRY, 'burst')!.valueLane!;
    expect(burst.label).toBe('m_flow.buy_sol [2s] >= 0.9');
    expect(burst.thresholds).toEqual([0.9]);
    expect(burst.points.at(-1)).toEqual({ timeSec: t(3), value: 1.4 });
    for (const reason of ['TakeProfit', null, undefined]) {
      expect(metricConditionBands(BANDS, D, REGISTRY, reason)!.valueLane?.label).toBe('m_flow.buy_sol [2s] >= 0.9');
    }
  });

  it('draws both edges of one arm, and nothing for several arms', () => {
    const band = rule({ always: [{ if: [{ metric: 'm_flow.buy_sol', span: '2s', is: [{ operator: '>', value: 0.2 }, { operator: '<', value: 0.9 }] }], sell: true }] });
    expect(metricConditionBands(band, D, REGISTRY, null)!.valueLane?.thresholds).toEqual([0.2, 0.9]);
    const ored = rule({ always: [{ if: [{ metric: 'm_flow.buy_sol', span: '2s', is: [[{ operator: '>=', value: 0.9 }], [{ operator: '<', value: 0.1 }]] }], sell: true }] });
    expect(metricConditionBands(ored, D, REGISTRY, null)!.valueLane?.thresholds).toEqual([]);
  });

  it('has nothing to draw without rows or without conditions', () => {
    expect(metricConditionBands(BANDS, { ...D, at: [] }, REGISTRY, null)).toBeNull();
    expect(metricConditionBands(rule({}), D, REGISTRY, null)).toBeNull();
  });
});
