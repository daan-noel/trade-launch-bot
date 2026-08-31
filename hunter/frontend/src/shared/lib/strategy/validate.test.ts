import { describe, expect, it } from 'vitest';
import { emptyRuleParams, type ExitStage } from './ruleParams';
import {
  isPnlAdvancedMetric,
  pnlSugarDuplicateErrors,
  validateRuleParams,
} from './validate';

describe('isPnlAdvancedMetric', () => {
  it('flags only m_position.pnl', () => {
    expect(isPnlAdvancedMetric('m_position', 'pnl')).toBe(true);
    expect(isPnlAdvancedMetric('m_position', 'retrace')).toBe(false);
    expect(isPnlAdvancedMetric('m_state', 'pnl')).toBe(false);
  });
});

describe('pnlSugarDuplicateErrors', () => {
  it('is empty when pnl is unused or sugar is off', () => {
    expect(pnlSugarDuplicateErrors(emptyRuleParams())).toEqual([]);
    expect(
      pnlSugarDuplicateErrors({
        ...emptyRuleParams(),
        take_profit: 50,
        exit: { m_position: [{ strict: {}, metrics: { pnl: [[{ operator: '<=', value: -25 }]] } }] },
      }),
    ).toEqual([]);
  });

  it('blocks pnl >= that restates take_profit', () => {
    const errs = pnlSugarDuplicateErrors({
      ...emptyRuleParams(),
      take_profit: 50,
      exit: { m_position: [{ strict: {}, metrics: { pnl: [[{ operator: '>=', value: 50 }]] } }] },
    });
    expect(errs).toHaveLength(1);
    expect(errs[0]).toMatch(/duplicates take_profit/);
  });

  it('blocks pnl <= that restates stop_loss', () => {
    const errs = pnlSugarDuplicateErrors({
      ...emptyRuleParams(),
      stop_loss: 30,
      exit: { m_position: [{ strict: {}, metrics: { pnl: [[{ operator: '<=', value: -30 }]] } }] },
    });
    expect(errs).toHaveLength(1);
    expect(errs[0]).toMatch(/duplicates stop_loss/);
  });

  it('allows a different pnl bound beside sugar', () => {
    expect(
      pnlSugarDuplicateErrors({
        ...emptyRuleParams(),
        take_profit: 50,
        stop_loss: 30,
        exit: {
          m_position: [
            {
              strict: {},
              metrics: { pnl: [[{ operator: '<=', value: -25 }]] },
            },
          ],
        },
      }),
    ).toEqual([]);
  });

  it('flags pnl sugar inside a DNF way', () => {
    const errs = pnlSugarDuplicateErrors({
      ...emptyRuleParams(),
      take_profit: 50,
      exit: {},
      exitClauses: [
        { m_position: [{ strict: {}, metrics: { pnl: [[{ operator: '>=', value: 50 }]] } }] },
      ],
    });
    expect(errs).toHaveLength(1);
    expect(errs[0]).toMatch(/duplicates take_profit/);
  });

  it('hooks into validateRuleParams', () => {
    const errs = validateRuleParams(
      {
        ...emptyRuleParams(),
        take_profit: 100,
        exit: { m_position: [{ strict: {}, metrics: { pnl: [[{ operator: '>=', value: 100 }]] } }] },
      },
      undefined,
    );
    expect(errs.some((e) => e.includes('duplicates take_profit'))).toBe(true);
  });
});

describe('parked scale-out stages (disabled.scale_out)', () => {
  const stage = (over: Partial<ExitStage> = {}): ExitStage => ({
    sell_bps: 7000,
    take_profit: 50,
    conditions: {},
    ...over,
  });

  it('does NOT charge a parked stage against the ladder budget', () => {
    // A live 70% + three parked stages that would blow both the count and the sum
    // cap if they were in the ladder. Parking has to actually free the budget —
    // otherwise the toggle buys the author nothing.
    const errs = validateRuleParams(
      {
        ...emptyRuleParams(),
        scale_out: [stage()],
        disabled: {
          scale_out: [stage({ sell_bps: 9900 }), stage(), stage({ sell_bps: null })],
        },
      },
      undefined,
    );
    expect(errs).toEqual([]);
  });

  it('still applies the PER-stage rules, so it can always be switched back on', () => {
    const errs = validateRuleParams(
      {
        ...emptyRuleParams(),
        disabled: {
          scale_out: [
            stage({ take_profit: null }), // can never fire
            stage({ sell_bps: 99999 }), // out of range
          ],
        },
      },
      undefined,
    );
    expect(errs).toEqual([
      'disabled.scale_out[0]: stage needs take_profit and/or non-empty conditions',
      'disabled.scale_out[1].sell_bps must be an integer in [1, 9900]',
    ]);
  });
});

describe('DNF exit clauses', () => {
  it('rejects an empty way', () => {
    const errs = validateRuleParams(
      { ...emptyRuleParams(), exitClauses: [{}] },
      undefined,
    );
    expect(errs.some((e) => e.includes('exit[0] is empty'))).toBe(true);
  });
});

describe('entry_lock', () => {
  it('rejects a slot lock with no live entry_event', () => {
    const errs = validateRuleParams({ ...emptyRuleParams(), entry_lock: 'slot' }, undefined);
    expect(errs.some((e) => e.includes('entry_lock requires a non-empty entry_event'))).toBe(true);
  });

  it('accepts a slot lock with a live entry_event', () => {
    const errs = validateRuleParams(
      {
        ...emptyRuleParams(),
        entry_lock: 'slot',
        entry_event: {
          m_state: [{ strict: {}, metrics: { time: [[{ operator: '>', value: 1 }]] } }],
        },
      },
      undefined,
    );
    expect(errs.filter((e) => e.includes('entry_lock'))).toEqual([]);
  });
});
