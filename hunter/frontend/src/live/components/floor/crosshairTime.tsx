import { createContext, useContext, useMemo, useSyncExternalStore } from 'react';

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
 * produced the move, sixty times a second, to update a chip row beside it. Writes go
 * to a ref and one `requestAnimationFrame` flush notifies subscribers, so a burst of
 * pointer events costs one render of the readers and none of anything else.
 */
export interface CrosshairTimeStore {
  /** Publish the hovered instant (unix seconds), or `null` on pointer-out. */
  set(timeSec: number | null): void;
  subscribe(onChange: () => void): () => void;
  get(): number | null;
}

const CrosshairTimeContext = createContext<CrosshairTimeStore | null>(null);

export const CrosshairTimeProvider = CrosshairTimeContext.Provider;

/**
 * A fresh crosshair store, stable for the life of the host. Owned by whoever renders
 * the chart; `set` is safe to pass straight to `onCrosshairTimeChange`.
 */
export function useCrosshairTimeStore(): CrosshairTimeStore {
  return useMemo(() => {
    let current: number | null = null;
    let pending: number | null = null;
    let frame = 0;
    const listeners = new Set<() => void>();

    const flush = () => {
      frame = 0;
      if (pending === current) return;
      current = pending;
      for (const fn of listeners) fn();
    };

    return {
      set(timeSec) {
        pending = timeSec;
        // Coalesce a frame's worth of pointer events into one notification. A
        // pointer-out and the move that follows it inside the same frame collapse
        // to the later value, which is the one the user is looking at.
        if (!frame) frame = requestAnimationFrame(flush);
      },
      subscribe(onChange) {
        listeners.add(onChange);
        return () => {
          listeners.delete(onChange);
          if (!listeners.size && frame) {
            cancelAnimationFrame(frame);
            frame = 0;
          }
        };
      },
      get: () => current,
    };
  }, []);
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
