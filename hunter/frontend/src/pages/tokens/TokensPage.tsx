import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { useDispatch } from 'react-redux';
import { useSearchParams } from 'react-router-dom';
import { TokenTable } from 'components/tokens/TokenTable';
import { ALL_TOKEN_INFO_KEYS } from 'components/tokens/sharedTokenColumns';
import { FilterPanel } from 'components/tokens/FilterPanel';
import { TokenDetailPanel } from 'components/tokens/TokenDetailPanel';
import { LazyTokenTradeChart } from 'components/tokens/LazyTokenTradeChart';
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
import { IconButton } from 'components/ui/IconButton';
import { PageHeader } from 'components/ui/PageHeader';
import { SettingsIcon } from 'components/ui/icons';
import { StatusButton } from 'components/ui/StatusButton';
import { cn } from 'lib/cn';
import { FALLBACK_POLL_INTERVAL_MS } from 'services/config';
import { connectTokenCreatedStream, connectTradeStream } from 'services/sse';
import type { LiveTrade, TokenDetailRecord, TokenLiveStats, TokenRecord } from 'types';
import {
  apiErrorMessage,
  useGetTokenDetailQuery,
  useGetTokensPageQuery,
} from 'store/apiSlice';
import { sharedApi } from 'store/sharedEndpoints';
import type { AppDispatch } from 'store/types';
import { useTimezone } from 'context/TimezoneContext';
import { STORAGE_KEYS, getJSON, setJSON } from 'lib/storage';

const LS_LIVE_KEY = STORAGE_KEYS.tokensLive;

/** Stable empty reference so derived memos don't recompute every render. */
const EMPTY_TOKENS: TokenRecord[] = [];

/** Stable row-key accessor — hoisted so the DataTable receives the same
 *  reference every render instead of a fresh inline closure each time. */
const tokenRowKey = (r: TokenRecord) => r.mint_address;

/** Initial table view-state; pageSize matches the DataTable's default (10). */
const INITIAL_QUERY: TableQuery = {
  page: 1,
  pageSize: 10,
  sortKeys: [],
  search: '',
  colFilters: {},
};

function loadLive(): boolean {
  return getJSON<boolean>(LS_LIVE_KEY, false);
}

export interface TokensPageDetailChartArgs {
  detail: TokenDetailRecord | null;
  loading: boolean;
  error: string | null;
  mint: string;
}

