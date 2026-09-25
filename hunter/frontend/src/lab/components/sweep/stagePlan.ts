// The grouped sweep's Pass-2 **stage plan**: one rule `stages` array (the backend
// request's `stage_plans` is a list of them, `Stage[][]`). After ranking, each group's
// top-K combos are re-scored under the plan and under their own exit, and each keeps
// the better one. A plan reads our position only: the sweep records no coin columns
// for it, so the backend refuses any other read.

import { findMetric, type StrategyRegistry } from 'lib/strategy/registry';
import { emptyRuleDoc, liveMetricConds, ruleDocFromJson, ruleDocToJson, type Stage } from 'lib/strategy/ruleDoc';
import { validateRuleDoc } from 'lib/strategy/validate';

/** A stored plan (wire `stages` JSON) as editor stages. */
export function stagesFromWire(wire: unknown): Stage[] {
  return Array.isArray(wire) && wire.length ? ruleDocFromJson({ stages: wire }).stages : [];
}

/** Editor stages as the wire `stages` array the rule writes. */
export function stagesToWire(stages: Stage[]): unknown[] {
  const out = ruleDocToJson({ ...emptyRuleDoc(), stages }).stages;
  return Array.isArray(out) ? out : [];
}

/** Why the plan cannot run, one message per problem (empty = runnable). */
export function stagePlanErrors(stages: Stage[], reg: StrategyRegistry | undefined): string[] {
  if (stages.length === 0) return [];
  const doc = { ...emptyRuleDoc(), stages };
  const errors = [...validateRuleDoc(doc, reg).errors];
  for (const c of liveMetricConds(doc)) {
    const spec = findMetric(reg, c.ref.metric);
    if (spec && !spec.position) {
      errors.push(`${c.ref.metric}: a stage plan reads our position only (m_position), the sweep records no other column for it`);
    }
  }
  return errors;
}

/** A starting plan to edit: bank 70 % at +50 %, then sell the rest after 30 s held. */
export const STAGE_PLAN_TEMPLATE: { label: string; stages: unknown[] } = {
  label: 'bank 70 % at +50 %, rest after 30 s held',
  stages: [
    {
      name: 'bank',
      on: [
        {
          if: [{ metric: 'm_position.pnl_pct', is: [{ operator: '>=', value: 50 }] }],
          sell: 'bank',
          sell_pct: 70,
          go: 'rest',
        },
      ],
    },
    {
      name: 'rest',
      on: [{ if: [{ metric: 'm_position.held_sec', is: [{ operator: '>=', value: 30 }] }], sell: true }],
    },
  ],
};
