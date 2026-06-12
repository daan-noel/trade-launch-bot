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

type TpslStrategy = 'tpsl1' | 'tpsl2';

/**
 * Rule-list change signal for `strategy`. Fires when a rule is created / updated
 * / deleted or moves through a lifecycle transition (`tpsl_rules_changed`), AND
 * when a position changes (`tpsl_positions_changed`, which shifts the open-count
 * and lifecycle badge). The payload is a bare signal — the caller refetches the
 * list. Both event types are filtered to `strategy` client-side.
 */
export function connectTpslRulesChanged(
  strategy: TpslStrategy,
  onChanged: () => void,
): StreamHandle {
  const handle = (e: MessageEvent) => {
    if (typeof e.data !== 'string') return;
    try {
      const p = JSON.parse(e.data) as { strategy?: string };
      if (p.strategy === strategy) onChanged();
    } catch {
      /* ignore malformed frames */
    }
  };
  const unsubRules = subscribe('tpsl_rules_changed', handle);
  const unsubPos = subscribe('tpsl_positions_changed', handle);
  return {
    close: () => {
      unsubRules();
      unsubPos();
    },
  };
}

/**
 * Position change signal for `strategy`. Calls `onChanged` with the affected
 * `rule_id` so the caller refetches only that rule's positions.
 */
export function connectTpslPositionsChanged(
  strategy: TpslStrategy,
  onChanged: (ruleId: string) => void,
): StreamHandle {
  const unsub = subscribe('tpsl_positions_changed', (e) => {
    if (typeof e.data !== 'string') return;
    try {
      const p = JSON.parse(e.data) as { strategy?: string; rule_id?: string };
      if (p.strategy === strategy && p.rule_id) onChanged(p.rule_id);
    } catch {
      /* ignore malformed frames */
    }
  });
  return { close: unsub };
}
