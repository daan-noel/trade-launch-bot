import type { ReactNode } from 'react';
import { cn } from 'lib/cn';
import { pctGradeClass, signedToneClass, winRateGradeClass } from 'lib/signedTone';
import {
  isMetricExitReason,
  normalizeExitReasonFilter,
  parseMetricExitParts,
} from 'lib/strategy/exitReason';
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

/**
 * Good/bad tone. Pivot `0` (default) uses the signed PnL rule (`signedToneClass`:
 * strict sign, zero neutral). Non-zero pivots keep threshold semantics
 * (at/above → green, below → red) for profit-factor / etc. Win-rate uses
 * [`winRateGradeClass`] instead.
 */
export const goodBad = (v: number | null | undefined, pivot = 0) => {
  if (v == null || !Number.isFinite(v)) return 'text-text-dim';
  if (pivot === 0) return signedToneClass(v);
  return v >= pivot ? 'text-green' : 'text-red';
};

const pctOf = (v: number | null | undefined) =>
  v == null || !Number.isFinite(v) ? '—' : `${(v * 100).toFixed(0)}%`;

/**
 * **The one capital-weighted return** — the TS mirror of
 * `hunter_core::strategies::kernel::weighted_return_pct`. A percent shown beside a
 * SOL figure is that SOL over the capital that produced it: never a price ratio
 * (which charges no execution cost) and never a mean of other percents (buy size
 * is editable mid-run, so a 0.05 ◎ trade must not outvote a 1.0 ◎ one). Because
 * the denominator is capital, and capital is positive, the percent can never
 * disagree in sign with the ◎ it sits next to.
 *
 * Deliberately differs from the Rust fn in one way: no capital deployed returns
 * `null`, not `0`. The backend needs a total-order `f64` to sort a column by; a
 * tile needs to render `—` rather than assert an exact break-even that was never
 * measured.
 */
export function weightedReturnPct(
  sumPnlSol: number | null | undefined,
  sumCapitalSol: number | null | undefined,
): number | null {
  if (sumPnlSol == null || !Number.isFinite(sumPnlSol)) return null;
  if (sumCapitalSol == null || !Number.isFinite(sumCapitalSol) || sumCapitalSol <= 0) return null;
  return (sumPnlSol / sumCapitalSol) * 100;
}

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
  /** Equal-weighted mean of the per-trade returns. Under the fixed notional a
   *  backtest uses, this equals the capital-weighted return; a live rule whose
   *  buy size changed mid-run has varying notionals, so those surfaces send
   *  `return_pct` as well and the tile prefers it. */
  mean_pnl_pct: number;
  /** **Capital-weighted** realized return (`Σ pnl / Σ closed entry × 100`) —
   *  present only where the notional can vary (the live/paper positions
   *  summary). When set it REPLACES the Mean % tile, because with mixed trade
   *  sizes the mean is the figure that can disagree with Total PnL, not this. */
  return_pct?: number | null;
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
  /** Mean pnl% over all fired incl. open marks — present on sweep/sim wire. */
  mtm_pnl_pct?: number;
  /** Arithmetic **sum** of the closed positions' per-trade PnL% — the positions
   *  table's PnL% column added up, rendered as its own `Σ trade %` tile.
   *
   *  A tally, not a return: it weights every position equally regardless of buy
   *  size and grows with trade count, so it answers "what do my trades add up to"
   *  and not "what did my capital return" (that stays `bandReturnPct`). Carried
   *  only on the **realized** band — an open position has no realized percent to
   *  add, so the MTM band leaves it `null` and the tile drops rather than
   *  printing a sum over a different cohort than its ◎. */
  sum_pnl_pct?: number | null;
  score: number | null;
  avg_holding_secs: number;
  median_holding_secs: number;
  n_exit_take_profit: number;
  n_exit_stop_loss: number;
  n_exit_trailing: number;
  n_exit_stall: number;
  n_exit_time: number;
  n_exit_liquidity: number;
  n_exit_dead: number;
  /** Total metric-condition exits. Prefer the win/loss split when present. */
  n_exit_metrics: number;
  /** Metric exits with positive realized SOL. Optional for older wire payloads. */
  n_exit_metrics_win?: number;
  /** Metric exits that are not wins (loss / break-even). */
  n_exit_metrics_loss?: number;
  n_exit_open: number;
  /** Operator-initiated close. Optional because it has **no Rust `RunMetrics`
   *  peer**: the analysis kernel can't produce a manual close, so only the
   *  live/paper card and row-level aggregation ever populate it. */
  n_exit_manual?: number;
}

/** A run reported twice over the same positions — mirrors the Rust `RunSummary`. */
export interface RunSummary {
  realized: RunMetrics;
  mtm: RunMetrics;
}

/**
 * The percent that belongs beside **this band's** `total_pnl_sol` — the ONE
 * reader of the capital-weighted-vs-mean choice, so the hero tile and the band
 * tile below it can never give two answers to the same question.
 *
 * `return_pct` is present only where the notional varies between positions (a
 * live/paper book, whose `buy_amount_lamports` is editable mid-run). A backtest
 * prices every round trip at one fixed notional, and there the equal-weighted
 * `mean_pnl_pct` **is** the capital-weighted return — same number, no denominator
 * to ship. Each band carries its own: realized divides by the closed positions'
 * capital, MTM by every fired position's, because an open bag's capital is
 * deployed too.
 */
