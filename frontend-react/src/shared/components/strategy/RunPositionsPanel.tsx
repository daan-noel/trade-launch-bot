import { useCallback, useMemo, useState } from 'react';
import { DataTable } from 'components/table/DataTable';
import { SectionDivider } from 'components/ui/SectionDivider';
import { Badge } from 'components/ui/Badge';
import { InlineAlert } from 'components/ui/Modal';
import { SimSummaryCard } from 'components/tpsl1/SimSummaryCard';
import { mergeTokenData } from 'components/tokens/sharedTokenColumns';
import { useGetTokensByMintsQuery } from 'store/apiSlice';
import { useRulePositions, DEFAULT_POSITIONS_QUERY } from 'hooks/useRulePositions';
import { numericColKeys } from 'services/tableRequest';
import type { PositionScope } from 'services/api';
import type { TableRequestBody } from 'services/tableRequest';
import type { usePriceDisplay } from 'hooks/usePriceDisplay';
import type { ColumnDef, TableQuery } from 'components/table/types';
import type {
  PositionsSummary,
  RulePositionRecord,
  RulePositionsPage,
  RuleRecord,
} from 'types';
import { cn } from 'lib/cn';

/** Stable row key — no inline lambda (keeps DataTable memoization clean). */
const keyById = (r: { id: string }) => r.id;

/** Leading "Run" column for the old-runs table: shows each position's originating
 *  run so, together with the banding below, run boundaries read at a glance.
 *  Display-only (not sortable/searchable/filterable) — the server orders history
 *  rows by run already, so a client sort here would be a no-op false affordance. */
const RUN_COLUMN: ColumnDef<RulePositionRecord> = {
  key: 'run_seq',
  label: 'Run',
  tooltip: 'Which run this position belonged to',
  width: '52px',
  render: (r) =>
    r.run_seq != null ? (
      <span className="font-mono text-[11px] text-text-dim">#{r.run_seq}</span>
    ) : (
      <span className="text-text-dim">—</span>
    ),
  searchValue: () => '',
  sortable: false,
};

/** Alternating band per run (by `run_seq` parity, not row parity) so contiguous
 *  runs share a subtle tint — the "notice them separately at a glance" cue. */
const historyRowClass = (r: RulePositionRecord): string | undefined =>
  r.run_seq != null && r.run_seq % 2 === 0 ? 'bg-info/[0.06]' : undefined;

interface RunPositionsPanelProps {
  /** SSE / positions strategy key (`tpsl1` | `tpsl2` | `swing_1`). */
  strategy: 'tpsl1' | 'tpsl2' | 'swing_1';
  selectedRuleId: string | null;
  selectedRuleName: string | null;
  rules: RuleRecord[];
  /** Positions table columns (the old-runs table prepends a run column). */
  columns: ColumnDef<RulePositionRecord>[];
  /** Scope-aware page fetch (accepts `'current'`/`'history'`). */
  fetchPositions: (
    ruleId: string,
    body: TableRequestBody,
    signal?: AbortSignal,
    scope?: PositionScope,
  ) => Promise<RulePositionsPage>;
  /** Scope-aware summary fetch. */
  fetchSummary: (
    ruleId: string,
    signal?: AbortSignal,
    scope?: PositionScope,
  ) => Promise<PositionsSummary>;
  price: ReturnType<typeof usePriceDisplay>;
  /** Inspect highlight + open. */
  selectedKey: string | null;
  onInspect: (row: RulePositionRecord | null) => void;
  /** Real-rule sell wiring — enables the "Sell ALL" action on the current-run table
   *  only (old runs are closed). Omit for paper/lab, where there's nothing to sell. */
  isReal?: boolean;
  sellingPositionMint?: string | null;
  onSellPosition?: (mint: string) => void;
}

/** Heading row for a positions section (marker + title + count + rule name). */
function SectionHeading({
  title,
  count,
  subtitle,
}: {
  title: string;
  count?: number;
  subtitle?: string;
}) {
  return (
    <div className="mb-3.5 flex items-center gap-2.5">
      <span className="h-4 w-1 rounded-full bg-info" />
      <h3 className="text-sm font-bold text-text">{title}</h3>
      {count != null && (
        <Badge variant="info" size="sm" className="font-mono font-normal">
          {count}
        </Badge>
      )}
      {subtitle && (
        <span className="truncate font-mono text-[11px] text-text-dim">{subtitle}</span>
      )}
    </div>
  );
}

/**
 * Renders a rule's positions as two sections — **Current run** (the latest run;
 * live) and **Old runs** (every prior run; static, banded by run) — each with its
 * own server-side table + run-wide summary. The current section keeps the live
 * SSE/poll path; the history section is fetch-only (immutable) and only appears when
 * there's actually a prior run. Both scope their own paging + summary via the
 * `scope` query param, so the two summaries measure different populations (unrealized
 * + in-progress vs. realized history).
 */
