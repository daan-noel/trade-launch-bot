import { useEffect } from 'react';
import { useDispatch } from 'react-redux';
import type { AppDispatch } from 'store/types';
import {
  bindTokenTradesLiveDispatch,
  watchTokenTradesMint,
} from 'lib/tokenTradesLive';

/**
 * App-wide: bind RTK dispatch so chart mint watches can patch `getTokenTrades`.
 * Mount once in live (and lab if charts should tick — lab has no trade SSE).
 */
export function useTokenTradesLiveBootstrap(): void {
  const dispatch = useDispatch<AppDispatch>();
  useEffect(() => bindTokenTradesLiveDispatch(dispatch), [dispatch]);
}

/**
 * While mounted, append `trade_executed` frames for `mint` into the RTK
 * `getTokenTrades` cache. No-op when mint is empty.
 */
export function useWatchTokenTradesLive(mint: string | undefined | null): void {
  useEffect(() => {
    if (!mint) return;
    return watchTokenTradesMint(mint);
  }, [mint]);
}
