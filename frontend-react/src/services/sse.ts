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

/**
 * Listen for `paper_test_finished` events — broadcast when a paper-test rule
 * reaches its max-total cap and all holdings have exited (the rule is then
 * auto-deactivated). Delivered to every subscriber regardless of mint filter.
 */
export function connectPaperTestStream(
  onFinished: (data: import('types').PaperTestFinishedEvent) => void,
): EventSource {
  const es = new EventSource(sseUrl());
  es.addEventListener('paper_test_finished', (e) => {
    if (typeof e.data !== 'string') return;
    try {
      onFinished(JSON.parse(e.data) as import('types').PaperTestFinishedEvent);
    } catch {
      /* ignore malformed frames */
    }
  });
  return es;
}
