import { describe, expect, it } from 'vitest';
import { ruleParamsJsonEqual } from './ruleParams';

describe('ruleParamsJsonEqual', () => {
  it('treats null / missing leaves as equal', () => {
    expect(ruleParamsJsonEqual({ take_profit: 50, stop_loss: null }, { take_profit: 50 })).toBe(
      true,
    );
  });

  it('ignores key order', () => {
    expect(
      ruleParamsJsonEqual(
        { stop_loss: 30, take_profit: 50 },
        { take_profit: 50, stop_loss: 30 },
      ),
    ).toBe(true);
  });

  it('detects different TP/SL', () => {
    expect(ruleParamsJsonEqual({ take_profit: 50 }, { take_profit: 40 })).toBe(false);
  });

  it('compares nested entry conditions', () => {
    const a = {
      take_profit: 50,
      entry: { m_snapshot: { time: [{ operator: '>', value: 10 }] } },
    };
    const b = {
      take_profit: 50,
      entry: { m_snapshot: { time: [{ operator: '>', value: 10 }] } },
    };
    const c = {
      take_profit: 50,
      entry: { m_snapshot: { time: [{ operator: '>', value: 20 }] } },
    };
    expect(ruleParamsJsonEqual(a, b)).toBe(true);
    expect(ruleParamsJsonEqual(a, c)).toBe(false);
  });
});
