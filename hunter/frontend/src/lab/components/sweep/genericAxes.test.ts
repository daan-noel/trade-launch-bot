import { describe, expect, it } from 'vitest';
import type { StrategyRegistry } from 'lib/strategy/registry';
import {
  axisRowError,
  comboCount,
  newAxisRow,
  parseValueList,
  serializeAxisRows,
  sharedWindowError,
  type GenericAxisRow,
} from './genericAxes';

// A tiny registry mirroring the real one (m_snapshot static, m_time_window dynamic).
const REG: StrategyRegistry = {
  operators: ['>', '>=', '<', '<=', '=', '!='],
  groups: [
    {
      name: 'm_snapshot',
      kind: 'static',
      strict_params: [],
      metrics: [
        { name: 'time', unit: 'seconds', eq_tolerance: 0.5, monotonic: true },
        { name: 'liquidity', unit: 'sol', eq_tolerance: 0.1, monotonic: false },
      ],
    },
    {
      name: 'm_time_window',
      kind: 'dynamic',
      strict_params: [{ name: 'window_size_sec', required: true }],
      metrics: [{ name: 'net_flow', unit: 'sol', eq_tolerance: 0.1, monotonic: false }],
    },
  ],
};

function metricRow(over: Partial<GenericAxisRow>): GenericAxisRow {
  return { ...newAxisRow('metric', REG), ...over };
}

describe('parseValueList', () => {
  it('parses a comma list, deduped + ascending', () => {
    expect(parseValueList('100, 50, 200, 50')).toEqual([50, 100, 200]);
  });
  it('expands lo..hi step s inclusive', () => {
    expect(parseValueList('10..40 step 10')).toEqual([10, 20, 30, 40]);
  });
  it('accepts colon step and rounds float drift', () => {
    expect(parseValueList('0..1 step 0.25')).toEqual([0, 0.25, 0.5, 0.75, 1]);
  });
  it('range without step yields the two endpoints', () => {
    expect(parseValueList('1..5')).toEqual([1, 5]);
  });
  it('mixes list and range and drops blanks/NaN', () => {
    expect(parseValueList('5, 10..20 step 5, x, , 100')).toEqual([5, 10, 15, 20, 100]);
  });
  it('flips a reversed range', () => {
    expect(parseValueList('40..10 step 10')).toEqual([10, 20, 30, 40]);
  });
});

describe('axisRowError', () => {
  it('flags an empty value list', () => {
    expect(axisRowError(metricRow({ group: 'm_snapshot', metric: 'time', valuesText: '' }), REG)).toBe(
      'add at least one value',
    );
  });
  it('flags a missing group/metric/operator', () => {
    expect(axisRowError(metricRow({ valuesText: '5' }), REG)).toBe('pick a metric group');
    expect(axisRowError(metricRow({ group: 'm_snapshot', valuesText: '5' }), REG)).toBe('pick a metric');
  });
  it('requires a window on a dynamic group', () => {
    const row = metricRow({ group: 'm_time_window', metric: 'net_flow', window: '', valuesText: '1' });
    expect(axisRowError(row, REG)).toBe('window (s) > 0 required');
    expect(axisRowError({ ...row, window: '10' }, REG)).toBeNull();
  });
  it('accepts a valid static metric row', () => {
    expect(
      axisRowError(metricRow({ group: 'm_snapshot', metric: 'time', operator: '>', valuesText: '5, 10' }), REG),
    ).toBeNull();
  });
  it('rejects TP/SL values <= 0', () => {
    expect(axisRowError({ ...newAxisRow('take_profit'), valuesText: '0, 100' }, REG)).toBe(
      'TP / SL values must be > 0',
    );
    expect(axisRowError({ ...newAxisRow('take_profit'), valuesText: '50, 100' }, REG)).toBeNull();
  });
});

describe('serializeAxisRows + comboCount', () => {
  const rows: GenericAxisRow[] = [
    metricRow({ group: 'm_snapshot', metric: 'time', operator: '>', valuesText: '5, 10, 15' }),
    metricRow({ side: 'entry', group: 'm_time_window', metric: 'net_flow', operator: '>', window: '10', valuesText: '0, 2.5' }),
    { ...newAxisRow('take_profit'), valuesText: '50, 100, 200' },
  ];

  it('drops the window on static metrics and keeps it on dynamic', () => {
    const specs = serializeAxisRows(rows, REG);
    expect(specs[0]).toMatchObject({ kind: 'metric', group: 'm_snapshot', metric: 'time', values: [5, 10, 15] });
    expect(specs[0].window).toBeUndefined();
    expect(specs[1]).toMatchObject({ group: 'm_time_window', window: 10, values: [0, 2.5] });
    expect(specs[2]).toEqual({ kind: 'take_profit', values: [50, 100, 200] });
  });

  it('combo count is the product of value counts', () => {
    expect(comboCount(rows, REG)).toBe(3 * 2 * 3);
  });

  it('combo count is 0 when an axis has no values', () => {
    expect(comboCount([metricRow({ group: 'm_snapshot', metric: 'time', valuesText: '' })], REG)).toBe(0);
  });
});

describe('sharedWindowError', () => {
  it('rejects conflicting windows on the same side', () => {
    const rows: GenericAxisRow[] = [
      metricRow({ side: 'entry', group: 'm_time_window', metric: 'net_flow', window: '10', valuesText: '1' }),
      metricRow({ side: 'entry', group: 'm_time_window', metric: 'net_flow', window: '20', valuesText: '1' }),
    ];
    expect(sharedWindowError(rows, REG)).toMatch(/conflicting entry time-window/);
  });
  it('allows the same window twice / different sides', () => {
    const rows: GenericAxisRow[] = [
      metricRow({ side: 'entry', group: 'm_time_window', metric: 'net_flow', window: '10', valuesText: '1' }),
      metricRow({ side: 'exit', group: 'm_time_window', metric: 'net_flow', window: '20', valuesText: '1' }),
    ];
    expect(sharedWindowError(rows, REG)).toBeNull();
  });
});
