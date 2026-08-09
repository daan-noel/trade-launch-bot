/**
 * **The one Console History request body.** Table page, summary strip, and chart
 * cohort are all built here, from the same three inputs — the URL cohort, the
 * chart focus lens, and the table's own view state (search + per-column filters).
 *
 * Why this module exists: the strip and the deck sit *above* a table that filters
 * itself, and a summary computed over a different population than the rows under
 * it is worse than no summary — it looks authoritative and is quietly answering a
 * different question. Before this, the cohort narrowed the table only, so typing
 * `>0.1` into Entry ◎ moved the table and left the charts and counts stating the
 * unfiltered book. One builder makes that class of divergence unrepresentable:
 * the three consumers differ *only* in pagination and sort, which is exactly what
 * an aggregate is allowed to ignore.
 *
 * The `range` window is the cohort's, intersected with a day/week focus — an
 * empty intersection becomes a deliberately empty far-future window rather than
 * a dropped bound, so "no rows" reads as no rows instead of "all rows".
 */

import type { TableQuery } from 'components/table/types';
import type { FilterSpec } from 'components/table/numericFilter';
import {
  toSummaryBody,
  toTableRequest,
  type TableRequestBody,
} from 'services/tableRequest';
import type { HistoryCohort } from './historyCohort';
import {
  dayBoundsUtcIso,
  intersectUtcWindow,
  pctFocusFilter,
  weekBoundsUtcIso,
} from './historyFocus';
import { isHistoryMetricExitNeedle } from './historyExitFilter';

/** A window that can match nothing — an empty focus∩cohort intersection. */
const EMPTY_WINDOW_ISO = '2099-01-01T00:00:00.000Z';

export interface HistoryScopeOpts {
  /**
   * `false` drops the chart focus, leaving the **parent** cohort — what the
   * charts deck needs. The deck applies the lens itself, and does so
   * asymmetrically on purpose: the timing grids (calendar, day×hour heatmap)
   * stay on the parent and draw a selection ring, because a calendar that
   * empties to the single day you just clicked has destroyed the context that
   * made the click meaningful. Hand it a pre-focused cohort and that design is
   * silently gone.
   *
   * It also means clicking through chart cells never re-downloads the cohort —
   * only the aggregate, which is a single row.
   */
  includeFocus?: boolean;
}

export interface HistoryRequestInput {
  cohort: HistoryCohort;
  /** The table's own view state (search + per-column filters + page + sort). */
  query: TableQuery;
  /** Column keys that filter numerically — from `numericColKeys(columns)`. */
  numericCols: ReadonlySet<string>;
  timezone: string;
}

/**
 * Server-side filters contributed by the cohort bar, the summary tiles, and the
 * chart focus — everything *except* the table's own per-column filters (which
 * `toTableRequest` serializes from `query`).
 *
 * Synthetic Metric± needles (`metric_win` / `metric_loss`) are deliberately NOT
 * emitted as a SQL `contains`: no substring can express the win/loss split, so
 * they stay a client-side lens (see {@link historyNeedsClientScan}).
 */
export function historyCohortFilters(
  cohort: HistoryCohort,
  opts: HistoryScopeOpts = {},
): Record<string, FilterSpec> {
  const filters: Record<string, FilterSpec> = {};
  if (cohort.ruleId) filters.rule_id = { op: 'eq', val: cohort.ruleId };
  if (cohort.mode !== 'all') filters.mode = { op: 'eq', val: cohort.mode };
  if (cohort.status) filters.status = { op: 'eq', val: cohort.status };
  // Fired / Closed / Open tiles. `entry_price > 0` is the "entered" predicate —
  // the aggregate's `entry_price IS NOT NULL`, expressed through the numeric
  // filter grammar (a filled entry price is never 0, so the two agree). Paired
  // with the status test it reproduces each partition exactly: closed = entered
  // and ended, open = entered and not ended (Holding / ExitPending / ExitStuck /
  // ExitUnconfirmed all qualify, which is why `open` can't be a status value).
  if (cohort.lane) {
    filters.entry_price = { op: 'gt', val: 0 };
    if (cohort.lane === 'closed') filters.status = { op: 'eq', val: 'End' };
    if (cohort.lane === 'open') filters.status = { op: 'neq', val: 'End' };
  }
  if (cohort.exitReason && !isHistoryMetricExitNeedle(cohort.exitReason)) {
    filters.exit_reason = { op: 'contains', val: cohort.exitReason };
  }
  // Win% / Worst% tiles. Realized SOL, matching the server's `is_win` rule
  // (`exit_lamports > entry_lamports`) — a break-even close is not a win.
  if (cohort.outcome) {
    filters.pnl_sol = cohort.outcome === 'win' ? { op: 'gt', val: 0 } : { op: 'lte', val: 0 };
  }
  // Migrated tile. `is_migrated` lives on `tokens_info` behind the LEFT JOIN;
  // the server COALESCEs it so an un-enriched token counts as not-migrated.
  if (cohort.migrated != null) {
    filters.is_migrated = { op: 'eq', val: cohort.migrated ? 'true' : 'false' };
  }
  const focus = opts.includeFocus === false ? null : cohort.focus;
  if (focus?.kind === 'rule') filters.rule_id = { op: 'eq', val: focus.ruleId };
  if (focus?.kind === 'pct') filters.pnl_pct = pctFocusFilter(focus.lo, focus.hi);
  if (focus?.kind === 'pos') filters.id = { op: 'eq', val: focus.positionId };
  return filters;
}

