import { cn } from 'lib/cn';
import { formatDecimalTrim } from 'utils/format';
import type { SummaryStat, SummarySection } from 'components/strategy/SummaryStatsPanel';

/**
 * **The one run-summary renderer.** Every surface that answers "how did this rule
 * do" — the single-rule Simulate page, the grouped-sweep combo drill-in, and the
 * live/paper positions card — builds its summary here, so the three cannot drift
 * in which metrics they show, what they're called, or how they're formatted.
 *
 * Before this existed there were three independent builders over two wire shapes:
 * Simulate showed 5 tiles with a headline that silently folded in unrealized
 * marks, the sweep showed ~25 across realized/MTM bands, and the live card showed
 * a third set again — with PnL rendered at 3dp/suffix in one place and
 * 4dp/signed-prefix in another. `SummaryStatsPanel` is only a layout shell (it
 * takes pre-formatted strings), so nothing stopped that. This module is the layer
 * that does (parity plan F1-F8).
 *
 * Mirrors `trading_core::strategies::kernel::RunSummary` field-for-field; the
 * backend sends this shape directly.
 */

// --- formatters (SSOT — the sweep columns re-export these) -------------------

/** Holding time, auto-scaled. `—` for absent/zero. */
export function fmtSecs(v: number | null | undefined): string {
  if (v == null || !Number.isFinite(v) || v <= 0) return '—';
  if (v < 90) return `${Math.round(v)}s`;
  if (v < 5400) return `${(v / 60).toFixed(1)}m`;
  return `${(v / 3600).toFixed(1)}h`;
}

/** Signed percent, 1dp, trimmed. Note `0` renders as `+0%`, **not** a dash — a
 *  genuine zero is a measurement, and showing it as `—` (as the old `dashPercent`
 *  did) reads as "no data" when the truth is "exactly break-even". */
export const pctText = (v: number | null | undefined) =>
  v == null || !Number.isFinite(v) ? '—' : `${v >= 0 ? '+' : ''}${formatDecimalTrim(v, 1)}%`;

/** Signed SOL, 4dp, `◎` prefix. Same zero-is-not-a-dash rule as [`pctText`]. */
export const solText = (v: number | null | undefined) =>
  v == null || !Number.isFinite(v) ? '—' : `◎${v >= 0 ? '+' : ''}${formatDecimalTrim(v, 4)}`;

/** Green at/above `pivot`, red below. The one good/bad tone rule. */
export const goodBad = (v: number | null | undefined, pivot = 0) =>
  v != null && Number.isFinite(v) && v >= pivot ? 'text-green' : 'text-red';

const pctOf = (v: number | null | undefined) =>
  v == null || !Number.isFinite(v) ? '—' : `${(v * 100).toFixed(0)}%`;

// --- wire shape --------------------------------------------------------------

/** One band of a run's metrics — mirrors the Rust `RunMetrics`. */
export interface RunMetrics {
  n_fired: number;
  n_open: number;
  n_closed: number;
  win_rate: number;
  total_pnl_sol: number;
  open_pnl_sol: number;
  expectancy_sol: number;
  mean_pnl_pct: number;
  /** Nullable because not every surface can compute an interior quantile: the
   *  backend always sends a number, but the live/paper card maps from a SQL
   *  aggregate that has no per-position distribution, and rendering `+0%` there
   *  would assert a measurement that was never taken. `null` renders `—`. */
  median_pnl_pct: number | null;
  p90_pnl_pct: number | null;
  best_pnl_pct: number | null;
  worst_pnl_pct: number | null;
  std_pnl_pct: number | null;
  profit_factor: number | null;
  score: number | null;
  avg_holding_secs: number;
  median_holding_secs: number;
  n_exit_take_profit: number;
  n_exit_stop_loss: number;
  n_exit_trailing: number;
  n_exit_stall: number;
  n_exit_time: number;
  n_exit_liquidity: number;
  n_exit_next_kill: number;
  n_exit_dead: number;
  n_exit_metrics: number;
  n_exit_open: number;
}

/** A run reported twice over the same positions — mirrors the Rust `RunSummary`. */
export interface RunSummary {
  realized: RunMetrics;
  mtm: RunMetrics;
}

/** A fired position, in the minimal form the client-side builder needs. */
export interface RunOutcomeRow {
  fired: boolean;
  exit: string;
  pnl_sol: number;
  pnl_pct: number;
  holding_secs: number;
}

// --- client-side aggregation (sweep drill-in) --------------------------------

