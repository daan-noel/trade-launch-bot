import { useEffect, useRef } from 'react';
import { useDispatch, useStore } from 'react-redux';
import { useUsdRate } from 'context/PriceUnitContext';
import { startWalletMarksLive } from '@live/lib/walletMarksLive';
import type { AppDispatch, RootState } from '@live/store';

/**
 * Tip Wallet/Home marks from `trade_executed` (SOL spot → USD via header rate).
 * Jupiter poll stays as a slow oracle for liquidity/24h and cold mints.
 */
export function useWalletMarksLive(): void {
  const dispatch = useDispatch<AppDispatch>();
  const store = useStore<RootState>();
  const { usdRate } = useUsdRate();
  const usdRateRef = useRef(usdRate);
  usdRateRef.current = usdRate;

  useEffect(() => {
    return startWalletMarksLive(dispatch, store.getState, () => usdRateRef.current);
  }, [dispatch, store]);
}
