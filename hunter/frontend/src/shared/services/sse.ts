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

/**
 * Connection health for the one shared EventSource. A raw `EventSource` gives no
 * status to the UI and silently retries on drop — so a delta-driven page (rule
 * lists, positions, token feed) can miss every frame during an outage and never
 * know to catch up. We surface a status signal + a reopen hook so consumers can
 * show a dot and refetch after a reconnect.
 */
export type SseStatus = 'connecting' | 'open' | 'error';

let status: SseStatus = 'connecting';
/** True once we've seen an error on the current connection, so the next `onopen`
 * is a *re*connect (missed frames) rather than the first open (nothing missed). */
let sawError = false;
const statusListeners = new Set<(s: SseStatus) => void>();
const reopenListeners = new Set<() => void>();

function setStatus(next: SseStatus): void {
  if (status === next) return;
  status = next;
  for (const fn of statusListeners) fn(next);
}

/** Current connection status (synchronous read for a fresh subscriber). */
export function getSseStatus(): SseStatus {
  return status;
}

/**
 * Subscribe to connection-status changes (for a header dot / banner). Fires
 * immediately with the current status, then on every transition. Returns an
 * unsubscribe fn.
 */
export function subscribeSseStatus(cb: (s: SseStatus) => void): () => void {
  statusListeners.add(cb);
  cb(status);
  return () => {
    statusListeners.delete(cb);
  };
}

/**
 * Register a callback to run when the shared stream **reconnects** after an
 * error (not on the first open) — i.e. when frames may have been missed and the
 * caller should refetch to resync. Returns an unsubscribe fn. The `connect*`
 * helpers whose callback is a bare "refetch" signal wire this automatically.
 */
export function onSseReopen(cb: () => void): () => void {
  reopenListeners.add(cb);
  return () => {
    reopenListeners.delete(cb);
  };
}

/** Combine several unsubscribe fns into one `close`-shaped handle. */
function combine(...unsubs: Array<() => void>): () => void {
  return () => {
    for (const u of unsubs) u();
  };
}

/**
 * Low-level: subscribe to a raw SSE event type on the shared connection,
 * returning an unsubscribe fn. The `connect*` helpers below wrap this with
 * payload parsing + id filtering; the background-jobs registry uses it directly
 * to receive ALL frames of a type (no id filter). */
export function sseSubscribe(type: string, cb: SseListener): () => void {
  return subscribe(type, cb);
}

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
    sawError = false;
    setStatus('connecting');
    shared.onopen = () => {
      // A reconnect (we previously errored) means frames were missed while the
      // stream was down — tell resync consumers to refetch. The very first open
      // missed nothing, so it only flips the status.
      if (sawError) {
        sawError = false;
        for (const fn of reopenListeners) fn();
      }
      setStatus('open');
    };
    shared.onerror = () => {
      // EventSource retries on its own; we don't close. Just record that the
      // current connection is unhealthy so the next `onopen` counts as a resync.
      sawError = true;
      setStatus('error');
    };
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
      sawError = false;
      setStatus('connecting');
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
  // Bare "refetch the current page" signal — also fire it after a reconnect so a
  // table that missed `token_created` frames during an outage catches up.
  const unsub = combine(
    subscribe('token_created', () => onCreated()),
    onSseReopen(onCreated),
  );
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

/**
 * Terminal signal for the in-flight grouped param-sweep — the single-flight run
 * ended (normal finish, error, or user cancel). Lets a global progress indicator
 * clear itself without polling. The payload is delivered for every strategy_id;
 * the single-flight sweep means the consumer needn't filter.
 */
export function connectSweepFinished(
  onFinished: (ev: import('types').SweepFinishedEvent) => void,
): StreamHandle {
  const unsub = subscribe('sweep_finished', (e) => {
    if (typeof e.data !== 'string') return;
    try {
      onFinished(JSON.parse(e.data) as import('types').SweepFinishedEvent);
    } catch {
      /* ignore malformed frames */
    }
  });
  return { close: unsub };
}

