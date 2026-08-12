/**
 * The Console History table — every position across every rule and run, paged by
 * the server (B1) under the same cohort the charts deck uses.
 *
 * This replaces the old 50-row "Recent closed" lane: that lane could only ever
 * show the tail of the session's SSE buffer, so "what happened last Tuesday" was
 * unanswerable without leaving the page.
 *
 * It rides `TokenTable` rather than `DataTable` for ONE reason: the toolbar's
 * **Charts** toggle. On, each row on the current page gets its token's price
 * chart below the table, carrying that position's own entry/exit/target markers
 * and a PnL/hold header — the per-position read, where the deck above is the
 * per-cohort one. The grid is lazy-mounted, so it costs nothing until toggled.
 */

import { memo, useCallback, useMemo } from 'react';
import { Link } from 'react-router-dom';
import { TokenTable } from 'components/tokens/TokenTable';
import { ALL_TOKEN_INFO_KEYS } from 'components/tokens/sharedTokenColumns';
import { inspectFromPosition, markerRowOverlay } from 'components/strategy/inspectTarget';
import {
  PositionChartCardExtra,
  positionChartFactsFromRule,
} from 'components/strategy/PositionChartCardExtra';
import type { ColumnDef, TableQuery } from 'components/table/types';
import { Badge } from 'components/ui/Badge';
import { InlineAlert } from 'components/ui/Modal';
import {
  OPEN_STATUS_LABEL,
  openStatusBadgeVariant,
} from '@live/components/floor/openPositionStatus';
import { usePositionArrowNav } from '@live/components/floor/usePositionArrowNav';
import { LazyLivePositionInspectModal } from '@live/components/strategy/LazyLivePositionInspectModal';
import { AddressDisplay } from 'components/ui/AddressDisplay';
import { AmountCell } from 'components/tokens/priceCells';
import { DateCell } from 'components/table/DateCell';
import { RelativeTimeCell } from 'components/table/RelativeTimeCell';
import { ModeBadge } from 'components/strategy/ModeBadge';
import { exitReasonBadge } from 'components/strategy/strategyColumns';
import { exitReasonSearchText } from 'lib/strategy/exitReason';
import { formatSignedPct, pctGradeClass, signedToneClass } from 'lib/signedTone';
import { holdLabel } from 'lib/holdLabel';
import { resolvePnlPct } from 'lib/pnlPct';
import { ruleAnalyzeHref } from 'lib/strategy/nav';
import { fetchPortfolioPositionsPage } from 'services/api';
import { useFlowPatternKeysForRule } from 'hooks/useFlowPatternKeys';
import { useServerTable } from 'hooks/useServerTable';
import type { RulePositionRecord } from 'types';
import type { HistoryCohort } from '@live/pages/console/historyCohort';
import {
  historyCohortKey,
  historyNeedsClientScan,
  historyTableBody,
} from '@live/pages/console/historyRequest';
import { applyHistoryClientLenses } from '@live/pages/console/historyPositions';

/** Stable row key — hoisted so DataTable doesn't see a fresh closure each render. */
const historyPositionRowKey = (r: RulePositionRecord) => r.id;

/** Entry/exit/target fill markers on each chart card — the same overlay the row's
 *  inspect modal draws, through the one `InspectTarget` adapter. */
const historyRowOverlay = markerRowOverlay(inspectFromPosition);

/** Hold / PnL / exit-reason facts in the chart card header, so a card reads like
 *  its table row. Hoisted — a fresh closure would remount every card. */
const historyChartCardExtra = (r: RulePositionRecord) => (
  <PositionChartCardExtra facts={positionChartFactsFromRule(r)} />
);

/** Each card's vol/non-vol overlay classifies against ITS OWN rule's
 *  `volume_ix_patterns` — a History page spans rules, so one grid-wide set would
 *  misclassify every card that isn't from that rule. Resolved through the same
 *  hook the row's inspect modal uses, so a card and its modal always agree.
 *  Called once per card as a hook (see `FlowPatternKeysHook`). */
const useHistoryRowFlowPatternKeys = (r: RulePositionRecord) =>
  useFlowPatternKeysForRule(r.rule_id);

/**
 * Columns. Keys that filter/sort server-side must match the backend whitelist
 * (`position_sort_sql` / `position_filter_sql`): `mint_address`, `status`,
 * `exit_reason`, `entry_sol`, `pnl_sol`, `pnl_pct`, `exit_time`. `rule` and
 * `hold` are display-only (derived), so they are deliberately not sortable —
 * a header that sorts nothing is worse than one that doesn't offer to.
 */
