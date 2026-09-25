import { describe, expect, it } from 'vitest';
import type { TagMatch } from 'lib/strategy/tagsDoc';
import {
  classifyFlowTrades,
  flowReasonsById,
  patternKeysFrom,
  type FlowTag,
  type FlowTradeLite,
} from './classifyFlow';

/** Cases mirror `hunter/engine/src/metrics/tags/state_tests.rs` one for one: the
 *  decision order and the forward-only state asserted here must stay in lockstep
 *  with `TagState::fold_half`. */

const tag = (match: TagMatch, opts: Partial<FlowTag> = {}): FlowTag => ({
  name: 't',
  match,
  side: null,
  sticky: false,
  exclude_creation_slot: false,
  ...opts,
});

const tr = (
  wallet: string,
  sol: number,
  labels: string[] | null,
  extra: Partial<FlowTradeLite> = {},
): FlowTradeLite => ({ wallet_address: wallet, sol, ix_labels: labels, side: 'buy', ...extra });

const CREATE_BUY = ['Pump.Fun: Create', 'Pump.Fun: Buy'];

describe('classifyFlowTrades: matchers', () => {
  it('shape, sticky, creator and a missing ix (shape_sticky_creator_and_missing_ix)', () => {
    const t = tag({ ix_shape: [{ labels: CREATE_BUY }], creator: true }, { sticky: true });
    const out = classifyFlowTrades(
      [
        tr('creator', 1, null),
        tr('bot1', 2, CREATE_BUY),
        tr('bot1', 1, null, { side: 'sell' }),
        tr('normie', 1, null),
        tr('n2', 1, ['Pump.Fun: Buy']),
      ],
      { tag: t, creatorWallet: 'creator' },
    );
    expect(out.map((x) => x.reason)).toEqual(['creator', 'ix_shape', 'sticky', null, null]);
    expect(out.map((x) => x.half)).toEqual(['tagged', 'tagged', 'tagged', 'rest', 'rest']);
  });

  it('without sticky a later trade of a matched wallet is judged alone', () => {
    const t = tag({ ix_shape: [{ labels: ['vol'] }] });
    const out = classifyFlowTrades([tr('w', 4, ['vol']), tr('w', 6, ['other'])], { tag: t });
    expect(out.map((x) => x.isTagged)).toEqual([true, false]);
    expect(out[1]).toMatchObject({ taggedSol: 0, untaggedSol: 6 });
  });

  it('ix_contains catches a shape no list has seen', () => {
    const labels = ['Compute Budget: SetComputeUnitLimit', 'System Program: CreateAccountWithSeed', 'Pump.Fun: Buy'];
    const byShape = tag({ ix_shape: [{ labels: ['System Program: CreateAccountWithSeed', 'Pump.Fun: Buy'] }] });
    const byMarker = tag({ ix_contains: ['CreateAccountWithSeed'] });
    expect(classifyFlowTrades([tr('w', 1, labels)], { tag: byShape })[0].isTagged).toBe(false);
    expect(classifyFlowTrades([tr('w', 1, labels)], { tag: byMarker })[0].reason).toBe('ix_contains');
  });

  it('ix_lacks tags everything without the marker, label-less trades included', () => {
    const t = tag({ ix_lacks: ['Axiom Trade'] });
    const out = classifyFlowTrades([tr('a', 1, ['Axiom Trade: Buy']), tr('b', 1, null)], { tag: t });
    expect(out.map((x) => x.isTagged)).toEqual([false, true]);
  });

  it('without creator and sticky a tag reads the transaction alone', () => {
    const t = tag({ ix_contains: ['CreateAccountWithSeed'] });
    const out = classifyFlowTrades(
      [tr('dev', 1, ['Pump.Fun: Buy']), tr('dev', 1, ['System Program: CreateAccountWithSeed'])],
      { tag: t, creatorWallet: 'dev' },
    );
    expect(out.map((x) => x.isTagged)).toEqual([false, true]);
  });

  it('a sell-side tag never takes a buy; the creator matcher takes the creator sells', () => {
    const t = tag({ ix_shape: [{ labels: ['Pump.Fun: Sell'] }], creator: true }, { side: 'sell' });
    const out = classifyFlowTrades(
      [
        tr('a', 0.5, ['Pump.Fun: Sell'], { side: 'sell' }),
        tr('b', 3, ['Pump.Fun: Sell'], { side: 'buy' }),
        tr('c', 9, ['Other: Sell'], { side: 'sell' }),
        tr('dev', 1, null, { side: 'sell' }),
      ],
      { tag: t, creatorWallet: 'dev' },
    );
    expect(out.map((x) => x.reason)).toEqual(['ix_shape', null, null, 'creator']);
  });

  it('a wallet list reads both sides of a named wallet, never an empty one', () => {
    const t = tag({ wallet: ['Target111'] });
    const out = classifyFlowTrades(
      [tr('Target111', 1, null), tr('Target111', 0.4, null, { side: 'sell' }), tr('someone', 5, null), tr('', 5, null)],
      { tag: t },
    );
    expect(out.map((x) => x.isTagged)).toEqual([true, true, false, false]);
  });

  it('a program tags every variant it ships', () => {
    const t = tag({ program: ['Axiom Trade'] });
    const out = classifyFlowTrades(
      [tr('a', 1, ['Axiom Trade: new variant']), tr('b', 1, ['Photon: Buy']), tr('c', 1, null)],
      { tag: t },
    );
    expect(out.map((x) => x.reason)).toEqual(['program', null, null]);
  });

  it('an ix template matches its grain, not the exact sequence', () => {
    const t = tag({ ix_template: ['Axiom Trade|CU'] });
    const out = classifyFlowTrades(
      [
        tr('a', 1, ['Compute Budget: SetComputeUnitLimit', 'Axiom Trade: ix#00']),
        tr('b', 1, ['Compute Budget: SetComputeUnitPrice', 'Axiom Trade: ix#01']),
        tr('c', 1, ['Axiom Trade: ix#00']),
      ],
      { tag: t },
    );
    expect(out.map((x) => x.reason)).toEqual(['ix_template', 'ix_template', null]);
  });

  it('a pinned shape matches only the tx that carries that budget', () => {
    const t = tag({ ix_shape: [{ labels: ['A'], cu_limit: 300_000 }] });
    const out = classifyFlowTrades(
      [tr('a', 1, ['A'], { cu_limit: 300_000 }), tr('b', 1, ['A'], { cu_limit: 200_000 }), tr('c', 1, ['A'])],
      { tag: t },
    );
    expect(out.map((x) => x.isTagged)).toEqual([true, false, false]);
  });
});

