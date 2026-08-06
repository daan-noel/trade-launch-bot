/**
 * Console History exit-mix strip — fold the B2 closes-series into the shared
 * exit breakdown (same tiles / mix bar as Position Summary), and map tile
 * clicks onto `hexit` so they **compose** with chart `hfocus` (day / heat /
 * pct / …) instead of replacing it.
 *
 * Metric± use synthetic needles (`metric_win` / `metric_loss` / `metric`) —
 * client-only, not SQL contains — because a substring can't express the
 * win/loss split. Legacy `hfocus=exit:…` still highlights the matching tile.
 */

import {
  runSummaryFromRows,
  type RunOutcomeRow,
  type RunSummary,
} from 'lib/strategy/runSummary';
import { parseMetricExitParts } from 'lib/strategy/exitReason';
import type { ClosedTradePoint } from '@live/store/liveEndpoints';
import type { HistoryFocus } from './historyFocus';

/** Map a closes-series point onto the shared client summary row. */
export function closeToRunOutcome(c: ClosedTradePoint): RunOutcomeRow {
  return {
    fired: true,
    exit: c.exit_reason ?? 'Other',
    pnl_sol: c.pnl_sol,
    pnl_pct: c.entry_sol > 0 ? (c.pnl_sol / c.entry_sol) * 100 : 0,
    holding_secs: c.hold_secs ?? 0,
  };
}

/** Fold the History cohort closes into a `RunSummary` (realized band only matters). */
export function historyRunSummaryFromCloses(
  closes: readonly ClosedTradePoint[],
): RunSummary {
  return runSummaryFromRows(closes.map(closeToRunOutcome));
}

/**
 * Tile label from {@link runSummarySections} → `hexit` needle.
 * `Other` and unknown labels return null (no safe needle).
 */
export function exitTileToHistoryNeedle(tileLabel: string): string | null {
  switch (tileLabel) {
    case 'Take profit':
      return 'TakeProfit';
    case 'Stop loss':
      return 'StopLoss';
    case 'Dead':
      return 'Dead';
    case 'Manual':
      return 'Manual';
    case 'Trailing':
      return 'trail';
    case 'Stall':
      return 'stall';
    case 'Time':
      return 'time';
    case 'Liquidity':
      return 'liquidity';
    case 'Metric+':
      return 'metric_win';
    case 'Metric-':
      return 'metric_loss';
    case 'Metric':
      return 'metric';
    case 'Other':
      return null;
    default:
      // Detailed metric labels (`stall > 300`) — contains needle matches exactly
      // that stored reason (and no shorter substring false-positives when the
      // full spaced form is used).
      return tileLabel;
  }
}

/** @deprecated use {@link exitTileToHistoryNeedle} */
export function exitTileClickAction(
  tileLabel: string,
): { channel: 'filter'; needle: string } | null {
  const needle = exitTileToHistoryNeedle(tileLabel);
  return needle ? { channel: 'filter', needle } : null;
}

/** Which exit tile (if any) is active given the current cohort channels. */
export function activeExitTileLabel(
  exitReason: string | null | undefined,
  focus: HistoryFocus | null | undefined,
): string | null {
  // Legacy `hfocus=exit:…` deep-links still highlight the tile.
  if (focus?.kind === 'exit') return focus.tile;
  switch (exitReason) {
    case 'TakeProfit':
      return 'Take profit';
    case 'StopLoss':
      return 'Stop loss';
    case 'Dead':
      return 'Dead';
    case 'Manual':
      return 'Manual';
    case 'trail':
      return 'Trailing';
    case 'stall':
      return 'Stall';
    case 'time':
      return 'Time';
    case 'liquidity':
      return 'Liquidity';
    case 'metric_win':
      return 'Metric+';
    case 'metric_loss':
      return 'Metric-';
    case 'metric':
      return 'Metric';
    default:
      // Exact metric-condition labels from the detailed exit mix (`stall > 300`).
      // Metric-name dropdown needles (`pnl`, `trail`) are not tile labels.
      return exitReason && parseMetricExitParts(exitReason) ? exitReason : null;
  }
}
