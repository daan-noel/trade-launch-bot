import { describe, expect, it } from 'vitest';
import type { TableQuery } from 'components/table/types';
import type { HistoryCohort } from './historyCohort';
import {
  historyCohortFilters,
  historyCohortKey,
  historyNeedsClientScan,
  historyPopulationKey,
  historyRange,
  historySummaryBody,
  historyTableBody,
} from './historyRequest';

const NUMERIC = new Set(['entry_sol', 'pnl_sol', 'pnl_pct', 'entry_price']);
const TZ = 'UTC';

function cohort(over: Partial<HistoryCohort> = {}): HistoryCohort {
  return {
    range: '7d',
    fromIso: '2026-08-01T00:00:00.000Z',
    toIso: null,
    ruleId: null,
    mode: 'real',
    status: null,
    exitReason: null,
    lane: null,
    outcome: null,
    migrated: null,
    focus: null,
    seriesRange: '7d',
    ...over,
  };
}

function query(over: Partial<TableQuery> = {}): TableQuery {
  return { page: 3, pageSize: 25, sortKeys: [], search: '', colFilters: {}, ...over };
}

describe('the table and the summary describe one population', () => {
  /**
   * The load-bearing invariant of the whole section: the strip aggregates and
   * the charts fold whatever the table pages. If these two bodies can carry
   * different filters, the summary above the table starts answering a different
   * question than the rows below it — and looks authoritative doing it.
   */
  it('differ only in pagination and sort', () => {
    const input = {
      cohort: cohort({
        ruleId: 'rule-1',
        mode: 'paper',
        exitReason: 'TakeProfit',
        outcome: 'win' as const,
        migrated: true,
      }),
      query: query({
        search: 'bonk',
        colFilters: { entry_sol: '>0.1' },
        sortKeys: [{ col: 'pnl_sol', dir: 'desc' as const }],
      }),
      numericCols: NUMERIC,
      timezone: TZ,
    };
    const table = historyTableBody(input);
    const summary = historySummaryBody(input);

    expect(summary.filters).toEqual(table.filters);
    expect(summary.search).toEqual(table.search);
    expect(summary.range).toEqual(table.range);
    // …and the aggregate is free to ignore exactly these two.
    expect(summary.sorting).toEqual([]);
    expect(table.sorting).toHaveLength(1);
  });

  it('carries the table column filters into the summary', () => {
    const input = {
      cohort: cohort(),
      query: query({ colFilters: { entry_sol: '>0.1' } }),
      numericCols: NUMERIC,
      timezone: TZ,
    };
    // A numeric column filter must reach the aggregate: without it the strip
    // states the unfiltered book while the table shows a slice.
    expect(historySummaryBody(input).filters.entry_sol).toEqual({ op: 'gt', val: 0.1 });
  });
});

describe('summary tile lenses', () => {
  it('splits win / loss on realized SOL, matching the server is_win rule', () => {
    expect(historyCohortFilters(cohort({ outcome: 'win' })).pnl_sol).toEqual({
      op: 'gt',
      val: 0,
    });
    // `lte`, not `lt` — a break-even close is not a win, so it belongs here.
    expect(historyCohortFilters(cohort({ outcome: 'loss' })).pnl_sol).toEqual({
      op: 'lte',
      val: 0,
    });
  });

  it('reproduces the aggregate partitions for fired / closed / open', () => {
    const fired = historyCohortFilters(cohort({ lane: 'fired' }));
    expect(fired.entry_price).toEqual({ op: 'gt', val: 0 });
    expect(fired.status).toBeUndefined();

    const closed = historyCohortFilters(cohort({ lane: 'closed' }));
    expect(closed).toMatchObject({
      entry_price: { op: 'gt', val: 0 },
      status: { op: 'eq', val: 'End' },
    });

    // Open spans Holding / ExitPending / ExitStuck / ExitUnconfirmed, so it is
    // "entered and not ended" — never a single status equality.
    const open = historyCohortFilters(cohort({ lane: 'open' }));
    expect(open).toMatchObject({
      entry_price: { op: 'gt', val: 0 },
      status: { op: 'neq', val: 'End' },
    });
  });

  it('sends migrated as an explicit true/false, never a dropped key', () => {
    expect(historyCohortFilters(cohort({ migrated: true })).is_migrated).toEqual({
      op: 'eq',
      val: 'true',
    });
    expect(historyCohortFilters(cohort({ migrated: false })).is_migrated).toEqual({
      op: 'eq',
      val: 'false',
    });
    expect(historyCohortFilters(cohort({ migrated: null })).is_migrated).toBeUndefined();
  });

  it('keeps a synthetic Metric± needle off the SQL contains path', () => {
    // No substring can express the win/loss split, so it must stay a client
    // lens — emitting it as `contains` would match every metric exit.
    const c = cohort({ exitReason: 'metric_win' });
    expect(historyCohortFilters(c).exit_reason).toBeUndefined();
    expect(historyNeedsClientScan(c)).toBe(true);

    const plain = cohort({ exitReason: 'TakeProfit' });
    expect(historyCohortFilters(plain).exit_reason).toEqual({
      op: 'contains',
      val: 'TakeProfit',
    });
    expect(historyNeedsClientScan(plain)).toBe(false);
  });
});