/** The cohort window, narrowed by a day/week focus. */
export function historyRange(
  cohort: HistoryCohort,
  timezone: string,
  opts: HistoryScopeOpts = {},
): { from?: string; to?: string } | undefined {
  let fromIso = cohort.fromIso;
  let toIso = cohort.toIso;
  const focus = opts.includeFocus === false ? null : cohort.focus;
  if (focus?.kind === 'day' || focus?.kind === 'week') {
    const span =
      focus.kind === 'day'
        ? dayBoundsUtcIso(focus.day, timezone)
        : weekBoundsUtcIso(focus.weekStart, timezone);
    const hit = intersectUtcWindow(fromIso, toIso, span.fromIso, span.toIso);
    if (!hit) {
      // Force an empty half-open window — dropping the bound would silently
      // widen the cohort to everything instead of matching nothing.
      fromIso = EMPTY_WINDOW_ISO;
      toIso = EMPTY_WINDOW_ISO;
    } else {
      fromIso = hit.fromIso;
      toIso = hit.toIso;
    }
  }
  if (!fromIso && !toIso) return undefined;
  return {
    ...(fromIso ? { from: fromIso } : {}),
    ...(toIso ? { to: toIso } : {}),
  };
}

/**
 * True when the cohort carries a lens SQL can't express — heat (recurring
 * dow×hour), hold band, legacy exit focus, or a synthetic Metric± needle — so
 * the caller must scan wide and filter client-side.
 */
export function historyNeedsClientScan(cohort: HistoryCohort): boolean {
  const k = cohort.focus?.kind;
  return (
    k === 'heat' ||
    k === 'holdBand' ||
    k === 'exit' ||
    isHistoryMetricExitNeedle(cohort.exitReason)
  );
}

/** Merge the cohort/focus filters + range onto a serialized table body. */
function withCohort(
  base: TableRequestBody,
  cohort: HistoryCohort,
  timezone: string,
  opts: HistoryScopeOpts = {},
): TableRequestBody {
  const range = historyRange(cohort, timezone, opts);
  return {
    ...base,
    filters: { ...base.filters, ...historyCohortFilters(cohort, opts) },
    ...(range ? { range } : {}),
  };
}

/** One **page** of the History table (honors pagination + sort). */
export function historyTableBody(
  { cohort, query, numericCols, timezone }: HistoryRequestInput,
  /** Wide first page for the client-scanned lenses. */
  scanPageSize?: number,
): TableRequestBody {
  const base = toTableRequest(query, numericCols);
  const body = withCohort(base, cohort, timezone);
  return scanPageSize ? { ...body, pagination: { page: 1, pageSize: scanPageSize } } : body;
}

/**
 * The same population with pagination + sort dropped — for the summary aggregate
 * and the full-cohort chart walk. Identical filters to {@link historyTableBody}
 * by construction, which is the whole point of this module.
 */
export function historySummaryBody(
  { cohort, query, numericCols, timezone }: HistoryRequestInput,
  opts: HistoryScopeOpts = {},
): TableRequestBody {
  return withCohort(toSummaryBody(query, numericCols), cohort, timezone, opts);
}

/**
 * Identity of the **cohort** — everything the bar, the tiles, and the chart
 * focus narrow by. This is the table's `resetKey`: changing the cohort should
 * drop the table back to page 1, but the table's own search / column filters
 * must **not** appear here, or every keystroke would reset the table that
 * produced it.
 */
export function historyCohortKey(
  cohort: HistoryCohort,
  timezone: string,
  opts: HistoryScopeOpts = {},
): string {
  return JSON.stringify({
    r: cohort.range,
    f: cohort.fromIso,
    t: cohort.toIso,
    rule: cohort.ruleId,
    m: cohort.mode,
    st: cohort.status,
    x: cohort.exitReason,
    l: cohort.lane,
    o: cohort.outcome,
    mig: cohort.migrated,
    foc: opts.includeFocus === false ? null : cohort.focus,
    tz: timezone,
  });
}

/**
 * Identity of the whole **population** — the cohort plus the table's filters,
 * minus page and sort. The refetch key for the summary aggregate and the chart
 * walk: neither may re-run just because the user turned a page or re-sorted.
 */
export function historyPopulationKey(
  { cohort, query, timezone }: Omit<HistoryRequestInput, 'numericCols'>,
  opts: HistoryScopeOpts = {},
): string {
  return `${historyCohortKey(cohort, timezone, opts)}|${JSON.stringify({
    s: query.search,
    c: query.colFilters,
    sf: query.structuredFilters,
  })}`;
}