describe('classifyFlowTrades: cluster', () => {
  const crew = ['Axiom Trade: ix#00'];
  it('tags from the Nth close member (a_cluster_tags_from_the_nth_close_member)', () => {
    const t = tag({ cluster: { min_prints: 3, sol_tol_pct: 10 } });
    const out = classifyFlowTrades(
      [
        tr('a', 1.0, crew, { slot: 7 }),
        tr('b', 1.05, crew, { slot: 7 }),
        tr('c', 2.0, crew, { slot: 7 }),
        tr('d', 0.95, crew, { slot: 7 }),
        tr('e', 1.0, crew, { slot: 8 }),
        tr('f', 1.0, crew, { slot: 8, cu_limit: 200_000 }),
      ],
      { tag: t },
    );
    expect(out.map((x) => x.isTagged)).toEqual([false, false, false, true, false, false]);
    expect(out[3].reason).toBe('cluster');
  });

  it('runs last: a trade another matcher takes does not count into its group', () => {
    const t = tag({ wallet: ['w'], cluster: { min_prints: 2, sol_tol_pct: 0 } });
    const out = classifyFlowTrades(
      [tr('w', 1, crew, { slot: 1 }), tr('x', 1, crew, { slot: 1 }), tr('y', 1, crew, { slot: 1 })],
      { tag: t },
    );
    expect(out.map((x) => x.reason)).toEqual(['wallet', null, 'cluster']);
  });
});

describe('classifyFlowTrades: exclude_creation_slot', () => {
  it('creation-slot buyers count on neither side (creation_slot_buyers_are_excluded_from_both_halves)', () => {
    const t = tag({ creator: true }, { sticky: true, exclude_creation_slot: true });
    const out = classifyFlowTrades(
      [
        tr('dev', 0.5, CREATE_BUY, { slot: 100 }),
        tr('bundle', 2, ['Pump.Fun: Buy'], { slot: 100 }),
        tr('bundle', 1, ['Pump.Fun: Sell'], { slot: 105, side: 'sell' }),
        tr('retail', 3, ['Pump.Fun: Buy'], { slot: 105 }),
      ],
      { tag: t, creatorWallet: 'dev' },
    );
    expect(out.map((x) => x.half)).toEqual(['tagged', 'excluded', 'excluded', 'rest']);
    expect(out[1]).toMatchObject({ reason: 'creation_slot', taggedSol: 0, untaggedSol: 0 });
  });
});

describe('classifyFlowTrades: lens exclusions', () => {
  it('an excluded wallet is the rest and moves no state', () => {
    const t = tag({ ix_shape: [{ labels: ['A'] }] }, { sticky: true });
    const out = classifyFlowTrades([tr('me', 1, ['A']), tr('me', 2, ['Z'])], {
      tag: t,
      excludeWallets: new Set(['me']),
    });
    expect(out.map((x) => x.half)).toEqual(['rest', 'rest']);
  });
});

describe('flowReasonsById', () => {
  it('keys tagged and excluded trades by id and omits the rest', () => {
    const t = tag({ ix_shape: [{ labels: ['A'] }] });
    const map = flowReasonsById(
      [
        { id: '1', ...tr('w1', 1, ['A']) },
        { id: '2', ...tr('w2', 1, ['Z']) },
      ],
      { tag: t },
    );
    expect([...map]).toEqual([['1', 'ix_shape']]);
  });
});

describe('patternKeysFrom', () => {
  it('drops empty pattern arrays and de-dupes by content', () => {
    expect(patternKeysFrom([['a'], [], ['a']])).toEqual(new Set([JSON.stringify(['a'])]));
  });
});
