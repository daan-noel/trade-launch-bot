import { connectTradeStream, onSseReopen } from 'services/sse';
import type { AppDispatch } from 'store/types';
import { sharedApi } from 'store/sharedEndpoints';
import {
  applyTokenLiveStats,
  liveTradeToTradeRecord,
  tradeDedupeKey,
} from 'lib/liveTrade';
import type { LiveTrade, TokenLiveStats, TradeRecord } from 'types';

/**
 * Ref-counted mint watch set for live chart history. Charts register while
 * mounted; a single module-level SSE consumer patches only watched mints into
 * the RTK `getTokenTrades` (bars) and `getTokenDetail` (reference lines) caches.
 */
const watchCounts = new Map<string, number>();
const watched = new Set<string>();

let streamClose: (() => void) | null = null;
let reopenUnsub: (() => void) | null = null;
let dispatchRef: AppDispatch | null = null;
let pending: LiveTrade[] = [];
let flushTimer: number | undefined;

/** Rows kept across a resync refetch. The response is authoritative for
 *  everything the DbWriter has committed; its tail lags the feed by one flush
 *  (256 rows / 500 ms), so the newest appended rows are merged back rather than
 *  replaced away. Anything the response already carries is dropped by dedupe. */
const RESYNC_TAIL_KEEP = 512;

function compareTradeOrder(a: TradeRecord, b: TradeRecord): number {
  if (a.slot !== b.slot) return a.slot - b.slot;
  if (a.tx_index !== b.tx_index) return a.tx_index - b.tx_index;
  if (a.leg_index !== b.leg_index) return a.leg_index - b.leg_index;
  return Date.parse(a.block_time) - Date.parse(b.block_time);
}

function flushPending(): void {
  flushTimer = undefined;
  if (pending.length === 0 || !dispatchRef) return;
  const batch = pending;
  pending = [];

  const byMint = new Map<string, LiveTrade[]>();
  // Freshest stats snapshot per mint in this batch — cumulative, so only the
  // last one is worth writing (see `applyTokenLiveStats`).
  const statsByMint = new Map<string, TokenLiveStats>();
  for (const t of batch) {
    if (!watched.has(t.mint_address)) continue;
    let list = byMint.get(t.mint_address);
    if (!list) {
      list = [];
      byMint.set(t.mint_address, list);
    }
    list.push(t);
    if (t.live) statsByMint.set(t.mint_address, t.live);
  }

  for (const [mint, trades] of byMint) {
    mergeIntoCache(mint, trades.map(liveTradeToTradeRecord));
  }
  for (const [mint, stats] of statsByMint) {
    mergeDetailStats(mint, stats);
  }
}

/**
 * Patch a mint's pushed stats into the `getTokenDetail` cache.
 *
 * The chart's reference lines (ATH, and the ATH/FEP + price readouts beside it)
 * read `getTokenDetail`, which is a one-shot query — no polling, no invalidating
 * tag. Without this the bars stream in and print new highs while the ATH line
 * stays pinned to whatever was fetched when the chart mounted, so a live token
 * renders candles above its own all-time high. The token grid patches the same
 * snapshot into its rows (`TokensPage`); charts need it too.
 */
function mergeDetailStats(mint: string, stats: TokenLiveStats): void {
  if (!dispatchRef) return;
  dispatchRef(
    sharedApi.util.updateQueryData('getTokenDetail', mint, (draft) => {
      applyTokenLiveStats(draft, stats);
    }),
  );
}

/** Append `rows` to a mint's cached history, dropping ones already there and
 *  restoring canonical order. The ONE writer into `getTokenTrades` — live
 *  appends and resync merges must not diverge on dedupe or sort. */
function mergeIntoCache(mint: string, rows: readonly TradeRecord[]): void {
  if (rows.length === 0 || !dispatchRef) return;
  dispatchRef(
    sharedApi.util.updateQueryData('getTokenTrades', mint, (draft) => {
      const existing = new Set(draft.map(tradeDedupeKey));
      for (const row of rows) {
        const key = tradeDedupeKey(row);
        if (existing.has(key)) continue;
        existing.add(key);
        draft.push(row);
      }
      draft.sort(compareTradeOrder);
    }),
  );
}

/**
 * Refetch every watched mint's history after a stream gap (reconnect, or the
 * bridge's `sse_resync` when it lagged).
 *
 * A missed frame is not a missing row on a table nobody scrolled to: the chart's
 * vol/non-vol overlay is CUMULATIVE, so a hole shifts both lines for the rest of
 * the token's life and never heals on its own. Bounded by the mounted charts
 * (usually one), and a gap is rare — the full-history read stays the cold path
 * it is documented to be.
 */
/** State shape the `getTokenTrades` cache selector reads — derived from the
 *  selector itself so this shared module needs no mode-specific `RootState`. */
type TradesCacheState = Parameters<
  ReturnType<typeof sharedApi.endpoints.getTokenTrades.select>
>[0];

function resyncWatchedMints(): void {
  const dispatch = dispatchRef;
  if (!dispatch || watched.size === 0) return;
  for (const mint of [...watched]) {
    dispatch((_unused: unknown, getState: () => unknown) => {
      const selectTrades = sharedApi.endpoints.getTokenTrades.select(mint);
      const cached = selectTrades(getState() as TradesCacheState).data ?? [];
      const tail = cached.slice(-RESYNC_TAIL_KEEP);
      void dispatch(
        sharedApi.endpoints.getTokenTrades.initiate(mint, {
          subscribe: false,
          forceRefetch: true,
        }),
      ).then(() => {
        if (watched.has(mint)) mergeIntoCache(mint, tail);
      });
    });
  }
}

function ensureStream(): void {
  if (streamClose || !dispatchRef) return;
  const handle = connectTradeStream((raw) => {
    try {
      const t = JSON.parse(raw) as LiveTrade;
      if (!watched.has(t.mint_address)) return;
      pending.push(t);
      if (flushTimer === undefined) {
        flushTimer = window.setTimeout(flushPending, 250);
      }
    } catch {
      /* ignore */
    }
  });
  streamClose = () => handle.close();
  reopenUnsub = onSseReopen(resyncWatchedMints);
}

function maybeTeardown(): void {
  if (watched.size > 0) return;
  if (flushTimer !== undefined) {
    window.clearTimeout(flushTimer);
    flushTimer = undefined;
  }
  pending = [];
  streamClose?.();
  streamClose = null;
  reopenUnsub?.();
  reopenUnsub = null;
}

/**
 * Bind the RTK dispatch used to patch `getTokenTrades`. Call once from app
 * bootstrap — charts only register mints after this is set.
 */
export function bindTokenTradesLiveDispatch(dispatch: AppDispatch): () => void {
  dispatchRef = dispatch;
  if (watched.size > 0) ensureStream();
  return () => {
    if (dispatchRef === dispatch) {
      dispatchRef = null;
      maybeTeardown();
    }
  };
}

/** Register interest in live appends for `mint`. Returns an unwatch fn. */
export function watchTokenTradesMint(mint: string): () => void {
  if (!mint) return () => {};
  watchCounts.set(mint, (watchCounts.get(mint) ?? 0) + 1);
  watched.add(mint);
  ensureStream();
  return () => {
    const n = (watchCounts.get(mint) ?? 1) - 1;
    if (n <= 0) {
      watchCounts.delete(mint);
      watched.delete(mint);
      maybeTeardown();
    } else {
      watchCounts.set(mint, n);
    }
  };
}