describe('the parent cohort (includeFocus: false)', () => {
  /**
   * The charts deck lenses itself, and asymmetrically: the calendar and the
   * day×hour heatmap stay on the parent and draw a selection ring. Hand the deck
   * a pre-focused cohort and clicking a day empties the calendar that produced
   * the click — the design is gone with nothing failing.
   */
  it('drops every server-expressible focus', () => {
    const day = cohort({ focus: { kind: 'day', day: '2026-08-05' } });
    expect(historyRange(day, TZ, { includeFocus: false })?.from).toBe(day.fromIso);

    const rule = cohort({ focus: { kind: 'rule', ruleId: 'r-9' } });
    expect(historyCohortFilters(rule, { includeFocus: false }).rule_id).toBeUndefined();

    const pos = cohort({ focus: { kind: 'pos', positionId: 'p-1' } });
    expect(historyCohortFilters(pos, { includeFocus: false }).id).toBeUndefined();
  });

  it('keeps the cohort bar and the table filters', () => {
    const c = cohort({ ruleId: 'bar-rule', focus: { kind: 'rule', ruleId: 'focus-rule' } });
    const body = historySummaryBody(
      { cohort: c, query: query({ colFilters: { entry_sol: '>0.1' } }), numericCols: NUMERIC, timezone: TZ },
      { includeFocus: false },
    );
    // The bar's rule survives; only the focus lens is dropped.
    expect(body.filters.rule_id).toEqual({ op: 'eq', val: 'bar-rule' });
    expect(body.filters.entry_sol).toEqual({ op: 'gt', val: 0.1 });
  });

  it('gives the walk a key that ignores focus changes', () => {
    // Clicking through chart cells must re-fetch one aggregate row, not the
    // whole cohort.
    const base = { query: query(), timezone: TZ };
    const a = historyPopulationKey(
      { ...base, cohort: cohort({ focus: { kind: 'day', day: '2026-08-05' } }) },
      { includeFocus: false },
    );
    const b = historyPopulationKey({ ...base, cohort: cohort() }, { includeFocus: false });
    expect(a).toBe(b);
    // …while the aggregate's key does move.
    expect(
      historyPopulationKey({ ...base, cohort: cohort({ focus: { kind: 'day', day: '2026-08-05' } }) }),
    ).not.toBe(historyPopulationKey({ ...base, cohort: cohort() }));
  });
});

describe('the focus window', () => {
  it('intersects a day focus with the cohort window', () => {
    const r = historyRange(
      cohort({ fromIso: '2026-08-01T00:00:00.000Z', focus: { kind: 'day', day: '2026-08-05' } }),
      TZ,
    );
    expect(r?.from).toBe('2026-08-05T00:00:00.000Z');
    expect(r?.to).toBe('2026-08-06T00:00:00.000Z');
  });

  it('matches nothing when the focus falls outside the cohort', () => {
    // Dropping the bound instead would widen the cohort to everything — "no
    // rows" must not render as "all rows".
    const r = historyRange(
      cohort({ fromIso: '2026-08-01T00:00:00.000Z', focus: { kind: 'day', day: '2020-01-01' } }),
      TZ,
    );
    expect(r?.from).toBe(r?.to);
  });
});

describe('refetch keys', () => {
  it('ignores page and sort', () => {
    const base = { cohort: cohort(), timezone: TZ };
    expect(historyPopulationKey({ ...base, query: query({ page: 1 }) })).toBe(
      historyPopulationKey({
        ...base,
        query: query({ page: 9, sortKeys: [{ col: 'pnl_sol', dir: 'asc' }] }),
      }),
    );
  });

  it('leaves the table filters out of the cohort (reset) key', () => {
    // The reset key drops the table to page 1. Including the table's own search
    // would reset the table on every keystroke that produced it.
    expect(historyCohortKey(cohort(), TZ)).toBe(historyCohortKey(cohort(), TZ));
    expect(historyPopulationKey({ cohort: cohort(), query: query({ search: 'a' }), timezone: TZ })).not.toBe(
      historyPopulationKey({ cohort: cohort(), query: query({ search: 'b' }), timezone: TZ }),
    );
  });
});
