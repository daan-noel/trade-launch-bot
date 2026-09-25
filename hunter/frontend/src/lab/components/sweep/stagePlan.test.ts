import { describe, expect, it } from 'vitest';
import type { MetricSpec, StrategyRegistry } from 'lib/strategy/registry';
import { STAGE_PLAN_TEMPLATE, stagePlanErrors, stagesFromWire, stagesToWire } from './stagePlan';

function metric(path: string, position: boolean): MetricSpec {
  return {
    name: path.split('.')[1],
    path,
    phrase: path,
    unit: position ? 'percent' : 'seconds',
    summary: '',
    example: '',
    note: '',
    tags: 'none',
    tag_level: 'trade',
    spans: { life: true, window: false, since_age: false, slice: false },
    monotonic: false,
    eq_tolerance: 0.1,
    hue: 0,
    position,
  };
}

const REG: StrategyRegistry = {
  operators: ['>', '>=', '<', '<=', '=', '!='],
  families: [
    { name: 'm_state', title: '', summary: '', example: '', metrics: [metric('m_state.age_sec', false)] },
    {
      name: 'm_position',
      title: '',
      summary: '',
      example: '',
      metrics: [metric('m_position.pnl_pct', true), metric('m_position.held_sec', true)],
    },
  ],
  spans: [],
  tags: { summary: '', example: '', fields: [], markers: [], builtin: [] },
  rule_parts: [],
};

describe('stage plan', () => {
  it('the template is a valid plan and round-trips to the same wire', () => {
    const stages = stagesFromWire(STAGE_PLAN_TEMPLATE.stages);
    expect(stagePlanErrors(stages, REG)).toEqual([]);
    expect(stagesToWire(stages)).toEqual(STAGE_PLAN_TEMPLATE.stages);
  });

  it('an empty plan is off, not an error', () => {
    expect(stagesFromWire([])).toEqual([]);
    expect(stagePlanErrors([], REG)).toEqual([]);
  });

  it('refuses a coin read, which the sweep records no column for', () => {
    const stages = stagesFromWire([
      { name: 'late', on: [{ if: [{ metric: 'm_state.age_sec', is: [{ operator: '>=', value: 10 }] }], sell: true }] },
    ]);
    expect(stagePlanErrors(stages, REG).join('\n')).toMatch(/our position only/);
  });

  it('names a partial sell that goes nowhere', () => {
    const stages = stagesFromWire([
      { name: 'bank', on: [{ if: [{ metric: 'm_position.pnl_pct', is: [{ operator: '>=', value: 50 }] }], sell: true, sell_pct: 50 }] },
    ]);
    expect(stagePlanErrors(stages, REG).join('\n')).toMatch(/partial sell/);
  });
});