export function RunPositionsPanel({
  strategy,
  selectedRuleId,
  selectedRuleName,
  rules,
  columns,
  fetchPositions,
  fetchSummary,
  price,
  selectedKey,
  onInspect,
  isReal = false,
  sellingPositionMint,
  onSellPosition,
}: RunPositionsPanelProps) {
  const numericCols = useMemo(() => numericColKeys(columns), [columns]);
  const historyColumns = useMemo(() => [RUN_COLUMN, ...columns], [columns]);

  // Independent view-state per section (each table pages/sorts/filters on its own).
  const [currentQuery, setCurrentQuery] = useState<TableQuery>(DEFAULT_POSITIONS_QUERY);
  const [historyQuery, setHistoryQuery] = useState<TableQuery>(DEFAULT_POSITIONS_QUERY);

  // Scope-bound fetchers — stable per underlying fetch fn.
  const fetchCurrent = useCallback(
    (id: string, body: TableRequestBody, signal?: AbortSignal) =>
      fetchPositions(id, body, signal, 'current'),
    [fetchPositions],
  );
  const fetchCurrentSummary = useCallback(
    (id: string, signal?: AbortSignal) => fetchSummary(id, signal, 'current'),
    [fetchSummary],
  );
  const fetchHistory = useCallback(
    (id: string, body: TableRequestBody, signal?: AbortSignal) =>
      fetchPositions(id, body, signal, 'history'),
    [fetchPositions],
  );
  const fetchHistorySummary = useCallback(
    (id: string, signal?: AbortSignal) => fetchSummary(id, signal, 'history'),
    [fetchSummary],
  );

  // Current run: live (SSE deltas + fallback poll). Old runs: static (immutable).
  const current = useRulePositions(
    selectedRuleId, rules, fetchCurrent, fetchCurrentSummary, strategy, currentQuery, numericCols, true,
  );
  const history = useRulePositions(
    selectedRuleId, rules, fetchHistory, fetchHistorySummary, strategy, historyQuery, numericCols, false,
  );

  // One token-enrichment batch across both sections.
  const allMints = useMemo(() => {
    const s = new Set<string>();
    for (const p of current.positions) s.add(p.mint);
    for (const p of history.positions) s.add(p.mint);
    return [...s].sort();
  }, [current.positions, history.positions]);
  const { data: tokenBatch } = useGetTokensByMintsQuery(allMints, { skip: allMints.length === 0 });
  const tokenMap = useMemo(
    () => new Map((tokenBatch ?? []).map((t) => [t.mint_address, t])),
    [tokenBatch],
  );
  const currentRows = useMemo(
    () => mergeTokenData(current.positions, tokenMap),
    [current.positions, tokenMap],
  );
  const historyRows = useMemo(
    () => mergeTokenData(history.positions, tokenMap),
    [history.positions, tokenMap],
  );

  const onSelectCurrent = useCallback(
    (key: string | null) => onInspect(key ? current.positions.find((p) => p.id === key) ?? null : null),
    [current.positions, onInspect],
  );
  const onSelectHistory = useCallback(
    (key: string | null) => onInspect(key ? history.positions.find((p) => p.id === key) ?? null : null),
    [history.positions, onInspect],
  );

  const currentRowActions = useCallback(
    (row: RulePositionRecord) => {
      if (!onSellPosition || row.status !== 'Holding') return null;
      const isSelling = sellingPositionMint === row.mint;
      return (
        <div className="flex items-center justify-center" onClick={(e) => e.stopPropagation()}>
          <button
            type="button"
            disabled={isSelling}
            onClick={() => onSellPosition(row.mint)}
            className="rounded border border-red/50 bg-red/12 px-2 py-0.5 text-[11px] font-semibold text-red hover:bg-red/22 disabled:opacity-45"
          >
            {isSelling ? 'Selling…' : 'Sell ALL'}
          </button>
        </div>
      );
    },
    [onSellPosition, sellingPositionMint],
  );

  const ruleName = selectedRuleName ?? '';
  const showOldRuns = history.total > 0 || !!history.error;

  return (
    <>
      <SectionDivider />
      <section>
        <SectionHeading
          title="Current run"
          count={current.error ? undefined : current.total}
          subtitle={selectedRuleName ?? undefined}
        />
        {current.error && <InlineAlert variant="error">{current.error}</InlineAlert>}
        {!current.error && current.summary && current.summary.tokens > 0 && (
          <SimSummaryCard title="Current run" ruleName={ruleName} summary={current.summary} price={price} />
        )}
        {!current.error && (
          <DataTable
            columns={columns}
            rows={currentRows}
            rowKey={keyById}
            selectedKey={selectedKey}
            onSelect={onSelectCurrent}
            rowActions={isReal && onSellPosition ? currentRowActions : undefined}
            serverSide
            serverTotal={current.total}
            onQueryChange={setCurrentQuery}
            loading={current.loading}
            resetKey={selectedRuleId ?? ''}
            defaultPageSize={20}
            pageSizeOptions={[20, 50, 100]}
            colFilters
            colToggle
            tableId={`${strategy}_positions_current`}
            emptyMessage="No positions in the current run."
          />
        )}
      </section>

      {showOldRuns && (
        <section className={cn('mt-6')}>
          <SectionHeading
            title="Old runs"
            count={history.error ? undefined : history.total}
            subtitle="Previous runs of this rule"
          />
          {history.error && <InlineAlert variant="error">{history.error}</InlineAlert>}
          {!history.error && history.summary && history.summary.tokens > 0 && (
            <SimSummaryCard title="Old runs" ruleName={ruleName} summary={history.summary} price={price} />
          )}
          {!history.error && (
            <DataTable
              columns={historyColumns}
              rows={historyRows}
              rowKey={keyById}
              rowClassName={historyRowClass}
              selectedKey={selectedKey}
              onSelect={onSelectHistory}
              serverSide
              serverTotal={history.total}
              onQueryChange={setHistoryQuery}
              loading={history.loading}
              resetKey={selectedRuleId ?? ''}
              defaultPageSize={20}
              pageSizeOptions={[20, 50, 100]}
              colFilters
              colToggle
              tableId={`${strategy}_positions_history`}
              emptyMessage="No positions in previous runs."
            />
          )}
        </section>
      )}
    </>
  );
}
