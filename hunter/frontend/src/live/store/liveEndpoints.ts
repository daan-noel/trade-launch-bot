import { baseApi } from 'store/baseApi';
import type {
  WalletHolding,
  WalletPrice,
  PortfolioSummary,
  PortfolioPerformance,
  OpenStrategyPosition,
  CashbackStatus,
  CashbackClaimResult,
  PositionFill,
} from 'types';
import type { ArmedEntry } from 'lib/strategy/types';

/** The calendar windows the portfolio endpoints accept (the server's `range`
 *  grammar — `live::services::portfolio::range_since`). */
export type PortfolioRange = 'today' | '7d' | '30d' | 'all';

/** One closed trade — the atom of the charts deck (mirrors the backend
 *  `ClosedTradePoint`). `win` is `pnl_sol > 0` on a clean `End`. */
export interface ClosedTradePoint {
  exit_time: string;
  rule_id: string | null;
  pnl_sol: number;
  entry_sol: number;
  win: boolean;
}

/** `GET /api/portfolio/closes-series` (mirrors `live::services::portfolio::ClosesSeries`). */
export interface ClosesSeries {
  range: string;
  mode: string;
  since: string | null;
  /** Buys that never filled in the window — no SOL deployed, so never part of
   *  `closes`; carried as a count so entry-failure pressure stays visible. */
  entry_failed: number;
  closes: ClosedTradePoint[];
}

