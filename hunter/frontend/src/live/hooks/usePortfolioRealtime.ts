import { useCallback, useEffect, useRef } from 'react';
import { useDispatch } from 'react-redux';
import {
  connectStrategyPositionUpdate,
  connectTradeStream,
  onSseReopen,
} from 'services/sse';
import { useGetProfilesQuery } from 'store/sharedEndpoints';
import { liveApi } from '@live/store/liveEndpoints';
import type { AppDispatch } from '@live/store';
import type { LiveTrade } from 'types';

const COALESCE_MS = 500;

/** Statuses that imply the on-chain bag / PnL totals may have changed. */
const BAG_CHANGING = new Set(['Holding', 'End', 'ExitFailed']);

/**
 * Keep WalletHoldings-tagged RTK data (Home KPIs, Top Holdings, portfolio
 * summary) fresh from the same on-chain / engine notify path as Ops:
 *   - `strategy_position_update` (real bag-changing statuses only)
 *   - `trade_executed` for wallets in the `mine` profile → our fills
 *   - SSE reconnect / tab visible → catch-up invalidate
 *
 * Invalidate (not patch) — holdings/summary are RPC-composed; coalesced so a
 * burst of exits doesn't hammer the scan. Avoids a second holdings model.
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
  const invalidate = useCallback(() => {
    dispatch(liveApi.util.invalidateTags(['WalletHoldings']));
  }, [dispatch]);

  const scheduleInvalidate = useCallback(() => {
    if (timerRef.current !== undefined) return;
    timerRef.current = window.setTimeout(() => {
      timerRef.current = undefined;
      invalidate();
    }, COALESCE_MS);
  }, [invalidate]);

  useEffect(() => {
    const posH = connectStrategyPositionUpdate((d) => {
      if (d.trade_mode === 'paper') return;
      if (!BAG_CHANGING.has(d.status)) return;
      scheduleInvalidate();
    });
    const tradeH = connectTradeStream((raw) => {
      try {
        const t = JSON.parse(raw) as LiveTrade;
        if (mineWalletsRef.current.has(t.wallet)) scheduleInvalidate();
      } catch {
        /* ignore */
      }
    });
    const reopenUnsub = onSseReopen(() => invalidate());
    return () => {
      window.clearTimeout(timerRef.current);
      timerRef.current = undefined;
      posH.close();
      tradeH.close();
      reopenUnsub();
    };
  }, [invalidate, scheduleInvalidate]);

  useEffect(() => {
    const onVis = () => {
      if (document.visibilityState === 'visible') invalidate();
    };
    document.addEventListener('visibilitychange', onVis);
    return () => document.removeEventListener('visibilitychange', onVis);
  }, [invalidate]);
}
