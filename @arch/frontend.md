# Frontend — `frontend-react/` (React SPA, **two builds over a shared core**)

Stack: React 19 + TS + Vite, RTK Query + Redux Toolkit, React Router 7, Tailwind 4, lightweight-charts.
Deep-dive detail: `@plans/frontend/frontend-patterns.md`, `@plans/modes/frontend-split-plan.md` (the split itself), `@plans/token-analysis/*`.

## Split model (mirrors the backend two-bin split)

One `frontend-react` package, **three source trees** + **two Vite entries** — the mode is a
**build-time guarantee**, not a runtime `useCapabilities` guess:

| Tree | Alias | Bundled by |
| --- | --- | --- |
| `src/shared/` | `@shared/*` (+ legacy bare aliases `components/ hooks/ utils/ lib/ context/ types/ services/ store/` all repointed here) | both |
| `src/deploy/` | `@deploy/*` | deploy (LIVE) build only |
| `src/analysis/` | `@analysis/*` | analysis (workstation) build only |
| `src/pages/` | `pages/*` | shared pages (Home, Dashboard, Tokens, OtherProfiles, Settings, NotFound) |

- **Entries:** `index.html → /src/deploy/main.tsx` (the default; Rollup emits `dist/index.html` →
  Docker/nginx unchanged). `analysis.html → /src/analysis/main.tsx` (**dev-only**).
  `vite.config.ts` gates `rollupOptions.input` on `command === 'build'`: **prod builds emit DEPLOY
  ONLY** (no `analysis.html`, no sweep/swing/grouped chunks — the EC2 image is analysis-free).
- **Scripts:** `npm run dev` (deploy at `/`, analysis at `/analysis.html`), `npm run dev:local`
  (opens `/analysis.html`), `npm run build` (deploy-only `dist/index.html`). `tsc` type-checks
  **both** trees, so an analysis type error fails the deploy build (acceptable; single package).
- **Per-mode `App.tsx` + `nav.ts`:** static route table + `NavConfig` (`{accent, items[]}`), no
  gating. Deploy nav = teal accent + Live-mode toggle; analysis nav = violet accent, no toggle.

## Store — split `createApi` (the isolation seam)

