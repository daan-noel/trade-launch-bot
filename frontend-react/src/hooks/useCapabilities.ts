import { useGetCapabilitiesQuery } from 'store/apiSlice';

/**
 * Boot-time backend feature flags (deploy = live trading, local = analysis),
 * fetched once and shared via the RTK Query cache. Used to gate nav + lazy
 * routes so one SPA build serves either backend (T16).
 *
 * Until the fetch resolves, both flags default to `false` and `ready` is `false`
 * — callers hold the gated UI back rather than flash a full nav that then prunes
 * on the restricted box. If the request errors (e.g. an older backend without the
 * endpoint), both flags fail **open** to `true` so the SPA stays fully usable.
 */
export function useCapabilities(): {
  hasLiveTrading: boolean;
  hasAnalysis: boolean;
  ready: boolean;
} {
  const { data, isError } = useGetCapabilitiesQuery();
  if (isError) return { hasLiveTrading: true, hasAnalysis: true, ready: true };
  return {
    hasLiveTrading: data?.has_live_trading ?? false,
    hasAnalysis: data?.has_analysis ?? false,
    ready: data !== undefined,
  };
}
