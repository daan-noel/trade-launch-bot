import { describe, expect, it } from 'vitest';
import type { MetricSpec, StrategyRegistry } from 'lib/strategy/registry';
import {
  axisRowError,
  axisSummary,
  axisTagNames,
  axesSpecToRows,
  comboCount,
  invalidValueFragments,
  newAxisRow,
  parseValueList,
  serializeAxisRows,
  storedAxisRows,
  type GenericAxisRow,
} from './genericAxes';

function metric(path: string, over: Partial<MetricSpec> = {}): MetricSpec {
  return {
    name: path.split('.')[1],
    path,
    phrase: path,
    unit: 'sol',
    summary: '',
    example: '',
    note: '',
    tags: 'none',
    tag_level: 'trade',
    spans: { life: true, window: false, since_age: false, slice: false },
    monotonic: false,
    eq_tolerance: 0.1,
    hue: 0,
    position: false,
    ...over,
  };
}

// Tiny registry: a life-only state metric, a windowed flow metric with an optional
// tag, a two-window share metric, a wallet-class holdings metric and a position metric.
const REG: StrategyRegistry = {
  operators: ['>', '>=', '<', '<=', '=', '!='],
  families: [
    { name: 'm_state', title: 'State', summary: '', example: '', metrics: [metric('m_state.age_sec', { unit: 'seconds' })] },
    {
      name: 'm_flow',
      title: 'Flow',
      summary: '',
      example: '',
      metrics: [
        metric('m_flow.buy_sol', { tags: 'optional', spans: { life: true, window: true, since_age: false, slice: false } }),
        metric('m_flow.slice_trade_share_pct', {
          unit: 'percent',
          spans: { life: false, window: true, since_age: false, slice: true },
        }),
      ],
    },
    {
      name: 'm_holdings',
      title: 'Holdings',
      summary: '',
      example: '',
      metrics: [metric('m_holdings.bag_share_pct', { unit: 'percent', tags: 'required', tag_level: 'wallet_class' })],
    },
    {
      name: 'm_position',
      title: 'Position',
      summary: '',
      example: '',
      metrics: [metric('m_position.retrace_pct', { unit: 'percent', position: true })],
    },
  ],
  spans: [],
  tags: { summary: '', example: '', fields: [], markers: [], builtin: [{ name: 'bundled', summary: '' }] },
  rule_parts: [],
};

function metricRow(over: Partial<GenericAxisRow>): GenericAxisRow {
  return { ...newAxisRow('metric', REG), valuesText: '5', ...over };
}

describe('parseValueList', () => {
  it('parses a comma list, deduped and ascending', () => {
    expect(parseValueList('100, 50, 200, 50')).toEqual([50, 100, 200]);
  });
  it('expands lo..hi step s inclusive and rounds float drift', () => {
    expect(parseValueList('10..40 step 10')).toEqual([10, 20, 30, 40]);
    expect(parseValueList('0..1 : 0.25')).toEqual([0, 0.25, 0.5, 0.75, 1]);
  });
  it('a range without a step is its two ends; a reversed range flips', () => {
    expect(parseValueList('1..5')).toEqual([1, 5]);
    expect(parseValueList('40..10 step 10')).toEqual([10, 20, 30, 40]);
  });
  it('off (any case) is one leading null', () => {
    expect(parseValueList('10, OFF, off, 20')).toEqual([null, 10, 20]);
  });
  it('names the fragments that parse to nothing', () => {
    expect(invalidValueFragments('5, of, 10..20 step 5, 1O, off, ')).toEqual(['of', '1O']);
  });
});

