// Trader Analysis — wallet-level PnL analytics. Pure, DB-free helpers that fold
// the already-fetched `TraderTokenRow[]` (one row per mint the wallet traded,
// enriched with `kernel::wallet_mint_pnl`'s reconstructed PnL — see
// `hunter/lab/src/api/handlers/wallets.rs`) into the summary stat cards + the
// analytics panel's five chart views. No network calls here: everything below
// re-derives from the SAME rows the token table already has, so filtering the
// table (via `TokenTable`'s `onFilteredRowsChange`) re-derives every chart for
// free.
//
// Grain caveat (deliberate — see the Trader Analysis design decision): these
// rows are per-MINT aggregates, not per-episode. A wallet that re-entered a mint
// many times collapses to one row here, so `wallet_hold_seconds` below is a
// first-trade→last-trade span across ALL episodes on that mint, not any single
// episode's hold — read it as "how long this mint stayed on the wallet's radar",
// not a scalp duration.

// The generic folds (equity curve, distribution buckets, day×hour heatmap,
// ranking) now live in `components/analytics/pnlSeries` — shared with the live
// app's Console History / Portfolio decks. What stays here is the wallet-
// specific part: the `TraderTokenRow` → `PnlPoint` mapping and the wallet
// summary/scatter that have no live counterpart.
import {
  DOW_ROWS,
  HOURS,
  buildEquityCurve as buildEquityCurveGeneric,
  buildHoldScatterPoints,
  buildPnlHeatCells as buildPnlHeatCellsGeneric,
  dowHourInTz,
  pnlDistributionBuckets as pnlDistributionBucketsGeneric,
  type PnlDistDensity,
  type EquityPoint,
  type HoldScatterPoint,
  type PnlBucket,
  type PnlHeatCell,
  type PnlPoint,
  type RankedBarRow,
} from 'components/analytics/pnlSeries';
import type { TraderTokenRow } from 'types';

export { DOW_ROWS, HOURS, dowHourInTz };
export type { EquityPoint, HoldScatterPoint, PnlBucket, PnlHeatCell };

/** First→last trade span (seconds) for one mint in the window. `null` when
 *  either timestamp is missing/invalid. Not a single-episode hold — see the
 *  grain caveat at the top of this file. */
export function walletHoldSeconds(r: TraderTokenRow): number | null {
  const firstMs = r.wallet_first_trade_at_ms ?? Date.parse(r.wallet_first_trade_at);
  const lastMs = r.wallet_last_trade_at_ms ?? Date.parse(r.wallet_last_trade_at);
  if (!Number.isFinite(firstMs) || !Number.isFinite(lastMs)) return null;
  return (lastMs - firstMs) / 1000;
}

/**
 * The wallet grain → the shared `PnlPoint` grain.
 *
 * Bucketed by the mint's most-recent trade in the window
 * (`wallet_last_trade_at_ms`) — the per-mint grain's only single instant that
 * reads as "when this position was decided" (an exit for a closed mint, the
 * latest re-entry for an open one).
 */
/** Wallet grain → shared `PnlPoint` (decision instant = last trade in window). */
export function toPnlPoints(rows: readonly TraderTokenRow[]): PnlPoint[] {
  return rows.map((r) => ({
    key: r.mint_address,
    timeMs: r.wallet_last_trade_at_ms ?? Date.parse(r.wallet_last_trade_at),
    pnlSol: r.wallet_total_pnl_sol,
    pnlPct: r.wallet_realized_pnl_pct,
    label: r.symbol || r.name || r.mint_address,
    isOpen: r.wallet_is_open,
  }));
}

// ── summary stats ───────────────────────────────────────────────────────────

export interface WalletPnlSummary {
  tokenCount: number;
  openCount: number;
  closedCount: number;
  partialDataCount: number;
  /** Only rows with a matched cost basis (i.e. a real win/loss verdict) count
   *  toward win/loss — a mint that's only ever been an open bag with no sells
   *  is neither. */
  winCount: number;
  lossCount: number;
  winRate: number | null;
  totalRealizedPnlSol: number;
  totalRealizedPnlSolNetOfFee: number;
  totalUnrealizedPnlSol: number;
  totalPnlSol: number;
  totalVolumeSol: number;
  avgWinSol: number | null;
  avgLossSol: number | null;
  /** `avgWinSol / |avgLossSol|` — the payoff ratio; the wallet-analysis doc's
   *  own finding is that this, not win rate, is what separates a good cohort
   *  from a bad one at the same hit rate. `null` when there are no losses to
   *  divide by (can't compute, not "infinite edge"). */
  payoffRatio: number | null;
  /** Σ wins / Σ|losses| over the matched (realized) portion only. `null` with
   *  no losses. */
  profitFactor: number | null;
}

const EMPTY_SUMMARY: WalletPnlSummary = {
  tokenCount: 0,
  openCount: 0,
  closedCount: 0,
  partialDataCount: 0,
  winCount: 0,
  lossCount: 0,
  winRate: null,
  totalRealizedPnlSol: 0,
  totalRealizedPnlSolNetOfFee: 0,
  totalUnrealizedPnlSol: 0,
  totalPnlSol: 0,
  totalVolumeSol: 0,
  avgWinSol: null,
  avgLossSol: null,
  payoffRatio: null,
  profitFactor: null,
};

