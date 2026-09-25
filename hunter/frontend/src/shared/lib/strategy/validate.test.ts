import { describe, expect, it } from 'vitest';

// The engine's own registry document, kept current by the engine test
// `registry_fixture_is_current`: these tests run against the real vocabulary.
import registryRaw from '../../../../../engine/fixtures/registry.json?raw';
import ruleRaw from '../../../../../engine/fixtures/rule_v2_full.json?raw';
import { checkRef, parseSpan, refLabel, spanKeys } from './metricRef';
import type { StrategyRegistry } from './registry';
import {
  emptyRuleDoc,
  metricCond,
  newLine,
  newStage,
  renameSignal,
  renameStage,
  ruleDocFromJson,
  ruleDocToJson,
  signalCond,
  type RuleDoc,
} from './ruleDoc';
import { autoLineLabel, condSentence, lineSentence } from './sentences';
import { tagsFromJson, tagsToJson, validateTags, withTagListValue } from './tagsDoc';
import { validateRuleDoc } from './validate';

const reg = JSON.parse(registryRaw) as StrategyRegistry;

/** A rule using every part of the grammar. The engine test
 *  `the_shared_full_rule_fixture_parses_and_round_trips` parses the same file, so the
 *  editor's model and the engine read and write one document. */
const FULL = JSON.parse(ruleRaw) as {
  stages: { name: string; then?: string }[];
};

describe('the rule document', () => {
  it('round-trips a full v2 rule key for key', () => {
    expect(ruleDocToJson(ruleDocFromJson(FULL))).toEqual(FULL);
  });

  it('refuses a format-1 rule rather than dropping its parts', () => {
    expect(() => ruleDocFromJson({ entry: { m_state: { time: [] } }, exit: {} })).toThrow(/format-1/);
  });

  it('carries a stage rename to every go and then', () => {
    const d = renameStage(ruleDocFromJson(FULL), 'late', 'hold');
    const out = ruleDocToJson(d) as typeof FULL;
    expect(out.stages[0].then).toBe('hold');
    expect(out.stages[1].name).toBe('hold');
    expect(out.stages[2].then).toBe('hold');
  });

  it('carries a signal rename to every line that uses it', () => {
    const d = renameSignal(ruleDocFromJson(FULL), 'cashout', 'exit_now');
    const out = JSON.stringify(ruleDocToJson(d));
    expect(out).not.toContain('cashout');
    expect(out.match(/exit_now/g)?.length).toBe(3);
  });
});

describe('validation mirrors the engine', () => {
  const tags = ['volume'];
  it('accepts the full rule', () => {
    expect(validateRuleDoc(ruleDocFromJson(FULL), reg, tags)).toEqual({ errors: [], warnings: [] });
  });

  const withLine = (f: (d: RuleDoc) => void): string[] => {
    const d = ruleDocFromJson(FULL);
    f(d);
    return validateRuleDoc(d, reg, tags).errors;
  };

  it('names a partial sell that does not move', () => {
    const errs = withLine((d) => {
      d.always[0].sell = { label: 'half', pct: 50 };
    });
    expect(errs.join()).toMatch(/partial sell must also go/);
  });

  it('names a line that does nothing', () => {
    expect(withLine((d) => (d.always[0].sell = null)).join()).toMatch(/does nothing/);
  });

  it('names a position read before the buy', () => {
    const errs = withLine((d) => d.enter.filters.push(metricCond({ metric: 'm_position.pnl_pct' }, [[{ operator: '>=', value: 1 }]])));
    expect(errs.join()).toMatch(/reads our position/);
  });

  it('names a deadline stage with nowhere to go', () => {
    const errs = withLine((d) => {
      d.stages[2].then = null;
    });
    expect(errs.join()).toMatch(/no stage follows/);
  });

  it('names a missing target and a missing signal', () => {
    const errs = withLine((d) => {
      d.always[0].go = 'nowhere';
      d.stages[1].on[0].if = [signalCond('nope')];
    });
    expect(errs.join()).toMatch(/no stage `nowhere`/);
    expect(errs.join()).toMatch(/no signal `nope`/);
  });

  it('names an expression that can never hold', () => {
    const errs = withLine((d) => d.enter.filters.push(metricCond({ metric: 'm_state.age_sec' }, [[{ operator: '>', value: 30 }, { operator: '<', value: 10 }]])));
    expect(errs.join()).toMatch(/can never hold/);
  });

  it('warns on a tag the fingerprint does not define, and saves', () => {
    const v = validateRuleDoc(ruleDocFromJson(FULL), reg, []);
    expect(v.errors).toEqual([]);
    expect(v.warnings.join()).toMatch(/defines no tag `volume`/);
  });

  it('asks for a value on a fresh condition', () => {
    const d = emptyRuleDoc();
    d.enter.event.push(metricCond({ metric: 'm_state.age_sec' }));
    expect(validateRuleDoc(d, reg).errors.join()).toMatch(/enter a value/);
  });

  it('allows an empty at-deadline line and refuses an empty on line', () => {
    const d = emptyRuleDoc();
    const s = newStage('a');
    s.ends = { basis: 'held_sec', secs: 60 };
    s.at_end.push(newLine());
    s.then = 'a';
    d.stages.push(s);
    expect(validateRuleDoc(d, reg).errors).toEqual([]);
    s.on.push(newLine());
    expect(validateRuleDoc(d, reg).errors.join()).toMatch(/no condition/);
  });
});

