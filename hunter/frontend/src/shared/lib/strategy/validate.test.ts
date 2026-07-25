import { describe, expect, it } from 'vitest';
import { emptyRuleParams } from './ruleParams';
import {
  isPnlAdvancedMetric,
  pnlSugarDuplicateErrors,
  validateRuleParams,
} from './validate';

describe('isPnlAdvancedMetric', () => {
  it('flags only m_position.pnl', () => {
    expect(isPnlAdvancedMetric('m_position', 'pnl')).toBe(true);
    expect(isPnlAdvancedMetric('m_position', 'retrace')).toBe(false);
    expect(isPnlAdvancedMetric('m_snapshot', 'pnl')).toBe(false);
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
