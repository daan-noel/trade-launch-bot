export const API_BASE = import.meta.env.VITE_API_BASE ?? '';

/**
 * Slow safety-net poll for views that are primarily SSE-driven (Tokens STREAM
 * row patches + `token_created` refetch). Heals dropped/lagged frames and
 * re-applies server sort that in-place patches can't — not the primary tick.
 */
export const FALLBACK_POLL_INTERVAL_MS = Number(
  import.meta.env.VITE_FALLBACK_POLL_INTERVAL_MS ?? 90_000,
);
