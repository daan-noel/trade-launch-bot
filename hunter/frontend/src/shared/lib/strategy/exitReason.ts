/**
 * Exit-reason display + filter vocabulary (SSOT).
 *
 * Persisted strings come from the engine (`TakeProfit`, `Metrics`, `Open`, …).
 * The Reason / Exit Reason columns show compact badges; filters must accept
 * either the stored name or the badge label (`TP`, `METRIC+`, `Open`).
 *
 * Metric-condition exits (`Metrics`) are further split by realized PnL sign so
 * wins (`METRIC+`) and losses (`METRIC-`) are glanceable — the engine stores
 * one reason; the UI overlays the outcome.
 */

/** Compact badge label for a persisted `exit_reason`. Still-open / null →
 *  `"Open"`. Unknown strings render as themselves (never silently relabeled
 *  Open — that made Metrics/Manual/Migrated unfilterable as "Open").
 *
 *  Pass `pnlSol` for `Metrics` rows so the badge splits win/loss
 *  (`METRIC+` / `METRIC-`); other reasons ignore it. */
export function exitReasonLabel(
  reason: string | null | undefined,
  pnlSol?: number | null,
): string {
  switch (reason) {
    case 'LiquidityExit':
      return 'LIQ';
    case 'TakeProfit':
      return 'TP';
    case 'StopLoss':
      return 'SL';
    case 'TrailingStop':
      return 'TRAIL';
    case 'Stall':
      return 'STALL';
    case 'TimeStop':
      return 'TIME';
    case 'ExitFailed':
      return 'FAIL';
    case 'Manual':
    case 'ManualClose':
      return 'MANUAL';
    case 'Dead':
      return 'DEAD';
    case 'Metrics':
      return metricsExitLabel(pnlSol);
    case 'Migrated':
      return 'MIG';
    case 'NoEntry':
      return 'No entry';
    case 'Open':
    case null:
    case undefined:
    case '':
      return 'Open';
    default:
      return reason;
  }
}

/** `Metrics` → `METRIC+` / `METRIC-` / `METRIC` from realized SOL PnL sign. */
export function metricsExitLabel(pnlSol?: number | null): string {
  if (pnlSol == null || !Number.isFinite(pnlSol) || pnlSol === 0) return 'METRIC';
  return pnlSol > 0 ? 'METRIC+' : 'METRIC-';
}

/** Per-column filter/search haystack: stored reason + badge label so both
 *  `TakeProfit` and `TP` (and `Open` for null) match on the client. Pass
 *  `pnlSol` so Metrics wins/losses are filterable as `METRIC+` / `METRIC-`. */
export function exitReasonSearchText(
  reason: string | null | undefined,
  pnlSol?: number | null,
): string {
  const stored = reason?.trim() ? reason.trim() : 'Open';
  const label = exitReasonLabel(reason, pnlSol);
  // Always keep bare `METRIC` in the haystack so a plain "metric" filter still
  // hits both win and loss Metrics rows.
  if (reason === 'Metrics' && label !== 'METRIC') {
    return `${stored} METRIC ${label}`;
  }
  return label === stored ? stored : `${stored} ${label}`;
}

/** Badge / loose labels → persisted `exit_reason` for server-side contains. */
const EXIT_REASON_FILTER_ALIASES: Readonly<Record<string, string>> = {
  tp: 'TakeProfit',
  takeprofit: 'TakeProfit',
  'take profit': 'TakeProfit',
  sl: 'StopLoss',
  stoploss: 'StopLoss',
  'stop loss': 'StopLoss',
  trail: 'TrailingStop',
  trailing: 'TrailingStop',
  trailingstop: 'TrailingStop',
  stall: 'Stall',
  time: 'TimeStop',
  timestop: 'TimeStop',
  liq: 'LiquidityExit',
  liquidity: 'LiquidityExit',
  liquidityexit: 'LiquidityExit',
  fail: 'ExitFailed',
  exitfailed: 'ExitFailed',
  manual: 'Manual',
  manualclose: 'Manual',
  dead: 'Dead',
  metric: 'Metrics',
  metrics: 'Metrics',
  // Signed Metrics badges — server only stores `Metrics`, so these collapse to
  // that; client `filterValue` still distinguishes METRIC+/METRIC- per row.
  'metric+': 'Metrics',
  'metrics+': 'Metrics',
  'metric-': 'Metrics',
  'metrics-': 'Metrics',
  mig: 'Migrated',
  migrated: 'Migrated',
  open: 'Open',
  noentry: 'NoEntry',
  'no entry': 'NoEntry',
  'not fired': 'NoEntry',
};

/** Map a Reason / Exit Reason filter needle to the persisted string when the
 *  user typed a badge abbrev (`TP` → `TakeProfit`). Unknown text is returned
 *  trimmed unchanged (substring contains still applies). */
export function normalizeExitReasonFilter(raw: string): string {
  const trimmed = raw.trim();
  if (!trimmed) return trimmed;
  return EXIT_REASON_FILTER_ALIASES[trimmed.toLowerCase()] ?? trimmed;
}

/** Column keys that filter on persisted `exit_reason`. */
export function isExitReasonFilterKey(key: string): boolean {
  return key === 'reason' || key === 'exit_reason';
}