export function historyColumns(
  ruleNameOf: (id: string | null) => string | null,
): ColumnDef<RulePositionRecord>[] {
  return [
    {
      key: 'mint_address',
      label: 'Token',
      render: (r) => <AddressDisplay address={r.mint_address} kind="token" display={r.symbol} />,
      searchValue: (r) => `${r.mint_address} ${r.symbol ?? ''}`,
    },
    {
      key: 'rule',
      label: 'Rule',
      render: (r) => {
        const name = ruleNameOf(r.rule_id);
        if (!r.rule_id) return <span className="text-text-dim">—</span>;
        return (
          <Link to={ruleAnalyzeHref(r.rule_id)} className="text-accent hover:underline">
            {name ?? `${r.rule_id.slice(0, 8)}…`}
          </Link>
        );
      },
      searchValue: (r) => ruleNameOf(r.rule_id) ?? r.rule_id ?? '',
    },
    {
      key: 'status',
      label: 'Status',
      sortable: true,
      render: (r) => (
        <span className="inline-flex flex-wrap items-center gap-1">
          <Badge variant={openStatusBadgeVariant(r.status)}>
            {OPEN_STATUS_LABEL[r.status] ?? r.status}
          </Badge>
          {(r.mode === 'paper' || r.mode === 'real') && <ModeBadge mode={r.mode} />}
        </span>
      ),
      sortValue: (r) => r.status,
      searchValue: (r) => OPEN_STATUS_LABEL[r.status] ?? r.status,
    },
    {
      key: 'exit_reason',
      label: 'Exit',
      sortable: true,
      render: (r) => exitReasonBadge(r.exit_reason, r.pnl_sol, r.last_entry_error),
      sortValue: (r) => r.exit_reason ?? '',
      searchValue: (r) =>
        [exitReasonSearchText(r.exit_reason, r.pnl_sol), r.last_entry_error ?? '']
          .filter(Boolean)
          .join(' '),
    },
    {
      key: 'entry_sol',
      label: 'Entry ◎',
      sortable: true,
      render: (r) =>
        r.entry_sol != null ? <AmountCell sol={r.entry_sol} /> : <span className="text-text-dim">—</span>,
      sortValue: (r) => r.entry_sol ?? null,
      searchValue: () => '',
      filterNumber: (r) => r.entry_sol ?? null,
      filterAmount: 'sol',
    },
    {
      key: 'pnl_sol',
      label: 'PnL ◎',
      sortable: true,
      render: (r) =>
        r.pnl_sol != null ? (
          <span className={`font-bold ${signedToneClass(r.pnl_sol)}`}>
            <AmountCell sol={r.pnl_sol} />
          </span>
        ) : (
          <span className="text-text-dim">—</span>
        ),
      sortValue: (r) => r.pnl_sol ?? null,
      searchValue: () => '',
      filterNumber: (r) => r.pnl_sol ?? null,
      filterAmount: 'sol',
    },
    {
      key: 'pnl_pct',
      label: 'PnL%',
      sortable: true,
      render: (r) => {
        const pct = resolvePnlPct({
          pnlSol: r.pnl_sol,
          entrySol: r.entry_sol,
          entryPrice: r.entry_price,
          exitPrice: r.exit_price,
        });
        return pct != null ? (
          <span className={`tabular-nums ${pctGradeClass(pct)}`}>{formatSignedPct(pct, 1)}</span>
        ) : (
          <span className="text-text-dim">—</span>
        );
      },
      sortValue: (r) => r.pnl_percent ?? null,
      searchValue: () => '',
      filterNumber: (r) => r.pnl_percent ?? null,
    },
    {
      key: 'hold',
      label: 'Held',
      render: (r) => {
        const h = holdLabel(r.entry_time, r.exit_time);
        return h ? (
          <span className="tabular-nums text-text-dim">{h}</span>
        ) : (
          <span className="text-text-dim">—</span>
        );
      },
      searchValue: () => '',
    },
    {
      key: 'exit_time',
      label: 'Closed',
      sortable: true,
      render: (r) => <DateCell iso={r.exit_time} />,
      sortValue: (r) => r.exit_time ?? '',
      searchValue: () => '',
    },
    {
      // `now - exit_time`. Derived from the same column the server sorts on, so
      // it is display-only — sort/filter through `Closed` instead of duplicating
      // a second server key for the same fact.
      key: 'exit_ago',
      label: 'Ago',
      render: (r) => <RelativeTimeCell iso={r.exit_time} />,
      searchValue: () => '',
    },
  ];
}

/** Heat focus can't be expressed as one SQL range (recurring dow×hour), so we
 *  pull a wide page and filter client-side. Cap matches the server page_size max. */
const HEAT_SCAN_PAGE_SIZE = 1000;

