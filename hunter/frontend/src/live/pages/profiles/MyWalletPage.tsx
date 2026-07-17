import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useDispatch } from 'react-redux';
import { Link } from 'react-router-dom';
import type { FetchBaseQueryError } from '@reduxjs/toolkit/query';
import type { SerializedError } from '@reduxjs/toolkit';
import { Badge } from 'components/ui/Badge';
import { Button } from 'components/ui/Button';
import { Input } from 'components/ui/Input';
import { InlineAlert, Modal } from 'components/ui/Modal';
import { walletColumns, WALLET_KEYS } from '@live/components/wallet/walletColumns';
import { HoldingsSummaryBar } from '@live/components/wallet/HoldingsSummaryBar';
import { CashbackCard } from '@live/components/wallet/CashbackCard';
import { TokenTable } from 'components/tokens/TokenTable';
import { tokenNumericColKeys } from 'components/tokens/sharedTokenColumns';
import { DEFAULT_PAGE_SIZE } from 'components/table/Pagination';
import type { TableQuery } from 'components/table/types';
import { useServerTable } from 'hooks/useServerTable';
import { toTableRequest, type TableRequestBody } from 'services/tableRequest';
import {
  fetchHoldingsPage,
  fetchHoldingsSummary,
  fetchHoldingByMint,
  type HoldingsTableSummary,
} from 'services/api';
import { apiErrorMessage } from 'store/apiSlice';
import {
  liveApi,
  useGetWalletPricesQuery,
  useBuyTokenMutation,
  useSellTokenMutation,
} from '@live/store/liveEndpoints';
import type { AppDispatch } from '@live/store';
import type { ManagedBy, WalletHolding } from 'types';
import { parseSlippageBps } from '@live/lib/slippage';

interface BuyDialog {
  mint_address: string;
  /// Known for row-triggered buys; undefined for manual buys (backend resolves on-chain).
  tokenProgramId?: string;
  solInput: string;
  /// Slippage tolerance as a percent string; blank = use the global default.
  slippageInput: string;
  manual: boolean;
}

interface SellDialog {
  /// Mint to sell; entered by the user (manual sell sells the full balance).
  mint_address: string;
  /// Slippage tolerance as a percent string; blank = use the global default.
  slippageInput: string;
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
  const [buyToken] = useBuyTokenMutation();
  const [sellToken] = useSellTokenMutation();

  // Server-side table view-state (paging/sort/filter). `TokenTable` emits the
  // query (already folding in the mint-set filter) and we serialize it to the
  // unified request body.
  const [query, setQuery] = useState<TableQuery>(INITIAL_QUERY);
  const [hideDust, setHideDust] = useState(false);

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

  // Dust hiding is a server-side filter on the scan-time value (`value_usd ≥ $1`),
  // so paging stays correct (full pages) and the summary agrees with the table.
  const tableBody = useMemo<TableRequestBody>(() => {
    const structuredFilters = {
      ...query.structuredFilters,
      ...(hideDust ? { value_usd: { op: 'gte' as const, val: DUST_USD } } : {}),
    };
    return toTableRequest({ ...query, structuredFilters }, numericCols);
  }, [query, hideDust, numericCols]);

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

  // Live prices for the CURRENT page's mints, decoupled from the (slow, RPC-bound)
  // holdings scan and polled on a short interval, so Value/Price tick without a
  // server round-trip. Keyed by the sorted page mints; paused while unfocused.
  const pageMints = useMemo(() => items.map((h) => h.mint_address).sort(), [items]);
  const { data: prices } = useGetWalletPricesQuery(pageMints, {
    skip: pageMints.length === 0,
    pollingInterval: 20000,
    skipPollingIfUnfocused: true,
  });

  // Overlay the latest polled prices onto the current page rows for DISPLAY only
  // (server sort/filter/dust use the scan-time snapshot). Falls back to the
  // scan-time price until the first poll lands.
  const priced = useMemo(
    () =>
      items.map((h) => {
        const p = prices?.[h.mint_address];
        if (!p) return h;
        return {
          ...h,
          price_usd: p.price_usd,
          value_usd: p.price_usd != null ? p.price_usd * h.ui_amount : null,
          liquidity: p.liquidity,
          price_change_24h: p.price_change_24h,
          token_created_at: p.token_created_at,
        };
      }),
    [items, prices],
  );
  const rowByMint = useMemo(() => new Map(priced.map((r) => [r.mint_address, r])), [priced]);

  const [actionError, setActionError] = useState<string | null>(null);
  const [actionSuccess, setActionSuccess] = useState<string | null>(null);
  const [sellingMint, setSellingMint] = useState<string | null>(null);
  const [buyDialog, setBuyDialog] = useState<BuyDialog | null>(null);
  const [sellDialog, setSellDialog] = useState<SellDialog | null>(null);
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