/**
 * Terminal signal for an in-flight rule simulation (backtest) — the run for some
 * `rule_id` ended. The per-rule analogue of {@link connectSweepFinished}; the
 * consumer keys off `rule_id`.
 */
export function connectSimulationFinished(
  onFinished: (ev: import('types').SimulationFinishedEvent) => void,
): StreamHandle {
  const unsub = subscribe('simulation_finished', (e) => {
    if (typeof e.data !== 'string') return;
    try {
      onFinished(JSON.parse(e.data) as import('types').SimulationFinishedEvent);
    } catch {
      /* ignore malformed frames */
    }
  });
  return { close: unsub };
}

/** Terminal signal for the in-flight flow-discovery job. */
export function connectFlowDiscoveryFinished(
  onFinished: (ev: import('types').FlowDiscoveryFinishedEvent) => void,
): StreamHandle {
  const unsub = subscribe('flow_discovery_finished', (e) => {
    if (typeof e.data !== 'string') return;
    try {
      onFinished(JSON.parse(e.data) as import('types').FlowDiscoveryFinishedEvent);
    } catch {
      /* ignore malformed frames */
    }
  });
  return { close: unsub };
}

/**
 * Generic-engine armed-state stream (`strategy_armed_changed`) — a (token, rule)
 * pair armed or disarmed. The live monitor keeps its own armed map from these
 * deltas; on reconnect it refetches the `/api/strategies/armed` snapshot via the
 * `onReopen` callback so a missed frame during an outage self-heals.
 */
export function connectArmedChanged(
  onDelta: (delta: import('lib/strategy/types').ArmedChangedEvent) => void,
  onReopen?: () => void,
): StreamHandle {
  const unsub = combine(
    subscribe('strategy_armed_changed', (e) => {
      if (typeof e.data !== 'string') return;
      try {
        onDelta(JSON.parse(e.data) as import('lib/strategy/types').ArmedChangedEvent);
      } catch {
        /* ignore malformed frames */
      }
    }),
    onReopen ? onSseReopen(onReopen) : () => {},
  );
  return { close: unsub };
}

/**
 * Generic-engine position stream (`strategy_position_update`) — one position
 * lifecycle delta the monitor / rules list patches in place.
 */
export function connectStrategyPositionUpdate(
  onDelta: (delta: import('lib/strategy/types').StrategyPositionUpdateEvent) => void,
): StreamHandle {
  const unsub = subscribe('strategy_position_update', (e) => {
    if (typeof e.data !== 'string') return;
    try {
      onDelta(JSON.parse(e.data) as import('lib/strategy/types').StrategyPositionUpdateEvent);
    } catch {
      /* ignore malformed frames */
    }
  });
  return { close: unsub };
}

/** Live progress of a long-running stop/sell action (Stop & close / Stop All). */
export interface ActionProgress {
  action_id: string;
  mint_address: string | null;
  rule_id?: string | null;
  kind: string;
  status: 'running' | 'partial' | 'done' | 'failed';
  done: number;
  total: number;
  error?: string | null;
}

/** Listen for `action_progress` — a stop/sell action advanced (start / N of M / terminal). */
export function connectActionProgressStream(
  onProgress: (p: ActionProgress) => void,
): StreamHandle {
  const unsub = subscribe('action_progress', (e) => {
    if (typeof e.data !== 'string') return;
    try {
      onProgress(JSON.parse(e.data) as ActionProgress);
    } catch {
      /* ignore malformed frames */
    }
  });
  return { close: unsub };
}

/**
 * Listen for `tpsl_rules_changed` — a rule was paused/activated/updated. Bare
 * signal; consumers refetch the rules list (and clear optimistic "Pausing…" state).
 */
export function connectTpslRulesChanged(onChanged: () => void): StreamHandle {
  const unsub = combine(
    subscribe('tpsl_rules_changed', () => onChanged()),
    onSseReopen(onChanged),
  );
  return { close: unsub };
}
