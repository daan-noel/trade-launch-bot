import { useEffect, useState } from 'react';
import { connectTradeStream } from '../services/sse';
import type { LiveTrade } from '../types';

const MAX_EVENTS = 500;

export function useTradeStream() {
  const [events, setEvents] = useState<LiveTrade[]>([]);

  useEffect(() => {
    const es = connectTradeStream((raw) => {
      try {
        const trade = JSON.parse(raw) as LiveTrade;
        setEvents((prev) => {
          const next = [trade, ...prev];
          if (next.length > MAX_EVENTS) next.length = MAX_EVENTS;
          return next;
        });
      } catch {
        /* ignore parse errors */
      }
    });
    return () => es.close();
  }, []);

  return events;
}
