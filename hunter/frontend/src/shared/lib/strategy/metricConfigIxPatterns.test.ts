import { describe, expect, it } from 'vitest';

import {
  dumpPatternsFromConfig,
  flowClassifierFromConfig,
  flowWalletRules,
  metricConfigWithDumpPatterns,
  metricConfigWithList,
  patternsForList,
  ixPatternsFromConfig,
  metricConfigWithFlowClassifier,
  metricConfigWithIxPatterns,
  withFlowWalletRules,
  metricConfigWithWorkingTemplates,
  workingTemplatesFromConfig,
  BURST_GROUP,
  type FlowClassifier,
} from './registry';

/** The writer spells both wallet rules ALWAYS — a row that omits them says nothing
 *  about which classifier it meant, which is how they came to be reverted unnoticed. */
const DEFAULT_RULES = { wallet_contagion: true, creator_is_tagged: true };

/** A PUT replaces the fingerprint row, so whatever this helper omits is DELETED.
 *
 *  It used to rebuild `m_flow_ix` from the pattern rows alone, which reverted
 *  `wallet_contagion` and `creator_is_tagged` to their `true` backend defaults on any
 *  save — and those defaults are a different classifier, not a tighter one. The form
 *  renders no control for either flag at the time, so the revert was invisible.
 */
describe('metricConfigWithIxPatterns', () => {
  const patterns = [['Pump.Fun: Sell', 'Token Program: CloseAccount']];

  it('keeps the other m_flow_ix keys, not just the other groups', () => {
    const prev = {
      m_flow_ix: {
        ix_patterns: [['old']],
        wallet_contagion: false,
        creator_is_tagged: false,
        untagged_ix_markers: ['Axiom Trade'],
      },
      m_state: { something: 1 },
    };
    const out = metricConfigWithIxPatterns(patterns, prev);
    expect(out.m_flow_ix).toEqual({
      ix_patterns: patterns,
      wallet_contagion: false,
      creator_is_tagged: false,
      untagged_ix_markers: ['Axiom Trade'],
    });
    expect(out.m_state).toEqual({ something: 1 });
  });

  it('round-trips through the reader', () => {
    const out = metricConfigWithIxPatterns(patterns, {});
    expect(ixPatternsFromConfig(out)).toEqual(patterns);
  });

  it('trims and drops empty rows and labels', () => {
    const out = metricConfigWithIxPatterns([['  A  ', ''], [], ['B']], {});
    expect(out.m_flow_ix).toEqual({ ...DEFAULT_RULES, ix_patterns: [['A'], ['B']] });
  });

  it('drops a PATTERN classifier when no pattern survives, keeping the other groups', () => {
    const out = metricConfigWithIxPatterns([[], ['   ']], {
      m_flow_ix: { ix_patterns: [['old']], wallet_contagion: false },
      m_state: { something: 1 },
    });
    expect(out).toEqual({ m_state: { something: 1 } });
  });

  /** The bug this pins deleted a live classifier on every save.
   *
   *  A MARKER classifier legitimately carries no patterns — the backend rejects
   *  `untagged_ix_markers` alongside `ix_patterns`, so an empty list is the only shape
   *  it can have. Dropping the group on an empty pattern list therefore wiped the whole
   *  `m_flow_ix` key whenever a pattern-only surface saved the fingerprint, and every
   *  rule bound to it silently fell to `NaN` on every flow metric. */
  it('keeps a MARKER classifier when the pattern list is empty', () => {
    const marker = {
      m_flow_ix: {
        untagged_ix_markers: ['Axiom Trade'],
        wallet_contagion: false,
        creator_is_tagged: false,
      },
      m_state: { something: 1 },
    };
    expect(metricConfigWithIxPatterns([], marker)).toEqual(marker);
  });

  it('defaults to an empty base, so a caller with nothing to preserve is unchanged', () => {
    expect(metricConfigWithIxPatterns(patterns)).toEqual({
      m_flow_ix: { ...DEFAULT_RULES, ix_patterns: patterns },
    });
  });

  it('ignores a malformed m_flow_ix rather than spreading it', () => {
    for (const bad of [null, 'x', 42, [['a']]]) {
      const out = metricConfigWithIxPatterns(patterns, { m_flow_ix: bad });
      expect(out.m_flow_ix).toEqual({ ...DEFAULT_RULES, ix_patterns: patterns });
    }
  });
});

