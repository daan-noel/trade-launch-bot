import { useSyncExternalStore } from 'react';
import { getSseStatus, subscribeSseStatus, type SseStatus } from 'services/sse';

/**
 * Live connection status of the one shared `EventSource`. Backed by the sse
 * service's status signal (not React state), so any component can show a health
 * dot without owning the stream. `'connecting'` until the first open; `'error'`
 * while the browser is retrying a dropped connection; `'open'` when healthy.
 */
export function useSseStatus(): SseStatus {
  return useSyncExternalStore(subscribeSseStatus, getSseStatus, getSseStatus);
}
