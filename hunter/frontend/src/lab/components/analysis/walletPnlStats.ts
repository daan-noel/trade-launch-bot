// Trader Analysis — wallet-level PnL analytics. Pure, DB-free helpers that fold
// the already-fetched `TraderTokenRow[]` into the summary tiles and the
// analytics panel's charts. No network calls here: everything re-derives from
// the SAME rows the token table already has, so filtering the table (via
// `TokenTable`'s `onFilteredRowsChange`) re-derives every chart for free.
//
// The grain is the TRADE: one round trip of the backend episode ledger
// (`hunter/core/src/strategies/wallet_ledger.rs`), from a buy with none held to
// the sell that leaves at most 0.1 % of what it bought. Its SOL is what the
// wallet moved (the transactions' payer net flow, every fee, tip and venue
// charge included). Only a CLOSED trade carries a PnL; open trades show apart as
// an estimate and incomplete ones are counted, never summed. The table stays one
// row per token, and a token's PnL is the sum of its closed trades.

// The generic folds (equity curve, distribution buckets, day×hour heatmap,
// ranking) live in `components/analytics/pnlSeries` — shared with the live
// app's Console History / Portfolio decks. What stays here is the wallet-
// specific part: the trade → `PnlPoint` mapping and the summary.
import {
  DOW_ROWS,
  HOURS,
  buildHoldScatterPoints,
  dowHourInTz,
  maxDrawdownSol,
  type EquityPoint,
  type HoldScatterPoint,
  type PnlBucket,
  type PnlHeatCell,
  type PnlPoint,
  type RankedBarRow,
} from 'components/analytics/pnlSeries';
import { quantileSorted, weightedReturnPct } from 'lib/strategy/runSummary';
import type { TraderTokenRow, WalletEpisode } from 'types';

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

/** A trade, and the basis every SOL figure below is on — written once, read by
 *  the definitions that need it. */
const TRADE =
  'A trade is one round trip: from a buy with none of the token held to the sell that leaves at most 0.1 % of what it bought. A re-entry is a new trade.';
const EXACT =
  "SOL is what the wallet moved: each transaction's change in the wallet, every fee, tip and venue charge included.";

/** Every Trader Analysis figure, defined once. Keyed by the summary field (or,
 *  for the `row*` entries, the per-token helper) it describes. */
export const WALLET_STATS = {
  tradeNetSol: {
    label: 'Net',
    def: `A closed trade's SOL the sells returned minus SOL the buys took. ${EXACT}`,
  },
  tradePct: {
    label: 'PnL %',
    def: "A closed trade's net ◎ ÷ SOL its buys took × 100.",
  },
  rowNetSol: {
    label: 'PnL',
    def: `Σ net ◎ of this token's closed trades. ${EXACT} Open and incomplete trades are not in it.`,
  },
  rowNetPct: {
    label: 'PnL %',
    def: "This token's PnL ÷ SOL its closed trades' buys took × 100.",
  },
  netSol: {
    label: 'Net PnL',
    def: `Σ net ◎ over closed trades. ${EXACT} Open and incomplete trades are not in it.`,
  },
  returnPct: {
    label: 'Return %',
    def: 'Net PnL ÷ SOL the buys of the closed trades took × 100: money over the capital that earned it. Always the same sign as Net PnL.',
  },
  openPnlSol: {
    label: 'Open (estimate)',
    def: 'Σ over open trades of SOL moved so far + tokens held × current spot price. An estimate: an open trade has no exact value until it sells. In no other figure.',
  },
  incompleteCount: {
    label: 'Incomplete',
    def: 'Trades that cannot be priced exactly, left out of every figure. No flow = a transaction without a recorded wallet change (older than the exact-flow recording, paid by another account, or shared with another token or wallet). Unseen buy = it sold tokens the ledger never saw bought (a transfer in, or a buy older than the data).',
  },
  tradeCount: {
    label: 'Trades',
    def: `Closed trades. ${TRADE}`,
  },
  winRate: {
    label: 'Win %',
    def: 'Share of closed trades with net ◎ above zero. Break-even counts as a loss.',
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
    def: 'Net PnL ÷ closed trades: the ◎ an average trade made.',
  },
  maxDrawdownSol: {
    label: 'Max drawdown',
    def: 'The most lost in one stretch: the deepest fall of the running Net PnL below its highest point so far, starting from 0, each trade added at its closing sell in tape order. Same line as the Equity chart.',
  },
  worstTradeSol: {
    label: 'Worst trade',
    def: "The largest single net loss in ◎, with that trade's PnL %.",
  },
  lossStreak: {
    label: 'Loss streak',
    def: 'Most losing trades in a row, in closing-sell order.',
  },
  profitFactor: {
    label: 'Profit factor',
    def: 'Σ net ◎ of winners ÷ |Σ net ◎ of losers|. Above 1 = the closed trades made money.',
  },
  payoffRatio: {
    label: 'Payoff',
    def: 'Average net win ◎ ÷ |average net loss ◎|. Read with Win %: a 30 % win rate needs a payoff above 2.3 to break even.',
  },
  avgWinLossSol: {
    label: 'Avg win / loss',
    def: 'Average net ◎ of the winning trades / of the losing trades.',
  },
  capitalInSol: {
    label: 'Capital in',
    def: 'Σ SOL the buys of the closed trades took from the wallet: the Return % denominator. Sub-line: Σ SOL their sells returned.',
  },
  medianBuySol: {
    label: 'Median size',
    def: "Median SOL a closed trade's buys took (all its buys added up, not one buy).",
  },
  medianHoldSecs: {
    label: 'Median hold',
    def: 'Median first buy → closing sell of a closed trade: one round trip.',
  },
  medianEntryCurvePct: {
    label: 'Median entry curve',
    def: "Median bonding-curve progress at the wallet's first buy on each token in the window: real pool SOL as a % of the ~85 SOL graduation line.",
  },
  tokenCount: {
    label: 'Tokens',
    def: 'Tokens the wallet traded in the window.',
  },
} as const satisfies Record<string, WalletStatDef>;