export function bandReturnPct(m: RunMetrics): number | null {
  if (m.return_pct != null && Number.isFinite(m.return_pct)) return m.return_pct;
  return Number.isFinite(m.mean_pnl_pct) ? m.mean_pnl_pct : null;
}

/** A fired position, in the minimal form the client-side builder needs. */
export interface RunOutcomeRow {
  fired: boolean;
  exit: string;
  pnl_sol: number;
  pnl_pct: number;
  holding_secs: number;
  /** Entry cost of this position, when the surface has it. Supplying it makes the
   *  fold's `return_pct` capital-weighted — required on a live/paper cohort,
   *  where buy size varies. Omit on a fixed-notional backtest: `mean_pnl_pct`
   *  already equals the capital-weighted return there. */
  entry_sol?: number;
}

// --- exit-reason vocabulary (SSOT) -------------------------------------------

/** The `RunMetrics` fields that are exit-reason counts. */
export type ExitCountKey =
  | 'n_exit_take_profit'
  | 'n_exit_stop_loss'
  | 'n_exit_metrics_win'
  | 'n_exit_metrics_loss'
  | 'n_exit_metrics'
  | 'n_exit_dead'
  | 'n_exit_manual'
  | 'n_exit_trailing'
  | 'n_exit_stall'
  | 'n_exit_time'
  | 'n_exit_liquidity';

/**
 * **Every** way a position can leave, in ladder order — the one list the
 * breakdown renders from, so a reason can't be silently dropped from the display.
 *
 * The old tile hard-coded four (`TP / SL / Met / Dead`) and omitted the five the
 * legacy tpsl/swing ladders actually emit, so on those runs the numbers shown
 * didn't add up to `n_closed` while *looking* like a complete breakdown.
 *
 * Colors are **status** semantics, not categorical: good (TP / Metric+) / bad
 * (SL / Metric-) / terminal (Dead). Metric exits are split by realized PnL so a
 * metric-heavy rule's win/loss mix is glanceable — same idea as the Reason badge.
 * Note `Dead` is deliberately `accent`, not `red` — it shared red with `StopLoss`
 * before, which made the two indistinguishable in a color-only tile. Every swatch
 * ships beside a text label, so color is never the sole identity cue.
 *
 * `n_exit_metrics` (unsplittable total) is a **legacy fallback** only — used when
 * the wire has a total but no win/loss split (older sweep rows).
 */
export const EXIT_KINDS: ReadonlyArray<{
  key: ExitCountKey;
  /** Tile label — also the React key, so these must stay unique. */
  label: string;
  /** Long form for the bar segment's tooltip. */
  full: string;
  cls: string;
  bar: string;
  /** Always shown even at zero: the two outcomes that define whether a rule
   *  works. A zero there is information ("nothing ever hit the stop"); a zero on
   *  a legacy reason the engine can't emit is just noise. */
  core?: boolean;
  /** Skip in the accounted sum when the win/loss split is present (avoids
   *  double-counting `n_exit_metrics` against Metric+/Metric-). */
  legacyMetricsTotal?: boolean;
}> = [
  { key: 'n_exit_take_profit', label: 'Take profit', full: 'Take profit', cls: 'text-green', bar: 'bg-green', core: true },
  { key: 'n_exit_stop_loss', label: 'Stop loss', full: 'Stop loss', cls: 'text-red', bar: 'bg-red', core: true },
  { key: 'n_exit_metrics_win', label: 'Metric+', full: 'Metric exit (win)', cls: 'text-green', bar: 'bg-green' },
  { key: 'n_exit_metrics_loss', label: 'Metric-', full: 'Metric exit (loss)', cls: 'text-red', bar: 'bg-red' },
  {
    key: 'n_exit_metrics',
    label: 'Metric',
    full: 'Metric exit condition',
    cls: 'text-info',
    bar: 'bg-info',
    legacyMetricsTotal: true,
  },
  { key: 'n_exit_dead', label: 'Dead', full: 'Died (liquidity gone)', cls: 'text-accent', bar: 'bg-accent' },
  { key: 'n_exit_manual', label: 'Manual', full: 'Closed by operator', cls: 'text-secondary', bar: 'bg-secondary' },
  { key: 'n_exit_trailing', label: 'Trailing', full: 'Trailing stop', cls: 'text-primary', bar: 'bg-primary' },
  { key: 'n_exit_stall', label: 'Stall', full: 'Stalled', cls: 'text-secondary', bar: 'bg-secondary' },
  { key: 'n_exit_time', label: 'Time', full: 'Time stop', cls: 'text-warning', bar: 'bg-warning' },
  { key: 'n_exit_liquidity', label: 'Liquidity', full: 'Liquidity exit', cls: 'text-text-mid', bar: 'bg-text-mid' },
];

/** Persisted `ExitReason` string → the `RunMetrics` counter it feeds. Mirrors the
 *  Rust `ExitCode::from_reason`; `Open`/`NoEntry` are not exits and are absent.
 *  `Metrics` is handled in [`countExits`] (PnL split) — not listed here. */
