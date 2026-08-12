/**
 * The Console Arms table — every arming episode the ledger holds, server-paged
 * under the same cohort the funnel strip above it reads.
 *
 * It rides `TokenTable` rather than `DataTable` for the toolbar's **Charts**
 * toggle: on, each row on the current page gets its token's price chart below
 * the table, so "what was this token doing while the rule watched it and passed"
 * is one click rather than a per-mint detour. The cards carry no fill markers —
 * an arming episode has no fill price — so the episode's facts ride the card
 * header instead (see `liveChartCards`).
 */

import { memo, useCallback, useMemo } from 'react';
import { Link } from 'react-router-dom';
import { TokenTable } from 'components/tokens/TokenTable';
import { ALL_TOKEN_INFO_KEYS } from 'components/tokens/sharedTokenColumns';
import { AddressDisplay } from 'components/ui/AddressDisplay';
import { InlineAlert } from 'components/ui/Modal';
import { DateCell } from 'components/table/DateCell';
import { RelativeTimeCell } from 'components/table/RelativeTimeCell';
import type { ColumnDef, TableQuery } from 'components/table/types';
import { ModeBadge } from 'components/strategy/ModeBadge';
import { useFlowPatternKeysForRule } from 'hooks/useFlowPatternKeys';
import { useServerTable } from 'hooks/useServerTable';
import { fetchArmsPage } from 'services/api';
import { formatDurationShort } from 'utils/format';
import { ruleAnalyzeHref } from 'lib/strategy/nav';
import type { StrategyArmRecord } from 'lib/strategy/types';
import { ARM_END_LABEL, ArmChartCardExtra, armEndBadge } from '@live/components/floor/liveChartCards';
import type { ArmCohort } from '@live/pages/console/armCohort';
import { armCohortKey, armTableBody } from '@live/pages/console/armRequest';

/** Stable row key. An episode is unique by `(rule, mint, armed_at)` — the same
 *  natural key the table is stored under, so a re-arm of the same pair is its
 *  own row rather than replacing the earlier episode. */
export const armRowKey = (r: StrategyArmRecord) =>
  `${r.rule_id}|${r.mint_address}|${r.armed_at}`;

/** Each card classifies vol/non-vol against ITS OWN rule's `volume_ix_patterns`
 *  — the Arms table spans rules, so one grid-wide set would misclassify every
 *  card that isn't from that rule. Called once per card as a hook. */
const useArmRowFlowPatternKeys = (r: StrategyArmRecord) => useFlowPatternKeysForRule(r.rule_id);

const armChartCardExtra = (r: StrategyArmRecord) => (
  <ArmChartCardExtra
    armedAtIso={r.armed_at}
    endedAtIso={r.ended_at}
    endReason={r.end_reason}
  />
);

/**
 * Columns. Keys that sort/filter server-side must match the backend whitelist
 * (`arm_sort_sql` / `arm_filter_sql`): `mint_address`, `symbol`, `rule_id`,
 * `mode`, `armed_at`, `ended_at`, `end_reason`, `waited_sec`. `rule` is
 * display-only (the name is resolved client-side), so it is deliberately not
 * sortable — a header that sorts nothing is worse than one that doesn't offer to.
 */
