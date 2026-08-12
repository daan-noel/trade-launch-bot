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
import type { HistoryRange } from 'lib/strategy/nav';

/** The calendar windows the portfolio endpoints accept (the server's `range`
 *  grammar — `live::services::portfolio::range_window`). Same vocabulary as the
 *  Console History cohort, `custom` included: the explicit `from`/`to` bounds
 *  carry the window then. */
export type PortfolioRange = HistoryRange;

/** A resolved portfolio window as the endpoints take it — the preset plus the
 *  `custom` bounds (UTC ISO, `to` exclusive; either side may be open). */
export interface PortfolioWindow {
  range?: PortfolioRange;
  from?: string | null;
  to?: string | null;
}

/** `range` + the two bounds, dropped when absent so a preset URL stays clean. */
function windowParams(w: PortfolioWindow, fallbackRange: PortfolioRange): URLSearchParams {
  const q = new URLSearchParams({ range: w.range ?? fallbackRange });
  if (w.range === 'custom') {
    if (w.from) q.set('from', w.from);
    if (w.to) q.set('to', w.to);
  }
  return q;
}

/** One closed trade — the atom of the charts deck (mirrors the backend
 *  `ClosedTradePoint`). `win` is `pnl_sol > 0` on a clean `End`. */
export interface ClosedTradePoint {
  id: string;
  exit_time: string;
  rule_id: string | null;
  mint_address: string;
  pnl_sol: number;
  entry_sol: number;
  win: boolean;
  /** `exit_time − entry_time` in seconds; null when entry_time is missing. */
  hold_secs: number | null;
  /** Persisted exit label — History charts filter this the same way as the table. */
  exit_reason: string | null;
}

/** `GET /api/portfolio/closes-series` (mirrors `live::services::portfolio::ClosesSeries`). */
export interface ClosesSeries {
  range: string;
  mode: string;
  since: string | null;
  /** Exclusive window end; `null` = up to now (every non-custom range). */
  until: string | null;
  /** Buys that never filled in the window — no SOL deployed, so never part of
   *  `closes`; carried as a count so entry-failure pressure stays visible. */
  entry_failed: number;
  closes: ClosedTradePoint[];
}

/**
 * What a condition **is**, independent of any instant (mirrors the backend's
 * `ConditionMetaOut`, which both response shapes flatten).
 */
export interface RuleConditionMeta {
  side: 'entry' | 'exit' | 'stage';
  /** Ladder index; present only on a `stage` read. */
  stage?: number;
  /** Whether the fold is currently evaluating this stage. */
  stage_active?: boolean;
  metric: string;
  group: string;
  unit: string;
  window_size_sec: number | null;
  /** Authored DNF: flat `[{operator,value}]` (one AND arm) or nested OR arms. */
  conditions: unknown;
  origin: 'authored' | 'take_profit' | 'stop_loss';
  /** PnL the trailing stop arms at, when gated. */
  arm_above_pct?: number;
}

/**
 * One condition of a rule at ONE instant, with the value the fold reads for it
 * (mirrors `live::api::handlers::strategies::rule_readout::ConditionOut`).
 *
 * `value` is `null` when the metric is unreadable — an unregistered window, a flow
 * metric with no fingerprint state, or a position metric with no position. Per the
 * engine convention that satisfies nothing, so `ok` is false.
 */
export interface RuleConditionRead extends RuleConditionMeta {
  value: number | null;
  ok: boolean;
  matched_operator?: string;
  matched_value?: number;
  /**
   * The trail is gated and not yet armed, so the fold **skips** this condition —
   * it is not being evaluated at all. Render it as dormant, never as a failing
   * condition, or the UI shows a stop that looks live when none is.
   */
  disarmed: boolean;
}

/** The same condition across every row of a series (`ConditionSeriesOut`). */
export interface RuleConditionSeries extends RuleConditionMeta {
  /** One per row of {@link RuleReadoutSeries.at}. */
  values: (number | null)[];
  ok: boolean[];
  /**
   * Per row, and **absent** unless this condition is a gated trail — the only kind
   * the fold ever skips. A trail arms and disarms as PnL crosses `arm_above_pct`,
   * so it cannot be one flag for the whole series.
   */
  disarmed?: boolean[];
}

/** Which instant a closed position's replay reads at. */
export type ReadoutAt = 'exit' | 'entry';

/** `GET .../positions/{id}/metrics` — the rule readout for one position. */
export interface RuleReadout {
  mint_address: string;
  rule_id: string;
  /**
   * `engine` — read out of the live fold; exactly what the engine is deciding on.
   * `replay` — reconstructed by folding stored trades (closed positions). Close, but
   * stored rows carry an *approximated* real-reserve value and any trade the feed saw
   * without persisting is absent, so it must be labelled rather than passed off as
   * engine truth.
   */
  source: 'engine' | 'replay';
  /** Engine arm state (`Armed` | `Entered` | …); `null` on a replay. */
  arm: string | null;
  stage: number | null;
  /** The one instant every `value` in this response is read at. */
  at: string;
  conditions: RuleConditionRead[];
}

/**
 * `GET .../positions/{id}/metric-series` — the same conditions at every row of the
 * engine's decision grid, so a chart crosshair indexes an array instead of asking
 * the box to re-fold the token's history per hover.
 *
 * Always `replay`: the engine holds one instant of state, not a history, so even an
 * open position's past instants can only be reconstructed.
 */
