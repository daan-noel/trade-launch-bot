import type { TradeRecord } from 'types';

/**
 * Priority spend: what the sender paid to be early, across both rails, in lamports.
 *
 * A sender picks ONE thing — how much to spend to land first — and the chain offers
 * two ways to pay it. The compute rail bills `cu_limit x cu_price` (the price is in
 * micro-lamports per compute unit, hence the 1e6); the tip rail is a plain transfer
 * to a tip account. Some clients use one, some the other, some both, so reading
 * either alone reads a fraction of the number that was chosen.
 *
 * This is why the raw parts are not comparable between trades: the same 0.001 SOL
 * spend shows as `cu_price` 3,333,333 at a 300k limit and 10,000,000 at a 100k one.
 * Rank, filter, and compare on THIS value; show the parts only to explain it.
 *
 * `null` when the trade carries neither rail — i.e. nothing was captured, which is
 * every trade ingested before migration `0013_trade_fee_budget.sql` and can never be
 * backfilled. A rail that is present but zero contributes 0 and does not make the
 * result null: `tip_lamports === 0` is a real reading ("transfers, none to a
 * recognised tip account"), not a missing one.
 *
 * The compute product stays well inside `Number.MAX_SAFE_INTEGER` at any limit the
 * runtime accepts (1.4M CU) paired with a sane price; it would need a `cu_price`
 * above ~6e9 micro-lamports — a fee of thousands of SOL — to lose a digit.
 */
export function tradePriorityLamports(
  t: Pick<TradeRecord, 'cu_limit' | 'cu_price' | 'tip_lamports'>,
): number | null {
  const compute =
    t.cu_limit != null && t.cu_price != null
      ? Math.ceil((t.cu_limit * t.cu_price) / 1_000_000)
      : null;
  if (compute == null && t.tip_lamports == null) return null;
  return (compute ?? 0) + (t.tip_lamports ?? 0);
}

/** Lamports in one SOL — the display conversion for the two lamport-valued fee
 *  fields. Matches the backend's `lamports_to_sol`; the wire keeps lamports exact
 *  and only the cell renders SOL. */
export const LAMPORTS_PER_SOL = 1_000_000_000;

/** Priority spend as human SOL, for the SOL/USD fee cells. `null` propagates. */
export function tradePrioritySol(
  t: Pick<TradeRecord, 'cu_limit' | 'cu_price' | 'tip_lamports'>,
): number | null {
  const l = tradePriorityLamports(t);
  return l == null ? null : l / LAMPORTS_PER_SOL;
}

/** Tip as human SOL. `null` = no top-level transfer at all; `0` stays 0, because
 *  "transfers but no recognised tip" is a reading, not an absence. */
export function tradeTipSol(t: Pick<TradeRecord, 'tip_lamports'>): number | null {
  return t.tip_lamports == null ? null : t.tip_lamports / LAMPORTS_PER_SOL;
}
