import { memo, useMemo } from 'react';
import { RankedPnlBars } from 'components/analytics/RankedPnlBars';
import { rankedPnlBarRows } from './walletPnlStats';
import type { TraderTokenRow } from 'types';

interface WalletRankedPnlBarsProps {
  rows: readonly TraderTokenRow[];
  /** Show the best/worst `maxEachSide` rows on either end (default 15) rather
   *  than every row — a 300-token wallet would otherwise render an unreadable
   *  wall of bars. */
  maxEachSide?: number;
}

/**
 * Per-token PnL ranked best → worst. Thin adapter over the shared
 * `components/analytics/RankedPnlBars`; ranked on `wallet_total_pnl_sol`
 * (mark-to-market), not win rate — see the wallet-analysis doc's finding that
 * ranking by hit rate alone can surface the worst-expectancy cohort.
 */
export const WalletRankedPnlBars = memo(function WalletRankedPnlBars({
  rows,
  maxEachSide = 15,
}: WalletRankedPnlBarsProps) {
  const bars = useMemo(() => rankedPnlBarRows(rows), [rows]);
  return <RankedPnlBars rows={bars} maxEachSide={maxEachSide} emptyMessage="No tokens to rank." />;
});
