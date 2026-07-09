import { useEffect, useMemo, useState } from 'react';
import type { ChartSwingLeg, ChartSwingOverlay } from 'components/token-price-chart';
import type { ChartOverlayHook } from 'components/tokens/TokenChartsGrid';
import {
  buildEventMarkers,
  type InspectTarget,
} from 'components/strategy/inspectTarget';
import { fetchSwing1Detect, type Swing1DetectParams } from '@lab/services/swing1Detect';

/**
 * Resolve a token's swing1 leg overlay for the trade-history chart. Shared by the
 * inspect modals (sweep combo / rule) and the per-row charts grid, so an inline
 * chart draws the exact same legs its modal does.
 *
 * `carriedLegs` win when present — the row's sim already resolved them over the
 * same corpus + params, so a re-detect is redundant. Otherwise, with `params`
 * set, it runs the SAME `swing1-detect` funnel the sim ran (all trades, full
 * history — `curveOnly:false`, matching the backtest's venue-agnostic
 * `find_by_mints_all`) so the legs match this row's entry/exit. The fetch is
 * best-effort: on failure the overlay is dropped and the chart still shows
 * entry/exit. Returns `null` while loading / when there are no legs.
 */
export function useSwing1DetectOverlay(
  mint: string,
  params: Swing1DetectParams | null,
  carriedLegs?: ChartSwingLeg[] | null,
): ChartSwingOverlay | null {
  const [legs, setLegs] = useState<ChartSwingLeg[] | null>(null);

  useEffect(() => {
    if (carriedLegs && carriedLegs.length) {
      setLegs(carriedLegs);
      return;
    }
    if (!mint || !params) {
      setLegs(null);
      return;
    }
    let cancelled = false;
    setLegs(null);
    fetchSwing1Detect(mint, params, { startMs: null, endMs: null, curveOnly: false })
      .then((res) => {
        if (!cancelled) setLegs(res.legs as unknown as ChartSwingLeg[]);
      })
      .catch(() => {
        if (!cancelled) setLegs(null);
      });
    return () => {
      cancelled = true;
    };
  }, [mint, params, carriedLegs]);

  return useMemo<ChartSwingOverlay | null>(() => {
    if (!legs || !legs.length) return null;
    return { legs, segmentMode: 'perLeg' as const, perLegFullSpanEnd: true };
  }, [legs]);
}

/**
 * Build a charts-grid overlay hook for a `lab` swing1 table: entry/exit markers
 * plus a swing overlay reconstructed per token via `swing1-detect` (the same
 * funnel the sim ran, keyed off this section's fixed `params`). `legsOf` lets a
 * row that already carries its legs (sim results) skip the fetch. Returns a
 * `use`-prefixed hook — each chart card invokes it once, satisfying the rules of
 * hooks.
 */
export function makeSwing1DetectRowOverlay<R extends { mint_address: string }>(
  toTarget: (r: R) => InspectTarget,
  params: Swing1DetectParams | null,
  legsOf?: (r: R) => ChartSwingLeg[] | null | undefined,
): ChartOverlayHook<R> {
  return function useSwing1RowOverlay(row, mint) {
    const swingOverlay = useSwing1DetectOverlay(mint, params, legsOf?.(row) ?? null);
    return { eventMarkers: buildEventMarkers(toTarget(row)), swingOverlay };
  };
}
