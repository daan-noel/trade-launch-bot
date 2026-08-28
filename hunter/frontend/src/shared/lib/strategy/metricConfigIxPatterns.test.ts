import { describe, expect, it } from 'vitest';

import {
  flowWalletRules,
  ixPatternsFromConfig,
  metricConfigWithIxPatterns,
  withFlowWalletRules,
} from './registry';

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
    expect(out.m_flow_ix).toEqual({ ix_patterns: [['A'], ['B']] });
  });

  it('drops the group when no pattern survives, keeping the other groups', () => {
    const out = metricConfigWithIxPatterns([[], ['   ']], {
      m_flow_ix: { ix_patterns: [['old']], wallet_contagion: false },
      m_state: { something: 1 },
    });
    expect(out).toEqual({ m_state: { something: 1 } });
  });

  it('defaults to an empty base, so a caller with nothing to preserve is unchanged', () => {
    expect(metricConfigWithIxPatterns(patterns)).toEqual({ m_flow_ix: { ix_patterns: patterns } });
  });

  it('ignores a malformed m_flow_ix rather than spreading it', () => {
    for (const bad of [null, 'x', 42, [['a']]]) {
      const out = metricConfigWithIxPatterns(patterns, { m_flow_ix: bad });
      expect(out.m_flow_ix).toEqual({ ix_patterns: patterns });
    }
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

  it('has nothing to attach to when no pattern configures the group', () => {
    const cfg = metricConfigWithIxPatterns([], { m_state: { x: 1 } });
    expect(withFlowWalletRules(cfg, rules)).toEqual({ m_state: { x: 1 } });
  });

  it('round-trips through the reader', () => {
    const cfg = withFlowWalletRules(metricConfigWithIxPatterns([['A']]), rules);
    expect(flowWalletRules(cfg)).toEqual(rules);
  });
});
