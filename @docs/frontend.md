# Frontend — `frontend-react/` (React SPA)

File-level map of `frontend-react/src/`. Stack: React 19 + TS + Vite, RTK Query + Redux Toolkit, React Router 7, Tailwind 4, lightweight-charts.
Dev: `npm run dev` (:5173, proxies `/api` → `:8081`). Build: `npm run build` (`tsc && vite build`).
Chart logic explainer: `@project_plans/token-analysis/token-history-chart-functionalities.md`, swing: `@project_plans/token-analysis/swing-detection-logic.md`.

## Entry & routing
- `main.tsx` — React root + Redux Provider.
- `App.tsx` — `BrowserRouter`; all pages nested under `<AppLayout>`. Routes: `/`, `/dashboard`, `/tokens`, `/token/sync`, `/transactions`, `/analysis/{general,swing-detection}`, `/profiles/{mine,other}`, `/strategies/{tpsl1,tpsl2}`, `/strategies/grouped-sweep`, `/settings`, `*`.

## Layout — `components/layout/`
`AppLayout.tsx` (Header + `<Outlet>`), `Header.tsx` (nav, live-mode toggle, SOL/USD fetch→PriceUnitContext, PriceUnitToggle, TimezoneSelect), `PageShell.tsx`.

## Pages — `pages/`
| Page | File | Notes |
|---|---|---|
| Home | `home/HomePage.tsx` | hero |
| Dashboard / Transactions | `dashboard/DashboardPage.tsx`, `transactions/TransactionsPage.tsx` | live trades via `useTradeStream()` + `tradeColumns()`, max 500 buffered |
| Tokens | `tokens/TokensPage.tsx` | server-side paginated DataTable (`getTokensPage`), SSE `token_created` refetch, memoized columns |
| Sync token | `tokens/SyncTokenPage.tsx` | single-mint sync w/ progress, `syncTokenSlice`, chart + trades table |
| Analysis | `analysis/AnalysisPage.tsx` | creators + results tables (`/api/analysis`, `/api/creators`) |
| Swing detection | `analysis/SwingDetectionPage.tsx` | server-paged tokens (`getTokensPage`), per-token + batch swing detect (chunks run ≤3 concurrent), `getProfiles` markers, chart overlay, `swingDetectionSlice` |
| My wallet | `profiles/MyWalletPage.tsx` | `getWalletHoldings` (cached 5min), `getWalletPrices` poll 20s, header **Manual Buy** + **Manual Sell** modals (`parseSlippageBps` shared), row Buy / **Sell All**. Sell sends no amount (backend clears the full balance + closes the account); `confirmTrade` polls the single traded mint until its raw amount moves, then patches/removes that one row |
| Other profiles | `profiles/OtherProfilesPage.tsx` | profile/wallet/tag CRUD |
| Settings | `settings/SettingsPage.tsx` | track_mayhem, track_post_migration, persist_raw, slippage_bps; optimistic update |
| TPSL1 / TPSL2 | `strategies/Tpsl{1,2}Page.tsx` | `usePolledRules`, `useRulePositions`; rule CRUD + lifecycle; sim/matched/paper reads routed through the RTK cache via `store/strategyResultCache.ts` (toggle state stays local); `SimProgressBar` (wraps shared `ui/ProgressBar`) shows determinate backtest progress from the `simulation_progress` SSE (real `processed/total`, not a trickle) **+ a Cancel button** → `cancelSimulateTpsl{1,2}Rule`; a cancelled run resolves to `{cancelled:true}` (handler drops the cached marker via `invalidateStrategyResult`) (near-identical) |
| Grouped Sweep | `strategies/GroupedSweepPage.tsx` | Generic across strategies (TPSL2 wired today). `SweepConfigForm` (created-at range, group-by field picker, editable param grid prefilled w/ backend defaults, method grid/random:N, min-tokens, curve-only, projected combo-count badge that blocks Run over the cap) → `startGroupedSweep`. While running, `SweepProgressBar` (wraps shared `ui/ProgressBar`) shows honest `processed/total` from the `sweep_progress` SSE + a Cancel button → `cancelGroupedSweep` (start resolves to `{cancelled:true}` on abort). Run picker → **group-summary** DataTable (`buildGroupColumns`, best-expectancy sort, `selectable`) → click a group → **drill-in** combo DataTable (reuses `buildSweepColumns`). Hooks `getGroupedSweep{Runs,Groups,Results}` (lazy results, `GroupedSweep` tag) |

