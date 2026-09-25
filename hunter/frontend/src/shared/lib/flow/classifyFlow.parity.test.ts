import { describe, expect, it } from 'vitest';

// Imported straight from the Rust crate - ONE copy of the vectors, so a case can
// never be added to one language's suite alone.
import fixture from '../../../../../engine/fixtures/flow_ix_parity.json';

import { classifyFlowTrades, type FlowTag } from './classifyFlow';

/**
 * The shared parity fixture, from the TS side. Its twin is
 * `the_shared_parity_fixture` in `hunter/engine/src/metrics/tags/state_tests.rs`,
 * which reads every case as the tag `{ix_shape: patterns, creator: true}` with
 * `sticky` - the v1 flow classifier's rules - and asserts it against the engine.
 *
 * Two implementations exist because the chart redraws a tag edit without a backend
 * round trip. That is only safe while they agree: a misclassified trade still
 * produces a plausible split, so a drift surfaces much later as "the chart disagrees
 * with the metric pane" with no obvious cause.
 */

interface FixtureTrade {
  wallet: string;
  side: 'buy' | 'sell';
  sol: number;
  labels: string[] | null;
}

interface FixtureCase {
  name: string;
  patterns: string[][];
  creator: string | null;
  trades: FixtureTrade[];
  expect: { tagged_buy: number; tagged_sell: number; untagged_buy: number; untagged_sell: number };
}

const { cases } = fixture as unknown as { cases: FixtureCase[] };

describe('classifyFlow matches the shared parity fixture', () => {
  it('loads the fixture the Rust suite asserts', () => {
    expect(cases.length).toBeGreaterThan(0);
  });

  for (const c of cases) {
    it(c.name, () => {
      const tag: FlowTag = {
        name: 'volume',
        match: { ix_shape: c.patterns.map((labels) => ({ labels })), creator: true },
        side: null,
        sticky: true,
        exclude_creation_slot: false,
      };
      const classified = classifyFlowTrades(
        c.trades.map((t) => ({ wallet_address: t.wallet, sol: t.sol, ix_labels: t.labels, side: t.side })),
        { tag, creatorWallet: c.creator },
      );

      const totals = { tagged_buy: 0, tagged_sell: 0, untagged_buy: 0, untagged_sell: 0 };
      for (const t of classified) {
        const key = `${t.isTagged ? 'tagged' : 'untagged'}_${t.side}` as keyof typeof totals;
        totals[key] += t.isTagged ? t.taggedSol : t.untaggedSol;
      }

      expect(totals.tagged_buy).toBeCloseTo(c.expect.tagged_buy, 9);
      expect(totals.tagged_sell).toBeCloseTo(c.expect.tagged_sell, 9);
      expect(totals.untagged_buy).toBeCloseTo(c.expect.untagged_buy, 9);
      expect(totals.untagged_sell).toBeCloseTo(c.expect.untagged_sell, 9);
    });
  }
});
