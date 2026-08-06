/**
 * The Console History table — every position across every rule and run, paged by
 * the server (B1) under the same cohort the charts deck uses.
 *
 * This replaces the old 50-row "Recent closed" lane: that lane could only ever
 * show the tail of the session's SSE buffer, so "what happened last Tuesday" was
 * unanswerable without leaving the page.
 */

import { memo, useCallback, useMemo, useState } from 'react';
import { Link } from 'react-router-dom';
import { DataTable } from 'components/table/DataTable';
import type { ColumnDef, TableQuery } from 'components/table/types';
import { Badge } from 'components/ui/Badge';
import { InlineAlert, Modal } from 'components/ui/Modal';
import { FloorPositionDetailWithFills } from '@live/components/floor/FloorPositionDetailWithFills';
import { AddressDisplay } from 'components/ui/AddressDisplay';
import { AmountCell } from 'components/tokens/priceCells';
import { DateCell } from 'components/table/DateCell';
import { exitReasonBadge } from 'components/strategy/strategyColumns';
import { exitReasonSearchText } from 'lib/strategy/exitReason';
import { pctGradeClass, signedToneClass } from 'lib/signedTone';
import { resolvePnlPct } from 'lib/pnlPct';
import { ruleAnalyzeHref } from 'lib/strategy/nav';
import { fetchPortfolioPositionsPage } from 'services/api';
import { numericColKeys, toTableRequest } from 'services/tableRequest';
import { DEFAULT_POSITIONS_QUERY, useServerTable } from 'hooks/useServerTable';
import type { RulePositionRecord } from 'types';
import type { HistoryCohort } from '@live/pages/console/historyCohort';

