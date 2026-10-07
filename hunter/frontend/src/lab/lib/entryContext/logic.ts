/**
 * **The logic under test**: the buys table's filter row, read as a rule, and checked
 * on both stretches of every buy.
 *
 * The filter row already says "a buy counts when Target tx % > 50 and ...". Here each
 * filter is sorted into one of two kinds:
 *
 * - **scope**: which buys are asked (token, time, his SOL). Applied the same way on
 *   both stretches.
 * - **signal**: what the tape looked like before him (the target and everyone
 *   columns, the probe). Checked on the last stretch as the table does, and on the
 *   Earlier stretch through the column's twin (`EntryAxis.earlier`, the probe's
 *   `control_*`). A signal with no twin (Tx % jump, Lag, ...) is left out of the
 *   Earlier check and named, which can only make Earlier match MORE often, so a
 *   comparison against Earlier errs low.
 *
 * `counts.ts` counts his buys against the idea: the pool row against `idea`, the
 * all row against `anyIx` (the idea without the filters that need the selected
 * IXs, `LogicCondition.needsIxs`). The probe filters stay the pool (`probe`), not
 * the idea.
 */

import type { ColumnDef } from 'components/table/types';
import { columnFilterPredicate } from 'components/table/columnFilter';
import { parseNumericPredicate } from 'components/table/numericFilter';
import type { PreEntryShow } from '@lab/components/analysis/usePreEntryProbe';
import type { PreEntryVerdict } from '@lab/lib/preEntryProbeTypes';
import { AXIS_BY_KEY, needsIxs } from './axes';
import type { EntryRow } from './types';

type Verdict = (e: EntryRow) => PreEntryVerdict | undefined;

/** Filters that pick which buys are asked, never what came before them. */
const SCOPE_KEYS = new Set(['token', 'at']);

export interface LogicCondition {
  key: string;
  /** The column's label. */
  label: string;
  /** The filter text as typed (`>50`, `1..5`, `matched`). */
  text: string;
  kind: 'scope' | 'signal';
  /** A signal also checked on the Earlier stretch. */
  earlierChecked: boolean;
  /** Read through the selected IXs (probe, Target, Earlier): the pool rows
   *  use it, the all rows skip it. The ix and reserve signal columns do not. */
  needsIxs: boolean;
}

export interface EntryLogic {
  conditions: LogicCondition[];
  /** Signal conditions set: 0 = there is no logic to test yet. */
  signals: number;
  inScope: (e: EntryRow) => boolean;
  /** The signals on the last stretch (the table's own filter). */
  last: (e: EntryRow) => boolean;
  /** The twinned signals on the Earlier stretch. */
  earlier: (e: EntryRow) => boolean;
  /**
   * The idea under test: signal filters that are not the probe (Target tx %, and
   * whatever column filter is added next). True for every buy when none are set.
   */
  idea: (e: EntryRow) => boolean;
  /** The idea without the filters that need the selected IXs: what an all row asks. */
  anyIx: (e: EntryRow) => boolean;
  /** Probe-column filters (hits, SOL, Show). True for every buy when none are set. */
  probe: (e: EntryRow) => boolean;
}

/** The probe verdict's state one stretch earlier: `unknown` stays unknown. */
function earlierState(v: PreEntryVerdict | undefined): string {
  if (!v) return '';
  if (v.state === 'unknown') return 'unknown';
  return v.control_matched ? 'matched' : 'no-match';
}

/** A probe column's Earlier twin, as a predicate on the filter text. */
function probeTwin(key: string, text: string, verdictOf: Verdict): ((e: EntryRow) => boolean) | null {
  if (key === 'pe_state') return (e) => earlierState(verdictOf(e)) === text;
  const num = key === 'pe_hits' ? 'control_hits' : key === 'pe_sol' ? 'control_sol' : null;
  const pred = num ? parseNumericPredicate(text) : null;
  if (!num || !pred) return null;
  return (e) => {
    const v = verdictOf(e);
    return v != null && pred(v[num]);
  };
}

/** An axis column's Earlier twin, as a predicate on the filter text. */
function axisTwin(key: string, text: string): ((e: EntryRow) => boolean) | null {
  const twin = AXIS_BY_KEY.get(key)?.earlier;
  const pred = twin ? parseNumericPredicate(text) : null;
  if (!twin || !pred) return null;
  return (e) => {
    const n = twin(e);
    return n != null && pred(n);
  };
}

const all = (preds: ((e: EntryRow) => boolean)[]) => (e: EntryRow) => preds.every((p) => p(e));

/**
 * The buys table's filters as a logic. `columns` are the table's own (a filter on a
 * column not on screen is ignored, as the table ignores it); `show` is the probe's
 * Show, which narrows the table the way a Pre-entry filter would.
 */
export function entryLogic(
  filters: Readonly<Record<string, string>>,
  columns: readonly ColumnDef<EntryRow>[],
  verdictOf: Verdict | null,
  show: PreEntryShow,
): EntryLogic {
  const conditions: LogicCondition[] = [];
  const scope: ((e: EntryRow) => boolean)[] = [];
  const last: ((e: EntryRow) => boolean)[] = [];
  const earlier: ((e: EntryRow) => boolean)[] = [];
  const idea: ((e: EntryRow) => boolean)[] = [];
  const anyIx: ((e: EntryRow) => boolean)[] = [];
  const probe: ((e: EntryRow) => boolean)[] = [];

  const add = (col: ColumnDef<EntryRow>, text: string) => {
    const pred = columnFilterPredicate(col, text);
    if (!pred) return;
    const axis = AXIS_BY_KEY.get(col.key);
    const isProbe = col.key.startsWith('pe_');
    const isScope = SCOPE_KEYS.has(col.key) || axis?.group === 'buy' || (!axis && !isProbe);
    if (isScope) {
      scope.push(pred);
      conditions.push({ key: col.key, label: col.label, text, kind: 'scope', earlierChecked: false, needsIxs: false });
      return;
    }
    const ix = isProbe || (axis != null && needsIxs(axis.group));
    const twin = axis ? axisTwin(col.key, text) : verdictOf ? probeTwin(col.key, text, verdictOf) : null;
    last.push(pred);
    if (twin) earlier.push(twin);
    // The probe is the pool (did the target signal). Every other signal filter is
    // the idea, and another one added later joins it.
    (isProbe ? probe : idea).push(pred);
    if (!ix) anyIx.push(pred);
    conditions.push({ key: col.key, label: col.label, text, kind: 'signal', earlierChecked: twin != null, needsIxs: ix });
  };

  for (const [key, raw] of Object.entries(filters)) {
    const col = columns.find((c) => c.key === key);
    if (col) add(col, raw.trim());
  }
  if (verdictOf && show !== 'all') {
    const col = columns.find((c) => c.key === 'pe_state');
    if (col) add({ ...col, label: 'Probe Show' }, show);
  }

  return {
    conditions,
    signals: last.length,
    inScope: all(scope),
    last: all(last),
    earlier: all(earlier),
    idea: all(idea),
    anyIx: all(anyIx),
    probe: all(probe),
  };
}
