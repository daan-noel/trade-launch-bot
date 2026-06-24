# Frontend — `frontend-react/` (React SPA)

File-level map of `frontend-react/src/`. Stack: React 19 + TS + Vite, RTK Query + Redux Toolkit, React Router 7, Tailwind 4, lightweight-charts.
Deep-dive detail: `@plans/frontend/frontend-patterns.md`, `@plans/token-analysis/token-history-chart-functionalities.md`, `@plans/token-analysis/swing-detection-logic.md`.

## Entry & routing

- `main.tsx` — React root + Redux Provider
- `App.tsx` — `BrowserRouter`; all pages nested under `<AppLayout>`; every route **code-split** via `React.lazy`; wrapped in one `<Suspense>`

## Pages — `pages/`

| Page | File | Notes |
| --- | --- | --- |
| Home | `home/HomePage.tsx` | hero |
| Dashboard | `dashboard/DashboardPage.tsx` | Token-creation-time bias panels; no SOL/USD subscription (no tick re-renders) |
| Transactions | `transactions/TransactionsPage.tsx` | Live trades via `useTradeStream()`, max 500 buffered |
| Tokens | `tokens/TokensPage.tsx` | Server-side paginated DataTable; SSE `token_created` refetch; two count badges (total / tracked) |
| Sync token | `tokens/SyncTokenPage.tsx` | Multi-mint sync w/ progress; bounded worker pools (DB ≤5, Helius ≤4) |
| Analysis | `analysis/AnalysisPage.tsx` | Creators + results tables |
| Swing detection | `analysis/SwingDetectionPage.tsx` | "Run All" = one detached backend job; chain columns sort server-side |
| My wallet | `profiles/MyWalletPage.tsx` | Manual Buy + Manual Sell modals; `confirmTrade` polls until raw amount moves |
| Other profiles | `profiles/OtherProfilesPage.tsx` | Profile/wallet/tag CRUD; optimistic RTK cache patches |
| Settings | `settings/SettingsPage.tsx` | watchdog, slippage_bps, max_committed_sol, persist_raw; optimistic update |
| TPSL1 / TPSL2 | `strategies/Tpsl{1,2}Page.tsx` | Rules + positions; delta-driven (no refetch on position churn); sim = start→wait→fetch (202) |
| Grouped Sweep | `strategies/sweep/GroupedSweepView.tsx` | Generic view; two thin wrappers (Tpsl1/Tpsl2); config form + run picker + group summary + drill-in combo table |

## Components — `components/`

- **`ui/`** — `Button`(+Group), `Badge`, `Modal`, `Input`/`Textarea`, `Checkbox`, `Switch`, `Select`, `Tabs`, `StatusButton`, `AddressDisplay`, `StatCard`, `ProgressBar`, `InfoTooltip`, `SuspenseFallback`. All compose via `cn()`.
- **`table/DataTable`** — generic `DataTable<R>`: client/server-side mode, pagination, sort, per-column filter, column-visibility (persisted by `tableId` into `mt:table.cols`), selection, memoized rows. `ColumnDef.renderHeader(SortCtx)` enables hidden sort-only columns.
- **`dashboard/`** — `CreationHeatmap`, `CreationTrendChart`, `GroupedCreationSection`, `GroupedCreationTrendChart`
- **`tokens/`** — `tokenColumns()`, `priceCells.tsx`, `TokenDetailPanel`, `TokenTradeChart`, `FilterPanel`
- **`token-price-chart/`** — lightweight-charts wrapper; `walletMarkersPlugin`, `chainHighlightPlugin`, `rangeSelectPlugin`; crosshair coalesced via `requestAnimationFrame`
- **`tpsl1/` & `tpsl2/`** — `ruleColumns`, `tableColumns`, `RuleFormModal` (per-group locks), `SimSummaryCard`, `TokenInspectModal`
- **`sweep/`** — `buildSweepColumns`, `buildGroupColumns`, `SweepConfigForm`, `FingerprintGroupPicker` (shared with dashboard), `fingerprintFilters.ts`
- **`layout/`** — `AppLayout`, `Header`, `BackgroundJobsIndicator` (app-wide long-running job registry)

## State, services, hooks

- **`store/`** — `apiSlice.ts` (RTK Query: all queries + mutations); `strategyResultCache.ts` (imperative start→wait→fetch for sim/swing); `swingDetectionSlice.ts`, `syncTokenSlice.ts`
- **`services/`** — `api.ts` (standalone fetch helpers); `sse.ts` (single shared `EventSource`; `connectTpslPositionsChanged` delivers `TpslPositionDelta` for in-place row patching)
- **`context/`** — `PriceUnitContext`, `TimezoneContext`, `BackgroundJobsContext` (split into stable actions + ticking state to minimize re-renders)
- **`hooks/`** — `useNow(granularityMs)`, `useTradeStream`, `usePolledRules`, `useRulePositions`, `usePriceDisplay`, `useLocalStorage`

## Perf patterns

- Column defs + price formatters memoized; cells read context directly (no prop re-thread)
- Single SSE `EventSource` multiplexes all streams; positions/rules = delta patch + debounce + visibility-gated fallback poll
- RTK Query structural sharing + 5min `keepUnusedDataFor` + `skipPollingIfUnfocused`
- All localStorage via `lib/storage` (keys namespaced `mt:`); column visibility shared in one `mt:table.cols` map keyed by `tableId`