/** The one writer, exercised on the shapes only the full editor can produce. */
describe('metricConfigWithFlowClassifier', () => {
  const base: FlowClassifier = {
    configured: true,
    ix_patterns: [],
    markers: [],
    markers_side: 'tagged',
    wallet_contagion: true,
    creator_is_tagged: true,
  };

  it('writes a marker classifier under the key for its side', () => {
    expect(
      metricConfigWithFlowClassifier({}, {
        ...base,
        markers: ['Axiom Trade', 'Photon'],
        markers_side: 'untagged',
        wallet_contagion: false,
        creator_is_tagged: false,
      }),
    ).toEqual({
      m_flow_ix: {
        untagged_ix_markers: ['Axiom Trade', 'Photon'],
        wallet_contagion: false,
        creator_is_tagged: false,
      },
    });
  });

  /** The backend rejects a row carrying both masks — they name opposite sides of one
   *  split — so switching sides must REMOVE the other key, not leave it for the save to
   *  fail on. */
  it('removes the other side rather than carrying both masks', () => {
    const prev = metricConfigWithFlowClassifier({}, {
      ...base,
      markers: ['Photon'],
      markers_side: 'untagged',
    });
    const flipped = metricConfigWithFlowClassifier(prev, {
      ...base,
      markers: ['Memo Program'],
      markers_side: 'tagged',
    });
    expect(flipped.m_flow_ix).toEqual({
      tagged_ix_markers: ['Memo Program'],
      ...DEFAULT_RULES,
    });
  });

  it('carries across m_flow_ix keys this model does not own', () => {
    const out = metricConfigWithFlowClassifier(
      { m_flow_ix: { future_key: 7 }, m_state: { x: 1 } },
      { ...base, ix_patterns: [['A']] },
    );
    expect(out).toEqual({
      m_state: { x: 1 },
      m_flow_ix: { future_key: 7, ix_patterns: [['A']], ...DEFAULT_RULES },
    });
  });

  it('drops the group when the classifier is unconfigured', () => {
    expect(
      metricConfigWithFlowClassifier(
        { m_flow_ix: { ix_patterns: [['A']] }, m_state: { x: 1 } },
        { ...base, configured: false },
      ),
    ).toEqual({ m_state: { x: 1 } });
  });
});

/** The reader is the writer's inverse, on every shape the editor can produce. */
describe('flowClassifierFromConfig', () => {
  it('round-trips every classifier shape', () => {
    const shapes: FlowClassifier[] = [
      {
        configured: true,
        ix_patterns: [{ labels: ['A', 'B'] }],
        markers: [],
        markers_side: 'tagged',
        wallet_contagion: true,
        creator_is_tagged: false,
      },
      // A pinned row is a classifier shape like any other, and must survive the
      // round trip with its budget — a reader that dropped it would widen the fire
      // set on the next save with nothing on screen to say so.
      {
        configured: true,
        ix_patterns: [
          { labels: ['A', 'B'], cu_limit: 300_000, cu_price: 3_333_333 },
          { labels: ['C'] },
        ],
        markers: [],
        markers_side: 'tagged',
        wallet_contagion: true,
        creator_is_tagged: true,
      },
      {
        configured: true,
        ix_patterns: [],
        markers: ['Photon'],
        markers_side: 'untagged',
        wallet_contagion: false,
        creator_is_tagged: false,
      },
      {
        configured: true,
        ix_patterns: [],
        markers: ['CreateAccountWithSeed'],
        markers_side: 'tagged',
        wallet_contagion: false,
        creator_is_tagged: true,
      },
    ];
    for (const c of shapes) {
      expect(flowClassifierFromConfig(metricConfigWithFlowClassifier({}, c))).toEqual(c);
    }
  });

  it('reads an absent key as unconfigured, not as a classifier that tags nothing', () => {
    expect(flowClassifierFromConfig({}).configured).toBe(false);
    expect(flowClassifierFromConfig({ m_flow_ix: {} }).configured).toBe(true);
  });

  /** The registry carries the engine's own default, so the two cannot drift. */
  it('takes the boolean defaults from the registry when it has one', () => {
    const reg = {
      operators: [],
      groups: [
        {
          name: 'm_flow_ix',
          kind: 'static' as const,
          strict_params: [],
          metrics: [],
          fingerprint_config: [
            { name: 'wallet_contagion', value_type: 'bool', required: false, default: false },
          ],
        },
      ],
    };
    expect(flowClassifierFromConfig({ m_flow_ix: {} }, reg).wallet_contagion).toBe(false);
    // No registry entry falls back to the documented engine default.
    expect(flowClassifierFromConfig({ m_flow_ix: {} }).wallet_contagion).toBe(true);
  });
});

