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
// not a scalp duration. A "trade" on this page is therefore one token the wallet
// sold at least part of.
//
// One basis, page-wide: realized PnL is NET of the pump.fun fee. The summary,
// the table's PnL columns, every chart and every focus lens read the per-row
// helpers below, so a Winners count, a histogram bucket and the rows it focuses
// all agree on what a win is.

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
  maxDrawdownSol,
  pnlDistributionBuckets as pnlDistributionBucketsGeneric,
  type PnlDistDensity,
  type EquityPoint,
  type HoldScatterPoint,
  type PnlBucket,
  type PnlHeatCell,
  type PnlPoint,
  type RankedBarRow,
} from 'components/analytics/pnlSeries';
import { quantileSorted, weightedReturnPct } from 'lib/strategy/runSummary';
import type { TraderTokenRow } from 'types';

export { DOW_ROWS, HOURS, dowHourInTz };
export type { EquityPoint, HoldScatterPoint, PnlBucket, PnlHeatCell };

// ── metric definitions (the one text every tile / column tooltip renders) ──

/** One wallet metric: the label it is shown under and its one-line definition
 *  (what it measures, unit, basis). The UI renders both from here and never
 *  restates a formula of its own. */
export interface WalletStatDef {
  label: string;
  def: string;
}

/** Every Trader Analysis figure, defined once. Keyed by the summary field (or,
 *  for the `row*` entries, the per-token helper) it describes. */
export const WALLET_STATS = {
  rowTotalSol: {
    label: 'PnL',
    def: 'Net realized + open mark on this token, in ◎. Net = after the 1.25 % pump.fun fee on each leg.',
  },
  rowNetPct: {
    label: 'PnL %',
    def: 'Net realized ◎ ÷ the cost of the tokens sold (avg buy price × tokens sold) × 100. Blank until something is sold.',
  },
  netRealizedSol: {
    label: 'Net realized',
    def: 'Σ over tokens of (sell proceeds − cost of the tokens sold at the avg buy price), minus the 1.25 % pump.fun fee on both legs. ◎, sold portion only. Network fee and tip are not seen here.',
  },
  openMarkSol: {
    label: 'Open mark',
    def: 'Σ over still-held tokens of (current price − avg buy price) × tokens held. ◎, no fee: nothing was sold yet.',
  },
  totalSol: {
    label: 'Total',
    def: 'Net realized + open mark, in ◎. The figure the equity curve, calendar, heatmap and ranking sum.',
  },
  returnPct: {
    label: 'Return %',
    def: 'Σ net realized ÷ Σ cost of the tokens sold × 100: money over the capital that earned it. Always the same sign as Net realized. Sub-line: that Σ cost in ◎.',
  },
  tradeCount: {
    label: 'Trades',
    def: 'Tokens the wallet sold at least part of in the window. One token = one trade, however many times it re-entered.',
  },
  winRate: {
    label: 'Win %',
    def: 'Share of trades whose net realized ◎ is above zero. Break-even counts as a loss.',
  },
  meanPct: {
    label: 'Mean %',
    def: 'Plain average of the per-trade PnL %. Every trade weighs the same whatever its size, so it can point the other way from Return %.',
  },
  medianPct: {
    label: 'Median %',
    def: "The middle trade's PnL %: half the trades did better, half worse. Nearest rank.",
  },
  p10p90Pct: {
    label: 'P10 / P90 %',
    def: 'PnL % of the trade 10 % from the bottom and 10 % from the top: the typical bad and good outcome, without the single extremes.',
  },
  bestWorstPct: {
    label: 'Best / Worst %',
    def: 'Highest and lowest per-trade PnL %.',
  },
  expectancySol: {
    label: 'Expectancy',
    def: 'Σ net realized ÷ trades: the ◎ an average trade made.',
  },
  maxDrawdownSol: {
    label: 'Max drawdown',
    def: 'Deepest fall of the running Total ◎ below its previous peak, tokens ordered by their last trade. Same curve as the Equity chart.',
  },
  worstTradeSol: {
    label: 'Worst trade',
    def: "The largest single net realized loss in ◎, with that trade's PnL %.",
  },
  lossStreak: {
    label: 'Loss streak',
    def: "Most losing trades in a row, ordered by each token's last trade.",
  },
  profitFactor: {
    label: 'Profit factor',
    def: 'Σ net ◎ of winners ÷ |Σ net ◎ of losers|. Above 1 = the wallet made money on the sold portion.',
  },
  payoffRatio: {
    label: 'Payoff',
    def: 'Average net win ◎ ÷ |average net loss ◎|. Read with Win %: a 30 % win rate needs a payoff above 2.3 to break even.',
  },
  avgWinLossSol: {
    label: 'Avg win / loss',
    def: 'Average net realized ◎ of the winning trades / of the losing trades.',
  },
  capitalInSol: {
    label: 'Capital in',
    def: 'Σ ◎ spent on buys in the window (curve-side, before fee). Sub-line: buys + sells volume.',
  },
  medianBuySol: {
    label: 'Median size',
    def: 'Median ◎ bought per token in the window (all its buys added up, not one buy leg).',
  },
  medianHoldSecs: {
    label: 'Median hold',
    def: 'Median first → last trade span per trade. Spans every re-entry on that token, so it is an exposure envelope, not one round trip.',
  },
  medianEntryCurvePct: {
    label: 'Median entry curve',
    def: "Median bonding-curve progress at the wallet's first buy: real pool SOL as a % of the ~85 SOL graduation line.",
  },
  tokenCount: {
    label: 'Tokens',
    def: 'Tokens the wallet traded in the window. Partial = sold more than it bought here, so the cost basis predates the window and its PnL is an estimate.',
  },
} as const satisfies Record<string, WalletStatDef>;