export function TokensPage({
  /** Lab injects chart + metric panes; live keeps the default trade chart. */
  renderDetailChart,
}: {
  renderDetailChart?: (args: TokensPageDetailChartArgs) => ReactNode;
} = {}) {
  const dispatch = useDispatch<AppDispatch>();
  const [searchParams, setSearchParams] = useSearchParams();
  const mintFromUrl = searchParams.get('mint');

  // Built once and held stable: the rate-dependent cells read the unit/USD-rate
  // from context themselves (see priceCells), so a rate tick no longer rebuilds
  // every column def and re-renders the whole grid — only the price cells update.
  const columns = useMemo(() => tokenColumns(), []);

  const [live, setLive] = useState(loadLive);
  const [trackedOnly, setTrackedOnly] = useState(false);
  const [showAdvancedFilters, setShowAdvancedFilters] = useState(false);
  const [filters, setFilters] = useState<TokenFilters>(loadStoredTokenFilters);
  const [selectedMint, setSelectedMint] = useState<string | null>(mintFromUrl);
  // View-state emitted by the DataTable (page/sort/search/col-filters). The
  // backend does the filtering/sorting/paging; we just forward it + the global
  // `filters` panel as query args.
  const [tableQuery, setTableQuery] = useState<TableQuery>(INITIAL_QUERY);

  // Selected project timezone — sent so datetime-range filters normalize from
  // picker wall-clock to the exact UTC instant at the query boundary.
  const { timezone } = useTimezone();

  // Sync `?mint=` ↔ selection (deep link from Metric panes redirect / external).
  useEffect(() => {
    if (mintFromUrl && mintFromUrl !== selectedMint) {
      setSelectedMint(mintFromUrl);
    }
  }, [mintFromUrl]); // eslint-disable-line react-hooks/exhaustive-deps -- seed from URL only

  const selectMint = useCallback(
    (mint: string | null) => {
      setSelectedMint(mint);
      setSearchParams(
        (prev) => {
          const next = new URLSearchParams(prev);
          if (mint) next.set('mint', mint);
          else next.delete('mint');
          return next;
        },
        { replace: true },
      );
    },
    [setSearchParams],
  );

  const detailRef = useRef<HTMLDivElement>(null);
  // Scroll detail into view on select (including `?mint=` deep-links).
  useEffect(() => {
    if (!selectedMint) return;
    const id = window.requestAnimationFrame(() => {
      detailRef.current?.scrollIntoView({ behavior: 'smooth', block: 'start' });
    });
    return () => window.cancelAnimationFrame(id);
  }, [selectedMint]);

  // The query args, shared by the live query and the adjacent-page prefetch
  // below so both hit identical cache keys.
  const queryArgs = useMemo(
    () => ({
      page: tableQuery.page,
      pageSize: tableQuery.pageSize,
      sortKeys: tableQuery.sortKeys,
      search: tableQuery.search,
      colFilters: tableQuery.colFilters,
      // The mint-set paste box (TokenTable) rides here so it reaches the backend
      // `filters` map as the `in`-on-`mint` op.
      structuredFilters: tableQuery.structuredFilters,
      filters,
      timezone,
      trackedOnly,
    }),
    [tableQuery, filters, timezone, trackedOnly],
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
    // SSE drives the live view: `trade_executed` patches row stats; `token_created`
    // pulls new rows. This poll is a slow safety-net (default 90s) for sort
    // re-apply + missed frames — not the primary tick.
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
  // Live, cache-tracked subset of `total` (≤ total) — shown beside it so the
  // page reports both the whole universe and the actively-tracked count.
  const tracked = tokensData?.tracked ?? 0;
  const error = apiErrorMessage(tokensError, 'Failed to load tokens');

  // Warm the adjacent pages so forward/back paging resolves from cache instead
  // of a fresh round-trip. The short `keepUnusedDataFor` on `getTokensPage`
  // keeps these entries alive long enough for the click that follows.
  const prefetchPage = sharedApi.usePrefetch('getTokensPage');
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

  // Resets the table to page 1 when the global filter panel or tracked-only mode changes.
  const filtersResetKey = useMemo(() => JSON.stringify({ filters, trackedOnly }), [filters, trackedOnly]);
  const filterCount = activeFilterCount(filters);
  // Whether any reduction is active — drives the "matched" vs "total" badge.
  // Unfiltered, `total` is the whole DB-backed token universe (not just the
  // cache-tracked subset); with any reduction it's the filtered count.
  const anyActive =
    filterCount > 0 ||
    !!tableQuery.search ||
    Object.values(tableQuery.colFilters).some(Boolean) ||
    Object.keys(tableQuery.structuredFilters ?? {}).length > 0;

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
    setJSON(LS_LIVE_KEY, live);
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
        sharedApi.util.updateQueryData('getTokensPage', queryArgsRef.current, (draft) => {
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
        if (t.live && typeof t.live === 'object' && visibleMintsRef.current.has(t.mint_address)) {
          pending.set(t.mint_address, t.live);
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

  return (
    <div>
      <PageHeader
        title="Tokens"
        description={
          renderDetailChart
            ? 'Universe · select a row for chart + metric panes'
            : 'Universe · select a row for detail + chart'
        }
        actions={
          <>
            <Badge
              variant={trackedOnly ? 'neutral' : 'primary'}
              className="cursor-pointer font-mono"
              onClick={() => setTrackedOnly(false)}
            >
              {total} {anyActive ? 'matched' : 'all'}
            </Badge>
            <Badge
              variant={trackedOnly ? 'primary' : 'neutral'}
              className="cursor-pointer font-mono"
              onClick={() => setTrackedOnly(true)}
            >
              {tracked} tracked
            </Badge>
            <StatusButton
              state={live ? 'live' : 'dead'}
              label={live ? 'STREAM ON' : 'STREAM OFF'}
              title="Table live updates only — not the header trading switch"
              onClick={() => setLive((v) => !v)}
              className={cn('px-4 py-0.5 text-xs', live && 'animate-pulse')}
            />
          </>
        }
      />

      <div className="mb-1.5 flex gap-1.5 justify-end">
        <IconButton
          variant="subtle"
          size="md"
          active={showAdvancedFilters || filterCount > 0}
          onClick={() => setShowAdvancedFilters((v) => !v)}
          title={
            filterCount > 0 ? `Advanced filters (${filterCount})` : 'Advanced filters'
          }
          aria-label={
            filterCount > 0 ? `Advanced filters (${filterCount})` : 'Advanced filters'
          }
        >
          <SettingsIcon />
        </IconButton>
      </div>

      {showAdvancedFilters && (
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
        <TokenTable
          columns={columns}
          rows={tokens}
          existingKeys={ALL_TOKEN_INFO_KEYS}
          rowKey={tokenRowKey}
          mintSetFilter
          charts
          selectedKey={selectedMint}
          onSelect={selectMint}
          serverSide
          serverTotal={total}
          onQueryChange={setTableQuery}
          loading={loading}
          resetKey={filtersResetKey}
          searchable
          colFilters
          colToggle
          hoverable
          tableId="tokens"
          emptyMessage="No tokens found"
        />
      )}

      {/* Detail lives BELOW the table (outside its x-scroll) so the chart
          sizes to page width. Row select highlights + fills this panel. */}
      {selectedMint && (
        <div
          ref={detailRef}
          id={`detail-${selectedMint}`}
          className="mt-3.5 scroll-mt-16 flex flex-col gap-2.5 rounded-lg border border-white/6 bg-bg-panel p-3"
        >
          <TokenDetailPanel detail={detail ?? null} loading={detailLoading} error={detailError} />
          {renderDetailChart ? (
            renderDetailChart({
              detail: detail ?? null,
              loading: detailLoading,
              error: detailError,
              mint: selectedMint,
            })
          ) : (
            <LazyTokenTradeChart tableId="token_detail_trades" detail={detail ?? null} />
          )}
        </div>
      )}
    </div>
  );
}