export interface RuleReadoutSeries {
  position_id: string;
  mint_address: string;
  rule_id: string;
  source: 'replay';
  /** Row instants as epoch **milliseconds** — at one row per decision tick, quoted
   *  RFC3339 timestamps would be the largest line item in the payload. */
  at: number[];
  conditions: RuleConditionSeries[];
  /** The row cap cut the fold short; coverage ends at `covered_until`. */
  truncated: boolean;
  covered_until: string;
  /** Where coverage **starts**. On an entered position the row budget is spent around
   *  the entry, not around token creation, so this is not the first trade — a
   *  crosshair left of it has no row either. */
  covered_from: string;
  /** The window the server recorded, or `null` for the whole history. Distinguishes
   *  the two reasons the head can start late: with a window, rows to its left exist
   *  and were withheld; without one, the token had simply not traded yet. */
  record_from: string | null;
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
    // cached like the token list; a trade refresh invalidates the WalletHoldings
    // tag rather than re-fetching the whole wallet eagerly.
    getPortfolioHoldings: builder.query<WalletHolding[], void>({
      query: () => '/api/portfolio/holdings',
      providesTags: ['WalletHoldings'],
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
      (PortfolioWindow & { mode?: 'real' | 'paper' }) | void
    >({
      query: (arg) => {
        const a = arg && typeof arg === 'object' ? arg : {};
        const q = windowParams(a, 'today');
        q.set('mode', a.mode ?? 'real');
        return `/api/portfolio/performance?${q.toString()}`;
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
      (PortfolioWindow & { mode?: 'real' | 'paper'; ruleId?: string | null }) | void
    >({
      query: (arg) => {
        const a = arg && typeof arg === 'object' ? arg : {};
        const q = windowParams(a, '7d');
        q.set('mode', a.mode ?? 'real');
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
    /**
     * Append-only fill ledger for one position (entry + every sell leg).
     *
     * Tagged per position id so a scale-out leg that lands while the detail is
     * open refreshes both the ledger and the chart's exit arrows —
     * `useLivePositionFills` invalidates this off the position SSE. Without the
     * tag the cache shell's 5-minute retention would keep serving the pre-leg
     * ledger, and reopening the modal would not refetch either.
     */
    getPositionFills: builder.query<PositionFill[], string>({
      query: (positionId) =>
        `/api/strategies/generic/positions/${encodeURIComponent(positionId)}/fills`,
      providesTags: (_result, _error, positionId) => [
        { type: 'PositionFills' as const, id: positionId },
      ],
    }),
    /**
     * A position's rule conditions with the values behind them. One endpoint for
     * open and closed: the backend reads live engine state when it has the position
     * and replays stored trades when it does not, and `source` says which.
     *
     * Polled (see the caller's `pollingInterval`), never pushed: the position SSE
     * bus already carries one frame per ingested trade and sheds under feed load
     * (`live::api::handlers::strategies::action_progress`), so a per-tick metric
     * frame would degrade the stream the cockpit depends on. A replay answer never
     * changes, so the caller stops polling once it sees one.
     *
     * `at` selects the replay instant (`exit` = why it closed, `entry` = what it saw
     * when it bought) and is ignored on the engine path.
     */
    getPositionMetrics: builder.query<RuleReadout, { positionId: string; at?: ReadoutAt }>({
      query: ({ positionId, at }) =>
        `/api/strategies/generic/positions/${encodeURIComponent(positionId)}/metrics${
          at ? `?at=${at}` : ''
        }`,
    }),
    /**
     * The whole readout as a series — one fold per modal, fetched lazily on the
     * first crosshair move so modal-open cost is unchanged and the fold is
     * pay-per-use. Never polled: it is a reconstruction of the past, which does not
     * move (the tail past `covered_until` grows, but the crosshair lives inside the
     * span the chart already drew).
     */
    getPositionMetricSeries: builder.query<RuleReadoutSeries, string>({
      query: (positionId) =>
        `/api/strategies/generic/positions/${encodeURIComponent(positionId)}/metric-series`,
    }),
    /** The same readout for an ARMED (token, rule) pair — a Waiting row has no
     *  position id, so it keys on the pair instead. */
    getArmedMetrics: builder.query<RuleReadout, { mint: string; ruleId: string }>({
      query: ({ mint, ruleId }) =>
        `/api/strategies/armed/metrics?mint=${encodeURIComponent(mint)}&rule=${encodeURIComponent(ruleId)}`,
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
  useGetPortfolioPerformanceQuery,
  useGetPortfolioClosesSeriesQuery,
  useGetWalletPricesQuery,
  useManualBuyPositionMutation,
  useSetManualExitConfigMutation,
  useSellTokenMutation,
  useCloseRulePositionMutation,
  useGetPositionFillsQuery,
  useGetPositionMetricsQuery,
  useGetPositionMetricSeriesQuery,
  useGetArmedMetricsQuery,
  useGetCashbackStatusQuery,
  useClaimCashbackMutation,
  useGetLiveModeQuery,
  useSetLiveModeMutation,
  useReloadCachesMutation,
} = liveApi;