export type WalletStatKey = keyof typeof WALLET_STATS;

// ── per-token figures (the only place a row's PnL basis is chosen) ──────────

/** First→last trade span (seconds) for one mint in the window. `null` when
 *  either timestamp is missing/invalid. Not a single-episode hold — see the
 *  grain caveat at the top of this file. */
export function walletHoldSeconds(r: TraderTokenRow): number | null {
  const firstMs = r.wallet_first_trade_at_ms ?? Date.parse(r.wallet_first_trade_at);
  const lastMs = r.wallet_last_trade_at_ms ?? Date.parse(r.wallet_last_trade_at);
  if (!Number.isFinite(firstMs) || !Number.isFinite(lastMs)) return null;
  return (lastMs - firstMs) / 1000;
}

/** A row is a TRADE once something was sold against a cost basis — the
 *  `realized_pnl_pct != null` condition of the kernel. An open bag with no
 *  sells is neither a win nor a loss. */
export function isWalletTrade(r: TraderTokenRow): boolean {
  return r.wallet_matched_cost_sol > 0;
}

/** [`WALLET_STATS.rowNetPct`] — `null` when nothing was sold. */
export function walletNetPct(r: TraderTokenRow): number | null {
  return weightedReturnPct(r.wallet_realized_pnl_sol_net_of_fee, r.wallet_matched_cost_sol);
}

/** [`WALLET_STATS.rowTotalSol`]. */
export function walletTotalSol(r: TraderTokenRow): number {
  return r.wallet_realized_pnl_sol_net_of_fee + (r.wallet_unrealized_pnl_sol ?? 0);
}

/** Decision instant of a row — its last trade in the window, epoch-ms. */
function lastTradeMs(r: TraderTokenRow): number {
  return r.wallet_last_trade_at_ms ?? Date.parse(r.wallet_last_trade_at);
}

/**
 * The wallet grain → the shared `PnlPoint` grain.
 *
 * Bucketed by the mint's most-recent trade in the window
 * (`wallet_last_trade_at_ms`) — the per-mint grain's only single instant that
 * reads as "when this position was decided" (an exit for a closed mint, the
 * latest re-entry for an open one).
 */
export function toPnlPoints(rows: readonly TraderTokenRow[]): PnlPoint[] {
  return rows.map((r) => ({
    key: r.mint_address,
    timeMs: lastTradeMs(r),
    pnlSol: walletTotalSol(r),
    pnlPct: walletNetPct(r),
    label: r.symbol || r.name || r.mint_address,
    isOpen: r.wallet_is_open,
  }));
}

// ── summary stats ───────────────────────────────────────────────────────────

/** Every field is defined in [`WALLET_STATS`] under the same (or the paired)
 *  key. `null` = not measurable on this cohort (rendered `—`), never `0`. */
