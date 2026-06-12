import { useEffect, useState } from 'react';
import { connectTradeStream } from 'services/sse';
import type { LiveTrade } from 'types';

const MAX_EVENTS = 500;
/** Coalesce bursts of trade frames into one state write per tick. */
const FLUSH_INTERVAL_MS = 250;

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
        if (timer === undefined) timer = window.setTimeout(flush, FLUSH_INTERVAL_MS);
      } catch {
        /* ignore parse errors */
      }
    });

    return () => {
      window.clearTimeout(timer);
      es.close();
    };
  }, []);

  return events;
}
