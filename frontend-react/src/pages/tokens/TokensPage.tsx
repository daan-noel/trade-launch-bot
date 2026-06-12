import { useEffect, useMemo, useState } from 'react';
import { DataTable } from 'components/table/DataTable';
import { FilterPanel } from 'components/tokens/FilterPanel';
import { TokenDetailPanel } from 'components/tokens/TokenDetailPanel';
import { TokenTradeChart } from 'components/tokens/TokenTradeChart';
import { tokenColumns } from 'components/tokens/tokenColumns';
import {
  activeFilterCount,
  defaultFilters,
  loadStoredTokenFilters,
  saveStoredTokenFilters,
  type TokenFilters,
} from 'components/tokens/filters';
import type { TableQuery } from 'components/table/types';
import { Badge } from 'components/ui/Badge';
import { Button } from 'components/ui/Button';
import { StatusButton } from 'components/ui/StatusButton';
import { POLL_INTERVAL_MS } from 'services/config';
import type { TokenRecord } from 'types';
import { usePriceDisplay } from 'hooks/usePriceDisplay';
import {
  apiErrorMessage,
  useGetTokenDetailQuery,
  useGetTokensPageQuery,
} from 'store/apiSlice';
import { cn } from 'lib/cn';

const LS_LIVE_KEY = 'tokens_live';

/** Stable empty reference so derived memos don't recompute every render. */
const EMPTY_TOKENS: TokenRecord[] = [];

/** Initial table view-state; pageSize matches the DataTable's default (10). */
const INITIAL_QUERY: TableQuery = {
  page: 1,
  pageSize: 10,
  sortCol: null,
  sortDir: 'asc',
  search: '',
  colFilters: {},
};

function loadLive(): boolean {
  try {
    return localStorage.getItem(LS_LIVE_KEY) === 'true';
  } catch {
    return false;
  }
}

export function TokensPage() {
  const price = usePriceDisplay();
  const columns = useMemo(() => tokenColumns(price), [price]);

  const [live, setLive] = useState(loadLive);
  const [showFilters, setShowFilters] = useState(false);
  const [filters, setFilters] = useState<TokenFilters>(loadStoredTokenFilters);
  const [selectedMint, setSelectedMint] = useState<string | null>(null);
  // View-state emitted by the DataTable (page/sort/search/col-filters). The
  // backend does the filtering/sorting/paging; we just forward it + the global
  // `filters` panel as query args.
  const [tableQuery, setTableQuery] = useState<TableQuery>(INITIAL_QUERY);

  // Server-side page: only one page crosses the wire. Polling re-runs the
  // current filtered/sorted page. `filters` (the global panel) ride along as
  // query args; changing them resets the table to page 1 via `resetKey`.
  const {
    data: tokensData,
    isFetching: loading,
    error: tokensError,
  } = useGetTokensPageQuery(
    {
      page: tableQuery.page,
      pageSize: tableQuery.pageSize,
      sortCol: tableQuery.sortCol,
      sortDir: tableQuery.sortDir,
      search: tableQuery.search,
      colFilters: tableQuery.colFilters,
      filters,
    },
    { pollingInterval: live ? POLL_INTERVAL_MS : 0 },
  );
  const tokens = tokensData?.items ?? EMPTY_TOKENS;
  const total = tokensData?.total ?? 0;
  const error = apiErrorMessage(tokensError, 'Failed to load tokens');

  // Resets the table to page 1 when the global filter panel changes.
  const filtersResetKey = useMemo(() => JSON.stringify(filters), [filters]);
  // Whether any reduction is active — drives the "matched" vs "tracked" badge,
  // since `total` is now the filtered count.
  const anyActive =
    activeFilterCount(filters) > 0 ||
    !!tableQuery.search ||
    Object.values(tableQuery.colFilters).some(Boolean);

  // Per-mint detail cached by mint, so re-selecting a token is instant.
  const {
    data: detail,
    isFetching: detailLoading,
    error: detailErrorRaw,
  } = useGetTokenDetailQuery(selectedMint ?? '', { skip: !selectedMint });
  const detailError = selectedMint
    ? apiErrorMessage(detailErrorRaw, 'Failed to load detail')
    : null;

  useEffect(() => {
    try {
      localStorage.setItem(LS_LIVE_KEY, live ? 'true' : 'false');
    } catch {
      /* ignore */
    }
  }, [live]);

  useEffect(() => {
    if (!selectedMint) return;
    const t = setTimeout(() => {
      document.getElementById(`detail-${selectedMint}`)?.scrollIntoView({
        behavior: 'smooth',
        block: 'nearest',
      });
    }, 300);
    return () => clearTimeout(t);
  }, [selectedMint, detail]);

  const filterCount = activeFilterCount(filters);

  return (
    <div>
      <div className="mb-3.5 flex flex-wrap items-center gap-3">
        <h2 className="text-lg font-extrabold text-text">Tokens</h2>
        <Badge variant="primary" className="font-mono">
          {total} {anyActive ? 'matched' : 'tracked'}
        </Badge>
        <StatusButton
          state={live ? 'live' : 'dead'}
          label={live ? 'ACTIVE' : 'PAUSED'}
          onClick={() => setLive((v) => !v)}
          className={cn(
            'px-4 py-0.5 text-[10px]',
            live && 'animate-pulse',
          )}
        />
      </div>

      <div className="mb-1.5 flex gap-1.5 justify-end">
        <Button
          variant="subtle"
          size="sm"
          active={showFilters || filterCount > 0}
          onClick={() => setShowFilters((v) => !v)}
        >
          {filterCount > 0 ? `Global Filters (${filterCount})` : 'Global Filters'}
        </Button>
      </div>

      {showFilters && (
        <FilterPanel
          filters={filters}
          onApply={(next) => {
            setFilters(next);
            saveStoredTokenFilters(next);
          }}
          onClear={() => {
            const empty = defaultFilters();
            setFilters(empty);
            saveStoredTokenFilters(empty);
          }}
        />
      )}

      {error && <p className="text-red">{error}</p>}
      {!error && (
        <DataTable
          columns={columns}
          rows={tokens}
          rowKey={(r) => r.mint_address}
          selectedKey={selectedMint}
          onSelect={setSelectedMint}
          serverSide
          serverTotal={total}
          onQueryChange={setTableQuery}
          loading={loading}
          resetKey={filtersResetKey}
          searchable
          colFilters
          colToggle
          hoverable
          storageKey="tokens_visible_cols"
          emptyMessage="No tokens found"
        />
      )}

      {/* Detail section lives BELOW the table, outside its horizontal scroll box,
          so the chart sizes to the page width and never inherits the table's
          x-scroll. Selecting a row highlights it and fills this panel. */}
      {selectedMint && (
        <div
          id={`detail-${selectedMint}`}
          className="mt-3.5 flex flex-col gap-2.5 rounded-lg border border-white/6 bg-bg-panel p-3"
        >
          <TokenDetailPanel detail={detail ?? null} loading={detailLoading} error={detailError} />
          <TokenTradeChart detail={detail ?? null} />
        </div>
      )}
    </div>
  );
}