/** Both rules default TRUE in `FlowPatterns::default`, so absent must read as ON.
 *  Reading absent as `false` shows a control saying the opposite of what the engine
 *  does — and these two decide WHICH classifier runs, not how tight it is. */
describe('flowWalletRules', () => {
  it('reads absent as on, matching the backend default', () => {
    for (const cfg of [null, undefined, {}, { m_flow_ix: {} }, { m_flow_ix: { ix_patterns: [] } }]) {
      expect(flowWalletRules(cfg)).toEqual({ wallet_contagion: true, creator_is_tagged: true });
    }
  });

  it('reads an explicit false', () => {
    expect(
      flowWalletRules({ m_flow_ix: { wallet_contagion: false, creator_is_tagged: false } }),
    ).toEqual({ wallet_contagion: false, creator_is_tagged: false });
  });

  it('reads each flag independently', () => {
    expect(flowWalletRules({ m_flow_ix: { wallet_contagion: false } })).toEqual({
      wallet_contagion: false,
      creator_is_tagged: true,
    });
  });

  it('falls back to on for a non-boolean or malformed value', () => {
    expect(flowWalletRules({ m_flow_ix: { wallet_contagion: 'no' } }).wallet_contagion).toBe(true);
    expect(flowWalletRules({ m_flow_ix: [] }).wallet_contagion).toBe(true);
  });
});

describe('withFlowWalletRules', () => {
  const rules = { wallet_contagion: false, creator_is_tagged: false };

  it('writes both flags explicitly, keeping the rest of m_flow_ix', () => {
    const cfg = metricConfigWithIxPatterns([['A']], {
      m_flow_ix: { untagged_ix_markers: ['Axiom Trade'] },
      m_state: { x: 1 },
    });
    expect(withFlowWalletRules(cfg, rules)).toEqual({
      m_state: { x: 1 },
      m_flow_ix: {
        ix_patterns: [['A']],
        untagged_ix_markers: ['Axiom Trade'],
        wallet_contagion: false,
        creator_is_tagged: false,
      },
    });
  });

  it('overwrites a stale saved pair rather than merging it', () => {
    const cfg = metricConfigWithIxPatterns([['A']], {
      m_flow_ix: { ix_patterns: [['old']], wallet_contagion: true, creator_is_tagged: true },
    });
    expect(withFlowWalletRules(cfg, rules).m_flow_ix).toEqual({
      ix_patterns: [['A']],
      wallet_contagion: false,
      creator_is_tagged: false,
    });
  });

  it('has nothing to attach to when nothing configures the group', () => {
    const cfg = metricConfigWithIxPatterns([], { m_state: { x: 1 } });
    expect(withFlowWalletRules(cfg, rules)).toEqual({ m_state: { x: 1 } });
  });

  it('round-trips through the reader', () => {
    const cfg = withFlowWalletRules(metricConfigWithIxPatterns([['A']]), rules);
    expect(flowWalletRules(cfg)).toEqual(rules);
  });
});

/** `m_dump_ix` is its own group with its own list. The two are written by different
 *  functions on purpose: a PUT replaces the row, so a writer that rebuilt one key
 *  from its pattern rows alone would land as a full write and drop the other. */