export interface WalletPnlSummary {
  tokenCount: number;
  openCount: number;
  closedCount: number;
  partialDataCount: number;
  tradeCount: number;
  winCount: number;
  /** Trades with net realized ≤ 0 — the shared focus lens's `loss`. */
  lossCount: number;
  winRate: number | null;
  grossRealizedSol: number;
  netRealizedSol: number;
  openMarkSol: number;
  totalSol: number;
  /** Σ matched cost — the `returnPct` denominator. */
  matchedCostSol: number;
  returnPct: number | null;
  meanPct: number | null;
  medianPct: number | null;
  p10Pct: number | null;
  p90Pct: number | null;
  bestPct: number | null;
  worstPct: number | null;
  expectancySol: number | null;
  avgWinSol: number | null;
  avgLossSol: number | null;
  payoffRatio: number | null;
  profitFactor: number | null;
  maxDrawdownSol: number;
  worstTradeSol: number | null;
  worstTradePct: number | null;
  longestLossStreak: number;
  capitalInSol: number;
  volumeSol: number;
  medianBuySol: number | null;
  medianHoldSecs: number | null;
  medianEntryCurvePct: number | null;
}

const EMPTY_SUMMARY: WalletPnlSummary = {
  tokenCount: 0,
  openCount: 0,
  closedCount: 0,
  partialDataCount: 0,
  tradeCount: 0,
  winCount: 0,
  lossCount: 0,
  winRate: null,
  grossRealizedSol: 0,
  netRealizedSol: 0,
  openMarkSol: 0,
  totalSol: 0,
  matchedCostSol: 0,
  returnPct: null,
  meanPct: null,
  medianPct: null,
  p10Pct: null,
  p90Pct: null,
  bestPct: null,
  worstPct: null,
  expectancySol: null,
  avgWinSol: null,
  avgLossSol: null,
  payoffRatio: null,
  profitFactor: null,
  maxDrawdownSol: 0,
  worstTradeSol: null,
  worstTradePct: null,
  longestLossStreak: 0,
  capitalInSol: 0,
  volumeSol: 0,
  medianBuySol: null,
  medianHoldSecs: null,
  medianEntryCurvePct: null,
};

const ascending = (vals: number[]) => vals.sort((a, b) => a - b);

