import { memo, useMemo } from 'react';
import { HoldPnlScatter } from 'components/analytics/HoldPnlScatter';
import { buildHoldScatter } from './walletPnlStats';
import type { TraderTokenRow } from 'types';

interface WalletHoldScatterProps {
  rows: readonly TraderTokenRow[];
  width?: number;
  height?: number;
}

/**
 * Hold-time vs realized PnL% scatter for Trader Analysis. Thin adapter over
 * the shared `HoldPnlScatter` — points come from `buildHoldScatter`.
 */
export const WalletHoldScatter = memo(function WalletHoldScatter({
  rows,
  width = 640,
  height = 280,
}: WalletHoldScatterProps) {
  const points = useMemo(() => buildHoldScatter(rows), [rows]);
  return <HoldPnlScatter points={points} width={width} height={height} />;
});
