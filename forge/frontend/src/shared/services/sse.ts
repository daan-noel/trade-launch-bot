/**
 * One shared EventSource multiplexes every push event across the whole app
 * (audit §1: poll → push). The backend's `GET /api/stream` emits `trade_executed`
 * (mint-scoped), `token_created` (mint-scoped), and `ingest_status` frames; this
 * module opens ONE unfiltered connection and fans each event type out to its
 * subscribers, who filter by mint client-side. The connection opens lazily on the
 * first subscriber and closes when the last one goes away, so we never burn
 * through the browser's ~6-connections-per-host HTTP/1.1 cap.
 *
 * Base URL is empty — same-origin in prod (the live bin serves the SPA) and via
 * the Vite `/api` proxy in dev.
 */

export function sseUrl(): string {
  return '/api/stream';
}

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
  // Attach a DOM listener for this event type once; it fans out to every current
  // subscriber of that type.
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

/** Disposable handle — kept `.close()`-shaped so call sites read cleanly. */
export interface StreamHandle {
  close: () => void;
}

/** A pushed trade. Mint-scoped on the wire; consumers still filter by mint since
 *  the shared connection is unfiltered. */
export interface TradeEvent {
  mint_address: string;
  trade_type: 'buy' | 'sell';
  amount_sol: number;
  token_amount: number;
  slot: number;
  tx_signature: string;
}

/** Listen for `trade_executed` — a new trade landed for some mint. The payload is
 *  parsed; malformed frames are ignored. */
export function connectTradeStream(onTrade: (t: TradeEvent) => void): StreamHandle {
  const unsub = subscribe('trade_executed', (e) => {
    if (typeof e.data !== 'string') return;
    try {
      onTrade(JSON.parse(e.data) as TradeEvent);
    } catch {
      /* ignore malformed frames */
    }
  });
  return { close: unsub };
}

/** Listen for `token_created` — the ingest pipeline started tracking a new token.
 *  A near-bare signal; the caller refetches its current list page. */
export function connectTokenCreatedStream(onCreated: () => void): StreamHandle {
  const unsub = subscribe('token_created', () => onCreated());
  return { close: unsub };
}

/** Listen for `ingest_status` — the Helius stream was paused/resumed. */
export function connectIngestStatusStream(onStatus: (live: boolean) => void): StreamHandle {
  const unsub = subscribe('ingest_status', (e) => {
    if (typeof e.data !== 'string') return;
    try {
      const p = JSON.parse(e.data) as { live?: boolean };
      if (typeof p.live === 'boolean') onStatus(p.live);
    } catch {
      /* ignore malformed frames */
    }
  });
  return { close: unsub };
}

/** A pushed launch/bundle status transition (confirm-watcher terminal outcome). */
export interface LaunchStatusEvent {
  launch_id: string;
  mint_address: string;
  launch_status: string;
  bundle_id: string | null;
  bundle_status: string | null;
}

/** Listen for `launch_status` — a launch reached created/failed/partial. The
 *  Launch Console refetches its status query when the id matches. */
export function connectLaunchStatusStream(
  onStatus: (e: LaunchStatusEvent) => void,
): StreamHandle {
  const unsub = subscribe('launch_status', (e) => {
    if (typeof e.data !== 'string') return;
    try {
      onStatus(JSON.parse(e.data) as LaunchStatusEvent);
    } catch {
      /* ignore malformed frames */
    }
  });
  return { close: unsub };
}

/** Listen for `wallet_pool` — a funding pass moved SOL / promoted wallets. A bare
 *  signal; the caller refetches `GET /api/wallet_pool`. */
export function connectWalletPoolStream(onChanged: () => void): StreamHandle {
  const unsub = subscribe('wallet_pool', () => onChanged());
  return { close: unsub };
}

/** A keystore-restore progress tick: which phase, and how far through it. */
export interface RestoreProgress {
  phase: 'wallets' | 'backfill' | 'positions';
  done: number;
  total: number;
}

/** The terminal keystore-restore summary (mirrors the backend `RestoreSummary`). */
export interface RestoreSummary {
  wallets_inserted: number;
  wallets_total: number;
  trades: number;
  launches: number;
  mints: number;
  positions_reconciled: number;
}

/** Listen for `restore_progress` — the keystore-restore job advanced a phase. */
export function connectRestoreProgressStream(
  onProgress: (p: RestoreProgress) => void,
): StreamHandle {
  const unsub = subscribe('restore_progress', (e) => {
    if (typeof e.data !== 'string') return;
    try {
      onProgress(JSON.parse(e.data) as RestoreProgress);
    } catch {
      /* ignore malformed frames */
    }
  });
  return { close: unsub };
}

/** Listen for `restore_complete` — the keystore-restore job finished; carries the
 *  final summary counts. The caller refetches the pool off it. */
export function connectRestoreCompleteStream(
  onComplete: (s: RestoreSummary) => void,
): StreamHandle {
  const unsub = subscribe('restore_complete', (e) => {
    if (typeof e.data !== 'string') return;
    try {
      onComplete(JSON.parse(e.data) as RestoreSummary);
    } catch {
      /* ignore malformed frames */
    }
  });
  return { close: unsub };
}
