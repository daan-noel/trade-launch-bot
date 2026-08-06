/**
 * Neutral PnL-series folds shared by every analytics deck (live Console History,
 * live Portfolio, lab Trader Analysis).
 *
 * The one shape everything below works on is [`PnlPoint`] — "one decided
 * position: when, how much, and whose". Callers map their own row type into it
 * (a live close from `/api/portfolio/closes-series`, a lab per-mint wallet
 * aggregate), and every chart then derives from the SAME points, so an equity
 * curve can never disagree with the histogram beside it.
 *
 * Pure and DB-free on purpose: charts re-derive for free when the cohort
 * changes, with no second fetch and no second aggregation to keep in step.
 */

import { DOW_ROWS, HOURS } from 'components/creation-stats/creationStats';

export { DOW_ROWS, HOURS };

/** One decided position — the atom every chart in this module folds. */
export interface PnlPoint {
  /** Stable identity (position id / mint) — React keys + dedupe. */
  key: string;
  /** The instant this position was decided (an exit, a last trade). Epoch ms. */
  timeMs: number;
  /** Realized SOL PnL. */
  pnlSol: number;
  /** Realized PnL as a percent of capital deployed; `null` when there is no
   *  matched cost basis to divide by (never `0` — "unknown" is not "flat"). */
  pnlPct: number | null;
  /** Display label (symbol / rule name). */
  label: string;
  /** Grouping key for per-series comparison (rule id, wallet, …). */
  groupId?: string | null;
  /** Still holding a bag — rendered as a marker, never folded into realized. */
  isOpen?: boolean;
}

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
 *  (mirrors `utils/date.ts`'s formatter cache) since this runs once per point
 *  per heatmap rebuild, not per render. */
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

/** `YYYY-MM-DD` for an instant in `timeZone` — the calendar day key. */
export function dayKeyInTz(ms: number, timeZone: string): string {
  // `en-CA` renders ISO-ordered `YYYY-MM-DD`, which sorts lexicographically.
  return new Intl.DateTimeFormat('en-CA', {
    timeZone,
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
  }).format(new Date(ms));
}

// ── equity curve ────────────────────────────────────────────────────────────

export interface EquityPoint {
  /** Epoch seconds — lightweight-charts' native `Time` unit. */
  time: number;
  cumPnlSol: number;
  /** Running peak of `cumPnlSol` — the drawdown reference line. */
  peakSol: number;
}

/** Cumulative PnL ordered oldest → newest. Ties (same-second points) collapse
 *  into one entry so a chart series never receives duplicate x-values. */
export function buildEquityCurve(points: readonly PnlPoint[]): EquityPoint[] {
  const sorted = [...points].filter((p) => Number.isFinite(p.timeMs)).sort((a, b) => a.timeMs - b.timeMs);
  const out: EquityPoint[] = [];
  let cum = 0;
  let peak = 0;
  for (const p of sorted) {
    cum += p.pnlSol;
    if (cum > peak) peak = cum;
    const time = Math.floor(p.timeMs / 1000);
    const last = out[out.length - 1];
    if (last && last.time === time) {
      last.cumPnlSol = cum;
      last.peakSol = peak;
    } else {
      out.push({ time, cumPnlSol: cum, peakSol: peak });
    }
  }
  return out;
}

/** Deepest peak-to-trough drop in SOL over the curve (`0` when never underwater). */
export function maxDrawdownSol(curve: readonly EquityPoint[]): number {
  let worst = 0;
  for (const p of curve) {
    const dd = p.peakSol - p.cumPnlSol;
    if (dd > worst) worst = dd;
  }
  return worst;
}

// ── distribution ────────────────────────────────────────────────────────────

export interface PnlBucket {
  label: string;
  /** `-1` loss / `0` straddles zero / `1` win — drives the bar color. */
  sign: -1 | 0 | 1;
  count: number;
}

/** Fixed bucket edges over PnL%. Points with no matched cost basis (`pnlPct`
 *  null) are excluded — there is no realized % to bucket. */
const DIST_EDGES = [-Infinity, -50, -20, -10, 0, 10, 20, 50, Infinity];

function bucketLabel(lo: number, hi: number): string {
  if (lo === -Infinity) return `< ${hi}%`;
  if (hi === Infinity) return `> ${lo}%`;
  return `${lo}…${hi}%`;
}

export function pnlDistributionBuckets(points: readonly PnlPoint[]): PnlBucket[] {
  const buckets: PnlBucket[] = [];
  for (let i = 0; i < DIST_EDGES.length - 1; i++) {
    const lo = DIST_EDGES[i]!;
    const hi = DIST_EDGES[i + 1]!;
    buckets.push({ label: bucketLabel(lo, hi), sign: hi <= 0 ? -1 : lo >= 0 ? 1 : 0, count: 0 });
  }
  for (const p of points) {
    const pct = p.pnlPct;
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

// ── heatmap (day-of-week × hour-of-day) ─────────────────────────────────────

export interface PnlHeatCell {
  dow: number;
  hour: number;
  pnl_sol: number;
  count: number;
}

/** Fold every point into a 7×24 day×hour grid in `timeZone`. Always returns all
 *  168 cells (zero-filled) so the heatmap never synthesizes missing slots. */
export function buildPnlHeatCells(
  points: readonly PnlPoint[],
  timeZone: string,
): PnlHeatCell[] {
  const grid = new Map<number, PnlHeatCell>();
  for (const row of DOW_ROWS) {
    for (const hour of HOURS) {
      grid.set(row.dow * 24 + hour, { dow: row.dow, hour, pnl_sol: 0, count: 0 });
    }
  }
  for (const p of points) {
    if (!Number.isFinite(p.timeMs)) continue;
    const { dow, hour } = dowHourInTz(p.timeMs, timeZone);
    const cell = grid.get(dow * 24 + hour);
    if (cell) {
      cell.pnl_sol += p.pnlSol;
      cell.count += 1;
    }
  }
  return [...grid.values()];
}

// ── calendar (daily PnL) ────────────────────────────────────────────────────

export interface PnlDay {
  /** `YYYY-MM-DD` in the caller's timezone. */
  day: string;
  pnlSol: number;
  count: number;
  wins: number;
}

/** Daily PnL, oldest → newest. Only days with at least one point appear —
 *  the calendar component fills the gaps it needs for its month grid. */
export function buildDailyPnl(points: readonly PnlPoint[], timeZone: string): PnlDay[] {
  const byDay = new Map<string, PnlDay>();
  for (const p of points) {
    if (!Number.isFinite(p.timeMs)) continue;
    const day = dayKeyInTz(p.timeMs, timeZone);
    const cur = byDay.get(day) ?? { day, pnlSol: 0, count: 0, wins: 0 };
    cur.pnlSol += p.pnlSol;
    cur.count += 1;
    if (p.pnlSol > 0) cur.wins += 1;
    byDay.set(day, cur);
  }
  return [...byDay.values()].sort((a, b) => a.day.localeCompare(b.day));
}

// ── ranked bars ─────────────────────────────────────────────────────────────

/** One bar of a ranked chart — deliberately shape-neutral so the live rule
 *  scoreboard and the lab per-mint ranking share one renderer. */
export interface RankedBarRow {
  key: string;
  label: string;
  value: number;
  /** Optional annotation chip (e.g. "open"). */
  tag?: string | null;
  /** Native tooltip / full identity behind a truncated label. */
  title?: string;
}

/** Rows sorted by `value`, best first. Ranking on realized money, not win rate:
 *  a hit-rate ranking can surface the worst-expectancy cohort at the best hit
 *  rate (see `docs/plans/strategies/wallet-analysis.md`). */
export function rankByValue(rows: readonly RankedBarRow[]): RankedBarRow[] {
  return [...rows].sort((a, b) => b.value - a.value);
}

// ── per-group comparison (the decay detector) ───────────────────────────────

export interface GroupWindowStats {
  groupId: string;
  label: string;
  n: number;
  pnlSol: number;
  winRate: number | null;
  /** Mean SOL per trade — expectancy. */
  expectancySol: number | null;
}

/** Stats over one slice of points, grouped by `groupId`. */
function statsOf(groupId: string, label: string, points: readonly PnlPoint[]): GroupWindowStats {
  const n = points.length;
  const pnlSol = points.reduce((s, p) => s + p.pnlSol, 0);
  const wins = points.filter((p) => p.pnlSol > 0).length;
  return {
    groupId,
    label,
    n,
    pnlSol,
    winRate: n > 0 ? (wins / n) * 100 : null,
    expectancySol: n > 0 ? pnlSol / n : null,
  };
}

export interface GroupTrend {
  groupId: string;
  label: string;
  /** The most recent `window` trades. */
  recent: GroupWindowStats;
  /** The `window` trades before those — the comparison baseline. */
  prior: GroupWindowStats;
  /** Percentage points of win rate lost vs the prior window (`null` when either
   *  window is too thin to compare). */
  winRateDeltaPp: number | null;
  /** Mean SOL/trade lost vs the prior window. */
  expectancyDeltaSol: number | null;
  /**
   * Decaying: both win rate AND expectancy fell, with a full comparison window
   * on each side. Requiring both is deliberate — win rate alone flips on a
   * single tail trade, and expectancy alone flips on one outlier.
   */
  decaying: boolean;
}

/**
 * Per-group rolling-window comparison — "is this rule getting worse". Points are
 * ordered oldest → newest per group; the last `window` trades are compared with
 * the `window` before them. Groups with fewer than `2 × window` trades are
 * reported with `decaying: false` (not enough evidence), never omitted — a rule
 * silently vanishing from a decay board reads as "healthy".
 */
export function groupTrends(
  points: readonly PnlPoint[],
  labelOf: (groupId: string) => string,
  window = 20,
): GroupTrend[] {
  const byGroup = new Map<string, PnlPoint[]>();
  for (const p of points) {
    const g = p.groupId;
    if (!g) continue;
    const arr = byGroup.get(g);
    if (arr) arr.push(p);
    else byGroup.set(g, [p]);
  }
  const out: GroupTrend[] = [];
  for (const [groupId, arr] of byGroup) {
    arr.sort((a, b) => a.timeMs - b.timeMs);
    const label = labelOf(groupId);
    const recentSlice = arr.slice(-window);
    const priorSlice = arr.slice(-2 * window, -window);
    const recent = statsOf(groupId, label, recentSlice);
    const prior = statsOf(groupId, label, priorSlice);
    const comparable = recentSlice.length >= window && priorSlice.length >= window;
    const winRateDeltaPp =
      comparable && recent.winRate != null && prior.winRate != null
        ? recent.winRate - prior.winRate
        : null;
    const expectancyDeltaSol =
      comparable && recent.expectancySol != null && prior.expectancySol != null
        ? recent.expectancySol - prior.expectancySol
        : null;
    out.push({
      groupId,
      label,
      recent,
      prior,
      winRateDeltaPp,
      expectancyDeltaSol,
      decaying:
        winRateDeltaPp != null &&
        expectancyDeltaSol != null &&
        winRateDeltaPp < 0 &&
        expectancyDeltaSol < 0,
    });
  }
  return out.sort((a, b) => b.recent.pnlSol - a.recent.pnlSol);
}

/** Per-group daily PnL series — the small-multiple sparklines on the scoreboard. */
export function groupDailyPnl(
  points: readonly PnlPoint[],
  timeZone: string,
): Map<string, PnlDay[]> {
  const byGroup = new Map<string, PnlPoint[]>();
  for (const p of points) {
    const g = p.groupId;
    if (!g) continue;
    const arr = byGroup.get(g);
    if (arr) arr.push(p);
    else byGroup.set(g, [p]);
  }
  const out = new Map<string, PnlDay[]>();
  for (const [groupId, arr] of byGroup) {
    out.set(groupId, buildDailyPnl(arr, timeZone));
  }
  return out;
}
