import { useCallback, useSyncExternalStore } from 'react';

/**
 * A single app-wide "now" clock (ported from hunter).
 *
 * One `setInterval` drives every live-ticking cell (token/wallet age, "2m ago"
 * relative times, …) instead of each cell owning its own timer or, worse, the
 * age freezing between polls. Cells subscribe via {@link useNow}; React batches
 * one update per tick and `useSyncExternalStore` bails out of any cell whose
 * coarsened snapshot didn't change.
 *
 * The interval runs at the *finest* granularity any mounted cell asked for, not
 * a fixed 1s: a page showing only minute/hour ages (30s granularity) wakes the
 * subscribers ~twice a minute instead of 60×, and drops back to per-second only
 * while a sub-minute cell is actually mounted. It also runs only while the tab
 * is visible — a backgrounded tab burns no CPU, and on re-focus we snap straight
 * to the real time before resuming (mirroring `skipPollingIfUnfocused`).
 */
let now = Date.now();
// Each subscriber is stored with the granularity it cares about, so the timer
// can run at the minimum across all of them.
const subscribers = new Map<() => void, number>();
let intervalId: ReturnType<typeof setInterval> | undefined;
let intervalMs = 0;
let visHandler: (() => void) | undefined;

function tick(): void {
  now = Date.now();
  for (const cb of subscribers.keys()) cb();
}

/** Finest granularity any current subscriber asked for (defaults to 1s). */
function minGranularity(): number {
  let m = Infinity;
  for (const g of subscribers.values()) if (g < m) m = g;
  return m === Infinity ? 1000 : m;
}

/** (Re)start the timer at the current min granularity; no-op if already correct. */
function start(): void {
  if (typeof document !== 'undefined' && document.visibilityState === 'hidden') return;
  if (subscribers.size === 0) return;
  const period = minGranularity();
  if (intervalId != null && intervalMs === period) return;
  if (intervalId != null) clearInterval(intervalId);
  intervalMs = period;
  intervalId = setInterval(tick, period);
}

function stop(): void {
  if (intervalId != null) {
    clearInterval(intervalId);
    intervalId = undefined;
    intervalMs = 0;
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

function subscribe(cb: () => void, granularityMs: number): () => void {
  subscribers.set(cb, granularityMs);
  if (subscribers.size === 1 && typeof document !== 'undefined' && !visHandler) {
    visHandler = onVisibility;
    document.addEventListener('visibilitychange', visHandler);
  }
  // (Re)start in case this subscriber needs a finer cadence than the running one.
  start();
  return () => {
    subscribers.delete(cb);
    if (subscribers.size === 0) {
      stop();
      if (typeof document !== 'undefined' && visHandler) {
        document.removeEventListener('visibilitychange', visHandler);
        visHandler = undefined;
      }
    } else {
      // A coarser cadence may now suffice — relax the timer.
      start();
    }
  };
}

/**
 * Current epoch-ms, re-rendering the caller only when the value crosses a
 * `granularityMs` boundary. A cell showing seconds passes 1000 (ticks every
 * second); one showing minutes/hours can pass 30_000 so it re-renders ~twice a
 * minute instead of 60×. The returned value is floored to that granularity, and
 * the shared timer runs no faster than the finest granularity in use.
 */
export function useNow(granularityMs = 1000): number {
  const subscribeWithGranularity = useCallback(
    (cb: () => void) => subscribe(cb, granularityMs),
    [granularityMs],
  );
  const snapshot = () => Math.floor(now / granularityMs) * granularityMs;
  const serverSnapshot = () => Math.floor(Date.now() / granularityMs) * granularityMs;
  return useSyncExternalStore(subscribeWithGranularity, snapshot, serverSnapshot);
}
