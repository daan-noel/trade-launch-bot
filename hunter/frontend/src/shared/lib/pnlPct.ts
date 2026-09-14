/**
 * PnL% helpers, money basis only: PnL over the SOL the wallet paid, the same
 * quantity as the backend's `pnl_percent`. A missing money basis is `null`
 * (rendered as a dash), never a price ratio: `(exit - entry) / entry` charges no
 * fee or fixed cost and can read green on a trade that lost SOL.
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

/**
 * One sell leg's PnL% on the money basis: the SOL the leg received against its
 * share of the SOL the wallet paid at entry, `entrySol × legTokens / entryTokens`.
 * The legs of a fully sold bag sum to the position's `pnl_sol`.
 */
export function legPnlPctFromSol(
  legSol: number | null | undefined,
  legTokens: number | null | undefined,
  entrySol: number | null | undefined,
  entryTokens: number | null | undefined,
): number | null {
  if (legSol == null || legTokens == null || entrySol == null || entryTokens == null) return null;
  if (!Number.isFinite(legSol) || !(legTokens > 0) || !(entryTokens > 0)) return null;
  const cost = entrySol * (legTokens / entryTokens);
  return pnlPctFromSol(legSol - cost, cost);
}