export type WalletStatKey = keyof typeof WALLET_STATS;

// ── the trade grain ─────────────────────────────────────────────────────────

/** One round trip with the token row it belongs to. */
export interface WalletTrade {
  /** `mint::entry_slot:entry_tx_index` — a trade's identity, and the `pos`
   *  focus lens's id. */
  key: string;
  row: TraderTokenRow;
  ep: WalletEpisode;
}

/** Every trade of `rows`, token by token, each token's in tape order. */
export function walletTrades(rows: readonly TraderTokenRow[]): WalletTrade[] {
  const out: WalletTrade[] = [];
  for (const row of rows) {
    for (const ep of row.episodes) {
      out.push({ key: `${row.mint_address}::${ep.entry_slot}:${ep.entry_tx_index}`, row, ep });
    }
  }
  return out;
}

/** A closed trade's net ◎ — the one condition every PnL figure counts on.
 *  `null` for an open or incomplete trade. */
export function tradeNetSol(ep: WalletEpisode): number | null {
  return ep.status === 'closed' ? ep.net_sol : null;
}

/** First buy → closing sell, seconds. `null` while open. */
export function tradeHoldSeconds(ep: WalletEpisode): number | null {
  return ep.exit_ms != null ? (ep.exit_ms - ep.entry_ms) / 1000 : null;
}

/** The instant a trade was decided: its closing sell, or its entry while open. */
export function tradeTimeMs(ep: WalletEpisode): number {
  return ep.exit_ms ?? ep.entry_ms;
}

function tradeLabel(t: WalletTrade): string {
  return t.row.symbol || t.row.name || t.row.mint_address.slice(0, 8);
}

/** Closed trades in closing-sell tape order: `(exit_slot, exit_tx_index)`, the
 *  only key finer than a second. */
function closedInTapeOrder(trades: readonly WalletTrade[]): WalletTrade[] {
  return trades
    .filter((t) => tradeNetSol(t.ep) != null)
    .sort((a, b) => (a.ep.exit_slot ?? 0) - (b.ep.exit_slot ?? 0) || (a.ep.exit_tx_index ?? 0) - (b.ep.exit_tx_index ?? 0));
}

/**
 * Closed trades → the shared `PnlPoint` grain, in closing-sell tape order (the
 * shared folds sort stably by time, so trades closing in the same second keep
 * it). Open and incomplete trades have no exact PnL and are not points.
 */
