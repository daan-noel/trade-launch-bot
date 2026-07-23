import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useSearchParams } from 'react-router-dom';
import type { FetchBaseQueryError } from '@reduxjs/toolkit/query';
import type { SerializedError } from '@reduxjs/toolkit';
import { IconButton } from 'components/ui/IconButton';
import { BuyIcon, SearchIcon, SellIcon, SpinnerIcon } from 'components/ui/icons';
import { Button } from 'components/ui/Button';
import { Input } from 'components/ui/Input';
import { InlineAlert, Modal } from 'components/ui/Modal';
import { PageHeader } from 'components/ui/PageHeader';
import { Badge } from 'components/ui/Badge';
import { TokenDetailPanel } from 'components/tokens/TokenDetailPanel';
import { LazyTokenTradeChart } from 'components/tokens/LazyTokenTradeChart';
import { apiErrorMessage, useGetTokenDetailQuery } from 'store/apiSlice';
import { parseSlippageBps } from '@live/lib/slippage';
import {
  useBuyTokenMutation,
  useSellTokenMutation,
  useGetPortfolioPositionsQuery,
} from '@live/store/liveEndpoints';

/** Loose base58 mint check (32–44 base58 chars) — the backend does the real
 *  on-chain validation; this only guards obviously-malformed input. */
const MINT_RE = /^[1-9A-HJ-NP-Za-km-z]{32,44}$/;

interface LogEntry {
  key: string;
  action: 'Buy' | 'Sell';
  mint_address: string;
  note: string;
  ok: boolean;
}

/**
 * Dedicated Trade page: enter a mint (or `?mint=`) → live detail + chart,
 * then Buy (SOL + slippage) or Sell (full balance).
 */
