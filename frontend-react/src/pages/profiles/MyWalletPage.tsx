import { useCallback, useEffect, useMemo, useState } from 'react';
import { useDispatch, useSelector } from 'react-redux';
import { DataTable } from 'components/table/DataTable';
import { Badge } from 'components/ui/Badge';
import { Button } from 'components/ui/Button';
import { Input } from 'components/ui/Input';
import { InlineAlert, Modal } from 'components/ui/Modal';
import { walletColumns } from 'components/wallet/walletColumns';
import { tradeBuy, tradeSell } from 'services/api';
import { useWalletPriceDisplay } from 'hooks/useWalletPriceDisplay';
import type { AppDispatch, RootState } from '../../store';
import { loadWalletHoldings } from 'store/walletSlice';

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
  const holdings = useSelector((s: RootState) => s.wallet.holdings);
  const loading = useSelector((s: RootState) => s.wallet.loading);
  const error = useSelector((s: RootState) => s.wallet.error);

  const [actionError, setActionError] = useState<string | null>(null);
  const [actionSuccess, setActionSuccess] = useState<string | null>(null);
  const [sellingMint, setSellingMint] = useState<string | null>(null);
  const [buyDialog, setBuyDialog] = useState<BuyDialog | null>(null);

  const loadHoldings = useCallback(() => {
    dispatch(loadWalletHoldings());
  }, [dispatch]);

  useEffect(() => {
    loadHoldings();
  }, [loadHoldings]);

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
        await tradeSell({
          mint,
          token_amount: tokenAmount,
          token_account: holding.token_account,
        });
        setActionSuccess('Sell successful! Refreshing…');
        setTimeout(loadHoldings, 1500);
      } catch (e) {
        setActionError(`Sell failed: ${e instanceof Error ? e.message : 'unknown error'}`);
      } finally {
        setSellingMint(null);
      }
    },
    [holdings, loadHoldings],
  );

  const handleBuyOpen = useCallback((mint: string, tokenProgramId: string) => {
    setBuyDialog({ mint, tokenProgramId, solInput: '0.1', slippageInput: '', manual: false });
  }, []);

  const handleManualBuyOpen = useCallback(() => {
    setBuyDialog({ mint: '', solInput: '0.1', slippageInput: '', manual: true });
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

    setActionError(null);
    setActionSuccess(null);
    setBuyDialog(null);

    try {
      await tradeBuy({
        mint,
        sol_amount: solAmount,
        // Omit for manual buys — the backend resolves the token program on-chain.
        ...(buyDialog.tokenProgramId
          ? { token_program_id: buyDialog.tokenProgramId }
          : {}),
        ...(slippageBps !== undefined ? { slippage_bps: slippageBps } : {}),
      });
      setActionSuccess('Buy successful! Refreshing…');
      setTimeout(loadHoldings, 1500);
    } catch (e) {
      setActionError(`Buy failed: ${e instanceof Error ? e.message : 'unknown error'}`);
    }
  }, [buyDialog, loadHoldings]);

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
        <Button variant="subtle" size="sm" onClick={loadHoldings} disabled={loading}>
          {loading ? 'Loading…' : '↻ Refresh'}
        </Button>
        <Button variant="primary" size="sm" onClick={handleManualBuyOpen}>
          + Manual Buy
        </Button>
      </div>

      {error && <InlineAlert variant="error">{error}</InlineAlert>}
      {actionError && <InlineAlert variant="error">{actionError}</InlineAlert>}
      {actionSuccess && <InlineAlert variant="success">{actionSuccess}</InlineAlert>}

      {loading && holdings.length === 0 ? (
        <p className="py-10 text-center text-text-dim">Loading wallet holdings from Solana…</p>
      ) : (
        <DataTable
          columns={columns}
          rows={holdings}
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
                step={0.01}
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
