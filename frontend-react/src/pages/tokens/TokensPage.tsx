import { useEffect, useMemo, useRef, useState } from 'react';
import { useDispatch } from 'react-redux';
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
import { FALLBACK_POLL_INTERVAL_MS } from 'services/config';
import { connectTokenCreatedStream, connectTradeStream } from 'services/sse';
import type { LiveTrade, TokenLiveStats, TokenRecord } from 'types';
import {
  apiSlice,
  apiErrorMessage,
  useGetTokenDetailQuery,
  useGetTokensPageQuery,
} from 'store/apiSlice';
import type { AppDispatch } from '../../store';
import { cn } from 'lib/cn';

const LS_LIVE_KEY = 'tokens_live';

/** Stable empty reference so derived memos don't recompute every render. */
const EMPTY_TOKENS: TokenRecord[] = [];

/** Stable row-key accessor — hoisted so the DataTable receives the same
 *  reference every render instead of a fresh inline closure each time. */
const tokenRowKey = (r: TokenRecord) => r.mint_address;

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
  const dispatch = useDispatch<AppDispatch>();
  // Built once and held stable: the rate-dependent cells read the unit/USD-rate
  // from context themselves (see priceCells), so a rate tick no longer rebuilds
  // every column def and re-renders the whole grid — only the price cells update.
  const columns = useMemo(() => tokenColumns(), []);

  const [live, setLive] = useState(loadLive);
  const [showFilters, setShowFilters] = useState(false);
  const [filters, setFilters] = useState<TokenFilters>(loadStoredTokenFilters);
  const [selectedMint, setSelectedMint] = useState<string | null>(null);
  // View-state emitted by the DataTable (page/sort/search/col-filters). The
  // backend does the filtering/sorting/paging; we just forward it + the global
  // `filters` panel as query args.
  const [tableQuery, setTableQuery] = useState<TableQuery>(INITIAL_QUERY);

  // The query args, shared by the live query and the adjacent-page prefetch
  // below so both hit identical cache keys.
  const queryArgs = useMemo(
    () => ({
      page: tableQuery.page,
      pageSize: tableQuery.pageSize,
      sortCol: tableQuery.sortCol,
      sortDir: tableQuery.sortDir,
      search: tableQuery.search,
      colFilters: tableQuery.colFilters,
      filters,
    }),
    [tableQuery, filters],
  );

  // Server-side page: only one page crosses the wire. Polling re-runs the
  // current filtered/sorted page. `filters` (the global panel) ride along as
  // query args; changing them resets the table to page 1 via `resetKey`.
  const {
    data: tokensData,
    isFetching: loading,
    error: tokensError,
    refetch,
  } = useGetTokensPageQuery(queryArgs, {
    // SSE drives the live view now: `trade_executed` frames patch each row's
    // stats in place (see the trade-stream effect below) and `token_created`
    // pulls in new rows. This poll is just a slow safety-net resync — it heals
    // dropped/lagged frames and re-applies the server sort that in-place patches
    // can't. Hence FALLBACK (30s) rather than the old 5s POLL_INTERVAL_MS.
    pollingInterval: live ? FALLBACK_POLL_INTERVAL_MS : 0,
    // Don't keep polling a background tab — the SSE refetch below catches it up
    // the moment it regains focus, and the timer resumes then.
    skipPollingIfUnfocused: true,
  });

  // Keep-previous-data: changing page/sort/filter targets a cache key that has
  // no data yet, so `tokensData` is briefly `undefined`. Rather than blanking
  // the grid to its empty/loading state on every interaction, hold the last
  // page we successfully rendered until the new one lands. `loading` still
  // drives a subtle busy state on the table.
  const lastItemsRef = useRef<TokenRecord[]>(EMPTY_TOKENS);
  if (tokensData?.items) lastItemsRef.current = tokensData.items;
  const tokens = tokensData?.items ?? lastItemsRef.current;
  const total = tokensData?.total ?? 0;
  const error = apiErrorMessage(tokensError, 'Failed to load tokens');

  // Warm the adjacent pages so forward/back paging resolves from cache instead
  // of a fresh round-trip. The short `keepUnusedDataFor` on `getTokensPage`
  // keeps these entries alive long enough for the click that follows.
  const prefetchPage = apiSlice.usePrefetch('getTokensPage');
  // Depend on the boolean existence of an adjacent page, not on `total` itself:
  // while live, every poll hands back a fresh `total` that's usually unchanged
  // page-wise, and depending on the raw number re-fired both prefetches each
  // tick. The booleans only flip when we actually cross a page boundary.
  const hasNextPage = tableQuery.page * tableQuery.pageSize < total;
  const hasPrevPage = tableQuery.page > 1;
  useEffect(() => {
    if (hasNextPage) prefetchPage({ ...queryArgs, page: tableQuery.page + 1 });
    if (hasPrevPage) prefetchPage({ ...queryArgs, page: tableQuery.page - 1 });
  }, [prefetchPage, queryArgs, tableQuery.page, hasNextPage, hasPrevPage]);

  // Resets the table to page 1 when the global filter panel changes.
  const filtersResetKey = useMemo(() => JSON.stringify(filters), [filters]);
  const filterCount = activeFilterCount(filters);
  // Whether any reduction is active — drives the "matched" vs "tracked" badge,
  // since `total` is now the filtered count.
  const anyActive =
    filterCount > 0 ||
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

  // Push-driven refresh: while live, a `token_created` SSE event refetches the
  // current page so new tokens surface promptly instead of waiting on the poll
  // timer. `refetch` and the current page are held in refs so re-subscribing
  // isn't tied to page/sort/filter changes (which would needlessly reopen the
  // EventSource). A burst of creations is debounced into one refetch.
  const refetchRef = useRef(refetch);
  refetchRef.current = refetch;
  const pageRef = useRef(tableQuery.page);
  pageRef.current = tableQuery.page;
  // Held in a ref so the trade-stream patch below targets the page that's
  // currently on screen without re-subscribing when args change.
  const queryArgsRef = useRef(queryArgs);
  queryArgsRef.current = queryArgs;

  // Mints currently on screen, held in a ref so the global trade stream can
  // discard frames for off-page mints WITHOUT re-subscribing. The feed carries
  // every mint's trades; only this page's rows (≤ pageSize) can be patched, so
  // buffering the rest is pure waste. Rebuilt whenever the page contents change
  // — including a pageSize change, since `tokens` reflects the live page size.
  const visibleMints = useMemo(
    () => new Set(tokens.map((t) => t.mint_address)),
    [tokens],
  );
  const visibleMintsRef = useRef(visibleMints);
  visibleMintsRef.current = visibleMints;

  // Push-driven row updates: every `trade_executed` frame carries the mint's
  // fresh stats (price / volume / market-cap / trade-count / ATH). Patch them
  // straight into the visible page's cache so the grid ticks in real time — no
  // poll round-trip. Trades are bursty, so coalesce: stash the latest stats per
  // mint and flush them in one cache write on a short timer. Mints not on the
  // current page are skipped; the fallback poll above re-sorts periodically.
  useEffect(() => {
    if (!live) return;
    const pending = new Map<string, TokenLiveStats>();
    let timer: number | undefined;
    const flush = () => {
      timer = undefined;
      if (pending.size === 0) return;
      const updates = new Map(pending);
      pending.clear();
      dispatch(
        apiSlice.util.updateQueryData('getTokensPage', queryArgsRef.current, (draft) => {
          for (const item of draft.items) {
            const s = updates.get(item.mint_address);
            if (!s) continue;
            item.current_price = s.current_price;
            item.volume_sol_total = s.volume_sol_total;
            item.market_cap = s.market_cap;
            item.trade_count = s.trade_count;
            item.ath_price = s.ath_price;
            item.ath_timestamp = s.ath_timestamp;
            item.last_trade_at = s.last_trade_at;
          }
        }),
      );
    };
    const es = connectTradeStream((raw) => {
      try {
        const t = JSON.parse(raw) as LiveTrade;
        // Skip mints not on the current page: only visible rows can be patched,
        // so buffering the rest just churns the Map and fires no-op cache writes.
        if (t.live && typeof t.live === 'object' && visibleMintsRef.current.has(t.mint)) {
          pending.set(t.mint, t.live);
          if (timer === undefined) timer = window.setTimeout(flush, 250);
        }
      } catch {
        /* ignore malformed frames */
      }
    });
    return () => {
      window.clearTimeout(timer);
      es.close();
    };
  }, [live, dispatch]);

  useEffect(() => {
    if (!live) return;
    let timer: number | undefined;
    const es = connectTokenCreatedStream(() => {
      // A freshly created token can only enter the view on the first page; while
      // the user is paging deeper, skip the refetch (the poll still covers any
      // in-place updates to rows already on screen).
      if (pageRef.current !== 1) return;
      window.clearTimeout(timer);
      timer = window.setTimeout(() => refetchRef.current(), 400);
    });
    return () => {
      window.clearTimeout(timer);
      es.close();
    };
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
  }, [selectedMint]);

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
          rowKey={tokenRowKey}
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
