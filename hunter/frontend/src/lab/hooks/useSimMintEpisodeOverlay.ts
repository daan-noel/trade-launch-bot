import { useEffect, useMemo, useRef, useState } from 'react';

import {
  buildEventMarkersForEpisodes,
  inspectFromSim,
  type InspectTarget,
} from 'components/strategy/inspectTarget';
import { simColumns } from 'components/strategy/strategyColumns';
import { tokenAmountColKeys, tokenNumericColKeys } from 'components/tokens/sharedTokenColumns';
import type { RowChartOverlay } from 'components/tokens/TokenChartsGrid';
import type { ChartEventMarker } from 'components/token-price-chart';
import { fetchEngineSimPage } from 'services/api';
import { toTableRequest, type TableRequestBody } from 'services/tableRequest';
import type { SimulatedTokenResult } from 'types';

const SIM_NUMERIC_COLS = tokenNumericColKeys(simColumns);
const SIM_AMOUNT_COLS = tokenAmountColKeys(simColumns);

/** Fired (entered) episodes only — `NoEntry` rows contribute no fill markers. */
function firedTargets(rows: SimulatedTokenResult[]): InspectTarget[] {
  return rows
    .filter((r) => r.fired !== false && r.exit_reason !== 'NoEntry')
    .map(inspectFromSim);
}

/** Table body that pages every sim-result row for one mint (re-entry episodes). */
function mintEpisodeBody(mint: string): TableRequestBody {
  return toTableRequest(
    {
      page: 1,
      pageSize: 1000,
      sortKeys: [],
      search: '',
      colFilters: {},
      structuredFilters: { mint_address: { op: 'in', val: [mint] } },
    },
    SIM_NUMERIC_COLS,
    { amountCols: SIM_AMOUNT_COLS },
  ) as TableRequestBody;
}

/**
 * Fetch every fired episode for `mint` from a simulate/dry-run result and build
 * the union of their fill entry/exit markers. Returns `null` on abort/failure so
 * callers can fall back to a single-episode or page-scoped set.
 */
export async function fetchSimEpisodeMarkers(
  runOrRuleId: string,
  mint: string,
  signal?: AbortSignal,
): Promise<ChartEventMarker[] | null> {
  try {
    const page = await fetchEngineSimPage(runOrRuleId, mintEpisodeBody(mint), signal);
    if (signal?.aborted) return null;
    return buildEventMarkersForEpisodes(firedTargets(page.items));
  } catch (e) {
    if (signal?.aborted || (e instanceof DOMException && e.name === 'AbortError')) return null;
    return null;
  }
}

/**
 * Hook-based `chartsGroupByMint` overlay for a server-paged sim/dry-run positions
 * table. The table is NOT sorted by mint, so re-entry episodes of one mint almost
 * never co-occur on the same page — fetch every row for this mint from the result
 * endpoint so the chart overlays every episode. `pageRows` seeds the chart
 * immediately and is the fallback if the fetch fails.
 */
export function useSimMintEpisodeOverlay(
  runOrRuleId: string,
  mint: string,
  pageRows: SimulatedTokenResult[],
): RowChartOverlay {
  const [markers, setMarkers] = useState<ChartEventMarker[] | null>(null);
  useEffect(() => {
    setMarkers(null);
    const ctrl = new AbortController();
    void fetchSimEpisodeMarkers(runOrRuleId, mint, ctrl.signal).then((m) => {
      if (!ctrl.signal.aborted) setMarkers(m);
    });
    return () => ctrl.abort();
  }, [runOrRuleId, mint]);

  const fallback = useMemo(
    () => buildEventMarkersForEpisodes(firedTargets(pageRows)),
    [pageRows],
  );
  return { eventMarkers: markers ?? fallback };
}

/**
 * All re-entry episode markers for the inspect modal. Null while loading / on
 * failure ⇒ the modal falls back to building markers from the clicked `target`.
 */
export function useSimInspectEpisodeMarkers(
  runOrRuleId: string | null,
  target: InspectTarget | null,
): ChartEventMarker[] | null {
  const [markers, setMarkers] = useState<ChartEventMarker[] | null>(null);
  const targetRef = useRef(target);
  targetRef.current = target;
  const mint = target?.mint_address ?? null;
  useEffect(() => {
    setMarkers(null);
    if (!runOrRuleId || !mint) return;
    const ctrl = new AbortController();
    void fetchSimEpisodeMarkers(runOrRuleId, mint, ctrl.signal).then((m) => {
      if (ctrl.signal.aborted) return;
      const fb = targetRef.current;
      if (m && m.length > 0) setMarkers(m);
      else if (fb) setMarkers(buildEventMarkersForEpisodes([fb]));
      else setMarkers(null);
    });
    return () => ctrl.abort();
  }, [runOrRuleId, mint]);
  return markers;
}