export function TradePage() {
  const [searchParams] = useSearchParams();
  const mintParam = searchParams.get('mint') ?? '';

  const [mintInput, setMintInput] = useState(mintParam);
  const [loadedMint, setLoadedMint] = useState(() => (MINT_RE.test(mintParam) ? mintParam : ''));
  const [buySol, setBuySol] = useState('0.001');
  const [buySlip, setBuySlip] = useState('');
  const [sellSlip, setSellSlip] = useState('');
  const [actionError, setActionError] = useState<string | null>(null);
  const [selling, setSelling] = useState(false);
  const [confirmSell, setConfirmSell] = useState(false);
  const [log, setLog] = useState<LogEntry[]>([]);
  const logSeq = useRef(0);

  // Deep-link: Positions / Armed / Wallet → `/trade?mint=…`
  useEffect(() => {
    if (!mintParam || !MINT_RE.test(mintParam)) return;
    setMintInput(mintParam);
    setLoadedMint(mintParam);
  }, [mintParam]);

  const {
    data: detail,
    isFetching: detailLoading,
    error: detailError,
  } = useGetTokenDetailQuery(loadedMint, { skip: !loadedMint });

  const { data: positions = [] } = useGetPortfolioPositionsQuery(true);
  const managedBy = useMemo(
    () => positions.find((p) => p.mint_address === loadedMint) ?? null,
    [positions, loadedMint],
  );

  const [buyToken] = useBuyTokenMutation();
  const [sellToken] = useSellTokenMutation();

  const pushLog = useCallback(
    (action: 'Buy' | 'Sell', mint: string, note: string, ok: boolean) => {
      const key = `${logSeq.current++}`;
      setLog((prev) => [{ key, action, mint_address: mint, note, ok }, ...prev].slice(0, 20));
    },
    [],
  );

  const loadMint = useCallback(() => {
    const mint = mintInput.trim();
    setActionError(null);
    if (!MINT_RE.test(mint)) {
      setActionError('Enter a valid token mint address');
      return;
    }
    setLoadedMint(mint);
  }, [mintInput]);

  const handleBuy = useCallback(async () => {
    const mint = loadedMint;
    if (!mint) {
      setActionError('Load a mint first');
      return;
    }
    const sol = parseFloat(buySol.trim());
    if (!Number.isFinite(sol) || sol <= 0) {
      setActionError('Enter a valid SOL amount > 0');
      return;
    }
    const { bps, error } = parseSlippageBps(buySlip);
    if (error) {
      setActionError(error);
      return;
    }
    setActionError(null);
    try {
      await buyToken({ mint_address: mint, amount_sol: sol, ...(bps !== undefined ? { slippage_bps: bps } : {}) }).unwrap();
      pushLog('Buy', mint, `◎${sol} submitted`, true);
    } catch (e) {
      const msg = apiErrorMessage(e as FetchBaseQueryError | SerializedError) ?? 'unknown error';
      setActionError(`Buy failed: ${msg}`);
      pushLog('Buy', mint, msg, false);
    }
  }, [loadedMint, buySol, buySlip, buyToken, pushLog]);

  const runSell = useCallback(async () => {
    const mint = loadedMint;
    const { bps, error } = parseSlippageBps(sellSlip);
    if (error) {
      setActionError(error);
      return;
    }
    setActionError(null);
    setSelling(true);
    try {
      await sellToken({ mint_address: mint, ...(bps !== undefined ? { slippage_bps: bps } : {}) }).unwrap();
      pushLog('Sell', mint, 'full balance submitted', true);
    } catch (e) {
      const msg = apiErrorMessage(e as FetchBaseQueryError | SerializedError) ?? 'unknown error';
      setActionError(`Sell failed: ${msg}`);
      pushLog('Sell', mint, msg, false);
    } finally {
      setSelling(false);
    }
  }, [loadedMint, sellSlip, sellToken, pushLog]);

  const handleSell = useCallback(() => {
    if (!loadedMint) {
      setActionError('Load a mint first');
      return;
    }
    // Bot-managed interlock (5.2): a manual sell can race the strategy's own exit.
    if (managedBy) {
      setConfirmSell(true);
      return;
    }
    void runSell();
  }, [loadedMint, managedBy, runSell]);

  return (
    <div>
      <PageHeader
        title="Trade"
        description="Mint-first execute desk · Wallet = bag overview · Positions = bot inventory"
      />

      <div className="mb-3 flex flex-wrap items-end gap-2">
        <label className="flex flex-1 flex-col gap-1.5" style={{ minWidth: 280 }}>
          <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">
            Mint Address
          </span>
          <Input
            type="text"
            fieldSize="md"
            placeholder="Token mint address"
            value={mintInput}
            onChange={(e) => setMintInput(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && loadMint()}
            className="font-mono"
          />
        </label>
        <IconButton
          variant="primary"
          size="lg"
          onClick={loadMint}
          label="Load"
          title="Load"
        >
          <SearchIcon />
        </IconButton>
      </div>

      {actionError && <InlineAlert variant="error">{actionError}</InlineAlert>}

      {loadedMint && (
        <>
          {managedBy && (
            <InlineAlert variant="warning">
              <strong>{managedBy.strategy_id}</strong> holds a real position on this mint
              (status: {managedBy.status}). A manual sell can race the bot's own exit.
            </InlineAlert>
          )}

          <div className="mt-3 grid grid-cols-1 gap-3 lg:grid-cols-2">
            <div className="rounded-lg border border-white/5 bg-white/2 p-3">
              <TokenDetailPanel
                detail={detail ?? null}
                loading={detailLoading}
                error={apiErrorMessage(detailError)}
              />
            </div>

            <div className="flex flex-col gap-3">
              {/* Buy */}
              <div className="rounded-lg border border-buy/20 bg-buy/5 p-3">
                <h3 className="mb-2 text-sm font-bold text-buy">Buy</h3>
                <div className="flex flex-wrap items-end gap-2">
                  <label className="flex flex-col gap-1">
                    <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">
                      SOL
                    </span>
                    <Input
                      type="number"
                      fieldSize="md"
                      min={0.001}
                      step={0.001}
                      value={buySol}
                      onChange={(e) => setBuySol(e.target.value)}
                      className="w-28"
                    />
                  </label>
                  <label className="flex flex-col gap-1">
                    <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">
                      Slippage %
                    </span>
                    <Input
                      type="number"
                      fieldSize="md"
                      min={0}
                      max={50}
                      step={0.1}
                      placeholder="Default"
                      value={buySlip}
                      onChange={(e) => setBuySlip(e.target.value)}
                      className="w-28"
                    />
                  </label>
                  <IconButton
                    variant="primary"
                    size="lg"
                    onClick={handleBuy}
                    label="Buy"
                    title="Buy"
                  >
                    <BuyIcon />
                  </IconButton>
                </div>
              </div>

              {/* Sell (full balance; partial deferred to 5.4) */}
              <div className="rounded-lg border border-sell/20 bg-sell/5 p-3">
                <h3 className="mb-2 text-sm font-bold text-sell">Sell (full balance)</h3>
                <div className="flex flex-wrap items-end gap-2">
                  <label className="flex flex-col gap-1">
                    <span className="text-[10px] font-bold uppercase tracking-wider text-text-dim">
                      Slippage %
                    </span>
                    <Input
                      type="number"
                      fieldSize="md"
                      min={0}
                      max={50}
                      step={0.1}
                      placeholder="Default"
                      value={sellSlip}
                      onChange={(e) => setSellSlip(e.target.value)}
                      className="w-28"
                    />
                  </label>
                  <IconButton
                    variant="danger"
                    size="lg"
                    disabled={selling}
                    onClick={handleSell}
                    label={selling ? 'Selling…' : 'Sell All'}
                    title={selling ? 'Selling…' : 'Sell All'}
                  >
                    {selling ? <SpinnerIcon /> : <SellIcon />}
                  </IconButton>
                </div>
              </div>
            </div>
          </div>

          {detail && (
            <div className="mt-3">
              <LazyTokenTradeChart key={detail.mint_address} tableId="trade_chart" detail={detail} />
            </div>
          )}
        </>
      )}

      {/* Recent manual-trades log (5.3) */}
      {log.length > 0 && (
        <div className="mt-4 rounded-lg border border-white/5 bg-white/2 p-3">
          <h3 className="mb-2 text-sm font-bold text-text">Recent manual trades</h3>
          <ul className="flex flex-col gap-1">
            {log.map((e) => (
              <li key={e.key} className="flex items-center gap-2 text-xs">
                <Badge variant={e.action === 'Buy' ? 'buy' : 'sell'}>{e.action}</Badge>
                <span className="font-mono text-text-mid">{e.mint_address.slice(0, 10)}…</span>
                <span className={e.ok ? 'text-green' : 'text-red'}>{e.note}</span>
              </li>
            ))}
          </ul>
        </div>
      )}

      <Modal title="⚠ Bot-managed position" open={confirmSell} onClose={() => setConfirmSell(false)}>
        <InlineAlert variant="warning">
          <strong>{managedBy?.strategy_id}</strong> is managing this mint (status:{' '}
          {managedBy?.status}). Selling manually can race the bot's own exit and double-sell.
          Sell anyway?
        </InlineAlert>
        <div className="mt-4 flex items-center justify-end gap-2.5">
          <Button variant="ghost" onClick={() => setConfirmSell(false)}>
            Cancel
          </Button>
          <IconButton
            variant="danger"
            size="lg"
            onClick={() => {
              setConfirmSell(false);
              void runSell();
            }}
            label="Sell Anyway"
            title="Sell Anyway"
          >
            <SellIcon />
          </IconButton>
        </div>
      </Modal>
    </div>
  );
}
