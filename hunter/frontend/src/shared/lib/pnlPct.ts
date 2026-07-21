/**
 * PnL% helpers — prefer SOL-basis (`pnl / entry`) when both legs known; fall
 * back to price-basis `(exit - entry) / entry`. Matches Evidence cells which
 * color off SOL PnL but display price-basis `pnl_percent` from the backend.
 */

/** `(pnlSol / entrySol) × 100` when entry is a positive finite stake. */
export function pnlPctFromSol(
  pnlSol: number | null | undefined,
  entrySol: number | null | undefined,
): number | null {
  if (pnlSol == null || entrySol == null) return null;
  if (!Number.isFinite(pnlSol) || !Number.isFinite(entrySol) || entrySol <= 0) return null;
  return (pnlSol / entrySol) * 100;
}

/** `((exit - entry) / entry) × 100` when entry is a positive finite price. */
export function pnlPctFromPrices(
  entryPrice: number | null | undefined,
  exitPrice: number | null | undefined,
): number | null {
  if (entryPrice == null || exitPrice == null) return null;
  if (!Number.isFinite(entryPrice) || !Number.isFinite(exitPrice) || entryPrice <= 0) return null;
  return ((exitPrice - entryPrice) / entryPrice) * 100;
}

/** SOL-basis first, then price-basis. */
export function resolvePnlPct(opts: {
  pnlSol?: number | null;
  entrySol?: number | null;
  entryPrice?: number | null;
  exitPrice?: number | null;
}): number | null {
  return (
    pnlPctFromSol(opts.pnlSol, opts.entrySol) ??
    pnlPctFromPrices(opts.entryPrice, opts.exitPrice)
  );
}
