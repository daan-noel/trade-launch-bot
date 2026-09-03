import { isCashHolding } from 'lib/assetKind';
import {
  liveTradeSpotSolPerRaw,
  spotSolPerRawToUsd,
  unrealizedFromValue,
  valueSolAtSpot,
} from 'lib/liveMark';
import { connectTradeStream } from 'services/sse';
import type { AppDispatch, RootState } from '@live/store';
import { liveApi } from '@live/store/liveEndpoints';
import type { CostModel, LiveTrade, WalletHolding, WalletPrice } from 'types';

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
  costs: CostModel | undefined,
): void {
  const spot = liveTradeSpotSolPerRaw(trade);
  if (spot == null) return;

  let decimals: number | null = null;

  dispatch(
    liveApi.util.updateQueryData('getPortfolioHoldings', undefined, (draft) => {
      const h = draft.find((x) => x.mint_address === trade.mint_address);
      if (!h || isCashHolding(h)) return;
      decimals = h.decimals;
      patchHoldingMark(h, spot, usdRate, costs, trade.reserve_sol ?? null);
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

/**
 * Patch one holding to a fresh spot. `value_sol` is a plain fact of the print, but
 * the PnL fields are NOT: they are net of the sell that would realize them, so
 * they are only patched when the served `costs` are in hand. Deriving them here
 * from `value - cost_basis` is what made a live row read ~4 pp better than the
 * same bag on an on-chain PnL tracker -- it charges no exit fee, no tip, no
 * impact, and it divides a gross value by an all-in basis. With no cost model
 * loaded the server's last net figures stand rather than being overwritten by a
 * gross one.
 */
function patchHoldingMark(
  h: WalletHolding,
  spotSolPerRaw: number,
  usdRate: number | null,
  costs: CostModel | undefined,
  reserveSol: number | null,
): void {
  const valueSol = valueSolAtSpot(spotSolPerRaw, h.amount);
  if (valueSol != null) {
    h.value_sol = valueSol;
    if (h.cost_basis_sol != null && costs) {
      const { pnlSol, pnlPct } = unrealizedFromValue(
        valueSol,
        h.cost_basis_sol,
        reserveSol,
        costs,
      );
      h.unrealized_pnl_sol = pnlSol;
      h.unrealized_pnl_pct = pnlPct;
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

  // Warm the cost model so the first tip can already net its mark.
  void dispatch(liveApi.endpoints.getCostModel.initiate());

  const flush = () => {
    timer = undefined;
    if (pending.size === 0) return;
    const batch = [...pending.values()];
    pending.clear();
    const rate = getUsdRate();
    // Served once, then read from cache on every tick — the fee and tip belong to
    // the backend, so the tip nets a bag with the same constants the engine does.
    const costs = liveApi.endpoints.getCostModel.select()(getState()).data;
    for (const t of batch) {
      applyWalletMarkFromTrade(dispatch, getState, t, rate, costs);
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