function median(vals: number[]): number {
  if (vals.length === 0) return 0;
  const s = [...vals].sort((a, b) => a - b);
  return s[Math.round((s.length - 1) * 0.5)];
}

/** Aggregate one cohort of settled rows into a `RunMetrics`. */
function metricsOf(closed: RunOutcomeRow[], nFired: number, nOpen: number, openPnl: number): RunMetrics {
  const n = closed.length;
  const pcts = closed.map((r) => r.pnl_pct);
  const total = closed.reduce((s, r) => s + r.pnl_sol, 0);
  const grossWin = closed.reduce((s, r) => (r.pnl_sol > 0 ? s + r.pnl_sol : s), 0);
  const grossLoss = closed.reduce((s, r) => (r.pnl_sol < 0 ? s - r.pnl_sol : s), 0);
  const holds = closed.map((r) => r.holding_secs).filter((v) => Number.isFinite(v) && v > 0);
  const zero = { n_exit_take_profit: 0, n_exit_stop_loss: 0, n_exit_trailing: 0, n_exit_stall: 0,
    n_exit_time: 0, n_exit_liquidity: 0, n_exit_next_kill: 0, n_exit_dead: 0,
    n_exit_metrics: 0, n_exit_open: 0 };
  return {
    n_fired: nFired,
    n_open: nOpen,
    n_closed: n,
    win_rate: n ? closed.filter((r) => r.pnl_sol > 0).length / n : 0,
    total_pnl_sol: total,
    open_pnl_sol: openPnl,
    expectancy_sol: n ? total / n : 0,
    mean_pnl_pct: n ? pcts.reduce((s, v) => s + v, 0) / n : 0,
    median_pnl_pct: median(pcts),
    p90_pnl_pct: n ? [...pcts].sort((a, b) => a - b)[Math.round((n - 1) * 0.9)] : 0,
    // reduce, not `Math.max(...pcts)` — a group can hold thousands of rows, past
    // the spread arg limit.
    best_pnl_pct: n ? pcts.reduce((m, v) => (v > m ? v : m), pcts[0]) : 0,
    worst_pnl_pct: n ? pcts.reduce((m, v) => (v < m ? v : m), pcts[0]) : 0,
    std_pnl_pct: 0,
    profit_factor: grossLoss > 0 ? grossWin / grossLoss : null,
    score: null,
    avg_holding_secs: holds.length ? holds.reduce((s, v) => s + v, 0) / holds.length : 0,
    median_holding_secs: median(holds),
    ...zero,
  };
}

/**
 * Build the two-band summary from per-token rows, client-side — the sweep
 * drill-in's path, because its summary tracks the table's *current* filters
 * rather than re-querying. Deliberately mirrors the Rust `kernel::run_summary`:
 * the MTM band is the same aggregation with the open rows reclassified as
 * settled, never a second copy of the arithmetic.
 */
export function runSummaryFromRows(rows: RunOutcomeRow[]): RunSummary {
  const fired = rows.filter((r) => r.fired);
  const closed = fired.filter((r) => r.exit !== 'Open');
  const open = fired.filter((r) => r.exit === 'Open');
  const openPnl = open.reduce((s, r) => s + r.pnl_sol, 0);
  return {
    realized: metricsOf(closed, fired.length, open.length, openPnl),
    mtm: metricsOf(fired, fired.length, open.length, openPnl),
  };
}

// --- the renderer ------------------------------------------------------------

/** Tone for the open cohort: past ~a quarter of the sample unsettled, the
 *  realized figures stop being a fair summary of what the run did. */
export function openTone(openShare: number): string {
  return openShare >= 0.5 ? 'text-red' : openShare >= 0.25 ? 'text-warning' : 'text-text-mid';
}

/** Render one band as a strip of tiles. Both bands go through this, so realized
 *  and mark-to-market are formatted identically and compare tile-for-tile down
 *  the column — the whole reason for showing them stacked. */
function bandStats(m: RunMetrics): SummaryStat[] {
  const empty = m.n_closed === 0;
  return [
    { label: 'Total PnL (◎)', value: solText(m.total_pnl_sol), cls: goodBad(m.total_pnl_sol) },
    {
      label: 'Win %',
      value: empty ? '—' : pctOf(m.win_rate),
      cls: empty ? undefined : goodBad(m.win_rate, 0.5),
    },
    { label: 'Expectancy (◎)', value: empty ? '—' : solText(m.expectancy_sol), cls: goodBad(m.expectancy_sol) },
    {
      label: 'Profit factor',
      value: empty ? '—' : m.profit_factor == null ? '∞' : m.profit_factor.toFixed(2),
      cls: empty ? undefined : goodBad(m.profit_factor ?? 10, 1),
    },
    { label: 'Median %', value: empty ? '—' : pctText(m.median_pnl_pct), cls: goodBad(m.median_pnl_pct) },
    { label: 'Mean %', value: empty ? '—' : pctText(m.mean_pnl_pct), cls: goodBad(m.mean_pnl_pct) },
    { label: 'Best %', value: empty ? '—' : pctText(m.best_pnl_pct), cls: empty ? undefined : 'text-green' },
    { label: 'Worst %', value: empty ? '—' : pctText(m.worst_pnl_pct), cls: empty ? undefined : 'text-red' },
  ];
}

