import { describe, expect, it } from 'vitest';
import type { StrategyRegistry } from 'lib/strategy/registry';
import {
  axisRowError,
  comboCount,
  invalidValueFragments,
  newAxisRow,
  parseValueList,
  pnlAxisSugarDuplicateError,
  serializeAxisRows,
  type GenericAxisRow,
} from './genericAxes';

// Tiny registry: static snapshot + lifetime flow, dynamic trailing flow.
const REG: StrategyRegistry = {
  operators: ['>', '>=', '<', '<=', '=', '!='],
  groups: [
    {
      name: 'm_snapshot',
      kind: 'static',
      strict_params: [],
      metrics: [
        { name: 'time', unit: 'seconds', eq_tolerance: 0.5, monotonic: true, hue: 200 },
        { name: 'liquidity', unit: 'sol', eq_tolerance: 0.1, monotonic: false, hue: 185 },
      ],
    },
    {
      name: 'm_flow_lifetime',
      kind: 'static',
      strict_params: [],
      metrics: [
        { name: 'gross_flow', unit: 'sol', eq_tolerance: 0.1, monotonic: true, hue: 278 },
        { name: 'buy', unit: 'sol', eq_tolerance: 0.1, monotonic: true, hue: 170 },
      ],
    },
    {
      name: 'm_flow_window',
      kind: 'dynamic',
      strict_params: [{ name: 'window_size_sec', required: true }],
      metrics: [
        { name: 'net_flow', unit: 'sol', eq_tolerance: 0.1, monotonic: false, hue: 285 },
        { name: 'gross_flow', unit: 'sol', eq_tolerance: 0.1, monotonic: false, hue: 290 },
      ],
    },
    {
      name: 'm_price_window',
      kind: 'dynamic',
      strict_params: [{ name: 'window_size_sec', required: true }],
      metrics: [{ name: 'trail', unit: 'percent', eq_tolerance: 0.1, monotonic: false, hue: 45 }],
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
  it('parses off (any case, deduped) as a leading null', () => {
    expect(parseValueList('off, 10, 20')).toEqual([null, 10, 20]);
    expect(parseValueList('10, OFF, Off, 20')).toEqual([null, 10, 20]);
  });
});

describe('invalidValueFragments', () => {
  it('surfaces fragments that parse to nothing', () => {
    expect(invalidValueFragments('5, of, 10..20 step 5, 1O, off, ')).toEqual(['of', '1O']);
    expect(invalidValueFragments('off, 5, 10')).toEqual([]);
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
    const row = metricRow({ group: 'm_flow_window', metric: 'net_flow', window: '', valuesText: '1' });
    expect(axisRowError(row, REG)).toBe('window (s) > 0 required');
    expect(axisRowError({ ...row, window: '10' }, REG)).toBeNull();
  });
  it('does not require a window on m_flow_lifetime (static)', () => {
    const row = metricRow({
      group: 'm_flow_lifetime',
      metric: 'gross_flow',
      operator: '>=',
      window: '',
      valuesText: '50',
    });
    expect(axisRowError(row, REG)).toBeNull();
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
  it('accepts off on a metric row, rejects it on TP/SL and alone', () => {
    expect(
      axisRowError(metricRow({ group: 'm_snapshot', metric: 'time', operator: '>', valuesText: 'off, 5' }), REG),
    ).toBeNull();
    expect(axisRowError({ ...newAxisRow('take_profit'), valuesText: 'off, 100' }, REG)).toMatch(/only applies to metric axes/);
    expect(
      axisRowError(metricRow({ group: 'm_snapshot', metric: 'time', operator: '>', valuesText: 'off' }), REG),
    ).toBe("add at least one number besides 'off'");
  });
  it('flags unrecognized fragments instead of silently dropping them', () => {
    expect(
      axisRowError(metricRow({ group: 'm_snapshot', metric: 'time', operator: '>', valuesText: '5, of' }), REG),
    ).toBe('unrecognized value: of');
  });
});

describe('serializeAxisRows + comboCount', () => {
  const rows: GenericAxisRow[] = [
    metricRow({ group: 'm_snapshot', metric: 'time', operator: '>', valuesText: '5, 10, 15' }),
    metricRow({ side: 'entry', group: 'm_flow_window', metric: 'net_flow', operator: '>', window: '10', valuesText: '0, 2.5' }),
    { ...newAxisRow('take_profit'), valuesText: '50, 100, 200' },
  ];

  it('drops the window on static metrics and keeps it on dynamic', () => {
    const withLifetime = [
      ...rows,
      metricRow({
        side: 'entry',
        group: 'm_flow_lifetime',
        metric: 'gross_flow',
        operator: '>=',
        window: '999',
        valuesText: '50',
      }),
    ];
    const specs = serializeAxisRows(withLifetime, REG);
    expect(specs[0]).toMatchObject({ kind: 'metric', group: 'm_snapshot', metric: 'time', values: [5, 10, 15] });
    expect(specs[0].window).toBeUndefined();
    expect(specs[1]).toMatchObject({ group: 'm_flow_window', window: 10, values: [0, 2.5] });
    expect(specs[2]).toEqual({ kind: 'take_profit', values: [50, 100, 200] });
    expect(specs[3]).toMatchObject({ group: 'm_flow_lifetime', metric: 'gross_flow', values: [50] });
    expect(specs[3].window).toBeUndefined();
  });

  it('combo count is the product of value counts', () => {
    expect(comboCount(rows, REG)).toBe(3 * 2 * 3);
  });

  it('combo count is 0 when an axis has no values', () => {
    expect(comboCount([metricRow({ group: 'm_snapshot', metric: 'time', valuesText: '' })], REG)).toBe(0);
  });

  it('serializes off as null and counts it as a grid point', () => {
    const withOff = [metricRow({ group: 'm_snapshot', metric: 'time', operator: '>', valuesText: 'off, 5, 10' })];
    expect(serializeAxisRows(withOff, REG)[0].values).toEqual([null, 5, 10]);
    expect(comboCount(withOff, REG)).toBe(3);
  });
});

describe('multi-window axes serialize independently', () => {
  // No cross-row window validation: different windows on the same (side, group)
  // and on distinct groups both serialize as their own axes. The backend
  // assembles one GroupConditions instance per distinct window_size_sec, exactly
  // the engine's multi-window-per-group model.
  it('same group, two windows → two wire axes carrying each window', () => {
    const rows: GenericAxisRow[] = [
      metricRow({ side: 'exit', group: 'm_flow_window', metric: 'buy', window: '30', valuesText: '1, 3' }),
      metricRow({ side: 'exit', group: 'm_flow_window', metric: 'buy', window: '60', valuesText: '1, 3' }),
    ];
    const specs = serializeAxisRows(rows, REG);
    expect(specs.map((s) => s.window)).toEqual([30, 60]);
  });
  it('distinct groups keep independent windows', () => {
    // The reported footgun: m_price_window(5) and m_flow_window(3) are DIFFERENT
    // dynamic groups, each with its own window_size_sec, so the sizes may differ.
    const rows: GenericAxisRow[] = [
      metricRow({ side: 'entry', group: 'm_price_window', metric: 'trail', window: '5', valuesText: '1' }),
      metricRow({ side: 'entry', group: 'm_flow_window', metric: 'gross_flow', window: '3', valuesText: '10' }),
    ];
    const specs = serializeAxisRows(rows, REG);
    expect(specs.map((s) => s.window)).toEqual([5, 3]);
  });
});

describe('pnlAxisSugarDuplicateError', () => {
  it('rejects exit pnl >= that overlaps a TP axis value', () => {
    const rows: GenericAxisRow[] = [
      { ...newAxisRow('take_profit'), valuesText: '50, 100' },
      metricRow({
        side: 'exit',
        group: 'm_position',
        metric: 'pnl',
        operator: '>=',
        valuesText: '50, 75',
      }),
    ];
    expect(pnlAxisSugarDuplicateError(rows)).toMatch(/duplicates a TP axis/);
  });

  it('rejects exit pnl <= that overlaps −SL', () => {
    const rows: GenericAxisRow[] = [
      { ...newAxisRow('stop_loss'), valuesText: '30' },
      metricRow({
        side: 'exit',
        group: 'm_position',
        metric: 'pnl',
        operator: '<=',
        valuesText: '-30, -25',
      }),
    ];
    expect(pnlAxisSugarDuplicateError(rows)).toMatch(/duplicates an SL axis/);
  });

  it('allows a non-overlapping catastrophe pnl beside SL', () => {
    const rows: GenericAxisRow[] = [
      { ...newAxisRow('stop_loss'), valuesText: '30' },
      metricRow({
        side: 'exit',
        group: 'm_position',
        metric: 'pnl',
        operator: '<=',
        valuesText: '-25',
      }),
    ];
    expect(pnlAxisSugarDuplicateError(rows)).toBeNull();
  });
});
