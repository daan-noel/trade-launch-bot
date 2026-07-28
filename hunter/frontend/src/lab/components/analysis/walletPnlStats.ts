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

import { DOW_ROWS, HOURS } from 'components/creation-stats/creationStats';
import type { TraderTokenRow } from 'types';

export { DOW_ROWS, HOURS };

// ── time bucketing ──────────────────────────────────────────────────────────

const WEEKDAY_TO_DOW: Record<string, number> = {
  Sun: 0,
  Mon: 1,
  Tue: 2,
  Wed: 3,
  Thu: 4,
  Fri: 5,
  Sat: 6,
};

const dtfCache = new Map<string, Intl.DateTimeFormat>();

/** Day-of-week (0=Sun..6=Sat) + hour-of-day (0..23) for an instant, in
 *  `timeZone`'s local wall-clock. Caches the `Intl.DateTimeFormat` per timezone
 *  (mirrors `utils/date.ts`'s formatter cache) since this runs once per row per
 *  heatmap rebuild, not per render. */
export function dowHourInTz(ms: number, timeZone: string): { dow: number; hour: number } {
  let dtf = dtfCache.get(timeZone);
  if (!dtf) {
    dtf = new Intl.DateTimeFormat('en-US', {
      timeZone,
      weekday: 'short',
      hour: '2-digit',
      hour12: false,
    });
    dtfCache.set(timeZone, dtf);
  }
  const parts = dtf.formatToParts(new Date(ms));
  const weekday = parts.find((p) => p.type === 'weekday')?.value ?? 'Sun';
  // `hour12: false` still renders midnight as "24" in some locales/engines —
  // normalize into 0..23 rather than trust the raw string.
  const hourRaw = Number(parts.find((p) => p.type === 'hour')?.value ?? '0');
  const hour = ((hourRaw % 24) + 24) % 24;
  return { dow: WEEKDAY_TO_DOW[weekday] ?? 0, hour };
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

// ── PnL heatmap (day-of-week × hour-of-day) ─────────────────────────────────

export interface PnlHeatCell {
  dow: number;
  hour: number;
  pnl_sol: number;
  count: number;
}

/**
 * Fold every row's `wallet_total_pnl_sol` into a 7×24 day×hour grid, bucketed
 * by the mint's most-recent trade in the window (`wallet_last_trade_at_ms`) —
 * the per-mint grain's only single instant that reads as "when this position
 * was decided" (an exit for a closed mint, the latest re-entry for an open
 * one). Always returns all 168 cells (zero-filled) so the heatmap component
 * never has to synthesize missing slots.
 */
export function buildPnlHeatCells(
  rows: readonly TraderTokenRow[],
  timeZone: string,
): PnlHeatCell[] {
  const grid = new Map<number, PnlHeatCell>();
  for (const row of DOW_ROWS) {
    for (const hour of HOURS) {
      grid.set(row.dow * 24 + hour, { dow: row.dow, hour, pnl_sol: 0, count: 0 });
    }
  }
  for (const r of rows) {
    const ms = r.wallet_last_trade_at_ms ?? Date.parse(r.wallet_last_trade_at);
    if (!Number.isFinite(ms)) continue;
    const { dow, hour } = dowHourInTz(ms, timeZone);
    const cell = grid.get(dow * 24 + hour);
    if (cell) {
      cell.pnl_sol += r.wallet_total_pnl_sol;
      cell.count += 1;
    }
  }
  return [...grid.values()];
}

// ── ranked bars ──────────────────────────────────────────────────────────────

/** Rows sorted by `wallet_total_pnl_sol`, best first — the ranking the
 *  wallet-analysis doc's own finding argues for (win_rate alone can pick the
 *  worst cohort at the best hit rate). */
export function rankByTotalPnl(rows: readonly TraderTokenRow[]): TraderTokenRow[] {
  return [...rows].sort((a, b) => b.wallet_total_pnl_sol - a.wallet_total_pnl_sol);
}

// ── win/loss distribution ───────────────────────────────────────────────────

export interface PnlBucket {
  label: string;
  /** `-1` loss / `0` breakeven (excluded from both win/loss but kept visible as
   *  its own bar) / `1` win — drives the bar color. */
  sign: -1 | 0 | 1;
  count: number;
}

/** Fixed bucket edges over `wallet_realized_pnl_pct` (rows with no matched cost
 *  basis — pure open bags — are excluded; there's no realized % to bucket). */
const DIST_EDGES = [-Infinity, -50, -20, -10, 0, 10, 20, 50, Infinity];

function bucketLabel(lo: number, hi: number): string {
  if (lo === -Infinity) return `< ${hi}%`;
  if (hi === Infinity) return `> ${lo}%`;
  return `${lo}…${hi}%`;
}

export function pnlDistributionBuckets(rows: readonly TraderTokenRow[]): PnlBucket[] {
  const buckets: PnlBucket[] = [];
  for (let i = 0; i < DIST_EDGES.length - 1; i++) {
    const lo = DIST_EDGES[i]!;
    const hi = DIST_EDGES[i + 1]!;
    buckets.push({ label: bucketLabel(lo, hi), sign: hi <= 0 ? -1 : lo >= 0 ? 1 : 0, count: 0 });
  }
  for (const r of rows) {
    const pct = r.wallet_realized_pnl_pct;
    if (pct == null) continue;
    for (let i = 0; i < DIST_EDGES.length - 1; i++) {
      const lo = DIST_EDGES[i]!;
      const hi = DIST_EDGES[i + 1]!;
      // Half-open [lo, hi) except the final bucket, which is closed at +Infinity.
      if (pct >= lo && (pct < hi || hi === Infinity)) {
        buckets[i]!.count += 1;
        break;
      }
    }
  }
  return buckets;
}

// ── equity curve ─────────────────────────────────────────────────────────────

export interface EquityPoint {
  /** Epoch seconds — lightweight-charts' native `Time` unit. */
  time: number;
  cumPnlSol: number;
}

/** Cumulative `wallet_total_pnl_sol` ordered by each mint's most-recent trade
 *  (ascending) — ties (same-ms `last_trade_at`) collapse into one point so a
 *  chart series never receives duplicate x-values. */
export function buildEquityCurve(rows: readonly TraderTokenRow[]): EquityPoint[] {
  const withMs = rows
    .map((r) => ({ ms: r.wallet_last_trade_at_ms ?? Date.parse(r.wallet_last_trade_at), pnl: r.wallet_total_pnl_sol }))
    .filter((r) => Number.isFinite(r.ms))
    .sort((a, b) => a.ms - b.ms);

  const points: EquityPoint[] = [];
  let cum = 0;
  for (const { ms, pnl } of withMs) {
    cum += pnl;
    const time = Math.floor(ms / 1000);
    const last = points[points.length - 1];
    if (last && last.time === time) {
      last.cumPnlSol = cum;
    } else {
      points.push({ time, cumPnlSol: cum });
    }
  }
  return points;
}

// ── hold-time vs PnL% scatter ────────────────────────────────────────────────

export interface HoldScatterPoint {
  mint_address: string;
  label: string;
  holdSeconds: number;
  pnlPct: number;
  sizeSol: number;
  isWin: boolean;
}

/** One point per row with BOTH a positive hold span and a realized verdict
 *  (rows that are pure open bags with no matched cost basis have no `pnlPct`
 *  to plot). `sizeSol` (total volume) drives the marker radius. */
export function buildHoldScatter(rows: readonly TraderTokenRow[]): HoldScatterPoint[] {
  const points: HoldScatterPoint[] = [];
  for (const r of rows) {
    if (r.wallet_realized_pnl_pct == null) continue;
    const firstMs = r.wallet_first_trade_at_ms ?? Date.parse(r.wallet_first_trade_at);
    const lastMs = r.wallet_last_trade_at_ms ?? Date.parse(r.wallet_last_trade_at);
    if (!Number.isFinite(firstMs) || !Number.isFinite(lastMs)) continue;
    const holdSeconds = (lastMs - firstMs) / 1000;
    if (holdSeconds <= 0) continue;
    points.push({
      mint_address: r.mint_address,
      label: r.symbol || r.name || r.mint_address,
      holdSeconds,
      pnlPct: r.wallet_realized_pnl_pct,
      sizeSol: r.wallet_buy_sol + r.wallet_sell_sol,
      isWin: r.wallet_realized_pnl_sol > 0,
    });
  }
  return points;
}
