import { useCallback, useEffect, useRef } from 'react';
import { useDispatch } from 'react-redux';
import { connectArmedChanged, connectStrategyPositionUpdate } from 'services/sse';
import { sharedApi } from 'store/sharedEndpoints';
import { liveApi } from '@live/store/liveEndpoints';
import {
  applyArmedDelta,
  applyPositionDelta,
  applySnapshot,
  snapshotFailed,
  snapshotStart,
} from '@live/slices/liveStatusSlice';
import type { AppDispatch } from '@live/store';

/**
 * App-wide Live Status SSOT bootstrap — one mount in live `App`.
 * Snapshot (armed + open + recent closes) on mount / SSE reconnect / tab visible /
 * sse_resync; deltas patch the `liveStatus` slice in place. Pages must read the
 * slice, not own a parallel Map.
 */
export function useLiveStatusBootstrap(): void {
  const dispatch = useDispatch<AppDispatch>();
  const inflight = useRef<AbortController | null>(null);

  const refetchSnapshot = useCallback(async () => {
    inflight.current?.abort();
    const ac = new AbortController();
    inflight.current = ac;
    dispatch(snapshotStart());
    try {
      const [armedRes, posRes, recentRes, rulesRes] = await Promise.all([
        dispatch(liveApi.endpoints.getArmed.initiate(undefined, { forceRefetch: true })),
        // `false` = real + paper so the store is mode-complete; UI filters.
        dispatch(liveApi.endpoints.getPortfolioPositions.initiate(false, { forceRefetch: true })),
        dispatch(liveApi.endpoints.getPortfolioRecentCloses.initiate(50, { forceRefetch: true })),
        dispatch(sharedApi.endpoints.getStrategyRules.initiate(undefined)),
      ]);
      if (ac.signal.aborted) return;
      if (armedRes.error || posRes.error) {
        dispatch(snapshotFailed());
        return;
      }
      const ruleNames: Record<string, string> = {};
      for (const r of rulesRes.data ?? []) {
        ruleNames[r.id] = r.rule_name;
      }
      dispatch(
        applySnapshot({
          armed: armedRes.data ?? [],
          positions: posRes.data ?? [],
          recentClosed: recentRes.error ? undefined : (recentRes.data ?? []),
          ruleNames,
        }),
      );
    } catch {
      if (!ac.signal.aborted) dispatch(snapshotFailed());
    }
  }, [dispatch]);

  useEffect(() => {
    void refetchSnapshot();
    return () => {
      inflight.current?.abort();
    };
  }, [refetchSnapshot]);

  useEffect(() => {
    const armedH = connectArmedChanged(
      (d) => {
        dispatch(applyArmedDelta(d));
      },
      () => {
        void refetchSnapshot();
      },
    );
    const posH = connectStrategyPositionUpdate(
      (d) => {
        dispatch(applyPositionDelta(d));
      },
      () => {
        void refetchSnapshot();
      },
    );
    return () => {
      armedH.close();
      posH.close();
    };
  }, [dispatch, refetchSnapshot]);

  useEffect(() => {
    const onVis = () => {
      if (document.visibilityState === 'visible') void refetchSnapshot();
    };
    document.addEventListener('visibilitychange', onVis);
    return () => document.removeEventListener('visibilitychange', onVis);
  }, [refetchSnapshot]);
}
