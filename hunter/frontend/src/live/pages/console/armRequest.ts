/**
 * **The one Console Arms request body.** The table page and the funnel strip are
 * both built here, from the URL cohort plus the table's own view state.
 *
 * Same reason `historyRequest` exists: a funnel computed over a different
 * population than the rows under it is worse than no funnel — it looks
 * authoritative while quietly answering another question. The two consumers
 * differ *only* in pagination and sort, which is exactly what an aggregate is
 * allowed to ignore.
 */

import type { TableQuery } from 'components/table/types';
import type { FilterSpec } from 'components/table/numericFilter';
import {
  toSummaryBody,
  toTableRequest,
  type TableRequestBody,
} from 'services/tableRequest';
import type { ArmCohort } from './armCohort';

export interface ArmRequestInput {
  cohort: ArmCohort;
  /** The table's own view state (search + per-column filters + page + sort). */
  query: TableQuery;
  /** Column keys that filter numerically — from `numericColKeys(columns)`. */
  numericCols: ReadonlySet<string>;
}

/** Server-side filters contributed by the cohort bar — everything except the
 *  table's own per-column filters (which `toTableRequest` serializes). */
export function armCohortFilters(cohort: ArmCohort): Record<string, FilterSpec> {
  const filters: Record<string, FilterSpec> = {};
  if (cohort.ruleId) filters.rule_id = { op: 'eq', val: cohort.ruleId };
  if (cohort.mode !== 'all') filters.mode = { op: 'eq', val: cohort.mode };
  // `waiting` is not a stored value: the server's `end_reason` filter column
  // COALESCEs NULL (a live episode) to it, so an eq here matches exactly the
  // episodes still evaluating entry.
  if (cohort.reason) filters.end_reason = { op: 'eq', val: cohort.reason };
  return filters;
}

/** The cohort window. Applies to `armed_at` server-side (`ARM_WHEN_SQL`) — the
 *  question is "what did the bot look at during this window", so keying on the
 *  end would drop every episode still waiting. */
export function armRange(cohort: ArmCohort): { from?: string; to?: string } | undefined {
  if (!cohort.fromIso && !cohort.toIso) return undefined;
  return {
    ...(cohort.fromIso ? { from: cohort.fromIso } : {}),
    ...(cohort.toIso ? { to: cohort.toIso } : {}),
  };
}

function withCohort(base: TableRequestBody, cohort: ArmCohort): TableRequestBody {
  const range = armRange(cohort);
  return {
    ...base,
    filters: { ...base.filters, ...armCohortFilters(cohort) },
    ...(range ? { range } : {}),
  };
}

/** One **page** of the Arms table (honors pagination + sort). */
export function armTableBody({ cohort, query, numericCols }: ArmRequestInput): TableRequestBody {
  return withCohort(toTableRequest(query, numericCols), cohort);
}

/** The same population with pagination + sort dropped — for the funnel. */
export function armSummaryBody({
  cohort,
  query,
  numericCols,
}: ArmRequestInput): TableRequestBody {
  return withCohort(toSummaryBody(query, numericCols), cohort);
}

/**
 * Identity of the **cohort** — the table's `resetKey`, and the funnel fetch's
 * dependency. Changing the cohort drops the table back to page 1; the table's
 * own search / column filters are included because the funnel must narrow with
 * them, but page and sort are deliberately excluded (paging must not re-run the
 * aggregate).
 */
export function armPopulationKey({ cohort, query, numericCols }: ArmRequestInput): string {
  const body = armSummaryBody({ cohort, query, numericCols });
  return JSON.stringify([body.filters, body.range ?? null, body.search ?? '']);
}

/** Identity of the cohort bar alone — what should snap the table to page 1. */
export function armCohortKey(cohort: ArmCohort): string {
  return [
    cohort.range,
    cohort.fromIso ?? '',
    cohort.toIso ?? '',
    cohort.ruleId ?? '',
    cohort.mode,
    cohort.reason ?? '',
  ].join('|');
}