/** `entry_time` → `exit_time` as a compact hold label. */
function holdLabel(r: RulePositionRecord): string | null {
  if (!r.entry_time) return null;
  const start = Date.parse(r.entry_time);
  const end = r.exit_time ? Date.parse(r.exit_time) : Date.now();
  if (!Number.isFinite(start) || !Number.isFinite(end) || end < start) return null;
  const s = Math.floor((end - start) / 1000);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ${s % 60}s`;
  return `${Math.floor(m / 60)}h ${m % 60}m`;
}

const STATUS_LABEL: Record<string, string> = {
  BuySubmitted: 'Buy submitted',
  Holding: 'Holding',
  ExitPending: 'Exit pending',
  ExitUnconfirmed: 'Exit unconfirmed',
  ExitStuck: 'Exit stuck',
  End: 'End',
  EntryFailed: 'Entry failed',
};

/**
 * Columns. Keys that filter/sort server-side must match the backend whitelist
 * (`position_sort_sql` / `position_filter_sql`): `mint_address`, `status`,
 * `exit_reason`, `entry_sol`, `pnl_sol`, `pnl_pct`, `exit_time`. `rule` and
 * `hold` are display-only (derived), so they are deliberately not sortable —
 * a header that sorts nothing is worse than one that doesn't offer to.
 */
function historyColumns(
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
          <Badge variant={r.status === 'EntryFailed' ? 'neutral' : 'primary'}>
            {STATUS_LABEL[r.status] ?? r.status}
          </Badge>
          {r.mode === 'paper' && <Badge variant="neutral">paper</Badge>}
        </span>
      ),
      sortValue: (r) => r.status,
      searchValue: (r) => STATUS_LABEL[r.status] ?? r.status,
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
          <span className={`tabular-nums ${pctGradeClass(pct)}`}>{pct.toFixed(1)}%</span>
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
        const h = holdLabel(r);
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
  ];
}

export const HistoryTable = memo(function HistoryTable({
  cohort,
  ruleNameOf,
  selectedKey,
  onSelect,
  reloadNonce,
}: {
  cohort: HistoryCohort;
  ruleNameOf: (id: string | null) => string | null;
  selectedKey: string | null;
  onSelect: (positionId: string | null, mint?: string) => void;
  /** Bumped by the live SSE terminal frames — refetches the current page. */
  reloadNonce: number;
}) {
  const [query, setQuery] = useState<TableQuery>(DEFAULT_POSITIONS_QUERY);
  const columns = useMemo(() => historyColumns(ruleNameOf), [ruleNameOf]);
  const numericCols = useMemo(() => numericColKeys(columns), [columns]);

  // The cohort is applied as structured filters + a range window on top of the
  // table's own per-column filters, so the server pages exactly the population
  // the charts above are drawn from.
  const body = useMemo(() => {
    const base = toTableRequest(query, numericCols);
    const filters = { ...base.filters };
    if (cohort.ruleId) filters.rule_id = { op: 'eq' as const, val: cohort.ruleId };
    if (cohort.mode !== 'all') filters.mode = { op: 'eq' as const, val: cohort.mode };
    if (cohort.status) filters.status = { op: 'eq' as const, val: cohort.status };
    if (cohort.exitReason) filters.exit_reason = { op: 'contains' as const, val: cohort.exitReason };
    return {
      ...base,
      filters,
      ...(cohort.fromIso || cohort.toIso
        ? {
            range: {
              ...(cohort.fromIso ? { from: cohort.fromIso } : {}),
              ...(cohort.toIso ? { to: cohort.toIso } : {}),
            },
          }
        : {}),
    };
  }, [query, numericCols, cohort]);

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

  const cohortKey = `${cohort.range}|${cohort.fromIso ?? ''}|${cohort.toIso ?? ''}|${
    cohort.ruleId ?? ''
  }|${cohort.mode}|${cohort.status ?? ''}|${cohort.exitReason ?? ''}`;

  // The selected row's full DB record — this is why History owns the closed
  // detail modal rather than the Console page: a position from any date opens
  // here, not just one still in the session's live lane.
  const inspect = selectedKey ? (items.find((r) => r.id === selectedKey) ?? null) : null;

  return (
    <>
      {error && <InlineAlert variant="error">History failed to load: {error}</InlineAlert>}
      <DataTable
        columns={columns}
        rows={items}
        rowKey={(r) => r.id}
        searchable
        colFilters
        loading={loading}
        serverSide
        serverTotal={total}
        onQueryChange={setQuery}
        resetKey={cohortKey}
        defaultSort={{ col: 'exit_time', dir: 'desc' }}
        defaultPageSize={25}
        tableId="console-history"
        emptyMessage="No positions in this cohort — widen the date range or clear the filters."
        selectedKey={selectedKey}
        onSelect={(key) => {
          const row = items.find((r) => r.id === key);
          onSelect(key, row?.mint_address);
        }}
      />

      {inspect && (
        <Modal
          title={`${inspect.symbol || `${inspect.mint_address.slice(0, 8)}…`} — position`}
          open
          onClose={() => onSelect(null)}
          size="xxl"
        >
          <FloorPositionDetailWithFills
            positionId={inspect.id}
            facts={{
              mint: inspect.mint_address,
              ruleId: inspect.rule_id,
              ruleName: ruleNameOf(inspect.rule_id),
              mode: inspect.mode ?? null,
              status: STATUS_LABEL[inspect.status] ?? inspect.status,
              entrySol: inspect.entry_sol ?? null,
              entryPrice: inspect.entry_price,
              exitPrice: inspect.exit_price,
              holdLabel: holdLabel(inspect),
              pnlSol: inspect.pnl_sol,
              pnlPct: resolvePnlPct({
                pnlSol: inspect.pnl_sol,
                entrySol: inspect.entry_sol,
                entryPrice: inspect.entry_price,
                exitPrice: inspect.exit_price,
              }),
              inspect: {
                mint_address: inspect.mint_address,
                entryTime: inspect.entry_time,
                entryPrice: inspect.entry_price,
                exitTime: inspect.exit_time,
                exitPrice: inspect.exit_price,
                exitLabel: inspect.exit_reason,
              },
            }}
            chartHeight={420}
          />
        </Modal>
      )}
    </>
  );
});
