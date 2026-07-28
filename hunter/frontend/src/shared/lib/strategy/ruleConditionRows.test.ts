import { describe, expect, it } from 'vitest';
import type { StrategyRegistry } from './registry';
import {
  duplicateConditionRowError,
  newRuleConditionRow,
  ruleConditionRowError,
  rowsToSide,
  sideToRows,
  sidesToRows,
  type RuleConditionRow,
} from './ruleConditionRows';

const REG: StrategyRegistry = {
  operators: ['>', '>=', '<', '<=', '=', '!='],
  groups: [
    {
      name: 'm_snapshot',
      kind: 'static',
      strict_params: [],
      metrics: [{ name: 'time', unit: 'seconds', eq_tolerance: 0.5, monotonic: true, hue: 200 }],
    },
    {
      name: 'm_price_window',
      kind: 'dynamic',
      strict_params: [{ name: 'window_size_sec', required: true }],
      metrics: [{ name: 'trail', unit: 'percent', eq_tolerance: 0.1, monotonic: false, hue: 45 }],
    },
    {
      name: 'm_position',
      kind: 'static',
      scope: 'position',
      strict_params: [{ name: 'arm_above_pct', required: false, allows_zero: true }],
      metrics: [{ name: 'retrace', unit: 'percent', eq_tolerance: 0.1, monotonic: false, hue: 15 }],
    },
  ],
};

function row(over: Partial<RuleConditionRow>): RuleConditionRow {
  return { ...newRuleConditionRow('entry'), ...over };
}

describe('rowsToSide', () => {
  it('folds two windows of one metric into two group instances', () => {
    const rows: RuleConditionRow[] = [
      row({ group: 'm_price_window', metric: 'trail', window: '5', arms: [[{ operator: '>=', value: 8 }]] }),
      row({ group: 'm_price_window', metric: 'trail', window: '30', arms: [[{ operator: '>=', value: 15 }]] }),
    ];
    const side = rowsToSide(rows, 'entry');
    expect(side.m_price_window).toHaveLength(2);
    const windows = side.m_price_window.map((g) => g.strict.window_size_sec).sort((a, b) => a - b);
    expect(windows).toEqual([5, 30]);
  });

  it('merges same (group, window) rows into one instance', () => {
    const rows: RuleConditionRow[] = [
      row({ group: 'm_price_window', metric: 'trail', window: '30', arms: [[{ operator: '>=', value: 8 }]] }),
      // A different metric on the same group+window shares the instance.
      row({ group: 'm_price_window', metric: 'trail', window: '30', arms: [[{ operator: '<', value: 40 }]] }),
    ];
    const side = rowsToSide(rows, 'entry');
    // Same (group, window) ⇒ one instance (last write wins for the same metric).
    expect(side.m_price_window).toHaveLength(1);
  });

  it('drops half-authored and empty-condition rows', () => {
    const rows: RuleConditionRow[] = [
      row({ group: '', metric: '', arms: [] }),
      row({ group: 'm_snapshot', metric: 'time', arms: [] }),
      row({ group: 'm_snapshot', metric: 'time', arms: [[{ operator: '>', value: 10 }]] }),
    ];
    const side = rowsToSide(rows, 'entry');
    expect(Object.keys(side)).toEqual(['m_snapshot']);
    expect(side.m_snapshot).toHaveLength(1);
  });

  it('only folds rows of the requested side', () => {
    const rows: RuleConditionRow[] = [
      row({ side: 'entry', group: 'm_snapshot', metric: 'time', arms: [[{ operator: '>', value: 10 }]] }),
      row({ side: 'exit', group: 'm_snapshot', metric: 'time', arms: [[{ operator: '>', value: 60 }]] }),
    ];
    expect(Object.keys(rowsToSide(rows, 'entry'))).toEqual(['m_snapshot']);
    expect(Object.keys(rowsToSide(rows, 'exit'))).toEqual(['m_snapshot']);
    expect(rowsToSide(rows, 'entry').m_snapshot[0].metrics.time[0][0].value).toBe(10);
    expect(rowsToSide(rows, 'exit').m_snapshot[0].metrics.time[0][0].value).toBe(60);
  });
});