export function computeWalletSummary(rows: readonly TraderTokenRow[]): WalletPnlSummary {
  if (rows.length === 0) return EMPTY_SUMMARY;

  let openCount = 0;
  let partialDataCount = 0;
  let grossRealized = 0;
  let netRealized = 0;
  let openMark = 0;
  let matchedCost = 0;
  let capitalIn = 0;
  let volume = 0;
  let winCount = 0;
  let sumWinSol = 0;
  let sumLossSol = 0; // magnitude of the losing trades' net ◎
  let worstTrade: TraderTokenRow | null = null;
  const trades: TraderTokenRow[] = [];
  const pcts: number[] = [];
  const buys: number[] = [];
  const holds: number[] = [];
  const entryCurves: number[] = [];

  for (const r of rows) {
    if (r.wallet_is_open) openCount++;
    if (r.wallet_partial_data) partialDataCount++;
    grossRealized += r.wallet_realized_pnl_sol;
    netRealized += r.wallet_realized_pnl_sol_net_of_fee;
    openMark += r.wallet_unrealized_pnl_sol ?? 0;
    capitalIn += r.wallet_buy_sol;
    volume += r.wallet_buy_sol + r.wallet_sell_sol;
    if (r.wallet_buy_sol > 0) buys.push(r.wallet_buy_sol);
    if (r.wallet_entry_curve_pct != null) entryCurves.push(r.wallet_entry_curve_pct);
    if (!isWalletTrade(r)) continue;

    trades.push(r);
    matchedCost += r.wallet_matched_cost_sol;
    const net = r.wallet_realized_pnl_sol_net_of_fee;
    const pct = walletNetPct(r);
    if (pct != null) pcts.push(pct);
    const hold = walletHoldSeconds(r);
    if (hold != null && hold > 0) holds.push(hold);
    if (net > 0) {
      winCount++;
      sumWinSol += net;
    } else {
      sumLossSol -= net;
      if (!worstTrade || net < worstTrade.wallet_realized_pnl_sol_net_of_fee) worstTrade = r;
    }
  }

  const tradeCount = trades.length;
  const lossCount = tradeCount - winCount;
  ascending(pcts);
  const q = (p: number) => quantileSorted(pcts, p);

  // Streak in decision order — the same last-trade instant every chart buckets on.
  trades.sort((a, b) => lastTradeMs(a) - lastTradeMs(b));
  let streak = 0;
  let longestLossStreak = 0;
  for (const r of trades) {
    streak = r.wallet_realized_pnl_sol_net_of_fee > 0 ? 0 : streak + 1;
    if (streak > longestLossStreak) longestLossStreak = streak;
  }

  const avgWinSol = winCount > 0 ? sumWinSol / winCount : null;
  const avgLossSol = lossCount > 0 ? -(sumLossSol / lossCount) : null;
  return {
    tokenCount: rows.length,
    openCount,
    closedCount: rows.length - openCount,
    partialDataCount,
    tradeCount,
    winCount,
    lossCount,
    winRate: tradeCount > 0 ? (winCount / tradeCount) * 100 : null,
    grossRealizedSol: grossRealized,
    netRealizedSol: netRealized,
    openMarkSol: openMark,
    totalSol: netRealized + openMark,
    matchedCostSol: matchedCost,
    returnPct: weightedReturnPct(netRealized, matchedCost),
    meanPct: pcts.length > 0 ? pcts.reduce((s, v) => s + v, 0) / pcts.length : null,
    medianPct: q(0.5),
    p10Pct: q(0.1),
    p90Pct: q(0.9),
    bestPct: pcts.length > 0 ? pcts[pcts.length - 1]! : null,
    worstPct: pcts.length > 0 ? pcts[0]! : null,
    expectancySol: tradeCount > 0 ? netRealized / tradeCount : null,
    avgWinSol,
    avgLossSol,
    payoffRatio: avgWinSol != null && avgLossSol != null && avgLossSol < 0 ? avgWinSol / -avgLossSol : null,
    profitFactor: sumLossSol > 0 ? sumWinSol / sumLossSol : null,
    maxDrawdownSol: maxDrawdownSol(buildEquityCurveGeneric(toPnlPoints(rows))),
    worstTradeSol: worstTrade ? worstTrade.wallet_realized_pnl_sol_net_of_fee : null,
    worstTradePct: worstTrade ? walletNetPct(worstTrade) : null,
    longestLossStreak,
    capitalInSol: capitalIn,
    volumeSol: volume,
    medianBuySol: quantileSorted(ascending(buys), 0.5),
    medianHoldSecs: quantileSorted(ascending(holds), 0.5),
    medianEntryCurvePct: quantileSorted(ascending(entryCurves), 0.5),
  };
}

// ── the promoted folds, adapted to the wallet grain ─────────────────────────
//
// Each is a one-line map into `PnlPoint` + the shared fold. Keeping the
// wallet-named wrappers means the lab call sites and their doc comments stay
// put; keeping the fold in `components/analytics` means the live decks and this
// page can't drift into two different definitions of "equity curve".

/** Day×hour grid of [`walletTotalSol`]. See `toPnlPoints` for the instant
 *  each mint is bucketed by (and its per-mint-grain caveat). */
export function buildPnlHeatCells(
  rows: readonly TraderTokenRow[],
  timeZone: string,
): PnlHeatCell[] {
  return buildPnlHeatCellsGeneric(toPnlPoints(rows), timeZone);
}

/** Rows as ranked bars on [`walletTotalSol`]. */
export function rankedPnlBarRows(rows: readonly TraderTokenRow[]): RankedBarRow[] {
  return rows.map((r) => ({
    key: r.mint_address,
    label: r.symbol || r.name || r.mint_address.slice(0, 8),
    value: walletTotalSol(r),
    tag: r.wallet_is_open ? 'open' : null,
    title: r.mint_address,
  }));
}

/** Count histogram over [`walletNetPct`] (open-only bags have no realized %
 *  and are excluded). */
export function pnlDistributionBuckets(
  rows: readonly TraderTokenRow[],
  density: PnlDistDensity = 'default',
): PnlBucket[] {
  return pnlDistributionBucketsGeneric(toPnlPoints(rows), density);
}

/** Cumulative [`walletTotalSol`] ordered by each mint's most-recent trade. */
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
      pnlPct: walletNetPct(r),
      sizeSol: r.wallet_buy_sol + r.wallet_sell_sol,
      pnlSol: r.wallet_realized_pnl_sol_net_of_fee,
      isWin: r.wallet_realized_pnl_sol_net_of_fee > 0,
    })),
  );
}
