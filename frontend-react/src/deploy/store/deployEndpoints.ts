import { baseApi } from 'store/baseApi';
import type {
  WalletHolding,
  WalletPrice,
  CashbackStatus,
  CashbackClaimResult,
} from 'types';

export interface BuyTokenArgs {
  mint: string;
  sol_amount: number;
  /// Omitted for manual buys — the backend resolves the token program on-chain.
  token_program_id?: string;
  /// Per-trade slippage in basis points; omit to use the global default.
  slippage_bps?: number;
}

export interface SellTokenArgs {
  mint: string;
  /// Optional token-account hint (row "Sell All" supplies it to skip a wallet
  /// scan; a manual sell by mint omits it). The backend always sells the full
  /// live balance, so no amount is sent.
  token_account?: string;
  slippage_bps?: number;
}

/**
 * Deploy-only RTK Query endpoints — bundled exclusively in the live-trading
 * build: wallet holdings/prices, buy/sell, cashback, and the live-mode kill
 * switch. The analysis (local) backend serves none of these.
 */
export const deployApi = baseApi.injectEndpoints({
  endpoints: (builder) => ({
    // Wallet holdings — an expensive read (full wallet RPC scan + Jupiter batch
    // price + migration resolution). Cached like the token list so revisiting
    // the page reuses it instead of re-scanning the chain. A manual trade
    // refreshes it surgically (see getWalletHolding) rather than re-fetching.
    getWalletHoldings: builder.query<WalletHolding[], void>({
      query: () => '/api/solana/wallet/tokens',
      providesTags: ['WalletHoldings'],
    }),
    // Single-mint counterpart used only for post-trade confirmation polling:
    // one cheap RPC + one price lookup. Not exposed as a hook — callers drive
    // it imperatively via `initiate` and patch the result into the list cache.
    getWalletHolding: builder.query<WalletHolding | null, string>({
      query: (mint) => `/api/solana/wallet/tokens/${encodeURIComponent(mint)}`,
    }),
    // Live prices for the held mints, decoupled from the balance read. Polled
    // on a short interval (see the page) so the value column ticks without
    // re-scanning the wallet. Keyed by the (sorted) mint list — caller passes
    // the mints already in the balances cache.
    getWalletPrices: builder.query<Record<string, WalletPrice>, string[]>({
      query: (mints) =>
        `/api/solana/prices?ids=${mints.map(encodeURIComponent).join(',')}`,
    }),
    buyToken: builder.mutation<{ success: boolean }, BuyTokenArgs>({
      query: (body) => ({ url: '/api/solana/wallet/buy', method: 'POST', body }),
    }),
    sellToken: builder.mutation<{ success: boolean }, SellTokenArgs>({
      query: (body) => ({ url: '/api/solana/wallet/sell', method: 'POST', body }),
    }),
    // Accrued pump.fun cashback — a read-only on-chain status (two account
    // reads). Cached, not polled: cashback accrues slowly, so the wallet card
    // refreshes on mount / after a claim, never on the live price tick.
    getCashbackStatus: builder.query<CashbackStatus, void>({
      query: () => '/api/cashback/status',
      providesTags: ['Cashback'],
    }),
    // Sweep both pots back to the wallet as native SOL. Off the trade hot path;
    // invalidates the status so the card reflects the drained balance.
    claimCashback: builder.mutation<CashbackClaimResult, void>({
      query: () => ({ url: '/api/cashback/claim', method: 'POST' }),
      invalidatesTags: ['Cashback'],
    }),
    getLiveMode: builder.query<boolean, void>({
      query: () => '/api/system/live',
      transformResponse: (r: { live: boolean }) => r.live,
      providesTags: ['LiveMode'],
    }),
    setLiveMode: builder.mutation<boolean, boolean>({
      query: (live) => ({
        url: '/api/system/live',
        method: 'PUT',
        body: { live },
      }),
      transformResponse: (r: { live: boolean }) => r.live,
      invalidatesTags: ['LiveMode'],
    }),
  }),
});

export const {
  useGetWalletHoldingsQuery,
  useGetWalletPricesQuery,
  useBuyTokenMutation,
  useSellTokenMutation,
  useGetCashbackStatusQuery,
  useClaimCashbackMutation,
  useGetLiveModeQuery,
  useSetLiveModeMutation,
} = deployApi;
