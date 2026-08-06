/**
 * Map surface rows → analytics `PnlPoint` / hold-scatter inputs for the shared
 * position summary charts. Open rows use unrealized PnL when present (`isOpen`).
 */

import type { PnlPoint } from 'components/analytics/pnlSeries';
import { buildHoldScatterPoints, type HoldScatterPoint } from 'components/analytics/pnlSeries';

/** Compact per-position atom every surface can project into for charts. */
export interface PositionChartPoint {
  key: string;
  label: string;
  mint_address: string;
  /** Exit time for closed; entry/created for open marks. Epoch ms. */
  timeMs: number;
  pnlSol: number;
  pnlPct: number | null;
  holdSeconds: number | null;
  entrySol: number;
  isOpen: boolean;
  exit_reason?: string | null;
  is_migrated?: boolean | null;
}

export function toPnlPoints(points: readonly PositionChartPoint[]): PnlPoint[] {
  return points.map((p) => ({
    key: p.key,
    timeMs: p.timeMs,
    pnlSol: p.pnlSol,
    pnlPct: p.pnlPct,
    label: p.label,
    isOpen: p.isOpen,
  }));
}

export function toHoldScatterPoints(points: readonly PositionChartPoint[]): HoldScatterPoint[] {
  return buildHoldScatterPoints(
    points.map((p) => ({
      key: p.key,
      label: p.label,
      holdSeconds: p.holdSeconds,
      pnlPct: p.pnlPct,
      sizeSol: p.entrySol,
      pnlSol: p.pnlSol,
      isWin: p.pnlSol > 0,
    })),
  );
}
