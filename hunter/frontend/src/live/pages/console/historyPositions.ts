/**
 * Console History's **one population**, in the three shapes its surfaces need:
 * table rows stay `RulePositionRecord`, the charts deck takes `ClosedTradePoint`,
 * and the summary strip takes `RunOutcomeRow`.
 *
 * All three derive from the same server-filtered positions query, so the strip,
 * the deck, and the table can never describe different cohorts. The deck must not
 * fold `/api/portfolio/closes-series` here — that is a second population, and it
 * is closed-only, single-mode, and blind to the table's per-column filters.
 * (The endpoint itself is fine; Home's Review digest is its consumer.)
 *
 * The lenses SQL can't express — recurring dow×hour heat, a hold/PnL band brush,
 * a legacy exit focus, a synthetic Metric± needle — are applied here, once, so
 * every surface sees the same client-side narrowing.
 */

import { resolvePnlPct } from 'lib/pnlPct';
import type { RunOutcomeRow } from 'lib/strategy/runSummary';
import type { RulePositionRecord } from 'types';
import type { ClosedTradePoint } from '@live/store/liveEndpoints';
import type { HistoryCohort } from './historyCohort';
import {
  matchesExitFocus,
  matchesHeatFocus,
  matchesHoldBandFocus,
} from './historyFocus';
import {
  exitReasonMatchesFilter,
  isHistoryMetricExitNeedle,
} from './historyExitFilter';

/** Hold duration in seconds, or `null` when either end is missing. */
export function holdSecondsOf(
  entryIso: string | null | undefined,
  exitIso: string | null | undefined,
): number | null {
  if (!entryIso || !exitIso) return null;
  const a = Date.parse(entryIso);
  const b = Date.parse(exitIso);
  if (!Number.isFinite(a) || !Number.isFinite(b)) return null;
  const secs = (b - a) / 1000;
  return secs >= 0 ? secs : null;
}

/** Realized PnL % for a row — the same resolver the table's PnL% column uses. */
function pctOf(r: RulePositionRecord): number | null {
  return resolvePnlPct({
    pnlSol: r.pnl_sol,
    entrySol: r.entry_sol,
    entryPrice: r.entry_price,
    exitPrice: r.exit_price,
  });
}

/** A position that closed cleanly with capital deployed — the charts' atom. */
function isClosed(r: RulePositionRecord): boolean {
  return r.status === 'End' && r.entry_sol != null;
}

/**
 * Apply the lenses the server can't express. Order is irrelevant (they compose
 * as an AND), but they must all be applied in **one** place — the previous split
 * (table filtered here, charts filtered there) is how the two drifted.
 */
export function applyHistoryClientLenses(
  rows: readonly RulePositionRecord[],
  cohort: HistoryCohort,
  timezone: string,
): RulePositionRecord[] {
  const focus = cohort.focus;
  let out = applyHistoryCohortNeedle(rows, cohort);
  if (focus?.kind === 'heat') {
    out = out.filter((r) => matchesHeatFocus(r.exit_time, focus, timezone));
  }
  if (focus?.kind === 'holdBand') {
    out = out.filter((r) => matchesHoldBandFocus(r.entry_time, r.exit_time, pctOf(r), focus));
  }
  if (focus?.kind === 'exit') {
    out = out.filter((r) => matchesExitFocus(r.exit_reason, r.pnl_sol, focus));
  }
  return out;
}

/**
 * The **cohort-level** client lens only — the synthetic Metric± needle — leaving
 * every `hfocus` lens unapplied.
 *
 * This is what the charts deck's population goes through, because the deck owns
 * focus itself and applies it asymmetrically: the calendar and the day×hour
 * heatmap deliberately stay on the parent cohort and draw a selection ring
 * instead of narrowing, since a calendar that collapses to the one day you just
 * clicked has thrown away the context that made the click worth making.
 */
export function applyHistoryCohortNeedle(
  rows: readonly RulePositionRecord[],
  cohort: HistoryCohort,
): RulePositionRecord[] {
  if (!isHistoryMetricExitNeedle(cohort.exitReason)) return rows as RulePositionRecord[];
  return rows.filter((r) =>
    exitReasonMatchesFilter(r.exit_reason, cohort.exitReason, r.pnl_sol),
  );
}

/**
 * Closed rows → the charts deck's wire shape. Open / entry-failed rows are
 * dropped: every chart in the deck plots a realized outcome (equity path, return
 * distribution, hold-vs-PnL, the daily/hourly PnL grids), and an unsettled
 * position has none — charting its mark would put unrealized SOL into a curve
 * labelled realized.
 */
export function positionsToClosePoints(
  rows: readonly RulePositionRecord[],
): ClosedTradePoint[] {
  const out: ClosedTradePoint[] = [];
  for (const r of rows) {
    if (!isClosed(r) || !r.exit_time) continue;
    const pnlSol = r.pnl_sol ?? 0;
    out.push({
      id: r.id,
      exit_time: r.exit_time,
      rule_id: r.rule_id,
      mint_address: r.mint_address,
      pnl_sol: pnlSol,
      entry_sol: r.entry_sol ?? 0,
      // Mirrors the server's `is_win` (`exit_lamports > entry_lamports`) — a
      // break-even close is not a win.
      win: pnlSol > 0,
      hold_secs: holdSecondsOf(r.entry_time, r.exit_time),
      exit_reason: r.exit_reason,
    });
  }
  return out;
}

/**
 * Every **entered** row → the shared summary fold. Unlike the chart points this
 * keeps the open positions, tagged `Open`, because that is exactly what the MTM
 * band and the Open / Open-share tiles report: a rule holding its losers open
 * reads as profitable if they're dropped.
 *
 * Used only when a client-side lens is active — otherwise the strip renders the
 * server aggregate, which is exact past the fetch cap.
 */
export function positionsToRunOutcomes(
  rows: readonly RulePositionRecord[],
): RunOutcomeRow[] {
  const out: RunOutcomeRow[] = [];
  for (const r of rows) {
    // Never entered (an `EntryFailed` row) — no capital, no outcome to fold.
    if (r.entry_sol == null && r.entry_price == null) continue;
    const closed = isClosed(r);
    out.push({
      fired: true,
      exit: closed ? (r.exit_reason?.trim() ? r.exit_reason : 'Other') : 'Open',
      pnl_sol: r.pnl_sol ?? 0,
      pnl_pct: pctOf(r) ?? 0,
      holding_secs: holdSecondsOf(r.entry_time, r.exit_time) ?? 0,
    });
  }
  return out;
}

/** Buys that never filled in this cohort — no SOL deployed, so they never reach
 *  the charts, but entry-failure pressure has to stay visible somewhere. */
export function countEntryFailed(rows: readonly RulePositionRecord[]): number {
  let n = 0;
  for (const r of rows) {
    if (r.status === 'EntryFailed') n += 1;
  }
  return n;
}

/** Graduated-to-AMM count over a client-lensed cohort (server sends its own). */
export function countMigrated(rows: readonly RulePositionRecord[]): number {
  let n = 0;
  for (const r of rows) {
    if ((r.entry_sol != null || r.entry_price != null) && r.is_migrated) n += 1;
  }
  return n;
}
