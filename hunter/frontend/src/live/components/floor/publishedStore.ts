/**
 * A value one component publishes and another subscribes to, without either
 * re-rendering the other's parent.
 *
 * The position modal needs this twice, in opposite directions: the chart publishes
 * the crosshair instant down to the rule-condition strip, and the strip publishes
 * its condition timeline back up to the chart. Between them sits
 * `FloorPositionDetail`, which holds a `ReactNode` slot and must stay ignorant of
 * both — so neither value can be its state without re-rendering the whole detail,
 * including the chart, on every change.
 *
 * Writes coalesce onto one `requestAnimationFrame`. That is load-bearing for the
 * crosshair, which fires per pointer event, and merely harmless for the lanes.
 */
export interface PublishedStore<T> {
  set(value: T): void;
  subscribe(onChange: () => void): () => void;
  get(): T;
}

export function createPublishedStore<T>(initial: T): PublishedStore<T> {
  let current = initial;
  let pending = initial;
  let frame = 0;
  const listeners = new Set<() => void>();

  const flush = () => {
    frame = 0;
    // Identity compare: a burst that lands back on the current value — a
    // pointer-out and the move after it inside one frame — notifies nobody.
    if (pending === current) return;
    current = pending;
    for (const fn of listeners) fn();
  };

  return {
    set(value) {
      pending = value;
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
}
