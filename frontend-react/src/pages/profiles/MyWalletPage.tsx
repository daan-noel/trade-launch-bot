import { useCallback, useMemo, useState } from 'react';
import { useDispatch } from 'react-redux';
import type { FetchBaseQueryError } from '@reduxjs/toolkit/query';
import type { SerializedError } from '@reduxjs/toolkit';
import { DataTable } from 'components/table/DataTable';
import { Badge } from 'components/ui/Badge';
import { Button } from 'components/ui/Button';
import { Input } from 'components/ui/Input';
import { InlineAlert, Modal } from 'components/ui/Modal';
import { walletColumns } from 'components/wallet/walletColumns';
import { CashbackCard } from 'components/wallet/CashbackCard';
import {
  apiSlice,
  apiErrorMessage,
  useGetWalletHoldingsQuery,
  useGetWalletPricesQuery,
  useBuyTokenMutation,
  useSellTokenMutation,
} from 'store/apiSlice';
import { useWalletPriceDisplay } from 'hooks/useWalletPriceDisplay';
import type { AppDispatch } from '../../store';

interface BuyDialog {
  mint: string;
  /// Known for row-triggered buys; undefined for manual buys (backend resolves on-chain).
  tokenProgramId?: string;
  solInput: string;
  /// Slippage tolerance as a percent string; blank = use the global default.
  slippageInput: string;
  manual: boolean;
}