export interface SellTokenArgs {
  mint_address: string;
  /// Optional token-account hint (row "Sell All" supplies it to skip a wallet
  /// scan; a manual sell by mint omits it). The backend always sells the full
  /// live balance, so no amount is sent.
  token_account?: string;
  slippage_bps?: number;
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
    /**
     * Cross-rule closed PnL for the Portfolio page. Tagged `WalletHoldings` so a
     * real bag change refreshes it, PLUS `PortfolioPerf` so a *paper* close can
     * refresh it on its own without invalidating the real-wallet holdings reads.
     */
    getPortfolioPerformance: builder.query<
      PortfolioPerformance,
      { range?: PortfolioRange; mode?: 'real' | 'paper' } | void
    >({
      query: (arg) => {
        const range = arg && typeof arg === 'object' ? (arg.range ?? 'today') : 'today';
        const mode = arg && typeof arg === 'object' ? (arg.mode ?? 'real') : 'real';
        return `/api/portfolio/performance?range=${range}&mode=${mode}`;
      },
      providesTags: ['WalletHoldings', 'PortfolioPerf'],
    }),
    /**
     * The per-close array behind EVERY portfolio chart (B2): equity curve, PnL
     * histogram, calendar, day×hour heatmap, per-rule comparison. One fetch, so
     * the charts can't drift apart the way per-chart aggregate endpoints would.
     * Same tags as `getPortfolioPerformance` — a close refreshes both.
     */
    getPortfolioClosesSeries: builder.query<
      ClosesSeries,
      { range?: PortfolioRange; mode?: 'real' | 'paper'; ruleId?: string | null } | void
    >({
      query: (arg) => {
        const a = arg && typeof arg === 'object' ? arg : {};
        const q = new URLSearchParams({ range: a.range ?? '7d', mode: a.mode ?? 'real' });
        if (a.ruleId) q.set('rule_id', a.ruleId);
        return `/api/portfolio/closes-series?${q.toString()}`;
      },
      providesTags: ['WalletHoldings', 'PortfolioPerf'],
    }),
    // Jupiter oracle for held mints (liquidity / 24h / cold marks), decoupled
    // from the balance read. Live Value/Price tip from `trade_executed` SSE;
    // Wallet refetches this on mount / bag refresh / tab focus — no interval.
    // Keyed by the (sorted) mint list.
    getWalletPrices: builder.query<Record<string, WalletPrice>, string[]>({
      query: (mints) =>
        `/api/solana/prices?ids=${mints.map(encodeURIComponent).join(',')}`,
    }),
    // Console manual buy → a FULL tracked position (origin='manual'). 202
    // `{position_id}` returns immediately; the row appears as BuySubmitted and
    // every further truth arrives over `strategy_position_update` SSE — there is
    // no sync success to misreport (M2). TP/SL optional; absent = tracked-only.
    manualBuyPosition: builder.mutation<
      { position_id: string },
      { mint_address: string; amount_sol: number; tp_pct?: number; sl_pct?: number }
    >({
      query: (body) => ({ url: '/api/positions/manual-buy', method: 'POST', body }),
    }),
    // Set / replace / clear a manual position's TP/SL ([+TP/SL] on a Holding
    // row). Clearing (both absent) drops it back to tracked-only.
    setManualExitConfig: builder.mutation<
      { updated: boolean },
      { positionId: string; tp_pct?: number; sl_pct?: number }
    >({
      query: ({ positionId, tp_pct, sl_pct }) => ({
        url: `/api/strategies/generic/positions/${positionId}/manual-exit`,
        method: 'POST',
        body: { tp_pct, sl_pct },
      }),
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
    // `action` (default `retry`) selects how a stuck/unconfirmed row is resolved
    // (legality is backend-enforced per status — see the close-action matrix):
    //   'dump'     — sell with NO slippage floor (accept dust); clears a rugged,
    //                near-drained pool that reverts every normal-slippage sell.
    //   'writeoff' — book a stuck/unconfirmed position closed at a full loss with
    //                NO on-chain sell (a pool with no sellable liquidity at all).
    //   'verify'   — on-demand resolve: ExitUnconfirmed → PG-net heal-or-report;
    //                BuySubmitted → the reaper's adopt-or-drop logic.
    closeRulePosition: builder.mutation<
      {
        closing?: boolean;
        closed?: boolean;
        written_off?: boolean;
        verified?: boolean;
        cleared?: boolean;
        still_held?: boolean;
        adopted?: boolean;
        dropped?: boolean;
        unresolved?: boolean;
      },
      {
        strategy: string;
        positionId: string;
        action?: 'retry' | 'dump' | 'writeoff' | 'verify';
        /** Basis points of the initial bag (1..9900 = partial; omit = Sell ALL). */
        sellBps?: number;
      }
    >({
      query: ({ strategy, positionId, action, sellBps }) => {
        const params = new URLSearchParams();
        if (action && action !== 'retry') params.set('action', action);
        if (sellBps != null) params.set('sell_bps', String(sellBps));
        const qs = params.toString();
        return {
          url: `/api/strategies/${strategy}/positions/${positionId}/close${qs ? `?${qs}` : ''}`,
          method: 'POST',
        };
      },
    }),
    /** Append-only fill ledger for one position (entry + every sell leg). */
    getPositionFills: builder.query<PositionFill[], string>({
      query: (positionId) =>
        `/api/strategies/generic/positions/${encodeURIComponent(positionId)}/fills`,
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
    // Generic-engine armed snapshot for the live monitor — the currently-armed
    // (token, rule) pairs. Live deltas ride the `strategy_armed_changed` SSE;
    // this is the initial + reconnect refetch.
    //
    // There is deliberately NO armed-*history* read: an arm that never fired is
    // dropped from the in-memory runtime cache and never persisted, so the route
    // the old `ArmedHistoryPanel` called never existed server-side and the panel
    // 404'd on every render. Reviving it means designing durable arm storage
    // first (see the live-UI redesign plan, B3) — not adding a route.
    getArmed: builder.query<ArmedEntry[], void>({
      query: () => '/api/strategies/armed',
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
    /** Admin reseed of DB-backed in-memory caches (settings, engine, token seed). */
    reloadCaches: builder.mutation<
      { ok: boolean; steps: { name: string; ok: boolean; detail?: string | null }[] },
      void
    >({
      query: () => ({ url: '/api/system/reload-caches', method: 'POST' }),
      invalidatesTags: ['Settings', 'StrategyRule', 'WalletHoldings', 'Cashback'],
    }),
  }),
});

export const {
  useGetPortfolioHoldingsQuery,
  useGetPortfolioSummaryQuery,
  useGetPortfolioPositionsQuery,
  useGetPortfolioPerformanceQuery,
  useGetPortfolioClosesSeriesQuery,
  useGetWalletPricesQuery,
  useManualBuyPositionMutation,
  useSetManualExitConfigMutation,
  useSellTokenMutation,
  useCloseRulePositionMutation,
  useGetPositionFillsQuery,
  useGetCashbackStatusQuery,
  useClaimCashbackMutation,
  useGetArmedQuery,
  useGetLiveModeQuery,
  useSetLiveModeMutation,
  useReloadCachesMutation,
} = liveApi;
