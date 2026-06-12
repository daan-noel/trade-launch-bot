import { useEffect, useMemo, useState } from 'react';
import { DataTable } from 'components/table/DataTable';
import { FilterPanel } from 'components/tokens/FilterPanel';
import { TokenDetailPanel } from 'components/tokens/TokenDetailPanel';
import { TokenTradeChart } from 'components/tokens/TokenTradeChart';
import { tokenColumns } from 'components/tokens/tokenColumns';
import {
  activeFilterCount,
  defaultFilters,
  filtersEmpty,
  loadStoredTokenFilters,
  saveStoredTokenFilters,
  tokenPassesFilters,
  type TokenFilters,
} from 'components/tokens/filters';
import { Badge } from 'components/ui/Badge';
import { Button } from 'components/ui/Button';
import { StatusButton } from 'components/ui/StatusButton';
import { POLL_INTERVAL_MS } from 'services/config';
import type { TokenRecord } from 'types';
import { usePriceDisplay } from 'hooks/usePriceDisplay';
import {
  apiErrorMessage,
  TOKENS_LIST_LIMIT,
  useGetTokenDetailQuery,
  useGetTokensQuery,
} from 'store/apiSlice';
import { cn } from 'lib/cn';

const LS_LIVE_KEY = 'tokens_live';

/** Stable empty reference so derived memos don't recompute every render. */
const EMPTY_TOKENS: TokenRecord[] = [];

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

  // Shared cache: identical args to SwingDetectionPage, so the 5000-row list is
  // fetched once and reused across navigation. Live mode polls into the cache.
  const {
    data: tokensData,
    isLoading: loading,
    error: tokensError,
  } = useGetTokensQuery(
    { search: '', limit: TOKENS_LIST_LIMIT, offset: 0 },
    { pollingInterval: live ? POLL_INTERVAL_MS : 0 },
  );
  const tokens = tokensData?.items ?? EMPTY_TOKENS;
  const total = tokensData?.total ?? 0;
  const error = apiErrorMessage(tokensError, 'Failed to load tokens');

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

  const displayed = useMemo(() => {
    if (filtersEmpty(filters)) return tokens;
    return tokens.filter((t) => tokenPassesFilters(filters, t));
  }, [tokens, filters]);

  const filterCount = activeFilterCount(filters);

  return (
    <div>
      <div className="mb-3.5 flex flex-wrap items-center gap-3">
        <h2 className="text-lg font-extrabold text-text">Tokens</h2>
        <Badge variant="primary" className="font-mono">
          {total} tracked
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

      {loading && <p className="text-text-dim">Loading tokens…</p>}
      {error && !loading && <p className="text-red">{error}</p>}
      {!loading && !error && (
        <DataTable
          columns={columns}
          rows={displayed}
          rowKey={(r) => r.mint_address}
          selectedKey={selectedMint}
          onSelect={setSelectedMint}
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