const EXIT_KEY_BY_REASON: Readonly<Record<string, ExitCountKey>> = {
  TakeProfit: 'n_exit_take_profit',
  StopLoss: 'n_exit_stop_loss',
  Dead: 'n_exit_dead',
  Manual: 'n_exit_manual',
  TrailingStop: 'n_exit_trailing',
  Stall: 'n_exit_stall',
  TimeStop: 'n_exit_time',
  LiquidityExit: 'n_exit_liquidity',
};

/**
 * Glanceable text tone for a persisted exit reason **or** a History filter
 * substring (`Trailing` / `Time` / `Liquidity`). Same hues as {@link EXIT_KINDS}
 * so the cohort filter and the breakdown tile never disagree.
 */
export function exitReasonToneClass(reason: string | null | undefined): string {
  if (!reason) return 'text-text-dim';
  const normalized = normalizeExitReasonFilter(reason);
  const key = EXIT_KEY_BY_REASON[normalized];
  if (key) {
    for (const k of EXIT_KINDS) {
      if (k.key === key) return k.cls;
    }
  }
  if (isMetricExitReason(normalized)) return 'text-info';
  return 'text-text-dim';
}

/** All exit counters at zero — the base every builder starts from. */
export function zeroExitCounts(): Record<ExitCountKey, number> {
  return {
    n_exit_take_profit: 0,
    n_exit_stop_loss: 0,
    n_exit_metrics_win: 0,
    n_exit_metrics_loss: 0,
    n_exit_metrics: 0,
    n_exit_dead: 0,
    n_exit_manual: 0,
    n_exit_trailing: 0,
    n_exit_stall: 0,
    n_exit_liquidity: 0,
    n_exit_time: 0,
  };
}

/** One row of the rendered breakdown. */
export interface ExitSlice {
  label: string;
  full: string;
  n: number;
  /** Share of closed positions, 0..1. */
  share: number;
  cls: string;
  bar: string;
}

/**
 * Split a band's closed positions by exit reason, **reconciling to `n_closed`**.
 *
 * Wire-counter path: metric exits collapse to Metric+ / Metric- (PnL sign). Prefer
 * [`exitBreakdownFromRows`] when per-position `exit_reason` strings are available
 * so detail labels (`stall > 300`) stay distinct.
 *
 * Any closed position whose reason isn't one of the known counters lands in a
 * trailing `Other` slice rather than vanishing, so the parts always sum to the
 * whole. That makes a miscount *visible* instead of silent — which is how the
 * `"Manual"`/`"Migrated"` reasons (mapped to `Open` by the Rust
 * `ExitCode::from_reason`, so they never reach a counter) show up as a
 * discrepancy the reader can act on, rather than quietly skewing the mix.
 */
export function exitBreakdown(m: RunMetrics): ExitSlice[] {
  const closed = m.n_closed;
  const denom = closed > 0 ? closed : 1;
  const slices: ExitSlice[] = [];
  let accounted = 0;
  const metricsWin = m.n_exit_metrics_win ?? 0;
  const metricsLoss = m.n_exit_metrics_loss ?? 0;
  const metricsSplit = metricsWin + metricsLoss;
  // Prefer the PnL split; fall back to the unsplittable total only when the
  // wire has no win/loss counters (older sweep rows).
  const useMetricsSplit = metricsSplit > 0 || (m.n_exit_metrics ?? 0) === 0;
  for (const k of EXIT_KINDS) {
    if (k.legacyMetricsTotal && useMetricsSplit) continue;
    if (
      !useMetricsSplit &&
      (k.key === 'n_exit_metrics_win' || k.key === 'n_exit_metrics_loss')
    ) {
      continue;
    }
    const n = m[k.key] ?? 0;
    accounted += n;
    if (n > 0 || k.core) {
      slices.push({ label: k.label, full: k.full, n, share: n / denom, cls: k.cls, bar: k.bar });
    }
  }
  const other = closed - accounted;
  if (other > 0) {
    slices.push({
      label: 'Other',
      full: 'Closed with an unrecognised exit reason',
      n: other,
      share: other / denom,
      cls: 'text-text-mid',
      bar: 'bg-text-mid',
    });
  }
  return slices;
}

/** Minimal row for a detailed exit mix (closed positions only). */
export interface ExitOutcomeRow {
  exit: string;
  pnl_sol: number;
}

function metricSliceTone(pnlSol: number): { cls: string; bar: string } {
  if (pnlSol > 0) return { cls: 'text-green', bar: 'bg-green' };
  if (pnlSol < 0) return { cls: 'text-red', bar: 'bg-red' };
  return { cls: 'text-info', bar: 'bg-info' };
}

/**
 * Exit mix from per-position reasons — system exits keep EXIT_KINDS labels;
 * metric-condition exits keep the **stored** detail (`stall > 300`, …).
 * Legacy bare `Metrics` still splits Metric+ / Metric- by PnL.
 */