## Components — `components/`
- **`ui/`** reusable primitives: `Button`(+`ButtonGroup`), `Badge`, `Modal`(+`InlineAlert`), `Input`/`Textarea`, `Checkbox`, `Switch`, `Select`, `Tabs`, `PriceUnitToggle`, `TimezoneSelect`, `StatusButton`, `AddressDisplay`, `StatCard`, `NavDropdown`, `InfoTooltip`, `VisibilityToggleButton`, `ProgressBar` (honest determinate bar + optional Cancel — shared by sim + sweep). All compose via `cn()` (`lib/cn.ts`).
- **`table/` DataTable** — generic `DataTable<R>`: client- or server-side mode, pagination (`Pagination.tsx`, localStorage), sort, per-column filter (`numericFilter.ts`), column visibility, selection, search, **memoized rows**, pure-CSS `:has()` hover. Cells: `DateCell`, `RelativeTimeCell`, `AgeCell` (subscribe `useNow`). Types in `types.ts` (`ColumnDef`, `TableQuery`).
- **`tokens/`** — `tokenColumns()`, `priceCells.tsx` (factories that bake formatter once, ignore rate ticks), `TokenDetailPanel`, `TokenTradeChart`, `FilterPanel` + `filters.ts` (`TokenFilters`, localStorage).
- **`token-price-chart/`** — lightweight-charts wrapper (`TokenPriceChart`). Transforms `chartBars.ts`, `swingOverlay.ts`, `chartViewport.ts` (localStorage per mint), `chartTimezone.ts`. Plugins: `walletMarkersPlugin`, `chainHighlightPlugin`, `rangeSelectPlugin`. Tooltips: Bar/Swing/WalletMarkers/ChainHighlight/RangeSelect. `index.ts` exports `aggregateTradesToBars`, `barsTo{Line,Candle}Data`, `swingsToColoredLineData`, etc.
- **`analysis/`** — `swingColumns`, `swingChainColumns`, `analysisColumns`/`creatorColumns`, `swingParams.tsx`, `swingFilter.ts`, `swingChains.ts`.
- **`tpsl1/` & `tpsl2/`** (near-identical) — `ruleColumns`, `tableColumns` (`simColumns`/`matchedColumns`/`positionColumns`), `RuleFormModal`, `SimSummaryCard`, `TokenInspectModal`, `utils.ts`.
  - **`RuleFormModal` per-group locks**: when editing, fields are grouped (`fingerprint`/`sizing`/`exit`, plus `entry` in tpsl2) by `LockGroup`, each locked by default with its own 🔓/🔒 toggle to prevent accidental edits. A *live* rule (`is_active || open_positions > 0`) freezes every group except `sizing` (buy amount + concurrency caps — the hot knobs safe mid-run); those toggles are disabled. `buildUpdatePayload(form, unlocked)` emits keys for **unlocked groups only**, so a locked param can't be zeroed (backend merge leaves it untouched). On the rule table, Edit is always enabled (so hot params stay reachable) but Delete is disabled while live.
- **`transactions/`** — `tradeColumns(price)`, `tokenTradeColumns(price)`. **`wallet/`** — `walletColumns(price)`.
- **`sweep/`** — sweep page helpers (shared by the flat + grouped pages). `buildSweepColumns(paramKeys)` (param cols derived from run params + metric cols: win%, total/expectancy/median PnL, profit factor, holding, exit mix; whole table ordered by one `COLUMN_ORDER` list — param keys prefixed `p_<k>`), `types.ts` (`SweepRunRecord`, `SweepResultRecord`). Grouped-sweep additions: `groupedTypes.ts` (`GROUP_FIELDS`/labels, `TPSL2_AXES` editable-axis defs mirroring backend defaults, run/group/result + start-arg types), `groupColumns.tsx` (`buildGroupColumns` — fingerprint-key chips, token/fired counts, best-expectancy, best-combo chips), `SweepConfigForm.tsx` (the config form; parses the comma-separated grid, projects combo count vs `MAX_COMBOS`).

