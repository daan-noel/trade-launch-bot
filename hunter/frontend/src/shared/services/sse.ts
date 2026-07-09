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
 * Low-level: subscribe to a raw SSE event type on the shared connection,
 * returning an unsubscribe fn. The `connect*` helpers below wrap this with
 * payload parsing + id filtering; the background-jobs registry uses it directly
 * to receive ALL frames of a type (no id filter). */
export function sseSubscribe(type: string, cb: SseListener): () => void {
  return subscribe(type, cb);
}

/**
 * Strategy-agnostic position-change subscriber — receives deltas for BOTH
 * tpsl1 and tpsl2 without filtering. Used by the global notification hook so
 * it works regardless of which page the user is on.
 */
export function connectAllPositionsChanged(
  onDelta: (delta: import('types').TpslPositionDelta & { strategy: string }) => void,
): StreamHandle {
  const unsub = subscribe('tpsl_positions_changed', (e) => {
    if (typeof e.data !== 'string') return;
    try {
      const p = JSON.parse(e.data) as {
        strategy?: string;
        rule_id?: string;
        rule_snapshot?: import('types').RuleNotifSnapshot | null;
        position?: import('types').RulePositionRecord | null;
        removed?: boolean;
        open_positions?: number;
        pending_positions?: number;
        total_positions?: number;
      };
      if (p.strategy && p.rule_id) {
        onDelta({
          strategy: p.strategy,
          ruleId: p.rule_id,
          ruleSnapshot: p.rule_snapshot ?? null,
          position: p.position ?? null,
          removed: !!p.removed,
          openPositions: p.open_positions ?? 0,
          pendingPositions: p.pending_positions ?? 0,
          totalPositions: p.total_positions ?? 0,
        });
      }
    } catch {
      /* ignore malformed frames */
    }
  });
  return { close: unsub };
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

// The client filters `tpsl_rules_changed` / `tpsl_positions_changed` frames by the
// payload's `strategy` string; the server already emits `swing_1` for swing1 rules
// (canonical `StrategyImpl::id`), so widening this type is all that's needed.
type TpslStrategy = 'tpsl1' | 'tpsl2' | 'swing_1';

/**
 * Rule-list change signal for `strategy` — fires when a rule is created /
 * updated / deleted or moves through a lifecycle transition
 * (`tpsl_rules_changed`). The payload is a bare signal; the caller refetches the
 * list. Position changes are NOT included here: they arrive via
 * {@link connectTpslPositionsChanged} as deltas the caller patches in place
 * (open-count badge + lifecycle), so a busy run no longer refetches the whole
 * rule list per position transition. Filtered to `strategy` client-side.
 */
export function connectTpslRulesChanged(
  strategy: TpslStrategy,
  onChanged: () => void,
): StreamHandle {
  const unsub = subscribe('tpsl_rules_changed', (e) => {
    if (typeof e.data !== 'string') return;
    try {
      const p = JSON.parse(e.data) as { strategy?: string };
      if (p.strategy === strategy) onChanged();
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

/**
 * Terminal signal for an in-flight "Swing Detection All" run — the run for some
 * `run_id` ended. The swing analogue of {@link connectSimulationFinished}; the
 * consumer keys off `run_id`.
 */
export function connectSwingDetectionFinished(
  onFinished: (ev: import('types').SwingDetectionFinishedEvent) => void,
): StreamHandle {
  const unsub = subscribe('swing_detection_finished', (e) => {
    if (typeof e.data !== 'string') return;
    try {
      onFinished(JSON.parse(e.data) as import('types').SwingDetectionFinishedEvent);
    } catch {
      /* ignore malformed frames */
    }
  });
  return { close: unsub };
}

/**
 * Position change signal for `strategy`. Delivers a {@link TpslPositionDelta}:
 * the changed row + the rule's live cap counters, so the caller patches one row
 * (and the badge) in place rather than refetching the list. `removed` rows still
 * carry their `position` so the caller knows which to drop. Filtered to
 * `strategy` client-side.
 */
export function connectTpslPositionsChanged(
  strategy: TpslStrategy,
  onDelta: (delta: import('types').TpslPositionDelta) => void,
): StreamHandle {
  const unsub = subscribe('tpsl_positions_changed', (e) => {
    if (typeof e.data !== 'string') return;
    try {
      const p = JSON.parse(e.data) as {
        strategy?: string;
        rule_id?: string;
        rule_snapshot?: import('types').RuleNotifSnapshot | null;
        position?: import('types').RulePositionRecord | null;
        removed?: boolean;
        open_positions?: number;
        pending_positions?: number;
        total_positions?: number;
      };
      if (p.strategy === strategy && p.rule_id) {
        onDelta({
          ruleId: p.rule_id,
          ruleSnapshot: p.rule_snapshot ?? null,
          position: p.position ?? null,
          removed: !!p.removed,
          openPositions: p.open_positions ?? 0,
          pendingPositions: p.pending_positions ?? 0,
          totalPositions: p.total_positions ?? 0,
        });
      }
    } catch {
      /* ignore malformed frames */
    }
  });
  return { close: unsub };
}