- `shared/store/baseApi.ts` — the `createApi` **shell**: `reducerPath:'api'`, `keepUnusedDataFor:300`,
  **all 9 `tagTypes` declared up front** (`injectEndpoints` can't add tag types), `endpoints:()=>({})`.
- Three `injectEndpoints` modules attach onto it, each bundled only in its build:
  `shared/store/sharedEndpoints.ts` (tokens, profiles, settings+optimistic, solPrice, creation
  heatmap, capabilities), `deploy/store/deployEndpoints.ts` (wallet holdings/prices, buy/sell,
  cashback, **live-mode**), `analysis/store/analysisEndpoints.ts` (grouped sweeps, strategy
  simulate/paper, grouped creation stats, creators/analysis).
- `shared/store/apiSlice.ts` is a **barrel re-exporting SHARED endpoints only** (+ `apiErrorMessage`,
  and `baseApi` as `apiSlice` for `util.invalidateTags`). Mode hooks are imported **directly** from
  `@deploy|@analysis/store/*Endpoints` so neither mode's `injectEndpoints` **side effect** leaks into
  the other bundle. Typed `.endpoints.X` / `util.updateQueryData('X')` callers import the owning
  typed api (`sharedApi` / `deployApi` / `analysisApi`).
- **Per-mode `configureStore`** (`deploy/store/index.ts`, `analysis/store/index.ts`): base reducer +
  only that mode's slice (`syncToken` / `swingDetection`), importing its endpoint modules
  **for side-effect** before the store reads them, + `setupListeners`. Each exports its own
  `RootState`/`AppDispatch`. Shared components import a **mode-agnostic** `AppDispatch` from
  `shared/store/types.ts` (`ThunkDispatch<any,…>`), assignable to either store.

## Parameterized chrome — `shared/components/layout/`

- `Header.tsx` — data-driven from `NavConfig` (`navTypes.ts`); accent via `lib/accent.ts`
  (`accentClasses[teal|violet]`). Live-mode kill switch injected via `rightSlot` (deploy passes
  `@deploy/components/LiveModeControl`). Shared: SOL/USD mirror, timezone, price-unit toggle.
- `AppLayout.tsx` — slots `{nav, rightSlot, beforeMain, footer}`: deploy passes
  `beforeMain=<NotificationMount/>` (mounts `usePositionNotifications`); analysis passes
  `footer=<BackgroundJobsIndicator/>`. `AppProviders` is mode-neutral (Timezone+PriceUnit+Toast);
  **analysis `App` nests `BackgroundJobsProvider` itself** (keeps its SSE out of deploy).
- **Dashboard split:** shared `DashboardPage` takes `extraSections?(ctx)` render-prop; analysis
  injects `GroupedCreationSection`, so the deploy build never pulls `getGroupedCreationStats`.

## Pages by mode

- **Shared:** Home, Dashboard, Tokens (live-ingest monitor — `token_created`/`trade_executed` SSE),
  OtherProfiles, Settings, NotFound.
- **Deploy (`@deploy/pages`):** SyncToken, Transactions, MyWallet (+ `InputSyncStatus`, `wallet/`,
  `transactions/` components; `useTradeStream`, `usePositionNotifications`; `syncTokenSlice`).
- **Analysis (`@analysis/pages`):** Analysis, SwingDetection, Tpsl{1,2}Page (authoring), Grouped
  Sweep ×2 (+ `analysis/`, `sweep/`, `strategy/` components; `useStreamedSweepResults`;
  `swingDetectionSlice`, `strategyResultCache`, `BackgroundJobsContext`).

## Services / hooks (shared, tree-shaken per entry)

- `services/http.ts` — shared `request<T>` fetch wrapper (throws backend `{error}`; 204→undefined).
- `services/api.ts` (imperative helpers) + `services/sse.ts` (single shared `EventSource`,
  `connect*` subscribers) stay shared and side-effect-free, so each entry tree-shakes the calls it
  doesn't use.

## Perf patterns (unchanged)

- Single SSE `EventSource` multiplexes all streams; positions/rules = delta patch + visibility-gated
  fallback poll. RTK Query structural sharing + 5min `keepUnusedDataFor` + `skipPollingIfUnfocused`.
- Memoized column defs/price formatters; cells read context directly. localStorage via `lib/storage`
  (`mt:` namespace); column visibility in one `mt:table.cols` map keyed by `tableId`.

## Known follow-ups (NOT yet done)

- **Strategy split by job (plan §7):** the analysis `Tpsl{1,2}Page` still carry live-only controls
  (activate/pause/stop, manual sell) that the **local** backend doesn't serve — so the analysis
  build still imports `useSellTokenMutation` from `@deploy/store/deployEndpoints` (a workstation-only
  leak; the deploy bundle is clean). Splitting into deploy `LiveStrategiesPage` (`/strategies/live`,
  full live control) + analysis `AuthoringTpsl{1,2}Page` (CRUD + simulate + paper only) is pending —
  it **depends on the backend follow-up** below.
- **Backend follow-up:** register rule **CRUD + activate/pause/stop** (and `/matched`) on the
  **deploy** bin; until then a deploy Live-Strategies page would 404. See
  `backend-deploy/src/strategies/tpsl_sniper_1/lifecycle.rs` (logic exists, no HTTP route).
- **Cosmetic deviation:** shared store core lives in `src/shared/store` but the legacy `store/*`
  alias still resolves there; the `deploy/services/strategyApi.ts` / `analysis/services/analysisApi.ts`
  file-level split was skipped (tree-shaking over one shared `services/api.ts` achieves the same
  bundle isolation since the helpers are side-effect-free).