describe('a read', () => {
  it('labels exactly like the engine', () => {
    expect(refLabel({ metric: 'm_flow.buy_sol', tag: '!volume', span: '10s' })).toBe('m_flow.buy_sol @!volume [10s]');
    expect(refLabel({ metric: 'm_flow.slice_sol_share_pct', span: '30s', slice: '2s' })).toBe('m_flow.slice_sol_share_pct [30s, slice 2s]');
    expect(refLabel({ metric: 'm_crowd.buyer_count', span: 'age60s' })).toBe('m_crowd.buyer_count [age60s]');
    expect(refLabel({ metric: 'm_state.age_sec' })).toBe('m_state.age_sec');
  });

  it('parses and writes spans back to the same text', () => {
    for (const [span, slice] of [['10s'], ['20sl'], ['5p'], ['10s@2'], ['age60s'], ['30sl@1', '4sl']] as const) {
      const m = parseSpan(span, slice);
      expect(typeof m).not.toBe('string');
      if (typeof m !== 'string') expect(spanKeys(m)).toEqual(slice ? { span, slice } : { span });
    }
    expect(parseSpan('30s', '2sl')).toMatch(/span's unit/);
    expect(parseSpan('2s', '30s')).toMatch(/wider/);
  });

  it('checks tags and spans against what the metric accepts', () => {
    expect(checkRef(reg, { metric: 'm_state.age_sec', span: '10s' })).toMatch(/takes no window/);
    expect(checkRef(reg, { metric: 'm_flow.buy_tx_count' })).toMatch(/needs a tag/);
    expect(checkRef(reg, { metric: 'm_state.age_sec', tag: 'volume' })).toMatch(/takes no tag/);
    expect(checkRef(reg, { metric: 'm_holdings.bag_share_pct', tag: 'volume' })).toMatch(/wallet class/);
    expect(checkRef(reg, { metric: 'm_slot.buy_count', tag: '!working' })).toMatch(/tagged trades only/);
    expect(checkRef(reg, { metric: 'm_crowd.buyer_count' })).toMatch(/since-age span/);
    expect(checkRef(reg, { metric: 'm_flow.slice_sol_share_pct', span: '30s' })).toMatch(/needs a slice/);
    expect(checkRef(reg, { metric: 'm_flow.buy_sol', tag: '!volume', span: '10s' })).toBeNull();
  });
});

describe('sentences', () => {
  it('writes a condition in words from the registry', () => {
    const c = metricCond({ metric: 'm_flow.buy_sol', tag: '!volume', span: '10s' }, [[{ operator: '>=', value: 2 }]]);
    expect(condSentence(reg, c)).toBe('SOL bought (trades without `volume`) in the last 10 s is at least 2 SOL');
    const f = metricCond({ metric: 'm_state.on_curve' }, [[{ operator: '=', value: 1 }]]);
    expect(condSentence(reg, f)).toBe('the coin is still on the curve: yes');
  });

  it('labels an unlabelled line the way the engine does', () => {
    const l = newLine({
      if: [metricCond({ metric: 'm_flow.buy_sol', tag: '!volume', span: '10s' }, [[{ operator: '>=', value: 2 }]])],
    });
    expect(autoLineLabel(l)).toBe('m_flow.buy_sol @!volume [10s] >= 2');
    expect(lineSentence(reg, { ...l, go: 'ride' })).toContain('then go to ride');
  });
});

describe('tags', () => {
  it('round-trips and drops matchers that classify nothing', () => {
    const doc = {
      volume: { match: { program: ['Axiom Trade'], creator: true, cluster: { min_prints: 3, sol_tol_pct: 10 } }, sticky: true },
      dump: { match: { ix_shape: [['Pump.Fun: Sell']], wallet: [] }, side: 'sell' },
    };
    const out = tagsToJson(tagsFromJson(doc));
    expect(out.volume).toEqual(doc.volume);
    expect(out.dump).toEqual({ match: { ix_shape: [['Pump.Fun: Sell']] }, side: 'sell' });
  });

  it('adds to a tag through the one writer, keeping the rest', () => {
    const doc = withTagListValue({ volume: { match: { creator: true }, sticky: true } }, 'volume', 'program', 'Axiom Trade');
    expect(doc).toEqual({ volume: { match: { program: ['Axiom Trade'], creator: true }, sticky: true } });
    expect(withTagListValue(doc, 'targets', 'wallet', 'w1')).toMatchObject({ targets: { match: { wallet: ['w1'] } } });
  });

  it('names every mistake the engine refuses', () => {
    const errs = validateTags(
      tagsFromJson({
        Volume: { match: { creator: true } },
        bundled: { match: { creator: true } },
        empty: { match: {} },
        t: { match: { ix_template: ['Axiom Trade'] } },
        c: { match: { cluster: { min_prints: 1, sol_tol_pct: 5 } } },
        m: { match: { ix_contains: ['Nope'] } },
      }),
      reg,
    ).join('\n');
    for (const want of ['a-z', 'built-in', 'no matcher', 'not a template', 'at least 2', 'unknown ix marker']) {
      expect(errs).toContain(want);
    }
  });
});
