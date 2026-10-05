import { describe, expect, it } from 'vitest';

import { copyEditorRegistry } from './copyRule';
import type { FamilySpec, MetricSpec, StrategyRegistry } from './registry';

function metric(path: string): MetricSpec {
  return { path, name: path.slice(path.indexOf('.') + 1) } as MetricSpec;
}

function family(name: string, paths: string[]): FamilySpec {
  return { name, title: name, summary: '', example: '', metrics: paths.map(metric) };
}

describe('copy editor catalog', () => {
  it('offers state, price, flow, and position, plus the copy readings', () => {
    const reg = {
      families: [
        family('m_state', ['m_state.age_sec']),
        family('m_price', ['m_price.trail_pct']),
        family('m_flow', ['m_flow.buy_sol', 'm_flow.sell_tx_count']),
        family('m_holdings', ['m_holdings.profit_sol']),
        family('m_position', ['m_position.pnl_pct']),
      ],
      copy: [metric('m_print.flat_before'), metric('m_print.sold_bag_pct')],
    } as StrategyRegistry;

    const names = copyEditorRegistry(reg).families.map((f) => f.name);
    expect(names).toEqual(['m_state', 'm_price', 'm_flow', 'm_position', 'm_print']);
    const paths = copyEditorRegistry(reg).families.flatMap((f) => f.metrics.map((m) => m.path));
    expect(paths).toContain('m_flow.sell_tx_count');
    expect(paths).toContain('m_print.sold_bag_pct');
    expect(paths).not.toContain('m_holdings.profit_sol');
  });
});