  // Refresh the server-paged table + summary + Home widgets from the fresh scan.
  const refreshAll = useCallback(() => {
    freshRef.current = true;
    reload();
    refreshSummary();
    dispatch(liveApi.util.invalidateTags(['WalletHoldings']));
  }, [reload, refreshSummary, dispatch]);

  // After a confirmed trade the wallet's new on-chain balance can lag the RPC
  // read by a moment. Poll just the traded mint — one cheap RPC + price each
  // attempt — until its raw amount actually moves, then reload the current page
  // + summary from a fresh scan (no full wallet re-scan per attempt). If the
  // change never lands within the window, refresh anyway so we can't sit stale.
  const confirmTrade = useCallback(
    async (mint: string, prevAmount: number | undefined, label: string) => {
      for (let attempt = 0; attempt < 5; attempt += 1) {
        await new Promise((resolve) => setTimeout(resolve, 1500));
        const sub = dispatch(
          liveApi.endpoints.getWalletHolding.initiate(mint, { forceRefetch: true }),
        );
        try {
          const holding = await sub.unwrap();
          if ((holding?.amount ?? undefined) !== prevAmount) {
            refreshAll();
            setActionSuccess(`${label} successful — holdings updated.`);
            return;
          }
        } catch {
          // Transient RPC/Jupiter error during confirmation; keep retrying.
        } finally {
          sub.unsubscribe();
        }
      }
      refreshAll();
      setActionSuccess(
        `${label} confirmed. Balances took a moment to update on-chain — refreshed.`,
      );
    },
    [dispatch, refreshAll],
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

  const handleManualSellOpen = useCallback(() => {
    setSellDialog({ mint_address: '', slippageInput: '' });
  }, []);

  const handleSellSubmit = useCallback(async () => {
    if (!sellDialog) return;
    const mint = sellDialog.mint_address.trim();
    if (!mint) {
      setActionError('Enter a mint address');
      return;
    }
    const { bps: slippageBps, error: slipError } = parseSlippageBps(sellDialog.slippageInput);
    if (slipError) {
      setActionError(slipError);
      return;
    }

    // Resolve the composed holding (authoritative managed_by / token account /
    // balance) from the warm scan — the typed mint may not be on the current page.
    let holding: WalletHolding | null = null;
    try {
      holding = await fetchHoldingByMint(mint);
    } catch {
      /* fall through to the not-held guard */
    }
    if (!holding || !holding.amount) {
      setActionError('Wallet holds no balance of this mint');
      return;
    }

    setActionError(null);
    setActionSuccess(null);
    setSellDialog(null);
    requestSell(mint, {
      tokenAccount: holding.token_account,
      slippageBps,
      prevAmount: holding.amount,
      managedBy: holding.managed_by,
    });
  }, [sellDialog, requestSell]);

  const handleBuyOpen = useCallback((mint: string, tokenProgramId: string) => {
    setBuyDialog({ mint_address: mint, tokenProgramId, solInput: '0.001', slippageInput: '', manual: false });
  }, []);

  const handleManualBuyOpen = useCallback(() => {
    setBuyDialog({ mint_address: '', solInput: '0.001', slippageInput: '', manual: true });
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

    // Slippage is entered as a percent; blank = let the backend use the global
    // default. Convert to basis points (1% = 100 bps).
    const { bps: slippageBps, error: slipError } = parseSlippageBps(buyDialog.slippageInput);
    if (slipError) {
      setActionError(slipError);
      return;
    }

    // Snapshot the pre-buy balance (undefined for a token we don't hold yet) so
    // the confirmation can tell when the new tokens have landed. Read from the
    // current page if present, else the warm scan.
    const prevAmount =
      rowByMint.get(mint)?.amount ?? (await fetchHoldingByMint(mint).catch(() => null))?.amount;

    setActionError(null);
    setActionSuccess(null);
    setBuyDialog(null);

    try {
      await buyToken({
        mint_address: mint,
        amount_sol: solAmount,
        // Omit for manual buys — the backend resolves the token program on-chain.
        ...(buyDialog.tokenProgramId ? { token_program_id: buyDialog.tokenProgramId } : {}),
        ...(slippageBps !== undefined ? { slippage_bps: slippageBps } : {}),
      }).unwrap();
      setActionSuccess('Buy submitted — confirming on-chain…');
      void confirmTrade(mint, prevAmount ?? undefined, 'Buy');
    } catch (e) {
      setActionError(
        `Buy failed: ${apiErrorMessage(e as FetchBaseQueryError | SerializedError) ?? 'unknown error'}`,
      );
    }
  }, [buyDialog, rowByMint, buyToken, confirmTrade]);

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
    ? buyDialog.manual
      ? 'Manual Buy'
      : `Buy ${rowByMint.get(buyDialog.mint_address)?.symbol ?? buyDialog.mint_address}`
    : '';

  return (
    <div>
      <div className="mb-3.5 flex flex-wrap items-center gap-3">
        <h1 className="text-lg font-extrabold text-text">Wallet</h1>
        <span className="text-sm text-text-mid">
          Bag overview · row Buy/Sell for quick fills · Trade desk for mint-first
        </span>
        <Badge variant="primary" className="font-mono">
          {total} tokens
        </Badge>
        <Button variant="subtle" size="sm" onClick={refreshAll} disabled={loading}>
          {loading ? 'Loading…' : '↻ Refresh'}
        </Button>
        <Button
          variant={hideDust ? 'primary' : 'subtle'}
          size="sm"
          onClick={() => setHideDust((v) => !v)}
        >
          {hideDust ? '✓ ' : ''}Hide dust
        </Button>
        <div className="flex-grow" />
        <Button variant="subtle" size="sm" onClick={handleManualBuyOpen}>
          + Manual Buy
        </Button>
        <Button variant="subtle" size="sm" onClick={handleManualSellOpen}>
          − Manual Sell
        </Button>
        <Link
          to="/trade"
          className="inline-flex min-h-8 items-center justify-center rounded-md border border-primary bg-primary/15 px-3 text-[12px] font-semibold text-primary transition hover:bg-primary/25"
        >
          Trade desk →
        </Link>
      </div>

      {summary && summary.positions > 0 && <HoldingsSummaryBar summary={summary} />}

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
        emptyMessage="No token holdings found in wallet."
        selectable={false}
      />

      <Modal
        title={buyTitle}
        open={buyDialog != null}
        onClose={() => setBuyDialog(null)}
      >
        {buyDialog && (
          <>
            {buyDialog.manual ? (
              <label className="mb-4 flex flex-col gap-1.5">
                <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">
                  Mint Address
                </span>
                <Input
                  type="text"
                  fieldSize="md"
                  placeholder="Token mint address"
                  value={buyDialog.mint_address}
                  onChange={(e) =>
                    setBuyDialog((d) => (d ? { ...d, mint: e.target.value } : d))
                  }
                  className="font-mono focus:shadow-[0_0_0_2px_rgba(19,206,175,0.15)]"
                />
              </label>
            ) : (
              <p className="mb-4 text-xs text-text-mid">Mint: {buyDialog.mint_address}</p>
            )}
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
            <label className="mb-4 flex flex-col gap-1.5">
              <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">
                Slippage % (optional)
              </span>
              <Input
                type="number"
                fieldSize="md"
                min={0}
                max={50}
                step={0.1}
                placeholder="Default"
                value={buyDialog.slippageInput}
                onChange={(e) =>
                  setBuyDialog((d) => (d ? { ...d, slippageInput: e.target.value } : d))
                }
                className="focus:shadow-[0_0_0_2px_rgba(19,206,175,0.15)]"
              />
            </label>
            <div className="flex items-center justify-end gap-2.5">
              <Button variant="ghost" onClick={() => setBuyDialog(null)}>
                Cancel
              </Button>
              <Button variant="primary" onClick={handleBuySubmit}>
                Confirm Buy
              </Button>
            </div>
          </>
        )}
      </Modal>

      <Modal
        title="Manual Sell"
        open={sellDialog != null}
        onClose={() => setSellDialog(null)}
      >
        {sellDialog && (
          <>
            <InlineAlert variant="warning">
              Sells the wallet's entire balance of this mint and closes the token
              account to reclaim rent.
            </InlineAlert>
            <label className="mb-4 mt-4 flex flex-col gap-1.5">
              <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">
                Mint Address
              </span>
              <Input
                type="text"
                fieldSize="md"
                placeholder="Token mint address"
                value={sellDialog.mint_address}
                onChange={(e) =>
                  setSellDialog((d) => (d ? { ...d, mint: e.target.value } : d))
                }
                className="font-mono focus:shadow-[0_0_0_2px_rgba(19,206,175,0.15)]"
              />
            </label>
            <label className="mb-4 flex flex-col gap-1.5">
              <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">
                Slippage % (optional)
              </span>
              <Input
                type="number"
                fieldSize="md"
                min={0}
                max={50}
                step={0.1}
                placeholder="Default"
                value={sellDialog.slippageInput}
                onChange={(e) =>
                  setSellDialog((d) => (d ? { ...d, slippageInput: e.target.value } : d))
                }
                className="focus:shadow-[0_0_0_2px_rgba(19,206,175,0.15)]"
              />
            </label>
            <div className="flex items-center justify-end gap-2.5">
              <Button variant="ghost" onClick={() => setSellDialog(null)}>
                Cancel
              </Button>
              <Button variant="danger" onClick={handleSellSubmit}>
                Sell All
              </Button>
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
              <Button variant="danger" onClick={confirmPendingSell}>
                Sell Anyway
              </Button>
            </div>
          </>
        )}
      </Modal>
    </div>
  );
}
