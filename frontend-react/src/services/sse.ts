import { API_BASE } from './config';

export function sseUrl(): string {
  return `${API_BASE}/api/stream`;
}

export function connectTradeStream(onTrade: (data: string) => void): EventSource {
  const es = new EventSource(sseUrl());
  es.addEventListener('trade_executed', (e) => {
    if (typeof e.data === 'string') onTrade(e.data);
  });
  return es;
}
