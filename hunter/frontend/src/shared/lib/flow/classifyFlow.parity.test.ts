import { describe, expect, it } from 'vitest';

// Imported straight from the Rust crate — ONE copy of the vectors, so a case can
// never be added to one language's suite alone.
import fixture from '../../../../../engine/fixtures/flow_split_parity.json';

import { classifyFlowTrades, patternKeysFrom } from './classifyFlow';

/**
 * The shared parity fixture, from the TS side. Its twin is
 * `flow_split_matches_the_shared_parity_fixture` in
 * `hunter/engine/src/metrics/flow_split.rs`, which asserts the SAME file against
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
  expect: { vol_buy: number; vol_sell: number; nonvol_buy: number; nonvol_sell: number };
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

      const totals = { vol_buy: 0, vol_sell: 0, nonvol_buy: 0, nonvol_sell: 0 };
      for (const t of classified) {
        // `classifyFlowTrades` reports magnitudes; the side split is the caller's,
        // exactly as `FlowTotals::add` does it on the Rust side.
        const key = `${t.isVol ? 'vol' : 'nonvol'}_${t.side}` as keyof typeof totals;
        totals[key] += t.isVol ? t.volSol : t.nonVolSol;
      }

      expect(totals.vol_buy).toBeCloseTo(c.expect.vol_buy, 9);
      expect(totals.vol_sell).toBeCloseTo(c.expect.vol_sell, 9);
      expect(totals.nonvol_buy).toBeCloseTo(c.expect.nonvol_buy, 9);
      expect(totals.nonvol_sell).toBeCloseTo(c.expect.nonvol_sell, 9);
    });
  }
});
