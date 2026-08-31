import { describe, expect, it } from 'vitest';
import type { StrategyRegistry } from './registry';
import {
  armAbovePctOrphanError,
  duplicateConditionRowError,
  paramsToConditionRows,
  newExitClauseId,
  newExitClauseRow,
  newRuleConditionRow,
  parkedSideWarnings,
  ruleConditionRowError,
  ruleRowEnabled,
  ruleRowIsTrailing,
  ruleRowNeedsSlice,
  rowsToSide,
  rowsToSides,
  setRowInstanceStrict,
  sideToRows,
  sidesToRows,
  type RuleConditionRow,
} from './ruleConditionRows';

const REG: StrategyRegistry = {
  operators: ['>', '>=', '<', '<=', '=', '!='],
  groups: [
    {
      name: 'm_state',
      kind: 'static',
      strict_params: [],
      metrics: [{ name: 'time', unit: 'seconds', eq_tolerance: 0.5, monotonic: true, hue: 200 }],
    },
    {
      name: 'm_price_window',
      kind: 'dynamic',
      // Mirrors the Rust registry: no size param is `required` on its own, because
      // "exactly one of the three" is a cross-param rule.
      strict_params: [
        { name: 'window_size_sec', required: false },
        { name: 'window_size_slots', required: false },
        { name: 'window_size_prints', required: false },
        { name: 'window_lag', required: false, allows_zero: true },
      ],
      metrics: [{ name: 'trail', unit: 'percent', eq_tolerance: 0.1, monotonic: false, hue: 45 }],
    },
    {
      name: 'm_flow_window',
      kind: 'dynamic',
      strict_params: [
        { name: 'window_size_sec', required: false },
        { name: 'window_size_slots', required: false },
        { name: 'window_size_prints', required: false },
        { name: 'window_lag', required: false, allows_zero: true },
        { name: 'slice_size_sec', required: false },
        { name: 'slice_size_slots', required: false },
        { name: 'slice_size_prints', required: false },
      ],
      metrics: [
        {
          name: 'trade_share',
          unit: 'percent',
          eq_tolerance: 0.5,
          monotonic: false,
          hue: 306,
          two_window: true,
        },
        // A single-window sibling in the SAME group - the shape that makes the slice
        // axis per-metric rather than per-group.
        { name: 'gross_flow', unit: 'sol', eq_tolerance: 0.1, monotonic: false, hue: 278 },
      ],
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
      row({ group: 'm_state', metric: 'time', arms: [] }),
      row({ group: 'm_state', metric: 'time', arms: [[{ operator: '>', value: 10 }]] }),
    ];
    const side = rowsToSide(rows, 'entry');
    expect(Object.keys(side)).toEqual(['m_state']);
    expect(side.m_state).toHaveLength(1);
  });

  it('only folds rows of the requested side', () => {
    const rows: RuleConditionRow[] = [
      row({ side: 'entry', group: 'm_state', metric: 'time', arms: [[{ operator: '>', value: 10 }]] }),
      row({ side: 'exit', group: 'm_state', metric: 'time', arms: [[{ operator: '>', value: 60 }]] }),
    ];
    expect(Object.keys(rowsToSide(rows, 'entry'))).toEqual(['m_state']);
    expect(Object.keys(rowsToSide(rows, 'exit'))).toEqual(['m_state']);
    expect(rowsToSide(rows, 'entry').m_state[0].metrics.time[0][0].value).toBe(10);
    expect(rowsToSide(rows, 'exit').m_state[0].metrics.time[0][0].value).toBe(60);
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
      [row({ group: 'm_state', metric: 'time', arms: [[{ operator: '>', value: 10 }]] })],
      'entry',
    );
    const exit = rowsToSide(
      [row({ side: 'exit', group: 'm_state', metric: 'time', arms: [[{ operator: '>', value: 60 }]] })],
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

describe('ruleRowIsTrailing', () => {
  it('is true only for m_position.retrace / .bounce', () => {
    expect(ruleRowIsTrailing(row({ group: 'm_position', metric: 'retrace' }))).toBe(true);
    expect(ruleRowIsTrailing(row({ group: 'm_position', metric: 'bounce' }))).toBe(true);
    expect(ruleRowIsTrailing(row({ group: 'm_position', metric: 'pnl' }))).toBe(false);
    expect(ruleRowIsTrailing(row({ group: 'm_price_window', metric: 'trail' }))).toBe(false);
  });
});

describe('arm_above_pct row validation', () => {
  it('rejects a negative arm value', () => {
    const r = row({
      side: 'exit',
      group: 'm_position',
      metric: 'retrace',
      arms: [[{ operator: '>=' as const, value: 3 }]],
      strict: { arm_above_pct: -1 },
    });
    expect(ruleConditionRowError(r, REG)).toMatch(/arm ≥ %/);
  });

  it('accepts arm_above_pct: 0 (arm at break-even)', () => {
    const r = row({
      side: 'exit',
      group: 'm_position',
      metric: 'retrace',
      arms: [[{ operator: '>=' as const, value: 3 }]],
      strict: { arm_above_pct: 0 },
    });
    expect(ruleConditionRowError(r, REG)).toBeNull();
  });
});

describe('armAbovePctOrphanError', () => {
  it('flags arm_above_pct authored with no trailing metric in the instance', () => {
    const rows: RuleConditionRow[] = [
      row({
        side: 'exit',
        group: 'm_position',
        metric: 'pnl',
        arms: [[{ operator: '>=' as const, value: 2 }]],
        strict: { arm_above_pct: 2 },
      }),
    ];
    expect(armAbovePctOrphanError(rows)).toMatch(/arm_above_pct gates the trailing metrics/);
  });

  it('is null when a trailing metric shares the instance', () => {
    const rows: RuleConditionRow[] = [
      row({
        side: 'exit',
        group: 'm_position',
        metric: 'retrace',
        arms: [[{ operator: '>=' as const, value: 3 }]],
        strict: { arm_above_pct: 2 },
      }),
    ];
    expect(armAbovePctOrphanError(rows)).toBeNull();
  });
});

describe('parked (disabled) rows', () => {
  const live = row({
    group: 'm_price_window',
    metric: 'trail',
    window: '30',
    arms: [[{ operator: '>=' as const, value: 20 }]],
  });
  // The whole point of the toggle: the value this one replaced, kept around.
  const parked = row({
    enabled: false,
    group: 'm_price_window',
    metric: 'trail',
    window: '30',
    arms: [[{ operator: '>=' as const, value: 12 }]],
  });

  it('treats a missing `enabled` as live (rows predate the toggle)', () => {
    const legacy = { ...row({ group: 'm_state', metric: 'time' }), enabled: undefined };
    expect(ruleRowEnabled(legacy)).toBe(true);
    expect(ruleRowEnabled(parked)).toBe(false);
  });

  it('folds parked rows into `disabled`, never the live side', () => {
    const { entry, exit, disabled } = rowsToSides([live, parked]);
    expect(entry.m_price_window).toHaveLength(1);
    expect(entry.m_price_window[0].metrics.trail[0][0].value).toBe(20);
    expect(disabled?.entry?.m_price_window[0].metrics.trail[0][0].value).toBe(12);
    expect(Object.keys(exit)).toHaveLength(0);
  });

  it('leaves `disabled` null when nothing is parked', () => {
    expect(rowsToSides([live]).disabled).toBeNull();
  });

  it('round-trips through sidesToRows with the flags intact', () => {
    const { entry, exit, disabled } = rowsToSides([live, parked]);
    const back = sidesToRows(entry, exit, disabled);
    expect(back).toHaveLength(2);
    expect(back.filter(ruleRowEnabled)).toHaveLength(1);
    // Re-folding the reloaded rows is a fixed point — a save+reload+save cycle must
    // not migrate a parked condition into the live side (or lose it).
    expect(rowsToSides(back)).toEqual({
      entry,
      entry_event: {},
      exit,
      disabled,
      exitClauses: undefined,
    });
  });

  it('does not flag a parked row as a duplicate of its live twin', () => {
    // Same side/group/window/metric — legal, because they go to different bags.
    expect(duplicateConditionRowError([live, parked])).toBeNull();
    // ...but two PARKED rows on the same key still collide inside the bag.
    expect(duplicateConditionRowError([parked, { ...parked, id: 'x' }])).toMatch(/set twice/);
  });

  it('orphans arm_above_pct when its trailing metric is parked', () => {
    const trailing = row({
      side: 'exit',
      group: 'm_position',
      metric: 'retrace',
      arms: [[{ operator: '>=' as const, value: 3 }]],
      strict: { arm_above_pct: 2 },
    });
    expect(armAbovePctOrphanError([trailing])).toBeNull();
    // Parking it moves it to the other bag — the live instance now has an arm with
    // nothing to gate, exactly what the backend rejects at save.
    expect(armAbovePctOrphanError([{ ...trailing, enabled: false }, { ...trailing, id: 'p', metric: 'pnl' }]))
      .toMatch(/arm_above_pct gates the trailing metrics/);
  });

  it('warns when every condition of a side is parked', () => {
    expect(parkedSideWarnings([live, parked])).toEqual([]);
    expect(parkedSideWarnings([parked])).toEqual([
      'every filter is off — the rule now buys on the fingerprint and event alone',
    ]);
    expect(parkedSideWarnings([{ ...parked, side: 'exit' }])).toEqual([
      'every exit condition is off — only TP / SL / death can close a position',
    ]);
    expect(parkedSideWarnings([{ ...parked, side: 'entry_event' }])).toEqual([
      'every event condition is off — the completing-print gate is gone (and entry_lock will not save)',
    ]);
    // A side with no authored conditions at all was never constrained — no warning.
    expect(parkedSideWarnings([row({ group: '', metric: '', arms: [] })])).toEqual([]);
  });
});

describe('entry_event rows', () => {
  it('loads completing-print conditions as event-side rows and round-trips', () => {
    const params = {
      take_profit: null,
      stop_loss: null,
      exclusive: false,
      priority: 0,
      entry_lock: 'slot' as const,
      entry_event: {
        m_state: [{ strict: {}, metrics: { time: [[{ operator: '>' as const, value: 2 }]] } }],
      },
      entry: {
        m_state: [{ strict: {}, metrics: { time: [[{ operator: '<' as const, value: 30 }]] } }],
      },
      exit: {},
    };
    const rows = paramsToConditionRows(params);
    expect(rows.filter((r) => r.side === 'entry_event')).toHaveLength(1);
    expect(rows.filter((r) => r.side === 'entry')).toHaveLength(1);
    const folded = rowsToSides(rows);
    expect(folded.entry_event.m_state[0].metrics.time[0][0].value).toBe(2);
    expect(folded.entry.m_state[0].metrics.time[0][0].value).toBe(30);
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

  it('clears arm_above_pct for the WHOLE instance, not just the edited row', () => {
    // The arm control renders on the trailing row only, but every row of the
    // instance carries the bag and `rowsToSide` merges them — patching one row
    // alone lets the sibling `pnl` row put the old value straight back on save.
    const side = {
      m_position: [
        {
          strict: { arm_above_pct: 2 },
          metrics: {
            retrace: [[{ operator: '>=' as const, value: 3 }]],
            pnl: [[{ operator: '<=' as const, value: -8 }]],
          },
        },
      ],
    };
    const rows = sideToRows(side, 'exit');
    const trailing = rows.find((r) => ruleRowIsTrailing(r))!;

    const cleared = setRowInstanceStrict(rows, trailing.id, {});
    expect(cleared.every((r) => r.strict?.arm_above_pct == null)).toBe(true);
    expect(rowsToSide(cleared, 'exit').m_position[0].strict).toEqual({});

    const retuned = setRowInstanceStrict(rows, trailing.id, { arm_above_pct: 5 });
    expect(rowsToSide(retuned, 'exit').m_position[0].strict).toEqual({ arm_above_pct: 5 });
  });

  it('scopes an instance strict edit to that instance', () => {
    // Two windows of one group = two instances; and a parked row is its own bag.
    const rows = [
      row({ id: 'a', side: 'exit', group: 'm_price_window', metric: 'trail', window: '5', arms: [[{ operator: '>=', value: 3 }]], strict: { arm_above_pct: 2 } }),
      row({ id: 'b', side: 'exit', group: 'm_price_window', metric: 'trail', window: '30', arms: [[{ operator: '>=', value: 3 }]], strict: { arm_above_pct: 2 } }),
      row({ id: 'c', side: 'exit', enabled: false, group: 'm_price_window', metric: 'trail', window: '5', arms: [[{ operator: '>=', value: 3 }]], strict: { arm_above_pct: 2 } }),
    ];
    const out = setRowInstanceStrict(rows, 'a', {});
    expect(out.map((r) => r.strict?.arm_above_pct)).toEqual([undefined, 2, 2]);
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

describe('two-window group (m_flow_window)', () => {
  const sliceRow = (window: string, slice: number, value: number): RuleConditionRow =>
    row({
      group: 'm_flow_window',
      metric: 'trade_share',
      window,
      strict: { slice_size_sec: slice },
      arms: [[{ operator: '>=' as const, value }]],
    });

  it('keeps two clauses that share a reference window but differ in the slice', () => {
    // Both axes are the group's identity. Keying instances on `window_size_sec`
    // alone merged these into one and the later slice silently won — one of the
    // two gates just disappeared on save.
    const side = rowsToSide([sliceRow('60', 3, 8), sliceRow('60', 10, 60)], 'entry');
    expect(side.m_flow_window).toHaveLength(2);
    expect(side.m_flow_window.map((i) => i.strict.slice_size_sec).sort((a, b) => a! - b!)).toEqual([3, 10]);
  });

  it('round-trips both axes through the row editor', () => {
    const side = {
      m_flow_window: [
        {
          strict: { window_size_sec: 60, slice_size_sec: 3 },
          metrics: { trade_share: [[{ operator: '>=' as const, value: 7.69 }]] },
        },
      ],
    };
    const rows = sideToRows(side, 'entry');
    expect(rows[0].window).toBe('60');
    expect(rows[0].strict).toEqual({ slice_size_sec: 3 });
    expect(rowsToSide(rows, 'entry')).toEqual(side);
  });

  it('asks the METRIC, not the group, whether a slice belongs', () => {
    // `m_flow_window` declares `slice_size_*` for every instance, so a group-level
    // test would put a slice control on `gross_flow` and the backend would then
    // reject the save as a span nothing reads. Both directions are pinned here.
    const grossRow = (strict: Record<string, number>): RuleConditionRow =>
      row({
        group: 'm_flow_window',
        metric: 'gross_flow',
        window: '60',
        strict,
        arms: [[{ operator: '>=', value: 5 }]],
      });
    expect(ruleRowNeedsSlice(grossRow({}), REG)).toBe(false);
    expect(ruleConditionRowError(grossRow({}), REG)).toBeNull();
    expect(ruleConditionRowError(grossRow({ slice_size_sec: 3 }), REG)).toMatch(/slice/);

    const shareRow = row({
      group: 'm_flow_window',
      metric: 'trade_share',
      window: '60',
      strict: { slice_size_sec: 3 },
      arms: [[{ operator: '>=', value: 8 }]],
    });
    expect(ruleRowNeedsSlice(shareRow, REG)).toBe(true);
    expect(ruleConditionRowError(shareRow, REG)).toBeNull();
  });

  it('requires the slice axis and rejects one that does not nest', () => {
    expect(
      ruleConditionRowError(
        row({
          group: 'm_flow_window',
          metric: 'trade_share',
          window: '60',
          arms: [[{ operator: '>=' as const, value: 8 }]],
        }),
        REG,
      ),
    ).toMatch(/slice/);
    expect(ruleConditionRowError(sliceRow('60', 90, 8), REG)).toMatch(/nest inside window 60/);
    expect(ruleConditionRowError(sliceRow('60', 3, 8), REG)).toBeNull();
  });
});

describe('slot windows and lag', () => {
  const slotRow = (over: Partial<RuleConditionRow> = {}) =>
    row({
      group: 'm_price_window',
      metric: 'trail',
      window: '30',
      windowUnit: 'slot',
      arms: [[{ operator: '>=', value: 5 }]],
      ...over,
    });

  it('writes exactly one size param, in the row unit', () => {
    const side = rowsToSide([slotRow()], 'entry');
    expect(side.m_price_window[0].strict).toEqual({ window_size_slots: 30 });
  });

  it('omits a zero lag so a pre-slot rule round-trips byte-identically', () => {
    const secs = row({
      group: 'm_price_window',
      metric: 'trail',
      window: '30',
      arms: [[{ operator: '>=', value: 5 }]],
    });
    expect(rowsToSide([secs], 'entry').m_price_window[0].strict).toEqual({
      window_size_sec: 30,
    });
  });

  it('carries the lag onto the instance when there is one', () => {
    const side = rowsToSide([slotRow({ lag: '1' })], 'entry');
    expect(side.m_price_window[0].strict).toEqual({ window_size_slots: 30, window_lag: 1 });
  });

  it('keeps two slot windows of one metric as TWO instances', () => {
    // The whole point of keying on the span rather than the size: before, both rows
    // had no `window_size_sec`, collapsed onto one instance, and the later row's
    // strict bag silently won — one of the two gates just disappeared on save.
    const side = rowsToSide([slotRow(), slotRow({ window: '1' })], 'entry');
    expect(side.m_price_window).toHaveLength(2);
    expect(side.m_price_window.map((i) => i.strict.window_size_slots).sort()).toEqual([1, 30]);
  });

  it('separates a slot window from a seconds window of the same size', () => {
    const side = rowsToSide([slotRow(), slotRow({ windowUnit: 'sec' })], 'entry');
    expect(side.m_price_window).toHaveLength(2);
  });

  it('separates a lagged window from an unlagged one of the same size', () => {
    const side = rowsToSide([slotRow(), slotRow({ lag: '1' })], 'entry');
    expect(side.m_price_window).toHaveLength(2);
  });

  it('round-trips a slot rule through rows and back', () => {
    const side = { m_price_window: [{ strict: { window_size_slots: 30, window_lag: 1 }, metrics: { trail: [[{ operator: '>=' as const, value: 5 }]] } }] };
    const rows = sideToRows(side, 'entry');
    expect(rows[0]).toMatchObject({ window: '30', windowUnit: 'slot', lag: '1' });
    // Nothing the row fields own may ALSO sit in the opaque strict bag, or a stale
    // copy would outlive an edit to the field.
    expect(rows[0].strict).toEqual({});
    expect(rowsToSide(rows, 'entry')).toEqual(side);
  });

  it('validates a slot row against its own unit, and accepts lag 0', () => {
    expect(ruleConditionRowError(slotRow(), REG)).toBeNull();
    expect(ruleConditionRowError(slotRow({ lag: '0' }), REG)).toBeNull();
    expect(ruleConditionRowError(slotRow({ window: '' }), REG)).toBe('window (sl) > 0 required');
    expect(ruleConditionRowError(slotRow({ lag: '-1' }), REG)).toBe(
      'lag (sl) must be a number ≥ 0',
    );
  });

  it('requires the slice axis in the reference unit and nested inside it', () => {
    const sliceRowOf = (over: Partial<RuleConditionRow> = {}) =>
      row({
        group: 'm_flow_window',
        metric: 'trade_share',
        window: '30',
        windowUnit: 'slot',
        arms: [[{ operator: '>=', value: 50 }]],
        ...over,
      });
    expect(ruleConditionRowError(sliceRowOf(), REG)).toBe('slice (sl) > 0 required');
    // A seconds slice on a slot row is not the row's slice at all - it reads as
    // absent, which is what the backend also rejects (both axes, one unit).
    expect(ruleConditionRowError(sliceRowOf({ strict: { slice_size_sec: 3 } }), REG)).toBe(
      'slice (sl) > 0 required',
    );
    expect(ruleConditionRowError(sliceRowOf({ strict: { slice_size_slots: 40 } }), REG)).toBe(
      'slice (sl) must nest inside window 30',
    );
    const ok = sliceRowOf({ strict: { slice_size_slots: 1 } });
    expect(ruleConditionRowError(ok, REG)).toBeNull();
    expect(rowsToSide([ok], 'entry').m_flow_window[0].strict).toEqual({
      window_size_slots: 30,
      slice_size_slots: 1,
    });
  });
});

describe('print windows', () => {
  const printRow = (over: Partial<RuleConditionRow> = {}) =>
    row({
      group: 'm_price_window',
      metric: 'trail',
      window: '20',
      windowUnit: 'print',
      arms: [[{ operator: '>=', value: 5 }]],
      ...over,
    });

  it('writes exactly one size param, in the row unit', () => {
    const side = rowsToSide([printRow()], 'entry');
    expect(side.m_price_window[0].strict).toEqual({ window_size_prints: 20 });
  });

  it('writes the one-transaction span', () => {
    const side = rowsToSide([printRow({ window: '1' })], 'entry');
    expect(side.m_price_window[0].strict).toEqual({ window_size_prints: 1 });
  });

  it('carries the lag onto the instance when there is one', () => {
    const side = rowsToSide([printRow({ lag: '1' })], 'entry');
    expect(side.m_price_window[0].strict).toEqual({ window_size_prints: 20, window_lag: 1 });
  });

  // Three bases at one size are three DIFFERENT reads: 20 prints, 20 slots and 20
  // seconds cover different tape. Merging any pair would silently drop a gate.
  it('separates a print window from a slot and a seconds window of the same size', () => {
    const side = rowsToSide(
      [printRow(), printRow({ windowUnit: 'slot' }), printRow({ windowUnit: 'sec' })],
      'entry',
    );
    expect(side.m_price_window).toHaveLength(3);
  });

  it('round-trips a print rule through rows and back', () => {
    const side = {
      m_price_window: [
        {
          strict: { window_size_prints: 1 },
          metrics: { trail: [[{ operator: '>=' as const, value: 5 }]] },
        },
      ],
    };
    const rows = sideToRows(side, 'entry');
    expect(rows[0]).toMatchObject({ window: '1', windowUnit: 'print', lag: '' });
    // Nothing the row fields own may ALSO sit in the opaque strict bag, or a stale
    // copy would outlive an edit to the field.
    expect(rows[0].strict).toEqual({});
    expect(rowsToSide(rows, 'entry')).toEqual(side);
  });

  it('validates a print row against its own unit', () => {
    expect(ruleConditionRowError(printRow(), REG)).toBeNull();
    expect(ruleConditionRowError(printRow({ window: '' }), REG)).toBe('window (p) > 0 required');
    expect(ruleConditionRowError(printRow({ lag: '-1' }), REG)).toBe(
      'lag (p) must be a number ≥ 0',
    );
  });

  it('requires the slice axis in the print unit and nested inside it', () => {
    const sliceRowOf = (over: Partial<RuleConditionRow> = {}) =>
      row({
        group: 'm_flow_window',
        metric: 'trade_share',
        window: '20',
        windowUnit: 'print',
        arms: [[{ operator: '>=', value: 50 }]],
        ...over,
      });
    expect(ruleConditionRowError(sliceRowOf(), REG)).toBe('slice (p) > 0 required');
    // A slot slice on a print row is not the row's slice at all - it reads as
    // absent, which is what the backend also rejects (both axes, one unit).
    expect(ruleConditionRowError(sliceRowOf({ strict: { slice_size_slots: 4 } }), REG)).toBe(
      'slice (p) > 0 required',
    );
    expect(ruleConditionRowError(sliceRowOf({ strict: { slice_size_prints: 40 } }), REG)).toBe(
      'slice (p) must nest inside window 20',
    );
    const ok = sliceRowOf({ strict: { slice_size_prints: 4 } });
    expect(ruleConditionRowError(ok, REG)).toBeNull();
    expect(rowsToSide([ok], 'entry').m_flow_window[0].strict).toEqual({
      window_size_prints: 20,
      slice_size_prints: 4,
    });
  });

  // Flipping the unit RE-SPELLS the slice param. A sibling left behind is the "two
  // spans claiming one axis" the backend rejects at save, and with three bases a
  // per-pair destructure is exactly what would leave one.
  it('never writes two size params on one axis after a unit flip', () => {
    const stale = row({
      group: 'm_flow_window',
      metric: 'trade_share',
      window: '20',
      windowUnit: 'print',
      arms: [[{ operator: '>=', value: 50 }]],
      strict: { slice_size_sec: 3, slice_size_slots: 1, slice_size_prints: 4 },
    });
    expect(rowsToSide([stale], 'entry').m_flow_window[0].strict).toEqual({
      window_size_prints: 20,
      slice_size_prints: 4,
    });
  });
});

describe('exit ways (DNF)', () => {
  it('collapses singleton ways to object-form', () => {
    const a = newExitClauseRow();
    const b = newExitClauseRow();
    const rows: RuleConditionRow[] = [
      { ...a, group: 'm_state', metric: 'time', arms: [[{ operator: '>', value: 10 }]] },
      { ...b, group: 'm_price_window', metric: 'trail', window: '30', arms: [[{ operator: '>=', value: 12 }]] },
    ];
    const folded = rowsToSides(rows);
    expect(folded.exitClauses).toBeUndefined();
    expect(folded.exit.m_state[0].metrics.time[0][0].value).toBe(10);
    expect(folded.exit.m_price_window[0].metrics.trail[0][0].value).toBe(12);
  });

  it('keeps a multi-metric way as array-form', () => {
    const id = newExitClauseId();
    const rows: RuleConditionRow[] = [
      { ...newExitClauseRow(id), group: 'm_state', metric: 'time', arms: [[{ operator: '>', value: 10 }]] },
      {
        ...newExitClauseRow(id),
        group: 'm_price_window',
        metric: 'trail',
        window: '30',
        arms: [[{ operator: '>=', value: 12 }]],
      },
    ];
    const folded = rowsToSides(rows);
    expect(folded.exitClauses).toHaveLength(1);
    expect(Object.keys(folded.exit)).toHaveLength(0);
    expect(folded.exitClauses![0].m_state[0].metrics.time).toBeTruthy();
    expect(folded.exitClauses![0].m_price_window[0].metrics.trail).toBeTruthy();
  });

  it('allows the same metric in two different ways', () => {
    const rows: RuleConditionRow[] = [
      { ...newExitClauseRow(), group: 'm_state', metric: 'time', arms: [[{ operator: '>', value: 10 }]] },
      { ...newExitClauseRow(), group: 'm_state', metric: 'time', arms: [[{ operator: '<', value: 3 }]] },
    ];
    expect(duplicateConditionRowError(rows)).toBeNull();
    const folded = rowsToSides(rows);
    expect(folded.exitClauses).toHaveLength(2);
  });

  it('round-trips array-form through paramsToConditionRows', () => {
    const id = newExitClauseId();
    const rows: RuleConditionRow[] = [
      { ...newExitClauseRow(id), group: 'm_state', metric: 'time', arms: [[{ operator: '>', value: 10 }]] },
      {
        ...newExitClauseRow(id),
        group: 'm_price_window',
        metric: 'trail',
        window: '5',
        arms: [[{ operator: '>=', value: 8 }]],
      },
    ];
    const folded = rowsToSides(rows);
    const back = paramsToConditionRows({
      take_profit: null,
      stop_loss: null,
      exclusive: false,
      priority: 0,
      entry: folded.entry,
      exit: folded.exit,
      exitClauses: folded.exitClauses,
      disabled: folded.disabled,
    });
    const again = rowsToSides(back);
    expect(again.exitClauses).toHaveLength(1);
    expect(again.exitClauses![0].m_state[0].metrics.time[0][0].value).toBe(10);
    expect(again.exitClauses![0].m_price_window[0].metrics.trail[0][0].value).toBe(8);
  });

  it('round-trips a parked AND way as one parked clause', () => {
    const id = newExitClauseId();
    const rows: RuleConditionRow[] = [
      {
        ...newExitClauseRow(id),
        group: 'm_state',
        metric: 'time',
        arms: [[{ operator: '>', value: 10 }]],
        enabled: false,
      },
      {
        ...newExitClauseRow(id),
        group: 'm_price_window',
        metric: 'trail',
        window: '5',
        arms: [[{ operator: '>=', value: 8 }]],
        enabled: false,
      },
    ];
    const folded = rowsToSides(rows);
    expect(folded.disabled?.exitClauses).toHaveLength(1);
    const back = paramsToConditionRows({
      take_profit: null,
      stop_loss: null,
      exclusive: false,
      priority: 0,
      entry: {},
      exit: {},
      disabled: folded.disabled,
    });
    expect(back.every((r) => !r.enabled)).toBe(true);
    expect(new Set(back.map((r) => r.clauseId)).size).toBe(1);
    const again = rowsToSides(back);
    expect(again.disabled?.exitClauses).toHaveLength(1);
    expect(again.disabled!.exitClauses![0].m_state[0].metrics.time[0][0].value).toBe(10);
    expect(again.disabled!.exitClauses![0].m_price_window[0].metrics.trail[0][0].value).toBe(8);
  });
});
