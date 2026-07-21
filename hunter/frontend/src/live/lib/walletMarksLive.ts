import { isCashHolding } from 'lib/assetKind';
import {
  liveTradeSpotSolPerRaw,
  spotSolPerRawToUsd,
  valueSolAtSpot,
} from 'lib/liveMark';
import { connectTradeStream } from 'services/sse';
import type { AppDispatch, RootState } from '@live/store';
import { liveApi } from '@live/store/liveEndpoints';
import type { LiveTrade, WalletHolding, WalletPrice } from 'types';

const COALESCE_MS = 250;

type GetState = () => RootState;

/**
 * Patch Home holdings + any cached Jupiter price maps from a `trade_executed`
 * tip. Does not replace the RPC holdings scan — display marks only.
 */
function applyWalletMarkFromTrade(
  dispatch: AppDispatch,
  getState: GetState,
  trade: LiveTrade,
  usdRate: number | null,
): void {
  const spot = liveTradeSpotSolPerRaw(trade);
  if (spot == null) return;

  let decimals: number | null = null;

  dispatch(
    liveApi.util.updateQueryData('getPortfolioHoldings', undefined, (draft) => {
      const h = draft.find((x) => x.mint_address === trade.mint_address);
      if (!h || isCashHolding(h)) return;
      decimals = h.decimals;
      patchHoldingMark(h, spot, usdRate);
    }),
  );

  if (decimals == null) {
    // Holdings cache cold — still try prices if we already know decimals from a
    // prior prices+holdings pair; otherwise skip (Wallet page tips locally).
    return;
  }

  const dec = decimals;
  const priceUsd =
    usdRate != null ? spotSolPerRawToUsd(spot, dec, usdRate) : null;
  if (priceUsd == null) return;

  const cachedArgs = liveApi.util.selectCachedArgsForQuery(
    getState(),
    'getWalletPrices',
  );
  for (const mints of cachedArgs) {
    if (!mints.includes(trade.mint_address)) continue;
    dispatch(
      liveApi.util.updateQueryData('getWalletPrices', mints, (draft) => {
        const prev: WalletPrice = draft[trade.mint_address] ?? {
          price_usd: null,
          liquidity: null,
          price_change_24h: null,
          token_created_at: null,
        };
        draft[trade.mint_address] = { ...prev, price_usd: priceUsd };
      }),
    );
  }
}

function patchHoldingMark(
  h: WalletHolding,
  spotSolPerRaw: number,
  usdRate: number | null,
): void {
  const valueSol = valueSolAtSpot(spotSolPerRaw, h.amount);
  if (valueSol != null) {
    h.value_sol = valueSol;
    if (h.cost_basis_sol != null) {
      h.unrealized_pnl_sol = valueSol - h.cost_basis_sol;
      h.unrealized_pnl_pct =
        h.cost_basis_sol > 0
          ? ((valueSol - h.cost_basis_sol) / h.cost_basis_sol) * 100
          : null;
    }
  }
  if (usdRate != null) {
    const priceUsd = spotSolPerRawToUsd(spotSolPerRaw, h.decimals, usdRate);
    if (priceUsd != null) {
      h.price_usd = priceUsd;
      h.value_usd = priceUsd * h.ui_amount;
    }
  }
}

/**
 * App-wide mark tip pipe — coalesces `trade_executed` into RTK holdings/prices
 * patches for Home (and any mounted wallet price query).
 */
export function startWalletMarksLive(
  dispatch: AppDispatch,
  getState: GetState,
  getUsdRate: () => number | null,
): () => void {
  const pending = new Map<string, LiveTrade>();
  let timer: number | undefined;

  const flush = () => {
    timer = undefined;
    if (pending.size === 0) return;
    const batch = [...pending.values()];
    pending.clear();
    const rate = getUsdRate();
    for (const t of batch) {
      applyWalletMarkFromTrade(dispatch, getState, t, rate);
    }
  };

  const handle = connectTradeStream((raw) => {
    try {
      const t = JSON.parse(raw) as LiveTrade;
      pending.set(t.mint_address, t);
      if (timer === undefined) {
        timer = window.setTimeout(flush, COALESCE_MS);
      }
    } catch {
      /* ignore */
    }
  });

  return () => {
    window.clearTimeout(timer);
    handle.close();
  };
}