/**
 * The shared hero row + bands for a run summary.
 *
 * **Reports every PnL figure twice, on purpose.** A still-open position has a
 * mark-to-last-price PnL but no realized outcome, so the `realized` band measures
 * closed trades only. Read alone that flatters a run which simply never closed
 * its losers — they never entered the sum. So the `Incl. open (MTM)` band values
 * every fired position beside it. Neither is "the" answer: realized is what
 * happened, MTM is what the run is currently worth, and the **gap between them is
 * the signal**. The MTM band is omitted when nothing is open (it would repeat the
 * realized band tile-for-tile).
 */
export function runSummarySections(s: RunSummary): {
  hero: SummaryStat[];
  sections: SummarySection[];
} {
  const { realized, mtm } = s;
  const nFired = realized.n_fired;
  const nOpen = realized.n_open;
  const nClosed = realized.n_closed;
  const openShare = nFired ? nOpen / nFired : 0;
  const tone = openTone(openShare);

  const hero: SummaryStat[] = [
    { label: 'PnL realized', value: solText(realized.total_pnl_sol), cls: goodBad(realized.total_pnl_sol) },
    { label: 'PnL incl. open', value: solText(mtm.total_pnl_sol), cls: goodBad(mtm.total_pnl_sol) },
    {
      label: 'Win % (real.)',
      value: nClosed === 0 ? '—' : pctOf(realized.win_rate),
      cls: nClosed === 0 ? undefined : goodBad(realized.win_rate, 0.5),
    },
    {
      label: 'Fired',
      node: (
        <>
          <span className="text-info">{nFired}</span>
          {nOpen > 0 && <span className={cn('ml-2 text-base font-bold', tone)}>{nOpen} open</span>}
        </>
      ),
    },
  ];

  const exits: Array<[string, number, string]> = [
    ['TakeProfit', realized.n_exit_take_profit, 'text-green'],
    ['StopLoss', realized.n_exit_stop_loss, 'text-red'],
    ['Metrics', realized.n_exit_metrics, 'text-text-mid'],
    ['Dead', realized.n_exit_dead, 'text-red'],
  ];

  const sections: SummarySection[] = [
    {
      title: 'Positions',
      hint: 'What the run did, before any PnL is counted',
      stats: [
        { label: 'Fired', value: String(nFired), cls: 'text-info' },
        { label: 'Closed', value: String(nClosed), cls: 'text-info' },
        { label: 'Open', value: String(nOpen), cls: nOpen > 0 ? tone : 'text-text-dim' },
        {
          label: 'Open share',
          value: nFired ? pctOf(openShare) : '—',
          cls: nFired ? tone : undefined,
        },
        {
          label: 'TP / SL / Met / Dead',
          node: (
            <>
              {exits.map(([tag, n, cls], i) => (
                <span key={tag}>
                  {i > 0 && <span className="text-text-dim"> / </span>}
                  <span className={cls}>{n}</span>
                </span>
              ))}
            </>
          ),
        },
        { label: 'Avg hold', value: fmtSecs(realized.avg_holding_secs), cls: 'text-accent' },
      ],
    },
    {
      title: 'Realized',
      hint:
        nOpen > 0
          ? `Closed positions only (${nClosed} of ${nFired}) — the ${nOpen} open are excluded`
          : `All ${nClosed} positions closed`,
      titleCls: 'text-green',
      stats: bandStats(realized),
    },
  ];

  if (nOpen > 0) {
    sections.push({
      title: 'Incl. open (MTM)',
      hint: `All ${nFired} fired — the ${nOpen} open valued at their last price (unrealized)`,
      titleCls: 'text-warning',
      stats: [
        ...bandStats(mtm),
        {
          label: 'of which unreal.',
          value: solText(realized.open_pnl_sol),
          cls: goodBad(realized.open_pnl_sol),
        },
      ],
    });
  }

  return { hero, sections };
}