describe('sideToRows / round-trip', () => {
  it('expands each (group, window, metric) into its own row and round-trips', () => {
    const rows: RuleConditionRow[] = [
      row({ group: 'm_price_window', metric: 'trail', window: '5', arms: [[{ operator: '>=', value: 8 }]] }),
      row({ group: 'm_price_window', metric: 'trail', window: '30', arms: [[{ operator: '>=', value: 15 }]] }),
    ];
    const side = rowsToSide(rows, 'entry');
    const back = sideToRows(side, 'entry');
    expect(back).toHaveLength(2);
    // Re-folding the expanded rows yields the same side.
    expect(rowsToSide(back, 'entry')).toEqual(side);
  });

  it('sidesToRows loads both sides', () => {
    const entry = rowsToSide(
      [row({ group: 'm_snapshot', metric: 'time', arms: [[{ operator: '>', value: 10 }]] })],
      'entry',
    );
    const exit = rowsToSide(
      [row({ side: 'exit', group: 'm_snapshot', metric: 'time', arms: [[{ operator: '>', value: 60 }]] })],
      'exit',
    );
    const rows = sidesToRows(entry, exit);
    expect(rows.filter((r) => r.side === 'entry')).toHaveLength(1);
    expect(rows.filter((r) => r.side === 'exit')).toHaveLength(1);
  });
});

describe('ruleConditionRowError', () => {
  it('walks group → metric → window → condition', () => {
    expect(ruleConditionRowError(row({}), REG)).toBe('pick a metric group');
    expect(ruleConditionRowError(row({ group: 'm_price_window', metric: '' }), REG)).toBe('pick a metric');
    expect(
      ruleConditionRowError(row({ group: 'm_price_window', metric: 'trail', window: '' }), REG),
    ).toBe('window (s) > 0 required');
    expect(
      ruleConditionRowError(row({ group: 'm_price_window', metric: 'trail', window: '30', arms: [] }), REG),
    ).toMatch(/add a condition/);
    expect(
      ruleConditionRowError(
        row({ group: 'm_price_window', metric: 'trail', window: '30', arms: [[{ operator: '>', value: 5 }]] }),
        REG,
      ),
    ).toBeNull();
  });

  it('rejects a position-scoped group on entry, accepts it on exit', () => {
    const base = { group: 'm_position', metric: 'retrace', arms: [[{ operator: '>=' as const, value: 3 }]] };
    expect(ruleConditionRowError(row({ ...base, side: 'entry' }), REG)).toMatch(/exit-only/);
    expect(ruleConditionRowError(row({ ...base, side: 'exit' }), REG)).toBeNull();
  });
});

describe('duplicateConditionRowError', () => {
  it('flags the same (side, group, window, metric) twice', () => {
    const rows: RuleConditionRow[] = [
      row({ group: 'm_price_window', metric: 'trail', window: '30', arms: [[{ operator: '>', value: 5 }]] }),
      row({ group: 'm_price_window', metric: 'trail', window: '30', arms: [[{ operator: '<', value: 40 }]] }),
    ];
    expect(duplicateConditionRowError(rows)).toMatch(/set twice/);
  });

  it('allows the same metric at different windows', () => {
    const rows: RuleConditionRow[] = [
      row({ group: 'm_price_window', metric: 'trail', window: '5', arms: [[{ operator: '>', value: 5 }]] }),
      row({ group: 'm_price_window', metric: 'trail', window: '30', arms: [[{ operator: '>', value: 5 }]] }),
    ];
    expect(duplicateConditionRowError(rows)).toBeNull();
  });
});

describe('non-window strict params', () => {
  it('round-trips a strict param the editor has no control for', () => {
    // `m_position.arm_above_pct` is authored by API/SQL today. Opening such a rule
    // in the row editor and saving it again must NOT drop the param — the row model
    // carries the whole strict bag, so new registry params survive by default.
    const side = {
      m_position: [
        {
          strict: { arm_above_pct: 2 },
          metrics: { retrace: [[{ operator: '>=' as const, value: 3 }]] },
        },
      ],
    };
    const rows = sideToRows(side, 'exit');
    expect(rows).toHaveLength(1);
    expect(rows[0].strict).toEqual({ arm_above_pct: 2 });
    expect(rowsToSide(rows, 'exit')).toEqual(side);
  });

  it('keeps arm_above_pct: 0 distinct from the param being absent', () => {
    const zero = sideToRows(
      { m_position: [{ strict: { arm_above_pct: 0 }, metrics: { retrace: [[{ operator: '>=' as const, value: 3 }]] } }] },
      'exit',
    );
    expect(rowsToSide(zero, 'exit').m_position[0].strict).toEqual({ arm_above_pct: 0 });

    const absent = sideToRows(
      { m_position: [{ strict: {}, metrics: { retrace: [[{ operator: '>=' as const, value: 3 }]] } }] },
      'exit',
    );
    expect(rowsToSide(absent, 'exit').m_position[0].strict).toEqual({});
  });

  it('does not leak the window into the strict bag', () => {
    const rows = sideToRows(
      { m_price_window: [{ strict: { window_size_sec: 30 }, metrics: { trail: [[{ operator: '>=' as const, value: 12 }]] } }] },
      'entry',
    );
    expect(rows[0].strict).toEqual({});
    expect(rowsToSide(rows, 'entry').m_price_window[0].strict).toEqual({ window_size_sec: 30 });
  });
});
