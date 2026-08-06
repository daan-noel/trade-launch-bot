import { memo } from 'react';
import { PnlHeatmap } from 'components/analytics/PnlHeatmap';
import type { PnlHeatCell } from './walletPnlStats';

interface WalletPnlHeatmapProps {
  cells: PnlHeatCell[];
}

/**
 * Day-of-week × hour-of-day heatmap of this wallet's PnL. Thin adapter over the
 * shared `components/analytics/PnlHeatmap`; the cells come from
 * `buildPnlHeatCells` (see its doc comment for the per-mint-grain caveat: a cell
 * counts MINTS decided in that slot, not individual trades).
 */
export const WalletPnlHeatmap = memo(function WalletPnlHeatmap({ cells }: WalletPnlHeatmapProps) {
  return <PnlHeatmap cells={cells} unitLabel="token" />;
});
