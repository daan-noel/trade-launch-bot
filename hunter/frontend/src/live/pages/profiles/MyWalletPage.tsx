import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useDispatch } from 'react-redux';
import { Link, useSearchParams } from 'react-router-dom';
import type { FetchBaseQueryError } from '@reduxjs/toolkit/query';
import type { SerializedError } from '@reduxjs/toolkit';
import { Badge } from 'components/ui/Badge';
import { IconButton } from 'components/ui/IconButton';
import { BuyIcon, RefreshIcon, SellIcon, SpinnerIcon } from 'components/ui/icons';
import { Button } from 'components/ui/Button';
import { Input } from 'components/ui/Input';
import { InlineAlert, Modal } from 'components/ui/Modal';
import { PageHeader } from 'components/ui/PageHeader';
import { walletColumns, WALLET_KEYS } from '@live/components/wallet/walletColumns';
import { HoldingsSummaryBar } from '@live/components/wallet/HoldingsSummaryBar';
import { CashbackCard } from '@live/components/wallet/CashbackCard';
import { isCashHolding, isCashMint } from 'lib/assetKind';
import { TokenTable } from 'components/tokens/TokenTable';
import { TokenDetailPanel } from 'components/tokens/TokenDetailPanel';
import { LazyTokenTradeChart } from 'components/tokens/LazyTokenTradeChart';
import { tokenAmountColKeys, tokenNumericColKeys } from 'components/tokens/sharedTokenColumns';
import { DEFAULT_PAGE_SIZE } from 'components/table/Pagination';
import type { TableQuery } from 'components/table/types';
import { useServerTable } from 'hooks/useServerTable';
import { toTableRequest, type TableRequestBody } from 'services/tableRequest';
import {
  fetchHoldingsPage,
  fetchHoldingsSummary,
  type HoldingsTableSummary,
} from 'services/api';
import { connectTradeStream } from 'services/sse';
import { apiErrorMessage, useGetTokenDetailQuery } from 'store/apiSlice';
import { useGetProfilesQuery } from 'store/sharedEndpoints';
import { useUsdRate } from 'context/PriceUnitContext';
import { useMintTradeStream } from 'hooks/useMintTradeStream';
import {
  liveTradeSpotSolPerRaw,
  spotSolPerRawToUsd,
  valueSolAtSpot,
} from 'lib/liveMark';
import {
  liveApi,
  useGetWalletPricesQuery,
  useManualBuyPositionMutation,
  useSellTokenMutation,
} from '@live/store/liveEndpoints';
import { onPortfolioBagRefresh } from '@live/lib/portfolioBagRefresh';
import type { AppDispatch } from '@live/store';
import type { LiveTrade, ManagedBy, WalletHolding } from 'types';

/** SSE tip overlay for Price/Value/PnL until the next bag scan clears it. */
interface MarkTip {
  price_usd: number | null;
  value_usd: number | null;
  value_sol: number | null;
  unrealized_pnl_sol: number | null;
  unrealized_pnl_pct: number | null;
}

/** Row-triggered buy dialog. Free-text manual trading moved to the Console —
 *  the old header modals (with their broken `mint` key, M4) are gone. Posts
 *  through the same `manual-buy` position path as Console (a full tracked
 *  position, not the old fire-and-forget wallet buy) — no token-program-id or
 *  per-trade slippage override, matching that path's contract. */
interface BuyDialog {
  mint_address: string;
  solInput: string;
}

/// Below-this USD value a holding is treated as dust and hidden when the toggle is on.
const DUST_USD = 1;

const INITIAL_QUERY: TableQuery = {
  page: 1,
  pageSize: DEFAULT_PAGE_SIZE,
  sortKeys: [],
  search: '',
  colFilters: {},
};