export function MyWalletPage() {
  const dispatch = useDispatch<AppDispatch>();
  const price = useWalletPriceDisplay();
  // RTK Query owns the fetch, the cache, and StrictMode/dedup. Navigating back
  // to this page reuses the cached holdings (keepUnusedDataFor) instead of
  // re-scanning the chain; a manual trade refreshes a single row (see below).
  const {
    data: holdings = [],
    isLoading,
    isFetching,
    error: holdingsError,
    refetch,
  } = useGetWalletHoldingsQuery();
  const [buyToken] = useBuyTokenMutation();
  const [sellToken] = useSellTokenMutation();

  // Live prices are fetched separately from the (slow, RPC-bound) balances and
  // polled on a short interval, so the Value/Price columns tick without ever
  // re-scanning the wallet. Keyed by the sorted held mints for a stable cache
  // key; paused while the tab is unfocused.
  const mints = useMemo(() => holdings.map((h) => h.mint).sort(), [holdings]);
  const { data: prices } = useGetWalletPricesQuery(mints, {
    skip: mints.length === 0,
    pollingInterval: 20000,
    skipPollingIfUnfocused: true,
  });

  // Overlay the latest polled prices onto each balance row. Falls back to the
  // price the balances endpoint returned at load time until the first poll
  // lands (and for any mint Jupiter doesn't return).
  const rows = useMemo(
    () =>
      holdings.map((h) => {
        const p = prices?.[h.mint];
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
    [holdings, prices],
  );

  const [actionError, setActionError] = useState<string | null>(null);
  const [actionSuccess, setActionSuccess] = useState<string | null>(null);
  const [sellingMint, setSellingMint] = useState<string | null>(null);
  const [buyDialog, setBuyDialog] = useState<BuyDialog | null>(null);

  const error = apiErrorMessage(holdingsError);

  // After a confirmed trade the wallet's new on-chain balance can lag the RPC
  // read by a moment. Rather than re-fetching (and re-pricing) the entire
  // wallet repeatedly, poll just the traded mint — one cheap RPC + price each
  // attempt — until its raw amount actually moves (buy → up / new token
  // appears, sell → down), then patch that single row into the cached list.
  // No full wallet re-scan. If the change never lands within the window, fall
  // back to one authoritative refresh so the table can't sit on stale data.
  const confirmTrade = useCallback(
    async (mint: string, prevAmount: number | undefined, label: string) => {
      for (let attempt = 0; attempt < 5; attempt += 1) {
        await new Promise((resolve) => setTimeout(resolve, 1500));
        const sub = dispatch(
          apiSlice.endpoints.getWalletHolding.initiate(mint, { forceRefetch: true }),
        );
        try {
          const holding = await sub.unwrap();
          if ((holding?.amount ?? undefined) !== prevAmount) {
            dispatch(
              apiSlice.util.updateQueryData('getWalletHoldings', undefined, (draft) => {
                const idx = draft.findIndex((h) => h.mint === mint);
                if (holding) {
                  if (idx >= 0) draft[idx] = holding;
                  else draft.unshift(holding);
                } else if (idx >= 0) {
                  draft.splice(idx, 1);
                }
              }),
            );
            setActionSuccess(`${label} successful — holdings updated.`);
            return;
          }
        } catch {
          // Transient RPC/Jupiter error during confirmation; keep retrying.
        } finally {
          // Drop the imperative subscription so the single-mint cache entry
          // doesn't linger past its purpose.
          sub.unsubscribe();
        }
      }
      dispatch(apiSlice.util.invalidateTags(['WalletHoldings']));
      setActionSuccess(
        `${label} confirmed. Balances took a moment to update on-chain — refreshed.`,
      );
    },
    [dispatch],
  );

  const handleSell = useCallback(
    async (mint: string, tokenAmount: number) => {
      const holding = holdings.find((h) => h.mint === mint);
      if (!holding) {
        setActionError('Token account not found for mint');
        return;
      }

      setSellingMint(mint);
      setActionError(null);
      setActionSuccess(null);

      try {
        await sellToken({
          mint,
          token_amount: tokenAmount,
          token_account: holding.token_account,
        }).unwrap();
        setActionSuccess('Sell submitted — confirming on-chain…');
        void confirmTrade(mint, holding.amount, 'Sell');
      } catch (e) {
        setActionError(
          `Sell failed: ${apiErrorMessage(e as FetchBaseQueryError | SerializedError) ?? 'unknown error'}`,
        );
      } finally {
        setSellingMint(null);
      }
    },
    [holdings, sellToken, confirmTrade],
  );

  const handleBuyOpen = useCallback((mint: string, tokenProgramId: string) => {
    setBuyDialog({ mint, tokenProgramId, solInput: '0.001', slippageInput: '', manual: false });
  }, []);

  const handleManualBuyOpen = useCallback(() => {
    setBuyDialog({ mint: '', solInput: '0.001', slippageInput: '', manual: true });
  }, []);

  const handleBuySubmit = useCallback(async () => {
    if (!buyDialog) return;
    const mint = buyDialog.mint.trim();
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
    let slippageBps: number | undefined;
    const slipRaw = buyDialog.slippageInput.trim();
    if (slipRaw) {
      const slipPct = parseFloat(slipRaw);
      if (!Number.isFinite(slipPct) || slipPct < 0 || slipPct > 50) {
        setActionError('Enter a valid slippage % between 0 and 50');
        return;
      }
      slippageBps = Math.round(slipPct * 100);
    }

    // Snapshot the pre-buy balance (undefined for a token we don't hold yet) so
    // the confirmation can tell when the new tokens have landed.
    const prevAmount = holdings.find((h) => h.mint === mint)?.amount;

    setActionError(null);
    setActionSuccess(null);
    setBuyDialog(null);

    try {
      await buyToken({
        mint,
        sol_amount: solAmount,
        // Omit for manual buys — the backend resolves the token program on-chain.
        ...(buyDialog.tokenProgramId
          ? { token_program_id: buyDialog.tokenProgramId }
          : {}),
        ...(slippageBps !== undefined ? { slippage_bps: slippageBps } : {}),
      }).unwrap();
      setActionSuccess('Buy submitted — confirming on-chain…');
      void confirmTrade(mint, prevAmount, 'Buy');
    } catch (e) {
      setActionError(
        `Buy failed: ${apiErrorMessage(e as FetchBaseQueryError | SerializedError) ?? 'unknown error'}`,
      );
    }
  }, [buyDialog, holdings, buyToken, confirmTrade]);

  const columns = useMemo(
    () =>
      walletColumns(
        {
          onBuy: handleBuyOpen,
          onSell: handleSell,
          sellingMint,
        },
        price,
      ),
    [handleBuyOpen, handleSell, sellingMint, price],
  );

  const buyTitle = buyDialog
    ? buyDialog.manual
      ? 'Manual Buy'
      : `Buy ${holdings.find((h) => h.mint === buyDialog.mint)?.symbol ?? buyDialog.mint}`
    : '';

  return (
    <div>
      <div className="mb-3.5 flex flex-wrap items-center gap-3">
        <h2 className="text-lg font-extrabold text-text">Wallet Holdings</h2>
        <Badge variant="primary" className="font-mono">
          {holdings.length} tokens
        </Badge>
        <Button variant="subtle" size="sm" onClick={() => refetch()} disabled={isFetching}>
          {isFetching ? 'Loading…' : '↻ Refresh'}
        </Button>
        <div className='flex-grow' />
        <Button variant="primary" size="md" onClick={handleManualBuyOpen}>
          + Manual Buy
        </Button>
      </div>

      <CashbackCard />

      {error && <InlineAlert variant="error">{error}</InlineAlert>}
      {actionError && <InlineAlert variant="error">{actionError}</InlineAlert>}
      {actionSuccess && <InlineAlert variant="success">{actionSuccess}</InlineAlert>}

      {isLoading ? (
        <p className="py-10 text-center text-text-dim">Loading wallet holdings from Solana…</p>
      ) : (
        <DataTable
          columns={columns}
          rows={rows}
          rowKey={(r) => r.mint}
          defaultPageSize={25}
          pageSizeOptions={[25, 50, 100]}
          searchable
          colFilters
          colToggle
          hoverable
          storageKey="wallet_visible_cols"
          emptyMessage="No token holdings found in wallet."
          selectable={false}
        />
      )}

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
                  value={buyDialog.mint}
                  onChange={(e) =>
                    setBuyDialog((d) => (d ? { ...d, mint: e.target.value } : d))
                  }
                  className="font-mono focus:shadow-[0_0_0_2px_rgba(19,206,175,0.15)]"
                />
              </label>
            ) : (
              <p className="mb-4 text-xs text-text-mid">Mint: {buyDialog.mint}</p>
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
    </div>
  );
}