## State, services, hooks
- **`store/`** — `index.ts` (store; RTK Query 5min cache, `refetchOnMountOrArgChange:false`), `apiSlice.ts` (queries: `getTokens`, `getTokensPage`, `getTokenDetail/Trades`, `getWalletHoldings/Holding/Prices`, `getSolPrice`, `getLiveMode`, `getSettings`, `getProfiles` (tracked-wallet markers; `Profiles` tag, 120s retention — shared by Swing + Sync), `getStrategy{Matched,Simulate,PaperResult}` (per-rule, 60s retention, `StrategyResult`/`StrategyPaper` tags; `getStrategySimulate` resolves to `SimulatedTokenResult[] | {cancelled:true}`), `getGroupedSweep{Runs,Groups,Results}` (grouped param-sweep, 120s retention; runs tagged `GroupedSweep`; all keyed by `strategyId`, results fetched lazily on group drill-in); mutations: `buyToken`, `sellToken`, `setLiveMode`, `updateSettings`, `startGroupedSweep` (DB-range grouped sweep, invalidates `GroupedSweep`)), `strategyResultCache.ts` (imperative `fetch{Matched,Simulate,PaperResult}Cached` + `invalidateStrategyResult` — dispatch→unwrap→unsubscribe so the strategy pages route their toggle-driven reads through the RTK cache), `swingDetectionSlice.ts`, `syncTokenSlice.ts`.
- **`services/`** — `api.ts` (standalone fetch helpers: tpsl{1,2} CRUD/lifecycle/sim/positions, `cancelSimulateTpsl{1,2}Rule` + `cancelGroupedSweep`, swing detect, profiles/tags, `syncToken` w/ progress), `sse.ts` (single shared `EventSource` to `/api/stream`; `connectTradeStream`, `connectTokenCreatedStream`, `connectPaperTestStream`, `connectSimulationProgress`, `connectSweepProgress`, `connectTpsl{RulesChanged,PositionsChanged}`), `config.ts` (`API_BASE`, `POLL_INTERVAL_MS`, `FALLBACK_POLL_INTERVAL_MS`).
- **`context/`** — `AppProviders`, `PriceUnitContext` (`usePriceUnit`: unit + usdRate, localStorage↔backend), `TimezoneContext` (`useTimezone`).
- **`hooks/`** — `useNow(granularityMs)` (shared clock, one interval/granularity, pauses when hidden), `useVisiblePolling`, `useTradeStream`, `usePolledRules`, `useRulePositions`, `usePriceDisplay` (rate folded out in SOL mode), `useWalletPriceDisplay`.
- **`types/index.ts`** — `TokenRecord`, `TokenDetailRecord`, `TradeRecord`, `RuleRecord`, `RulePositionRecord`, `WalletHolding`, `WalletPrice`, `LiveTrade`, `SwingDetectionResult`, paper/sim shapes, `WalletProfile`/`WalletEntry`/`WalletProfileTag`.
- **`utils/`** — `format.ts` (formatDecimal, formatPrice, formatCompact, formatAge, formatUsd, color classes), `date.ts` (timezone-aware), `addressLinks.ts` (Solscan/GMGN). **`lib/`** — `cn.ts`, `tpslParamHelp.ts`.

## Perf patterns (CLAUDE.md "Definition of done" — verify no extra re-render on rate/trade ticks)
- Column defs + price formatters memoized; cells read context directly (no prop re-thread); rate folded out in SOL mode.
- Shared `useNow` clock; cells bail when coarsened snapshot unchanged.
- Single SSE `EventSource` multiplexes all events; rules/positions = SSE refetch + debounce + visibility-gated fallback poll.
- RTK Query structural sharing + `keepUnusedDataFor` 5min + `skipPollingIfUnfocused`; optimistic mutations.
- localStorage caches page/sort/filters/column-visibility, timezone, price unit, chart viewport, sync previews.