export function armColumns(
  ruleNameOf: (id: string | null) => string | null,
): ColumnDef<StrategyArmRecord>[] {
  return [
    {
      key: 'mint_address',
      label: 'Token',
      render: (r) => (
        <AddressDisplay address={r.mint_address} kind="token" display={r.symbol ?? undefined} />
      ),
      searchValue: (r) => `${r.mint_address} ${r.symbol ?? ''}`,
    },
    {
      key: 'rule',
      label: 'Rule',
      render: (r) => (
        <span className="inline-flex flex-wrap items-center gap-1">
          <Link to={ruleAnalyzeHref(r.rule_id)} className="text-accent hover:underline">
            {ruleNameOf(r.rule_id) ?? `${r.rule_id.slice(0, 8)}…`}
          </Link>
          {(r.mode === 'paper' || r.mode === 'real') && <ModeBadge mode={r.mode} />}
        </span>
      ),
      searchValue: (r) => ruleNameOf(r.rule_id) ?? r.rule_id,
    },
    {
      key: 'end_reason',
      label: 'Outcome',
      sortable: true,
      render: (r) => armEndBadge(r.end_reason),
      sortValue: (r) => r.end_reason ?? '',
      searchValue: (r) => (r.end_reason ? (ARM_END_LABEL[r.end_reason] ?? r.end_reason) : 'waiting'),
    },
    {
      key: 'armed_at',
      label: 'Armed',
      sortable: true,
      render: (r) => <DateCell iso={r.armed_at} />,
      sortValue: (r) => r.armed_at,
      searchValue: () => '',
    },
    {
      key: 'waited_sec',
      label: 'Waited',
      sortable: true,
      render: (r) =>
        r.waited_sec != null ? (
          <span className="tabular-nums text-text-dim">
            {formatDurationShort(Math.round(r.waited_sec))}
          </span>
        ) : (
          <span className="text-text-dim">—</span>
        ),
      sortValue: (r) => r.waited_sec ?? null,
      searchValue: () => '',
      filterNumber: (r) => r.waited_sec ?? null,
    },
    {
      key: 'ended_at',
      label: 'Ended',
      sortable: true,
      render: (r) =>
        r.ended_at ? <DateCell iso={r.ended_at} /> : <span className="text-text-dim">—</span>,
      sortValue: (r) => r.ended_at ?? '',
      searchValue: () => '',
    },
    {
      // `now - armed_at`. Derived from the column the server sorts on, so it is
      // display-only — sort/filter through `Armed` instead of adding a second
      // server key for the same fact.
      key: 'armed_ago',
      label: 'Ago',
      render: (r) => <RelativeTimeCell iso={r.armed_at} />,
      searchValue: () => '',
    },
  ];
}

export const ArmsTable = memo(function ArmsTable({
  cohort,
  columns,
  numericCols,
  query,
  onQueryChange,
  selectedKey,
  onSelect,
  reloadNonce,
}: {
  cohort: ArmCohort;
  /** Built by the section — it needs the same defs to derive `numericCols`. */
  columns: ColumnDef<StrategyArmRecord>[];
  numericCols: ReadonlySet<string>;
  /** Owned by the section, so the funnel narrows with this table. */
  query: TableQuery;
  onQueryChange: (q: TableQuery) => void;
  selectedKey: string | null;
  onSelect: (key: string | null, mint?: string) => void;
  /** Bumped by live arm SSE frames — refetches the current page. */
  reloadNonce: number;
}) {
  const body = useMemo(
    () => armTableBody({ cohort, query, numericCols }),
    [cohort, query, numericCols],
  );
  const fetchPage = useCallback(
    (b: unknown, signal: AbortSignal) => fetchArmsPage(b as never, signal),
    [],
  );
  const { items, total, loading, error } = useServerTable<StrategyArmRecord>(
    true,
    body,
    fetchPage,
    undefined,
    undefined,
    `arms:${reloadNonce}`,
  );

  const handleSelect = useCallback(
    (key: string | null) => {
      const row = key ? items.find((r) => armRowKey(r) === key) : undefined;
      onSelect(key, row?.mint_address);
    },
    [items, onSelect],
  );

  return (
    <>
      {error && <InlineAlert variant="error">Arms failed to load: {error}</InlineAlert>}
      <TokenTable
        columns={columns}
        existingKeys={ALL_TOKEN_INFO_KEYS}
        rows={items}
        rowKey={armRowKey}
        searchable
        colFilters
        charts
        useRowChartFlowPatternKeys={useArmRowFlowPatternKeys}
        renderChartCardExtra={armChartCardExtra}
        loading={loading}
        serverSide
        serverTotal={total}
        onQueryChange={onQueryChange}
        resetKey={armCohortKey(cohort)}
        defaultSort={{ col: 'armed_at', dir: 'desc' }}
        defaultPageSize={25}
        tableId="console-arms"
        emptyMessage="No arming episodes in this cohort — widen the date range or clear the filters."
        selectedKey={selectedKey}
        onSelect={handleSelect}
      />
    </>
  );
});
