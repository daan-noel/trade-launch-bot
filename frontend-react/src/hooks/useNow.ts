import { useSyncExternalStore } from 'react';

/**
 * A single app-wide "now" clock.
 *
 * One `setInterval` drives every live-ticking cell (token age, "2m ago"
 * relative times, …) instead of each cell owning its own timer. Cells subscribe
 * via {@link useNow}; React batches one update per tick and `useSyncExternalStore`
 * bails out of any cell whose coarsened snapshot didn't change.
 *
 * The interval only runs while at least one cell is mounted AND the tab is
 * visible — a backgrounded tab burns no CPU, and on re-focus we snap straight to
 * the real time before resuming (mirroring the token list's
 * `skipPollingIfUnfocused`).
 */
let now = Date.now();
const subscribers = new Set<() => void>();
let intervalId: ReturnType<typeof setInterval> | undefined;
let visHandler: (() => void) | undefined;

function tick(): void {
  now = Date.now();
  for (const cb of subscribers) cb();
}

function start(): void {
  if (intervalId != null) return;
  if (typeof document !== 'undefined' && document.visibilityState === 'hidden') return;
  intervalId = setInterval(tick, 1000);
}

function stop(): void {
  if (intervalId != null) {
    clearInterval(intervalId);
    intervalId = undefined;
  }
}

function onVisibility(): void {
  if (document.visibilityState === 'visible') {
    tick(); // snap to the real time immediately, then resume ticking
    start();
  } else {
    stop();
  }
}

function subscribe(cb: () => void): () => void {
  subscribers.add(cb);
  if (subscribers.size === 1) {
    if (typeof document !== 'undefined' && !visHandler) {
      visHandler = onVisibility;
      document.addEventListener('visibilitychange', visHandler);
    }
    start();
  }
  return () => {
    subscribers.delete(cb);
    if (subscribers.size === 0) {
      stop();
      if (typeof document !== 'undefined' && visHandler) {
        document.removeEventListener('visibilitychange', visHandler);
        visHandler = undefined;
      }
    }
  };
}

/**
 * Current epoch-ms, re-rendering the caller only when the value crosses a
 * `granularityMs` boundary. A cell showing seconds passes 1000 (ticks every
 * second); one showing minutes/hours can pass 30_000 so it re-renders ~twice a
 * minute instead of 60×. The returned value is floored to that granularity.
 */
export function useNow(granularityMs = 1000): number {
  const snapshot = () => Math.floor(now / granularityMs) * granularityMs;
  const serverSnapshot = () => Math.floor(Date.now() / granularityMs) * granularityMs;
  return useSyncExternalStore(subscribe, snapshot, serverSnapshot);
}