describe('dump patterns', () => {
  const dump = [['Pump.Fun: Sell', 'Token Program: CloseAccount']];
  const tagged = [['Pump.Fun: Buy']];

  it('reads an absent group as an empty list', () => {
    for (const cfg of [null, undefined, {}, { m_flow_ix: { ix_patterns: tagged } }]) {
      expect(dumpPatternsFromConfig(cfg)).toEqual([]);
    }
  });

  it('writes the dump list without touching the flow classifier', () => {
    const prev = {
      m_flow_ix: { ix_patterns: tagged, wallet_contagion: false, creator_is_tagged: false },
      m_state: { x: 1 },
    };
    const out = metricConfigWithDumpPatterns(prev, dump);
    expect(out.m_dump_ix).toEqual({ ix_patterns: dump });
    expect(out.m_flow_ix).toEqual(prev.m_flow_ix);
    expect(out.m_state).toEqual({ x: 1 });
  });

  it('writes the flow list without touching the dump list', () => {
    const prev = { m_dump_ix: { ix_patterns: dump } };
    const out = metricConfigWithIxPatterns(tagged, prev);
    expect(out.m_dump_ix).toEqual({ ix_patterns: dump });
    expect(dumpPatternsFromConfig(out)).toEqual(dump);
  });

  it('keeps the two lists independent through a round trip', () => {
    let cfg: Record<string, unknown> = {};
    cfg = metricConfigWithList(cfg, tagged, 'tagged');
    cfg = metricConfigWithList(cfg, dump, 'dump');
    expect(patternsForList(cfg, 'tagged')).toEqual(tagged);
    expect(patternsForList(cfg, 'dump')).toEqual(dump);
  });

  // A dev's dump shape is a sell build of a family the flow split already tags, so
  // the same sequence on both lists is the normal configuration, not a conflict.
  // Neither writer may quietly evict it from the other list to "resolve" that.
  it('keeps one build on BOTH lists', () => {
    let cfg: Record<string, unknown> = {};
    cfg = metricConfigWithList(cfg, dump, 'tagged');
    cfg = metricConfigWithList(cfg, dump, 'dump');
    expect(patternsForList(cfg, 'tagged')).toEqual(dump);
    expect(patternsForList(cfg, 'dump')).toEqual(dump);
  });

  it('drops the group when the last pattern goes, so the metrics read NaN not 0', () => {
    const out = metricConfigWithDumpPatterns({ m_dump_ix: { ix_patterns: dump } }, []);
    expect('m_dump_ix' in out).toBe(false);
  });

  it('trims and ignores a malformed group', () => {
    expect(metricConfigWithDumpPatterns({}, [['  A  ', ''], []]).m_dump_ix).toEqual({
      ix_patterns: [['A']],
    });
    for (const bad of [null, 'x', 42, [['a']]]) {
      expect(dumpPatternsFromConfig({ m_dump_ix: bad })).toEqual([]);
    }
  });
});

describe('metricConfigWithWorkingTemplates', () => {
  it('keeps the other groups and other burst fields', () => {
    const prev = {
      m_state: { x: 1 },
      [BURST_GROUP]: { working_templates: ['old'], extra: true },
    };
    const out = metricConfigWithWorkingTemplates(prev, ['Axiom Trade|CU']);
    expect(out.m_state).toEqual({ x: 1 });
    expect(out[BURST_GROUP]).toEqual({ extra: true, working_templates: ['Axiom Trade|CU'] });
    expect(workingTemplatesFromConfig(out)).toEqual(['Axiom Trade|CU']);
  });

  it('drops the group when the list is empty, so burst metrics read NaN not 0', () => {
    const out = metricConfigWithWorkingTemplates(
      { m_state: { x: 1 }, [BURST_GROUP]: { working_templates: ['old'] } },
      [],
    );
    expect(out).toEqual({ m_state: { x: 1 } });
    expect(workingTemplatesFromConfig(out)).toEqual([]);
  });

  it('reads leftover working_programs into the one list and drops that key on write', () => {
    const prev = {
      [BURST_GROUP]: { working_templates: [], working_programs: ['Axiom Trade'] },
    };
    expect(workingTemplatesFromConfig(prev)).toEqual(['Axiom Trade']);
    const out = metricConfigWithWorkingTemplates(prev, ['Axiom Trade']);
    expect(out[BURST_GROUP]).toEqual({ working_templates: ['Axiom Trade'] });
  });
});