export function exitBreakdownFromRows(
  rows: readonly ExitOutcomeRow[],
): ExitSlice[] {
  const denom = rows.length > 0 ? rows.length : 1;
  type Bucket = {
    label: string;
    full: string;
    n: number;
    wins: number;
    pnl_sol: number;
    cls: string;
    bar: string;
    /** Sort: system ladder index, then metric details by count. */
    order: number;
  };
  const byKey = new Map<string, Bucket>();

  const bump = (
    key: string,
    init: Omit<Bucket, 'n' | 'wins' | 'pnl_sol'> & { pnl_sol: number; win: boolean },
  ) => {
    const cur = byKey.get(key);
    if (cur) {
      cur.n += 1;
      cur.pnl_sol += init.pnl_sol;
      if (init.win) cur.wins += 1;
      if (key.startsWith('metric:')) {
        const tone = metricSliceTone(cur.pnl_sol);
        cur.cls = tone.cls;
        cur.bar = tone.bar;
      }
      return;
    }
    byKey.set(key, {
      label: init.label,
      full: init.full,
      n: 1,
      wins: init.win ? 1 : 0,
      pnl_sol: init.pnl_sol,
      cls: init.cls,
      bar: init.bar,
      order: init.order,
    });
  };

  for (const r of rows) {
    const reason = (r.exit ?? '').trim();
    if (!reason || reason === 'Open' || reason === 'NoEntry') continue;
    const win = r.pnl_sol > 0;

    if (isMetricExitReason(reason)) {
      if (reason === 'Metrics') {
        const label = win ? 'Metric+' : 'Metric-';
        const kind = EXIT_KINDS.find((k) => k.label === label)!;
        bump(`legacy:${label}`, {
          label,
          full: kind.full,
          cls: kind.cls,
          bar: kind.bar,
          order: EXIT_KINDS.findIndex((k) => k.label === label),
          pnl_sol: r.pnl_sol,
          win,
        });
        continue;
      }
      // Exact stored detail — not Metric±.
      const tone = metricSliceTone(r.pnl_sol);
      bump(`metric:${reason}`, {
        label: reason,
        full: reason,
        cls: tone.cls,
        bar: tone.bar,
        order: 1000,
        pnl_sol: r.pnl_sol,
        win,
      });
      continue;
    }

    const countKey = EXIT_KEY_BY_REASON[reason];
    if (countKey) {
      const kind = EXIT_KINDS.find((k) => k.key === countKey);
      if (kind && !kind.legacyMetricsTotal) {
        bump(`sys:${kind.label}`, {
          label: kind.label,
          full: kind.full,
          cls: kind.cls,
          bar: kind.bar,
          order: EXIT_KINDS.findIndex((k) => k.key === countKey),
          pnl_sol: r.pnl_sol,
          win,
        });
        continue;
      }
    }
    bump('other', {
      label: 'Other',
      full: 'Closed with an unrecognised exit reason',
      cls: 'text-text-mid',
      bar: 'bg-text-mid',
      order: 2000,
      pnl_sol: r.pnl_sol,
      win,
    });
  }

  const system: Bucket[] = [];
  const metrics: Bucket[] = [];
  let other: Bucket | undefined;
  for (const b of byKey.values()) {
    if (b.label === 'Other') other = b;
    else if (b.order >= 1000 && b.order < 2000) metrics.push(b);
    else system.push(b);
  }
  system.sort((a, b) => a.order - b.order);
  metrics.sort((a, b) => b.n - a.n || a.label.localeCompare(b.label));

  // Always surface empty TP/SL cores when anything closed (same as wire path).
  const haveCore = new Set(system.map((s) => s.label));
  for (const k of EXIT_KINDS) {
    if (!k.core) continue;
    if (haveCore.has(k.label)) continue;
    system.push({
      label: k.label,
      full: k.full,
      n: 0,
      wins: 0,
      pnl_sol: 0,
      cls: k.cls,
      bar: k.bar,
      order: EXIT_KINDS.findIndex((x) => x.key === k.key),
    });
  }
  system.sort((a, b) => a.order - b.order);

  const toSlice = (b: Bucket): ExitSlice => {
    const parts = [b.full === b.label ? b.label : b.full, `n=${b.n}`, pctOf(b.n / denom)];
    if (b.n > 0 && (parseMetricExitParts(b.label) != null || b.label.startsWith('Metric'))) {
      parts.push(`${b.wins}W/${b.n - b.wins}L`, solText(b.pnl_sol));
    }
    return {
      label: b.label,
      full: parts.join(' · '),
      n: b.n,
      share: b.n / denom,
      cls: b.cls,
      bar: b.bar,
    };
  };

  const out: ExitSlice[] = [];
  for (const b of system) {
    if (b.n > 0 || EXIT_KINDS.some((k) => k.label === b.label && k.core)) {
      out.push(toSlice(b));
    }
  }
  for (const b of metrics) {
    if (b.n > 0) out.push(toSlice(b));
  }
  if (other && other.n > 0) out.push(toSlice(other));
  return out;
}

// --- client-side aggregation (sweep drill-in) --------------------------------

function median(vals: number[]): number {
  if (vals.length === 0) return 0;
  const s = [...vals].sort((a, b) => a - b);
  return s[Math.round((s.length - 1) * 0.5)];
}