describe('axisRowError', () => {
  it('needs values, a metric and a valid read', () => {
    expect(axisRowError(metricRow({ ref: { metric: 'm_state.age_sec' }, valuesText: '' }), REG)).toBe('add at least one value');
    expect(axisRowError(metricRow({ ref: { metric: '' } }), REG)).toBe('pick a metric');
    expect(axisRowError(metricRow({ ref: { metric: 'm_bogus.x' } }), REG)).toMatch(/unknown metric/);
    expect(axisRowError(metricRow({ ref: { metric: 'm_state.age_sec' } }), REG)).toBeNull();
  });

  it('checks the span with the rule editor check', () => {
    expect(axisRowError(metricRow({ ref: { metric: 'm_state.age_sec', span: '10s' } }), REG)).toMatch(/takes no window/);
    expect(axisRowError(metricRow({ ref: { metric: 'm_flow.buy_sol', span: '30x' } }), REG)).toMatch(/expected/);
    expect(axisRowError(metricRow({ ref: { metric: 'm_flow.buy_sol', span: '20sl@1' } }), REG)).toBeNull();
  });

  it('a two-window metric needs its slice, nested in the span', () => {
    const share = (span: string, slice?: string) =>
      metricRow({ ref: { metric: 'm_flow.slice_trade_share_pct', span, ...(slice ? { slice } : {}) } });
    expect(axisRowError(share('60s'), REG)).toMatch(/needs a slice/);
    expect(axisRowError(share('10s', '30s'), REG)).toMatch(/wider than its span/);
    expect(axisRowError(share('60s', '3s'), REG)).toBeNull();
  });

  it('a tag must be defined by the run tags; a wallet class needs none', () => {
    const tagged = metricRow({ ref: { metric: 'm_flow.buy_sol', tag: '!volume', span: '10s' } });
    expect(axisRowError(tagged, REG)).toMatch(/define no `volume`/);
    expect(axisRowError(tagged, REG, ['volume'])).toBeNull();
    const bundled = metricRow({ ref: { metric: 'm_holdings.bag_share_pct', tag: 'bundled' } });
    expect(axisRowError(bundled, REG)).toBeNull();
  });

  it('a position metric is exit only', () => {
    const base = { ref: { metric: 'm_position.retrace_pct' } };
    expect(axisRowError(metricRow({ ...base, side: 'entry' }), REG)).toMatch(/exit side/);
    expect(axisRowError(metricRow({ ...base, side: 'exit' }), REG)).toBeNull();
  });

  it('off belongs to metric axes and needs a number beside it', () => {
    expect(axisRowError(metricRow({ ref: { metric: 'm_state.age_sec' }, valuesText: 'off, 5' }), REG)).toBeNull();
    expect(axisRowError(metricRow({ ref: { metric: 'm_state.age_sec' }, valuesText: 'off' }), REG)).toMatch(/besides 'off'/);
    expect(axisRowError({ ...newAxisRow('take_profit'), valuesText: 'off, 100' }, REG)).toMatch(/metric axes only/);
    expect(axisRowError({ ...newAxisRow('take_profit'), valuesText: '0, 100' }, REG)).toMatch(/above 0/);
    expect(axisRowError(metricRow({ ref: { metric: 'm_state.age_sec' }, valuesText: '5, of' }), REG)).toBe('unrecognized value: of');
  });
});

describe('serializeAxisRows / axesSpecToRows', () => {
  const rows: GenericAxisRow[] = [
    metricRow({ ref: { metric: 'm_state.age_sec' }, operator: '>', valuesText: '5, 10, 15' }),
    metricRow({ side: 'exit', ref: { metric: 'm_flow.buy_sol', tag: '!volume', span: '10s' }, operator: '<', valuesText: 'off, 1, 2.5' }),
    { ...newAxisRow('take_profit'), valuesText: '50, 100, 200' },
  ];

  it('writes the backend AxisSpec, keys only when set', () => {
    const wire = serializeAxisRows(rows);
    expect(wire[0]).toEqual({ kind: 'metric', side: 'entry', metric: 'm_state.age_sec', operator: '>', values: [5, 10, 15] });
    expect(wire[1]).toEqual({
      kind: 'metric',
      side: 'exit',
      metric: 'm_flow.buy_sol',
      tag: '!volume',
      span: '10s',
      operator: '<',
      values: [null, 1, 2.5],
    });
    expect(wire[2]).toEqual({ kind: 'take_profit', values: [50, 100, 200] });
  });

  it('round-trips, and a kind-less wire axis is a metric axis', () => {
    const back = axesSpecToRows({ axes: serializeAxisRows(rows) });
    expect(serializeAxisRows(back)).toEqual(serializeAxisRows(rows));
    const [seed] = axesSpecToRows([{ side: 'entry', metric: 'm_state.age_sec', operator: '<=', values: [null, 20] }]);
    expect(seed.kind).toBe('metric');
    expect(seed.valuesText).toBe('off, 20');
  });

  it('counts combos as the product of value counts', () => {
    expect(comboCount(rows)).toBe(3 * 3 * 3);
    expect(comboCount([metricRow({ valuesText: '' })])).toBe(0);
  });

  it('lists the fingerprint tags the axes read', () => {
    const withClass = [...rows, metricRow({ ref: { metric: 'm_holdings.bag_share_pct', tag: 'bundled' } })];
    expect(axisTagNames(withClass, REG)).toEqual(['volume']);
  });

  it('says what an axis does', () => {
    expect(axisSummary(rows[0])).toBe('entry filter: m_state.age_sec > each of 5, 10, 15');
    expect(axisSummary(rows[1])).toBe(
      'exit line, sells everything when: m_flow.buy_sol @!volume [10s] < each of 1, 2.5, or left out',
    );
  });
});

describe('storedAxisRows', () => {
  it('keeps v2 rows and refuses rows saved before the read existed', () => {
    const v2 = [metricRow({ ref: { metric: 'm_state.age_sec' } })];
    expect(storedAxisRows(v2)).toEqual(v2);
    expect(storedAxisRows([{ id: 'a', kind: 'metric', group: 'm_state', metric: 'time', valuesText: '5' }])).toBeNull();
    expect(storedAxisRows(undefined)).toBeNull();
  });
});
