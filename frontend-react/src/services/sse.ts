import { API_BASE } from './config';

export function sseUrl(): string {
  return `${API_BASE}/api/stream`;
}

/**
 * One shared EventSource multiplexes every event type across the whole app.
 *
 * Each component subscribes only to the event(s) it cares about; the underlying
 * connection opens lazily on the first subscriber and closes when the last one
 * goes away. Previously every `connect*Stream` helper opened its OWN
 * EventSource, so mounting a few stream-consuming pages at once could burn
 * through the browser's ~6-connections-per-host HTTP/1.1 cap against a single
 * `/api/stream` endpoint. Now it's always exactly one connection.
 */
type SseListener = (e: MessageEvent) => void;

let shared: EventSource | null = null;
const listeners = new Map<string, Set<SseListener>>();
const attached = new Set<string>();

function subscribe(type: string, cb: SseListener): () => void {
  let set = listeners.get(type);
  if (!set) {
    set = new Set();
    listeners.set(type, set);
  }
  set.add(cb);

  if (!shared) {
    shared = new EventSource(sseUrl());
    attached.clear();
  }
  // Attach a DOM listener for this event type once; it fans out to every
  // current subscriber of that type.
  if (!attached.has(type)) {
    attached.add(type);
    shared.addEventListener(type, (e) => {
      const subs = listeners.get(type);
      if (!subs) return;
      for (const fn of subs) fn(e as MessageEvent);
    });
  }

  return () => {
    const subs = listeners.get(type);
    if (subs) {
      subs.delete(cb);
      if (subs.size === 0) listeners.delete(type);
    }
    // Tear the connection down only when nothing anywhere is listening.
    let anyLeft = false;
    for (const s of listeners.values()) {
      if (s.size) {
        anyLeft = true;
        break;
      }
    }
    if (!anyLeft && shared) {
      shared.close();
      shared = null;
      attached.clear();
    }
  };
}

/** Disposable handle — kept `.close()`-shaped so existing call sites are unchanged. */
interface StreamHandle {
  close: () => void;
}

export function connectTradeStream(onTrade: (data: string) => void): StreamHandle {
  const unsub = subscribe('trade_executed', (e) => {
    if (typeof e.data === 'string') onTrade(e.data);
  });
  return { close: unsub };
}

/**
 * Listen for `token_created` events — broadcast whenever the ingest pipeline
 * starts tracking a new token. The payload isn't needed (the Tokens table refetches
 * its current page from the server), so the callback is a bare signal. Delivered to
 * every subscriber that didn't open the stream with a `?mint` filter.
 */
export function connectTokenCreatedStream(onCreated: () => void): StreamHandle {
  const unsub = subscribe('token_created', () => onCreated());
  return { close: unsub };
}

/**
 * Listen for `paper_test_finished` events — broadcast when a paper-test rule
 * reaches its max-total cap and all holdings have exited (the rule is then
 * auto-deactivated). Delivered to every subscriber regardless of mint filter.
 */
export function connectPaperTestStream(
  onFinished: (data: import('types').PaperTestFinishedEvent) => void,
): StreamHandle {
  const unsub = subscribe('paper_test_finished', (e) => {
    if (typeof e.data !== 'string') return;
    try {
      onFinished(JSON.parse(e.data) as import('types').PaperTestFinishedEvent);
    } catch {
      /* ignore malformed frames */
    }
  });
  return { close: unsub };
}