/** Tally the exit reasons of a cohort of closed rows. A reason with no counter
 *  (`Manual`, `Migrated`, a typo) is intentionally *not* forced into a bucket —
 *  it goes unaccounted so [`exitBreakdown`] surfaces it as `Other`.
 *  Metric exits split by realized PnL (`Metric+` / `Metric-`).
 *
 *  Metric membership goes through [`isMetricExitReason`], never `=== 'Metrics'`:
 *  the sweep drill-in and the live engine both stamp the **detail** label
 *  (`retrace >= 12`, legacy compact `stall>`), and only legacy rows carry bare
 *  `Metrics`. Testing the bare form alone sent every metric exit to `Other` —
 *  which on a generic-engine rule is the entire mix. Mirrors the Rust
 *  `ExitCode::from_reason` (`is_metric_exit_label`). */
function countExits(closed: RunOutcomeRow[]): Record<ExitCountKey, number> {
  const counts = zeroExitCounts();
  for (const r of closed) {
    if (isMetricExitReason(r.exit)) {
      counts.n_exit_metrics += 1;
      if (r.pnl_sol > 0) counts.n_exit_metrics_win += 1;
      else counts.n_exit_metrics_loss += 1;
      continue;
    }
    const key = EXIT_KEY_BY_REASON[r.exit];
    if (key) counts[key] += 1;
  }
  return counts;
}

/**
 * Aggregate one cohort of settled rows into a `RunMetrics`.
 *
 * `exits` is passed in rather than derived from `closed` because the two bands
 * disagree on purpose: the MTM band reclassifies still-open positions as settled
 * to value them, but they have no exit reason, so mirroring the Rust
 * `kernel::run_summary` it reports zeroed exit counters instead of a mix that
 * would double-count the open cohort. Only `realized` carries the breakdown.
 */
