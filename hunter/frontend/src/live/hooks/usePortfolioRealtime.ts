import { useCallback, useEffect, useRef } from 'react';
import { useDispatch } from 'react-redux';
import {
  connectStrategyPositionUpdate,
  connectTradeStream,
  onSseReopen,
} from 'services/sse';
import { useGetProfilesQuery } from 'store/sharedEndpoints';
import { notifyPortfolioBagRefresh } from '@live/lib/portfolioBagRefresh';
import { liveApi } from '@live/store/liveEndpoints';
import type { AppDispatch } from '@live/store';
import type { LiveTrade } from 'types';

const COALESCE_MS = 500;

/** Statuses that imply the on-chain bag / PnL totals may have changed. */
const BAG_CHANGING = new Set(['Holding', 'End', 'ExitFailed']);

/** Terminal closes that change *closed-trade* performance (both real + paper). */
const PERF_CHANGING = new Set(['End', 'ExitFailed']);

/**
 * Keep WalletHoldings-tagged RTK data (Home KPIs, Top Holdings, portfolio
 * summary) fresh from the same on-chain / engine notify path as Ops:
 *   - `strategy_position_update` (real bag-changing statuses only)
 *   - `trade_executed` for wallets in the `mine` profile → our fills
 *   - SSE reconnect / tab visible → catch-up invalidate
 *
 * Also fans out to imperative Wallet reload via {@link notifyPortfolioBagRefresh}
 * so that page does not open a second SSE filter for the same signals.
 *
 * Invalidate (not patch) for aggregates — holdings/summary are RPC-composed.
 * Per-mint display marks tip separately via `useWalletMarksLive`.
 */
export function usePortfolioRealtime(): void {
  const dispatch = useDispatch<AppDispatch>();
  const { data: profiles } = useGetProfilesQuery();
  const mineWalletsRef = useRef<Set<string>>(new Set());
  mineWalletsRef.current = new Set(
    (profiles ?? [])
      .filter((p) => p.profile_type === 'mine')
      .flatMap((p) => p.wallets.map((w) => w.address)),
  );

  const timerRef = useRef<number | undefined>(undefined);
  const bump = useCallback(() => {
    dispatch(liveApi.util.invalidateTags(['WalletHoldings']));
    notifyPortfolioBagRefresh();
  }, [dispatch]);

  const scheduleBump = useCallback(() => {
    if (timerRef.current !== undefined) return;
    timerRef.current = window.setTimeout(() => {
      timerRef.current = undefined;
      bump();
    }, COALESCE_MS);
  }, [bump]);

  // Lightweight paper-close refresh: only re-fetch the Portfolio performance
  // read, NOT the real-wallet holdings/summary RPC scans (paper never touches
  // the on-chain bag). Kept on its own coalesce timer so it can't be starved by
  // — or starve — the heavier real bag bump above.
  const perfTimerRef = useRef<number | undefined>(undefined);
  const schedulePerfBump = useCallback(() => {
    if (perfTimerRef.current !== undefined) return;
    perfTimerRef.current = window.setTimeout(() => {
      perfTimerRef.current = undefined;
      dispatch(liveApi.util.invalidateTags(['PortfolioPerf']));
    }, COALESCE_MS);
  }, [dispatch]);

  useEffect(() => {
    const posH = connectStrategyPositionUpdate((d) => {
      if (d.trade_mode === 'paper') {
        // Paper closes don't move the wallet bag — just refresh closed PnL.
        if (PERF_CHANGING.has(d.status)) schedulePerfBump();
        return;
      }
      if (!BAG_CHANGING.has(d.status)) return;
      scheduleBump();
    });
    const tradeH = connectTradeStream((raw) => {
      try {
        const t = JSON.parse(raw) as LiveTrade;
        if (mineWalletsRef.current.has(t.wallet)) scheduleBump();
      } catch {
        /* ignore */
      }
    });
    const reopenUnsub = onSseReopen(() => bump());
    return () => {
      window.clearTimeout(timerRef.current);
      timerRef.current = undefined;
      window.clearTimeout(perfTimerRef.current);
      perfTimerRef.current = undefined;
      posH.close();
      tradeH.close();
      reopenUnsub();
    };
  }, [bump, scheduleBump, schedulePerfBump]);

  useEffect(() => {
    const onVis = () => {
      if (document.visibilityState === 'visible') bump();
    };
    document.addEventListener('visibilitychange', onVis);
    return () => document.removeEventListener('visibilitychange', onVis);
  }, [bump]);
}
