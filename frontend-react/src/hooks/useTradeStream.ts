import { useEffect, useState } from 'react';
import { connectTradeStream } from 'services/sse';
import type { LiveTrade } from 'types';

const MAX_EVENTS = 500;
/** Coalesce bursts of trade frames into one state write per tick. */
const FLUSH_INTERVAL_MS = 250;

/** Stable row-key accessor for live-trade tables — hoisted so the DataTable
 *  receives the same reference every render instead of a fresh inline closure
 *  on each ~4x/sec stream flush. */
export const tradeRowKey = (ev: LiveTrade) => `${ev.tx_signature}-${ev.slot}`;

export function useTradeStream() {
  const [events, setEvents] = useState<LiveTrade[]>([]);

  useEffect(() => {
    // Trades are bursty and high-volume. Prepending each frame individually
    // would hand the (client-side) DataTable a brand-new 500-element array per
    // frame, re-running its filter→sort→slice and re-rendering on every trade.
    // Instead buffer incoming frames and flush them in one batch on a short
    // timer, so a burst of N trades costs a single array rebuild + render.
    let buffer: LiveTrade[] = [];
    let timer: number | undefined;

    const flush = () => {
      timer = undefined;
      if (buffer.length === 0) return;
      const batch = buffer;
      buffer = [];
      setEvents((prev) => {
        // Newest-first: this tick's frames (already newest-first within the
        // batch as received order is oldest→newest, so reverse) ahead of prev.
        const next = batch.reverse().concat(prev);
        if (next.length > MAX_EVENTS) next.length = MAX_EVENTS;
        return next;
      });
    };

    const es = connectTradeStream((raw) => {
      try {
        buffer.push(JSON.parse(raw) as LiveTrade);
        // Cap the buffer so a backgrounded tab (which doesn't flush — see below)
        // can't grow it without bound; only the freshest MAX_EVENTS matter.
        if (buffer.length > MAX_EVENTS) buffer.splice(0, buffer.length - MAX_EVENTS);
        // While hidden, keep buffering but don't schedule the flush: no state
        // write, no 500-row rebuild, no re-render. Mirrors useNow /
        // skipPollingIfUnfocused — the hottest stream shouldn't burn CPU on a
        // tab nobody is watching. The pending frames flush on re-focus.
        if (timer === undefined && document.visibilityState !== 'hidden') {
          timer = window.setTimeout(flush, FLUSH_INTERVAL_MS);
        }
      } catch {
        /* ignore parse errors */
      }
    });

    // On re-focus, drain whatever buffered while hidden immediately, then let
    // incoming frames resume normal timer-coalesced flushing.
    const onVisibility = () => {
      if (document.visibilityState === 'visible' && timer === undefined) flush();
    };
    document.addEventListener('visibilitychange', onVisibility);

    return () => {
      document.removeEventListener('visibilitychange', onVisibility);
      window.clearTimeout(timer);
      es.close();
    };
  }, []);

  return events;
}
