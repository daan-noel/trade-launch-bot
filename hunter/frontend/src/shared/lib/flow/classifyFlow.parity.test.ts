import { describe, expect, it } from 'vitest';

// Imported straight from the Rust crate — ONE copy of the vectors, so a case can
// never be added to one language's suite alone.
import fixture from '../../../../../engine/fixtures/flow_ix_parity.json';

import { classifyFlowTrades, patternKeysFrom } from './classifyFlow';

/**
 * The shared parity fixture, from the TS side. Its twin is
 * `flow_ix_matches_the_shared_parity_fixture` in
 * `hunter/engine/src/metrics/flow_ix.rs`, which asserts the SAME file against
 * the Rust SSOT the engine actually decides on.
 *
 * Two implementations exist because the chart has to redraw a pattern edit without
 * a backend round trip. That is only safe while they agree, and reading both and
 * concluding "same logic" is not a test: when they drift, a misclassified trade
 * still produces a perfectly plausible vol/non-vol split, so the bug surfaces much
 * later as "the chart disagrees with the metric pane" with no obvious cause.
 *
 * Reads the fixture from the Rust crate deliberately — one copy, so a case can
 * never be added to one language's suite alone.
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
      const classified = classifyFlowTrades(
        c.trades.map((t) => ({
          wallet_address: t.wallet,
          sol: t.sol,
          ix_labels: t.labels,
          side: t.side,
        })),
        {
          patternKeys: patternKeysFrom(c.patterns),
          creatorWallet: c.creator,
        },
      );

      const totals = { tagged_buy: 0, tagged_sell: 0, untagged_buy: 0, untagged_sell: 0 };
      for (const t of classified) {
        // `classifyFlowTrades` reports magnitudes; the side split is the caller's,
        // exactly as `FlowTotals::add` does it on the Rust side.
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