export function toPnlPoints(trades: readonly WalletTrade[]): PnlPoint[] {
  return closedInTapeOrder(trades).map((t) => ({
    key: t.key,
    timeMs: tradeTimeMs(t.ep),
    pnlSol: t.ep.net_sol!,
    pnlPct: t.ep.pnl_pct,
    label: tradeLabel(t),
  }));
}

// ── per-token figures (what the table and the chart card show) ──────────────

/** First→last trade span (seconds) for one mint in the window, every re-entry
 *  inside it. `null` when either timestamp is missing/invalid. */
export function walletHoldSeconds(r: TraderTokenRow): number | null {
  const firstMs = r.wallet_first_trade_at_ms ?? Date.parse(r.wallet_first_trade_at);
  const lastMs = r.wallet_last_trade_at_ms ?? Date.parse(r.wallet_last_trade_at);
  if (!Number.isFinite(firstMs) || !Number.isFinite(lastMs)) return null;
  return (lastMs - firstMs) / 1000;
}

/** How many of a token's trades are closed / open / incomplete. */
export function walletRowCounts(r: TraderTokenRow): { closed: number; open: number; incomplete: number } {
  const n = { closed: 0, open: 0, incomplete: 0 };
  for (const ep of r.episodes) n[ep.status]++;
  return n;
}

/** [`WALLET_STATS.rowNetSol`] — `null` when the token has no closed trade. */
export function walletRowNetSol(r: TraderTokenRow): number | null {
  let sum: number | null = null;
  for (const ep of r.episodes) {
    const net = tradeNetSol(ep);
    if (net != null) sum = (sum ?? 0) + net;
  }
  return sum;
}

/** [`WALLET_STATS.rowNetPct`] — `null` when the token has no closed trade. */
export function walletRowPct(r: TraderTokenRow): number | null {
  let net = 0;
  let capital = 0;
  for (const ep of r.episodes) {
    if (tradeNetSol(ep) == null) continue;
    net += ep.net_sol!;
    capital += ep.sol_in ?? 0;
  }
  return capital > 0 ? weightedReturnPct(net, capital) : null;
}

/** Σ of a token's open-trade estimates; `null` when none is priced. */
export function walletRowOpenSol(r: TraderTokenRow): number | null {
  let sum: number | null = null;
  for (const ep of r.episodes) {
    if (ep.status === 'open' && ep.open_pnl_sol != null) sum = (sum ?? 0) + ep.open_pnl_sol;
  }
  return sum;
}

// ── summary stats ───────────────────────────────────────────────────────────

/** Every field is defined in [`WALLET_STATS`] under the same (or the paired)
 *  key. `null` = not measurable on this cohort (rendered `—`), never `0`. */
export interface WalletPnlSummary {
  tokenCount: number;
  tradeCount: number;
  openCount: number;
  incompleteCount: number;
  /** Incomplete trades by reason (a trade can carry both). */
  missingFlowCount: number;
  unseenBuyCount: number;
  winCount: number;
  /** Trades with net ≤ 0 — the shared focus lens's `loss`. */
  lossCount: number;
  winRate: number | null;
  netSol: number;
  returnPct: number | null;
  openPnlSol: number | null;
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
  capitalOutSol: number;
  medianBuySol: number | null;
  medianHoldSecs: number | null;
  medianEntryCurvePct: number | null;
}

const ascending = (vals: number[]) => vals.sort((a, b) => a - b);

/** Fold the trades (the summary's grain) and the token rows they came from
 *  (tokens and entry curve, which are per token). */
