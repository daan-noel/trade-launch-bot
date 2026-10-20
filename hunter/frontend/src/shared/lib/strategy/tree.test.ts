import { describe, expect, it } from 'vitest';

// The engine test `the_shared_tree_rule_fixture_parses_and_round_trips` parses the same
// file (the 6ix stage-tree rule), so the editor and the engine read one tree document.
import registryRaw from '../../../../../engine/fixtures/registry.json?raw';
import treeRaw from '../../../../../engine/fixtures/rule_v2_tree.json?raw';
import type { StrategyRegistry } from './registry';
import {
  cellBuys,
  cellEntryFor,
  metricCond,
  newLine,
  newStage,
  removeTreeName,
  renameTreeName,
  ruleDocFromJson,
  ruleDocToJson,
  type RuleDoc,
} from './ruleDoc';
import { validateRuleDoc } from './validate';

const reg = JSON.parse(registryRaw) as StrategyRegistry;
const TREE = JSON.parse(treeRaw) as Record<string, unknown>;
const tags = ['volume', 'dump'];

const errorsOf = (f: (d: RuleDoc) => void): string[] => {
  const d = ruleDocFromJson(TREE);
  f(d);
  return validateRuleDoc(d, reg, tags).errors;
};

describe('the stage-tree document', () => {
  it('round-trips the shared tree rule key for key', () => {
    expect(ruleDocToJson(ruleDocFromJson(TREE))).toEqual(TREE);
  });

  it('refuses half a tree rather than dropping it', () => {
    expect(() => ruleDocFromJson({ phases: [{ name: 'a' }] })).toThrow(/both `phases` and `situations`/);
  });

  it('picks the most specific entry for a cell, as the engine compiles it', () => {
    const t = ruleDocFromJson(TREE).tree!;
    expect(cellEntryFor(t, 'p4a', 'burst')?.phase).toBe('p4a');
    expect(cellEntryFor(t, 'p2', 'burst')?.phase).toBeNull();
    expect(cellEntryFor(t, 'p2', 'leaving')).toBeUndefined();
  });

  it('reads the buy cells, every cell when none are named', () => {
    const d = ruleDocFromJson(TREE);
    expect(cellBuys(d.enter.cells, 'p2', 'leaving')).toBe(true);
    expect(cellBuys(d.enter.cells, 'p1', 'leaving')).toBe(false);
    expect(cellBuys(d.enter.cells, 'p4b', 'shake_light')).toBe(false);
    expect(cellBuys([], 'p1', 'burst')).toBe(true);
    expect(cellBuys([['p2', null]], 'p2', 'burst')).toBe(true);
  });

  it('carries a phase rename to every cell and buy cell', () => {
    const out = JSON.stringify(ruleDocToJson(renameTreeName(ruleDocFromJson(TREE), 0, 'p4a', 'climb')));
    expect(out).not.toContain('p4a');
    expect(out.match(/climb/g)?.length).toBe(4);
  });

  it('drops the cells of a removed situation', () => {
    const d = removeTreeName(ruleDocFromJson(TREE), 1, 'burst');
    expect(d.tree!.cells).toHaveLength(0);
    expect(d.tree!.situations.map((s) => s.name)).not.toContain('burst');
  });
});

describe('tree validation mirrors the engine', () => {
  it('accepts the shared tree rule', () => {
    expect(validateRuleDoc(ruleDocFromJson(TREE), reg, tags)).toEqual({ errors: [], warnings: [] });
  });

  it('refuses a phase that could come back', () => {
    const fall = errorsOf((d) => {
      d.tree!.phases[3].until = [metricCond({ metric: 'm_state.liquidity_sol' }, [[{ operator: '>=', value: 45 }]])];
    });
    expect(fall.join()).toMatch(/can fall/);
    const below = errorsOf((d) => {
      d.tree!.phases[0].until = [metricCond({ metric: 'm_state.age_sec' }, [[{ operator: '<', value: 2 }]])];
    });
    expect(below.join()).toMatch(/use > or >=/);
    const windowed = errorsOf((d) => {
      d.tree!.phases[0].until = [metricCond({ metric: 'm_flow.buy_sol', span: '10s' }, [[{ operator: '>=', value: 2 }]])];
    });
    expect(windowed.join()).toMatch(/can fall/);
  });

  it('needs a catch-all last phase and situation and a when on the others', () => {
    expect(errorsOf((d) => d.tree!.phases[5].until.push(d.tree!.phases[0].until[0])).join()).toMatch(/last phase/);
    expect(errorsOf((d) => (d.tree!.situations[1].when = [])).join()).toMatch(/needs a when/);
  });

  it('refuses a go and a part sell in a cell', () => {
    const go = errorsOf((d) => (d.tree!.cells[0].on[0].go = 'x'));
    expect(go.join()).toMatch(/a cell has no go/);
    const part = errorsOf((d) => (d.tree!.cells[0].on[0].sell = { label: 'b', pct: 50 }));
    expect(part.join()).toMatch(/would sell again/);
  });

  it('refuses a cell two wide entries cover without its own', () => {
    const e = errorsOf((d) => d.tree!.cells.push({ id: 'x', phase: 'p2', situation: null, on: [newLine({ if: d.tree!.cells[0].on[0].if })] }));
    expect(e.join()).toMatch(/p2 x burst gets sell lines from both its row/);
  });

  it('refuses unknown names and a tree beside stages', () => {
    expect(errorsOf((d) => d.enter.cells.push(['p9', '*'])).join()).toMatch(/no phase `p9`/);
    expect(errorsOf((d) => d.stages.push(newStage('main'))).join()).toMatch(/not both/);
    const noTree = ruleDocFromJson(TREE);
    noTree.tree = null;
    expect(validateRuleDoc(noTree, reg, tags).errors.join()).toMatch(/needs a stage tree/);
  });
});