export function computeWalletSummary(rows: readonly TraderTokenRow[]): WalletPnlSummary {
  if (rows.length === 0) return EMPTY_SUMMARY;

  let openCount = 0;
  let partialDataCount = 0;
  let winCount = 0;
  let lossCount = 0;
  let totalRealized = 0;
  let totalRealizedNet = 0;
  let totalUnrealized = 0;
  let totalVolume = 0;
  let sumWinSol = 0;
  let sumLossSol = 0; // stored positive (magnitude)

  for (const r of rows) {
    if (r.wallet_is_open) openCount++;
    if (r.wallet_partial_data) partialDataCount++;
    totalRealized += r.wallet_realized_pnl_sol;
    totalRealizedNet += r.wallet_realized_pnl_sol_net_of_fee;
    totalUnrealized += r.wallet_unrealized_pnl_sol ?? 0;
    totalVolume += r.wallet_buy_sol + r.wallet_sell_sol;
    // A win/loss verdict needs a matched cost basis (realized_pnl_pct != null);
    // an open-only bag with no sells yet is neither a win nor a loss.
    if (r.wallet_realized_pnl_pct != null) {
      if (r.wallet_realized_pnl_sol > 0) {
        winCount++;
        sumWinSol += r.wallet_realized_pnl_sol;
      } else if (r.wallet_realized_pnl_sol < 0) {
        lossCount++;
        sumLossSol += -r.wallet_realized_pnl_sol;
      }
    }
  }

  const decided = winCount + lossCount;
  return {
    tokenCount: rows.length,
    openCount,
    closedCount: rows.length - openCount,
    partialDataCount,
    winCount,
    lossCount,
    winRate: decided > 0 ? (winCount / decided) * 100 : null,
    totalRealizedPnlSol: totalRealized,
    totalRealizedPnlSolNetOfFee: totalRealizedNet,
    totalUnrealizedPnlSol: totalUnrealized,
    totalPnlSol: totalRealized + totalUnrealized,
    totalVolumeSol: totalVolume,
    avgWinSol: winCount > 0 ? sumWinSol / winCount : null,
    avgLossSol: lossCount > 0 ? -(sumLossSol / lossCount) : null,
    payoffRatio: winCount > 0 && lossCount > 0 ? sumWinSol / winCount / (sumLossSol / lossCount) : null,
    profitFactor: sumLossSol > 0 ? sumWinSol / sumLossSol : null,
  };
}

// ── the promoted folds, adapted to the wallet grain ─────────────────────────
//
// Each is a one-line map into `PnlPoint` + the shared fold. Keeping the
// wallet-named wrappers means the lab call sites and their doc comments stay
// put; keeping the fold in `components/analytics` means the live decks and this
// page can't drift into two different definitions of "equity curve".

/** Day×hour grid of `wallet_total_pnl_sol`. See `toPnlPoints` for the instant
 *  each mint is bucketed by (and its per-mint-grain caveat). */
export function buildPnlHeatCells(
  rows: readonly TraderTokenRow[],
  timeZone: string,
): PnlHeatCell[] {
  return buildPnlHeatCellsGeneric(toPnlPoints(rows), timeZone);
}

/** Rows as ranked bars, best first, on `wallet_total_pnl_sol`. */
export function rankedPnlBarRows(rows: readonly TraderTokenRow[]): RankedBarRow[] {
  return rows.map((r) => ({
    key: r.mint_address,
    label: r.symbol || r.name || r.mint_address.slice(0, 8),
    value: r.wallet_total_pnl_sol,
    tag: r.wallet_is_open ? 'open' : null,
    title: r.mint_address,
  }));
}

/** Count histogram over `wallet_realized_pnl_pct` (open-only bags have no
 *  realized % and are excluded). */
export function pnlDistributionBuckets(
  rows: readonly TraderTokenRow[],
  density: PnlDistDensity = 'default',
): PnlBucket[] {
  return pnlDistributionBucketsGeneric(toPnlPoints(rows), density);
}

/** Cumulative `wallet_total_pnl_sol` ordered by each mint's most-recent trade. */
export function buildEquityCurve(rows: readonly TraderTokenRow[]): EquityPoint[] {
  return buildEquityCurveGeneric(toPnlPoints(rows));
}

// ── hold-time vs PnL% scatter ────────────────────────────────────────────────

/** One point per row with BOTH a positive hold span and a realized verdict
 *  (rows that are pure open bags with no matched cost basis have no `pnlPct`
 *  to plot). `sizeSol` (total volume) drives the marker radius. */
export function buildHoldScatter(rows: readonly TraderTokenRow[]): HoldScatterPoint[] {
  return buildHoldScatterPoints(
    rows.map((r) => ({
      key: r.mint_address,
      label: r.symbol || r.name || r.mint_address,
      holdSeconds: walletHoldSeconds(r),
      pnlPct: r.wallet_realized_pnl_pct,
      sizeSol: r.wallet_buy_sol + r.wallet_sell_sol,
      pnlSol: r.wallet_realized_pnl_sol,
      isWin: r.wallet_realized_pnl_sol > 0,
    })),
  );
}
