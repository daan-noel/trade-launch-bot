import { useCallback, useEffect, useMemo, useState } from 'react';
import { DataTable } from '../components/table/DataTable';
import { InlineAlert, Modal } from '../components/ui/Modal';
import { walletColumns } from '../components/wallet/walletColumns';
import { fetchWalletHoldings, tradeBuy, tradeSell } from '../services/api';
import type { WalletHolding } from '../types';
import { cn } from '../lib/cn';

interface BuyDialog {
  mint: string;
  tokenProgramId: string;
  solInput: string;
}

export function WalletPage() {
  const [holdings, setHoldings] = useState<WalletHolding[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [actionSuccess, setActionSuccess] = useState<string | null>(null);
  const [sellingMint, setSellingMint] = useState<string | null>(null);
  const [buyDialog, setBuyDialog] = useState<BuyDialog | null>(null);

  const loadHoldings = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const data = await fetchWalletHoldings();
      setHoldings(data);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to load holdings');
    } finally {
      setLoading(false);
    }
  }, []);

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
    setBuyDialog({ mint, tokenProgramId, solInput: '0.1' });
  }, []);

  const handleBuySubmit = useCallback(async () => {
    if (!buyDialog) return;
    const solAmount = parseFloat(buyDialog.solInput.trim());
    if (!Number.isFinite(solAmount) || solAmount <= 0) {
      setActionError('Enter a valid SOL amount > 0');
      return;
    }

    setActionError(null);
    setActionSuccess(null);
    setBuyDialog(null);

    try {
      await tradeBuy({
        mint: buyDialog.mint,
        sol_amount: solAmount,
        token_program_id: buyDialog.tokenProgramId,
      });
      setActionSuccess('Buy successful! Refreshing…');
      setTimeout(loadHoldings, 1500);
    } catch (e) {
      setActionError(`Buy failed: ${e instanceof Error ? e.message : 'unknown error'}`);
    }
  }, [buyDialog, loadHoldings]);

  const columns = useMemo(
    () =>
      walletColumns({
        onBuy: handleBuyOpen,
        onSell: handleSell,
        sellingMint,
      }),
    [handleBuyOpen, handleSell, sellingMint],
  );

  const buySymbol =
    buyDialog &&
    (holdings.find((h) => h.mint === buyDialog.mint)?.symbol ?? buyDialog.mint);

  return (
    <div>
      <div className="mb-3.5 flex flex-wrap items-center gap-3">
        <h2 className="text-lg font-extrabold text-text">Wallet Holdings</h2>
        <span className="rounded-md border border-primary bg-primary/15 px-2.5 py-0.5 font-mono text-[11px] font-bold tracking-wide text-primary">
          {holdings.length} tokens
        </span>
        <button
          type="button"
          onClick={loadHoldings}
          disabled={loading}
          className={cn(
            'rounded-md border border-white/8 bg-white/4 px-2.5 py-1 text-[11px] font-semibold uppercase tracking-wider text-text-dim transition hover:text-text disabled:opacity-45',
          )}
        >
          {loading ? 'Loading…' : '↻ Refresh'}
        </button>
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
          colToggle
          hoverable
          storageKey="wallet_visible_cols"
          emptyMessage="No token holdings found in wallet."
          selectable={false}
        />
      )}

      <Modal
        title={buyDialog ? `Buy ${buySymbol}` : ''}
        open={buyDialog != null}
        onClose={() => setBuyDialog(null)}
      >
        {buyDialog && (
          <>
            <p className="mb-4 text-xs text-text-mid">Mint: {buyDialog.mint}</p>
            <label className="mb-4 flex flex-col gap-1.5">
              <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">
                SOL Amount
              </span>
              <input
                type="number"
                min={0.001}
                step={0.01}
                value={buyDialog.solInput}
                onChange={(e) =>
                  setBuyDialog((d) => (d ? { ...d, solInput: e.target.value } : d))
                }
                className="w-full rounded-md border border-white/10 bg-white/4 px-2.5 py-2 font-mono text-[13px] text-text outline-none focus:border-primary focus:shadow-[0_0_0_2px_rgba(19,206,175,0.15)]"
              />
            </label>
            <div className="flex items-center justify-end gap-2.5">
              <button
                type="button"
                onClick={() => setBuyDialog(null)}
                className="rounded-md border border-white/10 px-4 py-1.5 text-[13px] text-text-dim hover:bg-white/5 hover:text-text"
              >
                Cancel
              </button>
              <button
                type="button"
                onClick={handleBuySubmit}
                className="rounded-md border border-primary bg-primary/15 px-4 py-1.5 text-[13px] font-bold text-primary hover:bg-primary/25"
              >
                Confirm Buy
              </button>
            </div>
          </>
        )}
      </Modal>
    </div>
  );
}