function metricsOf(
  closed: RunOutcomeRow[],
  nFired: number,
  nOpen: number,
  openPnl: number,
  exits: Record<ExitCountKey, number>,
): RunMetrics {
  const n = closed.length;
  const pcts = closed.map((r) => r.pnl_pct);
  const total = closed.reduce((s, r) => s + r.pnl_sol, 0);
  const grossWin = closed.reduce((s, r) => (r.pnl_sol > 0 ? s + r.pnl_sol : s), 0);
  const grossLoss = closed.reduce((s, r) => (r.pnl_sol < 0 ? s - r.pnl_sol : s), 0);
  const holds = closed.map((r) => r.holding_secs).filter((v) => Number.isFinite(v) && v > 0);
  // Capital-weighted return, but only when the rows carry their own entry cost —
  // that is a live/paper cohort, where buy size varies between positions and an
  // equal-weighted mean can therefore disagree in sign with the ◎ beside it. A
  // fixed-notional backtest ships no `entry_sol`, this stays `null`, and
  // `bandReturnPct` falls back to `mean_pnl_pct`, which is the same number there.
  //
  // All-or-nothing: the numerator sums EVERY row's PnL, so a denominator built
  // from only the rows that happen to carry a cost is a ratio between two
  // different cohorts, and it overstates the return without ever looking wrong.
  // Better to report no percent than a confidently inflated one.
  const hasCapital =
    closed.length > 0 &&
    closed.every((r) => r.entry_sol != null && Number.isFinite(r.entry_sol) && r.entry_sol > 0);
  const capital = hasCapital ? closed.reduce((s, r) => s + (r.entry_sol as number), 0) : 0;
  return {
    n_fired: nFired,
    n_open: nOpen,
    n_closed: n,
    win_rate: n ? closed.filter((r) => r.pnl_sol > 0).length / n : 0,
    total_pnl_sol: total,
    open_pnl_sol: openPnl,
    expectancy_sol: n ? total / n : 0,
    mean_pnl_pct: n ? pcts.reduce((s, v) => s + v, 0) / n : 0,
    return_pct: weightedReturnPct(total, capital),
    // The column sum. `null` on an empty cohort so the tile drops instead of
    // asserting a measured `+0%`, matching how `median_pnl_pct` handles absence.
    sum_pnl_pct: n ? pcts.reduce((s, v) => s + v, 0) : null,
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
    ...exits,
    n_exit_open: nOpen,
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
  const exits = countExits(closed);
  const mtmPct = fired.length
    ? fired.reduce((s, r) => s + r.pnl_pct, 0) / fired.length
    : 0;
  const realized = metricsOf(closed, fired.length, open.length, openPnl, exits);
  const mtm = metricsOf(fired, fired.length, open.length, openPnl, zeroExitCounts());
  realized.mtm_pnl_pct = mtmPct;
  mtm.mtm_pnl_pct = mtmPct;
  // The column sum is realized-only: the MTM band folded the open rows in, and an
  // open position contributes a `0` percent it never earned. Dropping the tile
  // there beats summing a cohort the ◎ beside it doesn't describe.
  mtm.sum_pnl_pct = null;
  return { realized, mtm };
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
function bandStats(m: RunMetrics, extended = false): SummaryStat[] {
  const empty = m.n_closed === 0;
  const stats: SummaryStat[] = [
    { label: 'Total PnL (◎)', value: solText(m.total_pnl_sol), cls: goodBad(m.total_pnl_sol) },
    {
      label: 'Win %',
      value: empty ? '—' : pctOf(m.win_rate),
      cls: empty ? undefined : winRateGradeClass(m.win_rate),
    },
    { label: 'Expectancy (◎)', value: empty ? '—' : solText(m.expectancy_sol), cls: goodBad(m.expectancy_sol) },
    {
      label: 'Profit factor',
      value: empty ? '—' : m.profit_factor == null ? '∞' : m.profit_factor.toFixed(2),
      cls: empty ? undefined : goodBad(m.profit_factor ?? 10, 1),
    },
    { label: 'Median %', value: empty ? '—' : pctText(m.median_pnl_pct), cls: empty ? undefined : pctGradeClass(m.median_pnl_pct) },
    // Value via `bandReturnPct` (the one reader); only the LABEL depends on which
    // of the two it picked, so the hero tile above can't print a different figure.
    {
      label: m.return_pct != null ? 'Return %' : 'Mean %',
      value: empty ? '—' : pctText(bandReturnPct(m)),
      cls: empty ? undefined : pctGradeClass(bandReturnPct(m)),
    },
    { label: 'Best %', value: empty ? '—' : pctText(m.best_pnl_pct), cls: empty ? undefined : pctGradeClass(m.best_pnl_pct) },
    // Dropped, not dashed, when the band has no per-trade sum: the MTM band and
    // any wire that predates the aggregate genuinely don't measure this, and a
    // `—` between two real percents reads as a failed measurement instead.
    ...(!empty && m.sum_pnl_pct != null
      ? [{
          label: 'Σ trade %',
          value: pctText(m.sum_pnl_pct),
          // Toned by sign only. `pctGradeClass` grades against per-trade
          // thresholds, and this figure is a tally of N trades — a +180% sum over
          // 300 closes is not a 180% trade and must not wear that badge.
          cls: goodBad(m.sum_pnl_pct),
        }]
      : []),
    { label: 'Worst %', value: empty ? '—' : pctText(m.worst_pnl_pct), cls: empty ? undefined : pctGradeClass(m.worst_pnl_pct) },
  ];
  // Extended tiles — the distribution's tails/spread, shown only where the summary
  // is server-computed (dry-run). The client-side row builder can't fill `std`/
  // `p90` faithfully, so its surfaces don't opt in and never show a misleading 0.
  if (extended) {
    stats.push(
      { label: 'P90 %', value: empty ? '—' : pctText(m.p90_pnl_pct), cls: empty ? undefined : pctGradeClass(m.p90_pnl_pct) },
      {
        label: 'Std %',
        value: empty || m.std_pnl_pct == null ? '—' : `${formatDecimalTrim(m.std_pnl_pct, 1)}%`,
        cls: 'text-text-mid',
      },
    );
  }
  return stats;
}

/**
 * A hero PnL tile: the ◎ with its return % inline beside it, one size down.
 *
 * The two are **one fact read two ways**, so they share a tile and a tone rather
 * than sitting in separate columns: `weightedReturnPct` sign-locks them, so a
 * reader who sees green ◎ and red % has found a bug, not a nuance. Sizing follows
 * the `Fired` tile's `N open` sub-figure, which is the same pattern — a headline
 * with a qualifier that must not compete with it.
 *
 * The percent is dropped entirely (not rendered as `—`) when the band has no
 * denominator to divide by: an empty slot reads as "this tile is just the ◎",
 * while a dash beside a real number reads as a measurement that failed.
 */
function pnlHeroTile(label: string, pnlSol: number, pct: number | null): SummaryStat {
  const tone = goodBad(pnlSol);
  return {
    label,
    node: (
      <>
        <span className={tone}>{solText(pnlSol)}</span>
        {pct != null && (
          <span className={cn('ml-2 text-base font-bold', tone)}>{pctText(pct)}</span>
        )}
      </>
    ),
  };
}

/**
 * The exit mix as one horizontal proportion bar — the at-a-glance read the old
 * slash-joined tile couldn't give, because four bare numbers make you do the
 * division yourself to see which exit dominates.
 *
 * Segments are flex-grown by count, so widths are the mix. Zero-count slices are
 * dropped here (they'd be invisible anyway) while their tile below still shows
 * the `0`. The 2px gaps are surface-colored separators, not padding: they keep
 * adjacent fills from reading as one blended block.
 */
function ExitMixBar({
  slices,
  activeReason,
  onSelect,
}: {
  slices: ExitSlice[];
  activeReason?: string | null;
  onSelect?: (label: string) => void;
}) {
  const shown = slices.filter((s) => s.n > 0);
  if (shown.length === 0) return null;
  return (
    <div className="flex h-1.5 w-full max-w-lg gap-0.5" role="img" aria-label="Exit reason mix">
      {shown.map((s) => {
        const active = activeReason === s.label;
        const title = `${s.full}: ${s.n} (${pctOf(s.share)})${onSelect ? ' — click to focus' : ''}`;
        if (!onSelect) {
          return (
            <div
              key={s.label}
              className={cn('h-full rounded-xs', s.bar)}
              style={{ flexGrow: s.n, flexBasis: 0 }}
              title={title}
            />
          );
        }
        return (
          <button
            key={s.label}
            type="button"
            onClick={() => onSelect(s.label)}
            title={title}
            className={cn(
              'h-full rounded-xs border-0 p-0',
              s.bar,
              'cursor-pointer transition hover:opacity-90',
              active && 'ring-1 ring-white/70',
            )}
            style={{ flexGrow: s.n, flexBasis: 0 }}
          />
        );
      })}
    </div>
  );
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
/** Optional click wiring for focus chips — capital / hold / score tiles stay inert. */
export interface RunSummaryInteraction {
  /** Currently active status lens, if any. */
  status?: 'open' | 'closed' | 'fired' | null;
  /** Active exit tile label (`Take profit`, `Metric+`, …). */
  exitLabel?: string | null;
  outcome?: 'win' | 'loss' | null;
  migrated?: boolean | null;
  onToggleStatus?: (status: 'open' | 'closed' | 'fired') => void;
  onToggleExit?: (tileLabel: string) => void;
  onToggleOutcome?: (outcome: 'win' | 'loss') => void;
  onToggleMigrated?: (migrated: boolean) => void;
}

export function runSummarySections(
  s: RunSummary,
  extras: {
    /** Distinct tokens whose creation axes matched the rule's fingerprint —
     *  the candidate pool `n_fired` positions are drawn from (entered one or
     *  more times, or never entered). Omit when the surface can't source it
     *  (e.g. live/paper has no fingerprint-scan concept yet); the tile is then
     *  hidden rather than shown as a misleading `0`. */
    matched?: number;
    /** Distinct tokens actually entered (≥1 fired episode) — narrower than
     *  `matched` and, under a re-entry rule, smaller than `n_fired` (every
     *  entry counts, so one mint's repeat visits each add to `n_fired` but not
     *  to this). Pair with `n_fired` to show re-entry volume; omit when the
     *  surface can't source it. */
    tokensEntered?: number;
    /** Count of fired tokens that graduated off the bonding curve to AMM
     *  (`is_migrated`) — a token-quality signal orthogonal to how the rule
     *  exited, so it lives beside the position counts rather than in the exit
     *  mix. Omit when the surface can't source it; the tile is then hidden
     *  rather than shown as a misleading `0`. */
    migrated?: number;
    /** Append the distribution's tail/spread tiles (P90 %, Std %) to each band
     *  and median-hold + score to Positions. Only for server-computed summaries
     *  (the dry-run) — the client-side row builder can't fill these faithfully. */
    extended?: boolean;
    /** Wire Open / Closed / Fired / exit / win-loss / migrated tiles into focus. */
    interaction?: RunSummaryInteraction;
    /**
     * Closed-position exit strings for a detailed mix (exact metric labels).
     * When non-empty, replaces the wire Metric+/− collapse.
     */
    exitOutcomes?: readonly ExitOutcomeRow[];
  } = {},
): {
  hero: SummaryStat[];
  /** Always-visible exit mix (when closed > 0) — pulled out of Details accordion. */
  exitMix?: ReactNode;
  /** Detail bands for the Details accordion (Positions / Exits tiles / Realized / MTM). */
  sections: SummarySection[];
} {
  const extended = extras.extended ?? false;
  const ix = extras.interaction;
  const { realized, mtm } = s;
  const nFired = realized.n_fired;
  const nOpen = realized.n_open;
  const nClosed = realized.n_closed;
  const openShare = nFired ? nOpen / nFired : 0;
  const tone = openTone(openShare);

  const hero: SummaryStat[] = [
    pnlHeroTile('PnL realized', realized.total_pnl_sol, bandReturnPct(realized)),
    pnlHeroTile('PnL incl. open', mtm.total_pnl_sol, bandReturnPct(mtm)),
    {
      label: 'Win % (real.)',
      value: nClosed === 0 ? '—' : pctOf(realized.win_rate),
      cls: nClosed === 0 ? undefined : winRateGradeClass(realized.win_rate),
      ...(ix?.onToggleOutcome
        ? {
            onClick: () => ix.onToggleOutcome!('win'),
            active: ix.outcome === 'win',
          }
        : {}),
    },
    {
      label: 'Fired',
      node: (
        <>
          <span className="text-info">{nFired}</span>
          {nOpen > 0 && (
            <span className={cn('ml-2 text-base font-bold', tone)}>{nOpen} open</span>
          )}
        </>
      ),
      ...(ix?.onToggleStatus
        ? { onClick: () => ix.onToggleStatus!('fired'), active: ix.status === 'fired' }
        : {}),
    },
  ];

  const slices =
    extras.exitOutcomes && extras.exitOutcomes.length > 0
      ? exitBreakdownFromRows(extras.exitOutcomes)
      : exitBreakdown(realized);

  const clickable = (
    stat: SummaryStat,
    onClick?: () => void,
    active?: boolean,
  ): SummaryStat => (onClick ? { ...stat, onClick, active } : stat);

  const sections: SummarySection[] = [
    {
      title: 'Positions',
      hint: 'What the run did, before any PnL is counted',
      stats: [
        // Candidate-pool / distinct-token tiles — only when the surface can
        // source them (Simulate; not yet the sweep drill-in or live/paper).
        // Kept ahead of `Fired` so the funnel reads left-to-right: matched →
        // entered (tokens) → fired (positions, re-entries included).
        ...(extras.matched != null
          ? [{ label: 'Matched', value: String(extras.matched), cls: 'text-text-dim' } satisfies SummaryStat]
          : []),
        ...(extras.tokensEntered != null
          ? [
              { label: 'Tokens', value: String(extras.tokensEntered), cls: 'text-info' } satisfies SummaryStat,
              {
                label: 'Re-entries',
                value: String(Math.max(nFired - extras.tokensEntered, 0)),
                cls: nFired > extras.tokensEntered ? 'text-secondary' : 'text-text-dim',
              } satisfies SummaryStat,
            ]
          : []),
        clickable(
          { label: 'Fired', value: String(nFired), cls: 'text-info' },
          ix?.onToggleStatus ? () => ix.onToggleStatus!('fired') : undefined,
          ix?.status === 'fired',
        ),
        clickable(
          { label: 'Closed', value: String(nClosed), cls: 'text-info' },
          ix?.onToggleStatus ? () => ix.onToggleStatus!('closed') : undefined,
          ix?.status === 'closed',
        ),
        clickable(
          { label: 'Open', value: String(nOpen), cls: nOpen > 0 ? tone : 'text-text-dim' },
          ix?.onToggleStatus ? () => ix.onToggleStatus!('open') : undefined,
          ix?.status === 'open',
        ),
        {
          label: 'Open share',
          value: nFired ? pctOf(openShare) : '—',
          cls: nFired ? tone : undefined,
        },
        // Graduated-to-AMM count. Only when the surface supplied it — a hidden
        // tile beats a `0` that could read as "none migrated" when the truth is
        // "this surface doesn't measure it".
        ...(extras.migrated != null
          ? [
              clickable(
                {
                  label: 'Migrated',
                  node: (
                    <span className="inline-flex items-baseline gap-1.5">
                      <span className={extras.migrated > 0 ? 'text-primary' : 'text-text-dim'}>
                        {extras.migrated}
                      </span>
                      {nFired > 0 && (
                        <span className="text-[10px] font-normal text-text-dim">
                          {pctOf(extras.migrated / nFired)}
                        </span>
                      )}
                    </span>
                  ),
                },
                ix?.onToggleMigrated ? () => ix.onToggleMigrated!(true) : undefined,
                ix?.migrated === true,
              ),
            ]
          : []),
        { label: 'Avg hold', value: fmtSecs(realized.avg_holding_secs), cls: 'text-accent' },
        ...(extended
          ? [
              { label: 'Median hold', value: fmtSecs(realized.median_holding_secs), cls: 'text-accent' },
              {
                label: 'Score',
                value: realized.score == null ? '—' : realized.score.toFixed(2),
                cls: 'text-secondary',
              },
            ]
          : []),
      ],
    },
    {
      title: 'Exits',
      hint:
        nClosed > 0
          ? `How the ${nClosed} closed position${nClosed === 1 ? '' : 's'} left`
          : 'Nothing has closed yet',
      stats: slices.map((s) =>
        clickable(
          {
            label: s.label,
            node: (
              <span className="inline-flex items-baseline gap-1.5">
                <span
                  className={cn('inline-block size-2 shrink-0 self-center rounded-sm', s.bar)}
                  aria-hidden
                />
                <span className={s.n > 0 ? s.cls : 'text-text-dim'}>{s.n}</span>
                {nClosed > 0 && (
                  <span className="text-[10px] font-normal text-text-dim">{pctOf(s.share)}</span>
                )}
              </span>
            ),
          },
          ix?.onToggleExit && s.n > 0 ? () => ix.onToggleExit!(s.label) : undefined,
          ix?.exitLabel === s.label,
        ),
      ),
      // Mix bar is also rendered always-visible via `exitMix`; keep lead here for
      // callers that still render the full Exits section without the shell.
      lead:
        nClosed > 0 ? (
          <ExitMixBar
            slices={slices}
            activeReason={ix?.exitLabel}
            onSelect={ix?.onToggleExit}
          />
        ) : undefined,
    },
    {
      title: 'Realized',
      hint:
        nOpen > 0
          ? `Closed positions only (${nClosed} of ${nFired}) — the ${nOpen} open are excluded`
          : `All ${nClosed} positions closed`,
      titleCls: 'text-green',
      stats: bandStats(realized, extended).map((stat) => {
        if (stat.label === 'Win %' && ix?.onToggleOutcome) {
          return clickable(stat, () => ix.onToggleOutcome!('win'), ix.outcome === 'win');
        }
        if (stat.label === 'Worst %' && ix?.onToggleOutcome) {
          return clickable(stat, () => ix.onToggleOutcome!('loss'), ix.outcome === 'loss');
        }
        return stat;
      }),
    },
  ];

  if (nOpen > 0) {
    sections.push({
      title: 'Incl. open (MTM)',
      hint: `All ${nFired} fired — the ${nOpen} open valued at their last price (unrealized)`,
      titleCls: 'text-warning',
      stats: [
        ...bandStats(mtm, extended),
        {
          label: 'of which unreal.',
          value: solText(realized.open_pnl_sol),
          cls: goodBad(realized.open_pnl_sol),
        },
      ],
    });
  }

  const exitMix =
    nClosed > 0 ? (
      <ExitMixBar slices={slices} activeReason={ix?.exitLabel} onSelect={ix?.onToggleExit} />
    ) : undefined;

  return { hero, exitMix, sections };
}
