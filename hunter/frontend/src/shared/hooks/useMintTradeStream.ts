import { useEffect, useRef } from 'react';
import { connectTradeStream } from 'services/sse';
import type { LiveTrade } from 'types';

export type MintSetRef = { current: ReadonlySet<string> | Set<string> };

/**
 * Subscribe to `trade_executed` for a dynamic mint set (ref so the SSE handler
 * never re-subscribes when the set changes). Frames for unwatched mints are
 * dropped. Bursty trades coalesce through `coalesceMs` before `onTrades` fires
 * with the batch (newest-last within the batch).
 *
 * One shared EventSource underneath (`connectTradeStream`); many callers = many
 * JS filters, not many connections.
 */
export function useMintTradeStream(
  mintsRef: MintSetRef,
  onTrades: (trades: LiveTrade[]) => void,
  coalesceMs = 250,
): void {
  const onTradesRef = useRef(onTrades);
  onTradesRef.current = onTrades;

  useEffect(() => {
    const buf: LiveTrade[] = [];
    let timer: number | undefined;
    const flush = () => {
      timer = undefined;
      if (buf.length === 0) return;
      const batch = buf.splice(0);
      onTradesRef.current(batch);
    };
    const es = connectTradeStream((raw) => {
      try {
        const t = JSON.parse(raw) as LiveTrade;
        if (!mintsRef.current.has(t.mint_address)) return;
        buf.push(t);
        if (timer === undefined) timer = window.setTimeout(flush, coalesceMs);
      } catch {
        /* ignore malformed frames */
      }
    });
    return () => {
      window.clearTimeout(timer);
      es.close();
    };
  }, [mintsRef, coalesceMs]);
}
