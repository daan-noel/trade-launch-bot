import { baseApi } from 'store/baseApi';
import type {
  WalletHolding,
  WalletPrice,
  PortfolioSummary,
  OpenStrategyPosition,
  RecentClosedPosition,
  CashbackStatus,
  CashbackClaimResult,
} from 'types';
import type { ArmedEntry } from 'lib/strategy/types';

export interface BuyTokenArgs {
  mint_address: string;
  amount_sol: number;
  /// Omitted for manual buys — the backend resolves the token program on-chain.
  token_program_id?: string;
  /// Per-trade slippage in basis points; omit to use the global default.
  slippage_bps?: number;
}

export interface SellTokenArgs {
  mint_address: string;
  /// Optional token-account hint (row "Sell All" supplies it to skip a wallet
  /// scan; a manual sell by mint omits it). The backend always sells the full
  /// live balance, so no amount is sent.
  token_account?: string;
  slippage_bps?: number;
}

/** One candidate that armed on the live feed but never fired (its entry trigger
 *  never came). In-memory, current-run only — resets when a fresh run starts. */
export interface ArmedRecord {
  mint_address: string;
  position_id: string;
  strategy_id: string;
  armed_at: string;
  ended_at: string;
}

/**
 * Live-only RTK Query endpoints — bundled exclusively in the live-trading
 * build: wallet holdings/prices, buy/sell, cashback, and the live-mode kill
 * switch. The lab (local) backend serves none of these.
 */
export const liveApi = baseApi.injectEndpoints({
  endpoints: (builder) => ({
    // Portfolio holdings — the position-manager read (full wallet RPC scan +
    // Jupiter marks + cost basis + unrealized PnL + bot-managed tag + token
    // enrichment, composed server-side by the portfolio service). Expensive, so
    // cached like the token list; a manual trade refreshes it surgically (see
    // getWalletHolding) rather than re-fetching the whole wallet.
    getPortfolioHoldings: builder.query<WalletHolding[], void>({
      query: () => '/api/portfolio/holdings',
      providesTags: ['WalletHoldings'],
    }),
    // Single-mint counterpart used only for post-trade confirmation polling:
    // one cheap RPC + one price lookup. Not exposed as a hook — callers drive
    // it imperatively via `initiate` and patch the result into the list cache.
    getWalletHolding: builder.query<WalletHolding | null, string>({
      query: (mint) => `/api/solana/wallet/tokens/${encodeURIComponent(mint)}`,
    }),
    // Wallet-wide roll-up for the Home KPI row (value/PnL totals + real-money
    // aggregates). Shares the WalletHoldings tag so a trade refresh invalidates it.
    getPortfolioSummary: builder.query<PortfolioSummary, void>({
      query: () => '/api/portfolio/summary',
      providesTags: ['WalletHoldings'],
    }),
    // All open strategy positions across every rule (Home per-strategy strip +
    // Live-Trading roll-up). `real` defaults to true (real-money monitor).
    getPortfolioPositions: builder.query<OpenStrategyPosition[], boolean | void>({
      query: (real = true) => `/api/portfolio/positions?real=${real}`,
      providesTags: ['WalletHoldings'],
    }),
    /** Latest End/ExitFailed rows — Ops Recent hydrate (DB, not session SSE). */
    getPortfolioRecentCloses: builder.query<RecentClosedPosition[], number | void>({
      query: (limit = 50) => `/api/portfolio/recent-closes?limit=${limit}`,
      providesTags: ['WalletHoldings'],
    }),
    // Jupiter oracle for held mints (liquidity / 24h / cold marks), decoupled
    // from the balance read. Live Value/Price tip from `trade_executed` SSE;
    // Wallet refetches this on mount / bag refresh / tab focus — no interval.
    // Keyed by the (sorted) mint list.
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
    // Per-row "Sell ALL" on the rule positions table. Unlike `sellToken` (a raw
    // wallet sell by mint), this force-closes the specific strategy position via the
    // position-aware path, so the row transitions Holding → ExitPending → closed over
    // the `strategy_position_update` SSE stream — live, reload-proof status. The backend
    // returns 202 as soon as the close begins; the terminal state arrives over SSE, so
    // no cache tag is invalidated here (the stream patches the row).
    closeRulePosition: builder.mutation<{ closing: boolean }, { strategy: string; positionId: string }>({
      query: ({ strategy, positionId }) => ({
        url: `/api/strategies/${strategy}/positions/${positionId}/close`,
        method: 'POST',
      }),
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
    // Current-run "armed but never fired" candidates for a rule — read straight
    // from the in-memory runtime cache (these rows are deleted on drop, so there's
    // no DB history). A convenience read; `ArmedHistoryPanel` refetches on
    // `strategy_armed_changed` / SSE reopen (no poll).
    // Generic-engine armed snapshot for the live monitor — the currently-armed
    // (token, rule) pairs. Live deltas ride the `strategy_armed_changed` SSE;
    // this is the initial + reconnect refetch.
    getArmed: builder.query<ArmedEntry[], void>({
      query: () => '/api/strategies/armed',
    }),
    getArmedHistory: builder.query<ArmedRecord[], { strategy: string; ruleId: string }>({
      query: ({ strategy, ruleId }) =>
        `/api/strategies/${strategy}/rules/${ruleId}/armed-history`,
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
  useGetPortfolioHoldingsQuery,
  useGetPortfolioSummaryQuery,
  useGetPortfolioPositionsQuery,
  useGetPortfolioRecentClosesQuery,
  useGetWalletPricesQuery,
  useBuyTokenMutation,
  useSellTokenMutation,
  useCloseRulePositionMutation,
  useGetCashbackStatusQuery,
  useClaimCashbackMutation,
  useGetArmedHistoryQuery,
  useGetArmedQuery,
  useGetLiveModeQuery,
  useSetLiveModeMutation,
} = liveApi;
