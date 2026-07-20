import { connectTradeStream } from 'services/sse';
import type { AppDispatch } from 'store/types';
import { sharedApi } from 'store/sharedEndpoints';
import { liveTradeToTradeRecord, tradeDedupeKey } from 'lib/liveTrade';
import type { LiveTrade, TradeRecord } from 'types';

/**
 * Ref-counted mint watch set for live chart history. Charts register while
 * mounted; a single module-level SSE consumer patches only watched mints into
 * the RTK `getTokenTrades` cache (append + dedupe).
 */
const watchCounts = new Map<string, number>();
const watched = new Set<string>();

let streamClose: (() => void) | null = null;
let dispatchRef: AppDispatch | null = null;
let pending: LiveTrade[] = [];
let flushTimer: number | undefined;

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
  for (const t of batch) {
    if (!watched.has(t.mint_address)) continue;
    let list = byMint.get(t.mint_address);
    if (!list) {
      list = [];
      byMint.set(t.mint_address, list);
    }
    list.push(t);
  }

  for (const [mint, trades] of byMint) {
    dispatchRef(
      sharedApi.util.updateQueryData('getTokenTrades', mint, (draft) => {
        const existing = new Set(draft.map(tradeDedupeKey));
        for (const t of trades) {
          const row = liveTradeToTradeRecord(t);
          const key = tradeDedupeKey(row);
          if (existing.has(key)) continue;
          existing.add(key);
          draft.push(row);
        }
        draft.sort(compareTradeOrder);
      }),
    );
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
