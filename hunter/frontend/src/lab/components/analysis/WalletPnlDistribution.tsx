import { memo, useMemo } from 'react';
import { PnlDistribution } from 'components/analytics/PnlDistribution';
import { usePnlDistDensity } from 'hooks/usePnlDistDensity';
import { pnlDistributionBuckets } from './walletPnlStats';
import type { TraderTokenRow } from 'types';

interface WalletPnlDistributionProps {
  rows: readonly TraderTokenRow[];
  height?: number;
}

/**
 * Win/loss size distribution over `wallet_realized_pnl_pct`. Thin adapter: the
 * bucketing and the bars are the shared `components/analytics` pair, so this
 * histogram and the live Console History one are the same chart with the same
 * density presets. Rows with no matched cost basis (pure open bags) are
 * excluded — see `pnlDistributionBuckets`.
 */
export const WalletPnlDistribution = memo(function WalletPnlDistribution({
  rows,
  height = 160,
}: WalletPnlDistributionProps) {
  const [density, setDensity] = usePnlDistDensity();
  const buckets = useMemo(() => pnlDistributionBuckets(rows, density), [rows, density]);
  return (
    <PnlDistribution
      buckets={buckets}
      height={height}
      density={density}
      onDensityChange={setDensity}
      emptyMessage="No closed round trips in this window (every row is still an open bag)."
    />
  );
});