export function MyWalletPage() {
  const dispatch = useDispatch<AppDispatch>();
  const [manualBuy] = useManualBuyPositionMutation();
  const [sellToken] = useSellTokenMutation();
  const [searchParams, setSearchParams] = useSearchParams();
  const mintFromUrl = searchParams.get('mint');

  // Server-side table view-state (paging/sort/filter). `TokenTable` emits the
  // query (already folding in the mint-set filter) and we serialize it to the
  // unified request body.
  const [query, setQuery] = useState<TableQuery>(INITIAL_QUERY);
  const [hideDust, setHideDust] = useState(false);

  // Master-detail: selected holding → detail panel + live trade chart below.
  // Cash (USDC) is not selectable — no useful meme tape / detail.
  const [selectedMint, setSelectedMint] = useState<string | null>(() =>
    mintFromUrl && !isCashMint(mintFromUrl) ? mintFromUrl : null,
  );
  const detailRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!mintFromUrl) return;
    if (isCashMint(mintFromUrl)) return;
    if (mintFromUrl !== selectedMint) setSelectedMint(mintFromUrl);
  }, [mintFromUrl]); // eslint-disable-line react-hooks/exhaustive-deps -- seed from URL only

  const selectMint = useCallback(
    (mint: string | null) => {
      if (mint && isCashMint(mint)) return;
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

  useEffect(() => {
    if (!selectedMint) return;
    detailRef.current?.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
  }, [selectedMint]);

  const {
    data: detail,
    isFetching: detailLoading,
    error: detailErrorRaw,
  } = useGetTokenDetailQuery(selectedMint ?? '', { skip: !selectedMint });
  const detailError = selectedMint
    ? apiErrorMessage(detailErrorRaw, 'Failed to load detail')
    : null;

  // Numeric-filtering keys must include the appended token-info columns so
  // `>5`/`1..10` on any column lowers to a structured op server-side.
  const baseColumnsForKeys = useMemo(
    () => walletColumns({ onBuy: () => {}, onSell: () => {}, sellingMint: null }),
    [],
  );
  const numericCols = useMemo(
    () => tokenNumericColKeys(baseColumnsForKeys),
    [baseColumnsForKeys],
  );
  const amountCols = useMemo(
    () => tokenAmountColKeys(baseColumnsForKeys),
    [baseColumnsForKeys],
  );

  // Dust hiding is a server-side filter on the scan-time value (`value_usd ≥ $1`),
  // so paging stays correct (full pages) and the summary agrees with the table.
  // Dust is injected via `structuredFilters` (already USD storage) so PriceUnit
  // conversion in `toTableRequest` does not rewrite it.
  const tableBody = useMemo<TableRequestBody>(() => {
    const structuredFilters = {
      ...query.structuredFilters,
      ...(hideDust ? { value_usd: { op: 'gte' as const, val: DUST_USD } } : {}),
    };
    return toTableRequest({ ...query, structuredFilters }, numericCols, {
      amountCols,
    });
  }, [query, hideDust, numericCols, amountCols]);

  // `fresh` (post-trade) busts the server scan cache for the next page fetch only;
  // the summary reads the freshly-warmed cache. Reset after each read.
  const freshRef = useRef(false);
  const fetchPage = useCallback((body: unknown, signal: AbortSignal) => {
    const fresh = freshRef.current;
    freshRef.current = false;
    return fetchHoldingsPage(body as TableRequestBody, signal, fresh);
  }, []);

  const {
    items,
    total,
    loading,
    error: tableError,
    reload,
  } = useServerTable<WalletHolding>(true, tableBody, fetchPage);

  // Summary bar totals over the whole *filtered* population (server-computed) —
  // refetched only when the filter-relevant body changes or after a trade.
  const [summary, setSummary] = useState<HoldingsTableSummary | null>(null);
  const [summaryNonce, setSummaryNonce] = useState(0);
  const summaryBody = useMemo<TableRequestBody>(
    () => ({
      pagination: { page: 1, pageSize: 1000 },
      sorting: [],
      search: tableBody.search,
      filters: tableBody.filters,
    }),
    [tableBody.search, tableBody.filters],
  );
  const summaryKey = useMemo(() => JSON.stringify(summaryBody), [summaryBody]);
  const refreshSummary = useCallback(() => setSummaryNonce((n) => n + 1), []);
  useEffect(() => {
    const ctrl = new AbortController();
    fetchHoldingsSummary(summaryBody, ctrl.signal)
      .then((s) => {
        if (!ctrl.signal.aborted) setSummary(s);
      })
      .catch(() => {
        /* non-blocking; keep last-known summary */
      });
    return () => ctrl.abort();
    // `summaryBody` is captured via `summaryKey`; refetch on filter change or trade.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [summaryKey, summaryNonce]);

  // Jupiter oracle for the CURRENT page's mints (liquidity / 24h / cold marks).
  // No interval — SSE tips live Value/Price; refetch on bag refresh / focus below.
  const pageMints = useMemo(() => items.map((h) => h.mint_address).sort(), [items]);
  const { data: prices, refetch: refetchPrices } = useGetWalletPricesQuery(pageMints, {
    skip: pageMints.length === 0,
  });

  const { usdRate } = useUsdRate();
  const usdRateRef = useRef(usdRate);
  usdRateRef.current = usdRate;
  const itemsRef = useRef(items);
  itemsRef.current = items;

  const pageMintSetRef = useRef<Set<string>>(new Set());
  pageMintSetRef.current = new Set(pageMints);

  const [tips, setTips] = useState<Record<string, MarkTip>>({});

  useMintTradeStream(pageMintSetRef, (batch) => {
    const rate = usdRateRef.current;
    setTips((prev) => {
      let next: Record<string, MarkTip> | null = null;
      for (const t of batch) {
        const h = itemsRef.current.find((x) => x.mint_address === t.mint_address);
        if (!h || isCashHolding(h)) continue;
        const spot = liveTradeSpotSolPerRaw(t);
        if (spot == null) continue;
        const valueSol = valueSolAtSpot(spot, h.amount);
        const priceUsd =
          rate != null ? spotSolPerRawToUsd(spot, h.decimals, rate) : null;
        const tip: MarkTip = {
          price_usd: priceUsd,
          value_usd: priceUsd != null ? priceUsd * h.ui_amount : null,
          value_sol: valueSol,
          unrealized_pnl_sol:
            valueSol != null && h.cost_basis_sol != null
              ? valueSol - h.cost_basis_sol
              : null,
          unrealized_pnl_pct:
            valueSol != null && h.cost_basis_sol != null && h.cost_basis_sol > 0
              ? ((valueSol - h.cost_basis_sol) / h.cost_basis_sol) * 100
              : null,
        };
        if (!next) next = { ...prev };
        next[t.mint_address] = tip;
      }
      return next ?? prev;
    });
  }, 250);

  // Overlay Jupiter + SSE tip onto page rows for DISPLAY only (server
  // sort/filter/dust use the scan-time snapshot). Tip wins on price/value/PnL;
  // Jupiter still supplies liquidity / 24h / created_at. Cash keeps face marks.
  const priced = useMemo(
    () =>
      items.map((h) => {
        if (isCashHolding(h)) return h;
        const p = prices?.[h.mint_address];
        const tip = tips[h.mint_address];
        if (!p && !tip) return h;
        const priceUsd = tip?.price_usd ?? p?.price_usd ?? h.price_usd;
        return {
          ...h,
          price_usd: priceUsd,
          value_usd:
            tip?.value_usd ??
            (priceUsd != null ? priceUsd * h.ui_amount : h.value_usd),
          value_sol: tip?.value_sol ?? h.value_sol,
          unrealized_pnl_sol: tip?.unrealized_pnl_sol ?? h.unrealized_pnl_sol,
          unrealized_pnl_pct: tip?.unrealized_pnl_pct ?? h.unrealized_pnl_pct,
          liquidity: p?.liquidity ?? h.liquidity,
          price_change_24h: p?.price_change_24h ?? h.price_change_24h,
          token_created_at: p?.token_created_at ?? h.token_created_at,
        };
      }),
    [items, prices, tips],
  );
  const rowByMint = useMemo(() => new Map(priced.map((r) => [r.mint_address, r])), [priced]);

  const [actionError, setActionError] = useState<string | null>(null);
  const [actionSuccess, setActionSuccess] = useState<string | null>(null);
  const [sellingMint, setSellingMint] = useState<string | null>(null);
  const [buyDialog, setBuyDialog] = useState<BuyDialog | null>(null);
  // A pending manual sell held back for confirmation because a live strategy
  // manages this bag — selling manually can race the bot's own exit (double-sell).
  const [pendingSell, setPendingSell] = useState<{
    mint_address: string;
    tokenAccount?: string;
    slippageBps?: number;
    prevAmount?: number;
    managedBy: ManagedBy;
  } | null>(null);

  const error = tableError;

  // Manual refresh (button / post-trade confirm): reload table + nudge Home tags.
  // Bag-changing SSE is owned by `usePortfolioRealtime` → `onPortfolioBagRefresh`.
  const refreshAll = useCallback(() => {
    freshRef.current = true;
    setTips({});
    reload();
    refreshSummary();
    void refetchPrices();
    dispatch(liveApi.util.invalidateTags(['WalletHoldings']));
  }, [reload, refreshSummary, refetchPrices, dispatch]);

  const { data: profiles } = useGetProfilesQuery();
  const mineWalletsRef = useRef<Set<string>>(new Set());
  mineWalletsRef.current = new Set(
    (profiles ?? [])
      .filter((p) => p.profile_type === 'mine')
      .flatMap((p) => p.wallets.map((w) => w.address)),
  );

  // Imperative table isn't RTK-tagged — subscribe to the one portfolio bag bus
  // (no second SSE filter). Do not re-invalidate tags here (already done).
  useEffect(() => {
    return onPortfolioBagRefresh(() => {
      freshRef.current = true;
      setTips({});
      reload();
      refreshSummary();
      void refetchPrices();
    });
  }, [reload, refreshSummary, refetchPrices]);

  // Quiet bags / Jupiter oracle fields: refresh when the tab becomes visible.
  useEffect(() => {
    const onVis = () => {
      if (document.visibilityState === 'visible' && pageMints.length > 0) {
        void refetchPrices();
      }
    };
    document.addEventListener('visibilitychange', onVis);
    return () => document.removeEventListener('visibilitychange', onVis);
  }, [refetchPrices, pageMints.length]);

  // After a manual buy/sell submit: wait for our fill on the ingest feed (or a
  // short timeout), then refresh — no RPC polling. `prevAmount` kept for call-
  // site compatibility; balance change is observed via the feed + scan refresh.
  const confirmTrade = useCallback(
    (mint: string, _prevAmount: number | undefined, label: string) => {
      let settled = false;
      const finish = (msg: string) => {
        if (settled) return;
        settled = true;
        tradeH.close();
        window.clearTimeout(timeout);
        refreshAll();
        setActionSuccess(msg);
      };
      const tradeH = connectTradeStream((raw) => {
        try {
          const t = JSON.parse(raw) as LiveTrade;
          if (t.mint_address !== mint) return;
          // Require a configured `mine` wallet match — otherwise fall through to timeout
          // (don't treat the next stranger fill on this mint as "our" confirm).
          if (
            mineWalletsRef.current.size === 0 ||
            !mineWalletsRef.current.has(t.wallet)
          ) {
            return;
          }
          finish(`${label} successful — holdings updated.`);
        } catch {
          /* ignore */
        }
      });
      const timeout = window.setTimeout(() => {
        finish(`${label} submitted — refreshed holdings.`);
      }, 8_000);
    },
    [refreshAll],
  );

  // Shared "sell all" submit for both the row button and the manual dialog. The
  // backend always sells the full live balance, so no amount is sent; the token
  // account is passed only as a hint when known (row sells) to skip a wallet
  // scan. `prevAmount` is the pre-sell balance so confirmTrade can detect the drop.
  const runSell = useCallback(
    async (mint: string, tokenAccount?: string, slippageBps?: number, prevAmount?: number) => {
      setSellingMint(mint);
      setActionError(null);
      setActionSuccess(null);

      try {
        await sellToken({
          mint_address: mint,
          ...(tokenAccount ? { token_account: tokenAccount } : {}),
          ...(slippageBps !== undefined ? { slippage_bps: slippageBps } : {}),
        }).unwrap();
        setActionSuccess('Sell submitted — confirming on-chain…');
        void confirmTrade(mint, prevAmount, 'Sell');
      } catch (e) {
        setActionError(
          `Sell failed: ${apiErrorMessage(e as FetchBaseQueryError | SerializedError) ?? 'unknown error'}`,
        );
      } finally {
        setSellingMint(null);
      }
    },
    [sellToken, confirmTrade],
  );

  // Gate every manual sell on the bot-managed check: if a live strategy holds
  // this bag, hold the sell for an explicit confirm (double-sell interlock);
  // otherwise sell straight through.
  const requestSell = useCallback(
    (
      mint: string,
      opts: { tokenAccount?: string; slippageBps?: number; prevAmount?: number; managedBy?: ManagedBy | null },
    ) => {
      if (opts.managedBy) {
        setPendingSell({
          mint_address: mint,
          tokenAccount: opts.tokenAccount,
          slippageBps: opts.slippageBps,
          prevAmount: opts.prevAmount,
          managedBy: opts.managedBy,
        });
        return;
      }
      void runSell(mint, opts.tokenAccount, opts.slippageBps, opts.prevAmount);
    },
    [runSell],
  );

  // Row "Sell All": the row is on the current page, so its composed holding
  // (managed_by / token account / balance) is in hand.
  const handleSell = useCallback(
    (mint: string) => {
      const row = rowByMint.get(mint);
      if (!row) {
        setActionError('Token account not found for mint');
        return;
      }
      requestSell(mint, {
        tokenAccount: row.token_account,
        prevAmount: row.amount,
        managedBy: row.managed_by,
      });
    },
    [rowByMint, requestSell],
  );

  // Proceed with a sell the user confirmed despite the bot-managed warning.
  const confirmPendingSell = useCallback(() => {
    if (!pendingSell) return;
    const { mint_address: mint, tokenAccount, slippageBps, prevAmount } = pendingSell;
    setPendingSell(null);
    void runSell(mint, tokenAccount, slippageBps, prevAmount);
  }, [pendingSell, runSell]);

  const handleBuyOpen = useCallback((mint: string) => {
    setBuyDialog({ mint_address: mint, solInput: '0.001' });
  }, []);

  const handleBuySubmit = useCallback(async () => {
    if (!buyDialog) return;
    const mint = buyDialog.mint_address.trim();
    if (!mint) {
      setActionError('Enter a mint address');
      return;
    }
    const solAmount = parseFloat(buyDialog.solInput.trim());
    if (!Number.isFinite(solAmount) || solAmount <= 0) {
      setActionError('Enter a valid SOL amount > 0');
      return;
    }

    setActionError(null);
    setActionSuccess(null);
    setBuyDialog(null);

    try {
      const res = await manualBuy({ mint_address: mint, amount_sol: solAmount }).unwrap();
      setActionSuccess(`Buy submitted — position ${res.position_id.slice(0, 8)}… tracked in Console`);
    } catch (e) {
      setActionError(
        `Buy failed: ${apiErrorMessage(e as FetchBaseQueryError | SerializedError) ?? 'unknown error'}`,
      );
    }
  }, [buyDialog, manualBuy]);

  const columns = useMemo(
    () =>
      walletColumns({
        onBuy: handleBuyOpen,
        onSell: handleSell,
        sellingMint,
      }),
    [handleBuyOpen, handleSell, sellingMint],
  );

  const buyTitle = buyDialog
    ? `Buy ${rowByMint.get(buyDialog.mint_address)?.symbol ?? buyDialog.mint_address}`
    : '';

  return (
    <div>
      <PageHeader
        title="Wallet"
        description="Balances · positions · execute"
        actions={
          <>
            <Badge variant="primary" className="font-mono">
              {total} positions
            </Badge>
            <IconButton
              variant="subtle"
              size="md"
              onClick={refreshAll}
              disabled={loading}
              title={loading ? 'Loading…' : 'Refresh'}
              aria-label={loading ? 'Loading…' : 'Refresh'}
            >
              {loading ? <SpinnerIcon /> : <RefreshIcon />}
            </IconButton>
            <Button
              variant={hideDust ? 'primary' : 'subtle'}
              size="sm"
              onClick={() => setHideDust((v) => !v)}
            >
              {hideDust ? '✓ ' : ''}Hide dust
            </Button>
            <Link
              to="/console"
              className="inline-flex min-h-8 items-center justify-center rounded-md border border-primary bg-primary/15 px-3 text-xs font-semibold text-primary transition hover:bg-primary/25"
            >
              Console →
            </Link>
          </>
        }
      />

      {summary && <HoldingsSummaryBar summary={summary} />}

      <CashbackCard />

      {error && <InlineAlert variant="error">{error}</InlineAlert>}
      {actionError && <InlineAlert variant="error">{actionError}</InlineAlert>}
      {actionSuccess && <InlineAlert variant="success">{actionSuccess}</InlineAlert>}

      <TokenTable
        columns={columns}
        rows={priced}
        existingKeys={WALLET_KEYS}
        mintSetFilter
        rowKey={(r) => r.mint_address}
        selectedKey={selectedMint}
        onSelect={selectMint}
        serverSide
        serverTotal={total}
        onQueryChange={setQuery}
        resetKey={hideDust ? 'dust' : 'all'}
        loading={loading}
        searchable
        colFilters
        colToggle
        hoverable
        tableId="wallet"
        emptyMessage="No meme positions in wallet."
      />

      {/* Detail below the table (outside x-scroll) so the chart is full-width.
          Live ticks: TokenTradeChart → useWatchTokenTradesLive → ingest SSE. */}
      {selectedMint && (
        <div
          ref={detailRef}
          id={`wallet-detail-${selectedMint}`}
          className="mt-3.5 scroll-mt-16 flex flex-col gap-2.5 rounded-lg border border-white/6 bg-bg-panel p-3"
        >
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-sm font-bold text-text">
              {detail?.symbol ??
                rowByMint.get(selectedMint)?.symbol ??
                selectedMint.slice(0, 8)}
            </span>
            <span className="font-mono text-[11px] text-text-dim">{selectedMint}</span>
            <div className="grow" />
            <Link
              to={`/console?mint=${encodeURIComponent(selectedMint)}`}
              className="rounded border border-white/15 bg-white/4 px-2 py-0.5 text-[11px] font-semibold text-accent hover:border-primary/40 hover:text-primary"
            >
              Open in Console →
            </Link>
            <Button variant="ghost" size="sm" onClick={() => selectMint(null)}>
              Close
            </Button>
          </div>
          <TokenDetailPanel
            detail={detail ?? null}
            loading={detailLoading}
            error={detailError}
          />
          {detail && (
            <LazyTokenTradeChart
              key={detail.mint_address}
              tableId="wallet_trades"
              detail={detail}
            />
          )}
        </div>
      )}

      <Modal
        title={buyTitle}
        open={buyDialog != null}
        onClose={() => setBuyDialog(null)}
      >
        {buyDialog && (
          <>
            <p className="mb-4 text-xs text-text-mid">Mint: {buyDialog.mint_address}</p>
            <label className="mb-4 flex flex-col gap-1.5">
              <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">
                SOL Amount
              </span>
              <Input
                type="number"
                fieldSize="md"
                min={0.001}
                step={0.001}
                value={buyDialog.solInput}
                onChange={(e) =>
                  setBuyDialog((d) => (d ? { ...d, solInput: e.target.value } : d))
                }
                className="focus:shadow-[0_0_0_2px_rgba(19,206,175,0.15)]"
              />
            </label>
            <div className="flex items-center justify-end gap-2.5">
              <Button variant="ghost" onClick={() => setBuyDialog(null)}>
                Cancel
              </Button>
              <IconButton
                variant="primary"
                size="lg"
                onClick={handleBuySubmit}
                label="Confirm Buy"
                title="Confirm Buy"
              >
                <BuyIcon />
              </IconButton>
            </div>
          </>
        )}
      </Modal>

      <Modal
        title="⚠ Bot-managed position"
        open={pendingSell != null}
        onClose={() => setPendingSell(null)}
      >
        {pendingSell && (
          <>
            <InlineAlert variant="warning">
              <strong>{pendingSell.managedBy.rule_name ?? 'A live strategy'}</strong> is
              managing this bag (status: {pendingSell.managedBy.status}). Selling
              manually can race the bot's own exit and double-sell. Consider stopping
              the rule first. Sell anyway?
            </InlineAlert>
            <div className="mt-4 flex items-center justify-end gap-2.5">
              <Button variant="ghost" onClick={() => setPendingSell(null)}>
                Cancel
              </Button>
              <IconButton
                variant="danger"
                size="lg"
                onClick={confirmPendingSell}
                label="Sell Anyway"
                title="Sell Anyway"
              >
                <SellIcon />
              </IconButton>
            </div>
          </>
        )}
      </Modal>
    </div>
  );
}