export function computeWalletSummary(
  rows: readonly TraderTokenRow[],
  trades: readonly WalletTrade[],
): WalletPnlSummary {
  let openCount = 0;
  let openPnl: number | null = null;
  let incompleteCount = 0;
  let missingFlowCount = 0;
  let unseenBuyCount = 0;
  for (const { ep } of trades) {
    if (ep.status === 'open') {
      openCount++;
      if (ep.open_pnl_sol != null) openPnl = (openPnl ?? 0) + ep.open_pnl_sol;
    } else if (ep.status === 'incomplete') {
      incompleteCount++;
      if (ep.missing_flow) missingFlowCount++;
      if (ep.unseen_buy) unseenBuyCount++;
    }
  }

  const closed = closedInTapeOrder(trades);
  let net = 0;
  let capitalIn = 0;
  let capitalOut = 0;
  let winCount = 0;
  let sumWinSol = 0;
  let sumLossSol = 0; // magnitude of the losing trades' net ◎
  let worst: WalletEpisode | null = null;
  let streak = 0;
  let longestLossStreak = 0;
  const pcts: number[] = [];
  const buys: number[] = [];
  const holds: number[] = [];
  for (const { ep } of closed) {
    const n = ep.net_sol!;
    net += n;
    capitalIn += ep.sol_in ?? 0;
    capitalOut += ep.sol_out ?? 0;
    if (ep.pnl_pct != null) pcts.push(ep.pnl_pct);
    if (ep.sol_in != null && ep.sol_in > 0) buys.push(ep.sol_in);
    const hold = tradeHoldSeconds(ep);
    if (hold != null && hold > 0) holds.push(hold);
    if (n > 0) {
      winCount++;
      sumWinSol += n;
      streak = 0;
    } else {
      sumLossSol -= n;
      if (!worst || n < worst.net_sol!) worst = ep;
      streak++;
      if (streak > longestLossStreak) longestLossStreak = streak;
    }
  }

  const entryCurves: number[] = [];
  for (const r of rows) if (r.wallet_entry_curve_pct != null) entryCurves.push(r.wallet_entry_curve_pct);

  const tradeCount = closed.length;
  const lossCount = tradeCount - winCount;
  ascending(pcts);
  const q = (p: number) => quantileSorted(pcts, p);
  const avgWinSol = winCount > 0 ? sumWinSol / winCount : null;
  const avgLossSol = lossCount > 0 ? -(sumLossSol / lossCount) : null;
  return {
    tokenCount: rows.length,
    tradeCount,
    openCount,
    incompleteCount,
    missingFlowCount,
    unseenBuyCount,
    winCount,
    lossCount,
    winRate: tradeCount > 0 ? (winCount / tradeCount) * 100 : null,
    netSol: net,
    returnPct: capitalIn > 0 ? weightedReturnPct(net, capitalIn) : null,
    openPnlSol: openPnl,
    meanPct: pcts.length > 0 ? pcts.reduce((s, v) => s + v, 0) / pcts.length : null,
    medianPct: q(0.5),
    p10Pct: q(0.1),
    p90Pct: q(0.9),
    bestPct: pcts.length > 0 ? pcts[pcts.length - 1]! : null,
    worstPct: pcts.length > 0 ? pcts[0]! : null,
    expectancySol: tradeCount > 0 ? net / tradeCount : null,
    avgWinSol,
    avgLossSol,
    payoffRatio: avgWinSol != null && avgLossSol != null && avgLossSol < 0 ? avgWinSol / -avgLossSol : null,
    profitFactor: sumLossSol > 0 ? sumWinSol / sumLossSol : null,
    maxDrawdownSol: maxDrawdownSol(toPnlPoints(closed)),
    worstTradeSol: worst ? worst.net_sol : null,
    worstTradePct: worst ? worst.pnl_pct : null,
    longestLossStreak,
    capitalInSol: capitalIn,
    capitalOutSol: capitalOut,
    medianBuySol: quantileSorted(ascending(buys), 0.5),
    medianHoldSecs: quantileSorted(ascending(holds), 0.5),
    medianEntryCurvePct: quantileSorted(ascending(entryCurves), 0.5),
  };
}

// ── chart rows ──────────────────────────────────────────────────────────────

/** Closed trades as ranked bars on their net ◎. */
export function rankedPnlBarRows(trades: readonly WalletTrade[]): RankedBarRow[] {
  return closedInTapeOrder(trades).map((t) => ({
    key: t.key,
    label: tradeLabel(t),
    value: t.ep.net_sol!,
    tag: null,
    title: t.row.mint_address,
  }));
}

/** One point per closed trade: X = its hold, Y = its PnL %, size = SOL in. */
export function buildHoldScatter(trades: readonly WalletTrade[]): HoldScatterPoint[] {
  return buildHoldScatterPoints(
    closedInTapeOrder(trades).map((t) => ({
      key: t.key,
      label: tradeLabel(t),
      holdSeconds: tradeHoldSeconds(t.ep),
      pnlPct: t.ep.pnl_pct,
      sizeSol: t.ep.sol_in ?? 0,
      pnlSol: t.ep.net_sol!,
    })),
  );
}
