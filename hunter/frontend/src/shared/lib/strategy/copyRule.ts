import type { StrategyRule } from 'lib/strategy/types';
import { familyName, findFamily, findMetric, type FamilySpec, type StrategyRegistry } from 'lib/strategy/registry';
import { ruleDocFromJson, type RuleDoc } from 'lib/strategy/ruleDoc';

/** A copy rule stores `copy: true` beside the episode. `{ min_buy_sol }` is an old shortcut the editor still opens. */
export function isCopyRule(rule: Pick<StrategyRule, 'params'>): boolean {
  const copy = rule.params.copy;
  return copy === true || (copy != null && typeof copy === 'object');
}

const isObj = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v);

/**
 * The episode a `{ copy: { min_buy_sol } }` shortcut expands to. Mirrors
 * `CopySpec::expand`: his buy on this print, from a flat bag, then each of his
 * sells sells that percent of what we still hold.
 */
export function copyEpisodeParams(minBuySol: number): Record<string, unknown> {
  const buy = {
    metric: 'm_flow.buy_sol',
    tag: 'targets',
    span: '1p',
    is: [{ operator: '>=', value: minBuySol }],
  };
  const flat = {
    metric: 'm_print.flat_before',
    tag: 'targets',
    is: [{ operator: '=', value: 1 }],
  };
  const mirror = (go: string) => ({
    if: [{ metric: 'm_print.sold_bag_pct', tag: 'targets', is: [{ operator: '>', value: 0 }] }],
    sell: 'his sell',
    sell_of: 'bag',
    go,
  });
  return {
    enter: { event: [buy, flat] },
    stages: [
      { name: 'a', on: [mirror('b')] },
      { name: 'b', on: [mirror('a')] },
    ],
    reentry: { cooldown_sec: 0, max_per_coin: 100 },
  };
}

/** Params the rule editor and the chain column read. A shortcut expands; a saved episode is itself. */
export function copyViewParams(raw: unknown): unknown {
  if (!isObj(raw)) return raw;
  const copy = raw.copy;
  if (!isObj(copy)) return raw;
  const n = copy.min_buy_sol;
  return typeof n === 'number' && Number.isFinite(n) ? copyEpisodeParams(n) : raw;
}

/** The document a new copy rule opens on. */
export function copyEpisodeDoc(minBuySol = 0.04): RuleDoc {
  return { ...ruleDocFromJson(copyEpisodeParams(minBuySol)), follow: 'bought' };
}

/** Stamp the episode so the save stays on the Copy page. */
export function withCopyFlag(params: Record<string, unknown>): Record<string, unknown> {
  return { ...params, copy: true };
}

/** Families the Copy editor offers in full. Copy readings (`m_print`) are added after. */
const COPY_FAMILIES = ['m_state', 'm_price', 'm_flow', 'm_position'] as const;

/**
 * The catalog the Copy editor picks from: pool state, price, flow, and position,
 * plus the copy readings (`m_print.flat_before`, `m_print.sold_bag_pct`).
 */
export function copyEditorRegistry(reg: StrategyRegistry): StrategyRegistry {
  const byFamily = new Map<string, FamilySpec['metrics']>();
  const push = (path: string) => {
    const spec = findMetric(reg, path) ?? reg.copy?.find((m) => m.path === path);
    if (!spec) return;
    const name = familyName(path);
    const list = byFamily.get(name) ?? [];
    if (!list.some((m) => m.path === path)) list.push(spec);
    byFamily.set(name, list);
  };
  for (const name of COPY_FAMILIES) {
    const fam = findFamily(reg, name);
    if (!fam) continue;
    for (const m of fam.metrics) push(m.path);
  }
  for (const m of reg.copy ?? []) push(m.path);
  const families: FamilySpec[] = [...byFamily.entries()].map(([name, metrics]) => {
      const fam = findFamily(reg, name);
      return {
        name,
        title: fam?.title ?? name,
        summary: fam?.summary ?? '',
        example: fam?.example ?? '',
        metrics,
      };
    });
  return { ...reg, families, copy: [] };
}
