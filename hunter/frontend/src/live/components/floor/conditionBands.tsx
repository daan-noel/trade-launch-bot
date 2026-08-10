import { createContext, useContext, useEffect, useMemo, useSyncExternalStore } from 'react';

import type { ChartTimeBand, ChartTimeSpan } from 'components/token-price-chart';
import { createPublishedStore, type PublishedStore } from './publishedStore';

/**
 * The rule-condition timeline, travelling from the condition strip **up** to the
 * chart that draws it as bottom-pane lanes.
 *
 * The mirror image of the crosshair channel, and it exists for the same reason:
 * `FloorPositionDetail` owns the chart but is deliberately free of the live
 * endpoints, so it cannot fetch a readout itself — it only relays whatever the
 * `conditions` slot chooses to publish, in the chart's own vocabulary
 * ({@link ChartTimeBand}). A closed position, a manual one, or a host that passes no
 * strip all publish nothing and the chart simply has no band.
 */
export interface ConditionBands {
  lanes: ChartTimeBand[];
  /** The stretch the lanes speak for — the reconstruction's covered range. */
  coverage: ChartTimeSpan | null;
}

export type ConditionBandsStore = PublishedStore<ConditionBands | null>;

const ConditionBandsContext = createContext<ConditionBandsStore | null>(null);

export const ConditionBandsProvider = ConditionBandsContext.Provider;

/** A fresh store, stable for the life of the host that renders the chart. */
export function useConditionBandsStore(): ConditionBandsStore {
  return useMemo(() => createPublishedStore<ConditionBands | null>(null), []);
}

/** Host side: the currently published lanes, re-rendering when they change. */
export function useConditionBands(store: ConditionBandsStore): ConditionBands | null {
  return useSyncExternalStore(store.subscribe, store.get, store.get);
}

/**
 * Publisher side: keep the context's store holding `bands`.
 *
 * Clears on unmount so a modal that switches to a position with no readout does not
 * leave the previous position's lanes on the chart.
 */
export function usePublishConditionBands(bands: ConditionBands | null): void {
  const store = useContext(ConditionBandsContext);
  useEffect(() => {
    if (!store) return;
    store.set(bands);
    return () => store.set(null);
  }, [store, bands]);
}
