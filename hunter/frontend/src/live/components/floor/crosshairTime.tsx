import { createContext, useContext, useMemo, useSyncExternalStore } from 'react';

import { createPublishedStore, type PublishedStore } from './publishedStore';

/**
 * Wall-clock unix **seconds** under the position modal's chart crosshair, or `null`
 * when the pointer is off the plot, published to whatever downstream surface wants
 * to follow it.
 *
 * A context rather than a prop because the two ends are not adjacent: the chart
 * emits it and the rule-condition strip reads it, but between them sits
 * `FloorPositionDetail`, whose `conditions` is a `ReactNode` slot. Keeping it a
 * `ReactNode` is what lets Console History, Rules Evidence and Portfolio pass a
 * strip — or nothing — without knowing this exists.
 *
 * A **store**, not a state, and that is the whole design: the crosshair moves once
 * per animation frame, so a `useState` on the detail would re-render the chart that
 * produced the move, sixty times a second, to update a chip row beside it. The
 * mechanism is in {@link createPublishedStore}, shared with the lanes travelling the
 * other way.
 */
export type CrosshairTimeStore = PublishedStore<number | null>;

const CrosshairTimeContext = createContext<CrosshairTimeStore | null>(null);

export const CrosshairTimeProvider = CrosshairTimeContext.Provider;

/**
 * A fresh crosshair store, stable for the life of the host. Owned by whoever renders
 * the chart; `set` is safe to pass straight to `onCrosshairTimeChange`.
 */
export function useCrosshairTimeStore(): CrosshairTimeStore {
  return useMemo(() => createPublishedStore<number | null>(null), []);
}

/** The hovered instant, or `null` when nothing is hovered / no provider above. */
export function useCrosshairTimeSec(): number | null {
  const store = useContext(CrosshairTimeContext);
  return useSyncExternalStore(
    store?.subscribe ?? noopSubscribe,
    store?.get ?? nullGetter,
    nullGetter,
  );
}

const noopSubscribe = () => () => {};
const nullGetter = () => null;