export const HistoryTable = memo(function HistoryTable({
  cohort,
  columns,
  numericCols,
  timezone,
  query,
  onQueryChange,
  ruleNameOf,
  selectedKey,
  onSelect,
  reloadNonce,
}: {
  cohort: HistoryCohort;
  /** Built by the section — it needs the same defs to derive `numericCols`. */
  columns: ColumnDef<RulePositionRecord>[];
  numericCols: ReadonlySet<string>;
  timezone: string;
  /** Owned by the section, so the strip + charts narrow with this table. */
  query: TableQuery;
  onQueryChange: (q: TableQuery) => void;
  ruleNameOf: (id: string | null) => string | null;
  selectedKey: string | null;
  onSelect: (positionId: string | null, mint?: string) => void;
  /** Bumped by the live SSE terminal frames — refetches the current page. */
  reloadNonce: number;
}) {
  const focus = cohort.focus;
  const clientScanFocus = historyNeedsClientScan(cohort);

  // Cohort + chart focus + this table's own filters, through the one builder the
  // summary and the charts also use — see `historyRequest`.
  const body = useMemo(
    () =>
      historyTableBody(
        { cohort, query, numericCols, timezone },
        clientScanFocus ? HEAT_SCAN_PAGE_SIZE : undefined,
      ),
    [cohort, query, numericCols, timezone, clientScanFocus],
  );

  const fetchPage = useCallback(
    (b: unknown, signal: AbortSignal) => fetchPortfolioPositionsPage(b as never, signal),
    [],
  );

  const { items, total, loading, error } = useServerTable<RulePositionRecord>(
    true,
    body,
    fetchPage,
    undefined,
    undefined,
    `history:${reloadNonce}`,
  );

  // The SQL-inexpressible lenses, through the same helper the strip and the
  // charts use — one predicate, so the three can't disagree about a row.
  const rows = useMemo(
    () => applyHistoryClientLenses(items, cohort, timezone),
    [items, cohort, timezone],
  );

  const cohortKey = historyCohortKey(cohort, timezone);

  // The selected row's full DB record — this is why History owns the closed
  // detail modal rather than the Console page: a position from any date opens
  // here, not just one still in the session's live lane.
  const inspect = selectedKey ? (rows.find((r) => r.id === selectedKey) ?? null) : null;
  const heatScanCapped = clientScanFocus && total >= HEAT_SCAN_PAGE_SIZE;

  const handleSelect = useCallback(
    (key: string | null) => {
      const row = key ? rows.find((r) => r.id === key) : undefined;
      onSelect(key, row?.mint_address);
    },
    [rows, onSelect],
  );

  const historyNavKeys = useMemo(() => rows.map((r) => r.id), [rows]);
  const onHistoryArrow = useCallback(
    (key: string) => {
      const row = rows.find((r) => r.id === key);
      onSelect(key, row?.mint_address);
    },
    [rows, onSelect],
  );
  usePositionArrowNav({
    enabled: !!inspect,
    keys: historyNavKeys,
    selectedKey,
    onSelect: onHistoryArrow,
  });

  return (
    <>
      {error && <InlineAlert variant="error">History failed to load: {error}</InlineAlert>}
      {heatScanCapped && (
        <InlineAlert variant="warning">
          Chart focus scanned the first {HEAT_SCAN_PAGE_SIZE} cohort rows — narrow the date range
          if results look incomplete.
        </InlineAlert>
      )}
      <TokenTable
        columns={columns}
        existingKeys={ALL_TOKEN_INFO_KEYS}
        rows={rows}
        rowKey={historyPositionRowKey}
        searchable
        colFilters
        charts
        useRowOverlay={historyRowOverlay}
        renderChartCardExtra={historyChartCardExtra}
        useRowChartFlowPatternKeys={useHistoryRowFlowPatternKeys}
        loading={loading}
        serverSide={!clientScanFocus}
        serverTotal={clientScanFocus ? rows.length : total}
        onQueryChange={onQueryChange}
        resetKey={cohortKey}
        defaultSort={{ col: 'exit_time', dir: 'desc' }}
        defaultPageSize={25}
        tableId="console-history"
        emptyMessage={
          focus
            ? 'No positions in this chart focus — click the cell again or clear Focus.'
            : 'No positions in this cohort — widen the date range or clear the filters.'
        }
        selectedKey={selectedKey}
        onSelect={handleSelect}
      />

      {inspect && (
        <LazyLivePositionInspectModal
          position={inspect}
          rule={null}
          ruleName={ruleNameOf(inspect.rule_id)}
          onClose={() => onSelect(null)}
        />
      )}
    </>
  );
});
