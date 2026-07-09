export const API_BASE = import.meta.env.VITE_API_BASE ?? '';
export const POLL_INTERVAL_MS = Number(import.meta.env.VITE_POLL_INTERVAL_MS ?? 5000);

/**
 * Slow safety-net poll for views that are primarily SSE-driven (strategy rules
 * and positions refetch on a backend push). This only exists to self-heal from a
 * dropped/lagged SSE frame, so it can be much slower than {@link POLL_INTERVAL_MS}.
 */
export const FALLBACK_POLL_INTERVAL_MS = Number(
  import.meta.env.VITE_FALLBACK_POLL_INTERVAL_MS ?? 30000,
);
