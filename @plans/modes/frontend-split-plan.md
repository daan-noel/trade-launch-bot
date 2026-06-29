# Frontend split: two builds (deploy + analysis) over a shared core

> **STATUS: Phases 0–6 DONE** (committed). Phase 7 deferred — see the status section at the
> bottom. The old crate names (`backend-deploy`, `backend-local`, `backend-core`) in this doc
> refer to the state *at plan-write time* — the current names are `live`, `lab`, `trading_core`.

## Context

The Rust backend was split into two bins over a shared lib — `backend-deploy` (LIVE box:
real trading) and `backend-local` (ANALYSIS box: sweeps/backtests/swing over big data) —
sharing `backend-core`. The frontend is still **one SPA** that detects the mode at runtime
([`useCapabilities`](frontend-react/src/hooks/useCapabilities.ts) → `GET /api/system/capabilities`)
and toggles nav/routes from two boolean flags ([App.tsx](frontend-react/src/App.tsx#L37-L76),
[Header.tsx](frontend-react/src/components/layout/Header.tsx#L87-L127)).

We want the frontend split **explicitly**, mirroring the backend: a shared core plus two thin
app shells, built as **two Vite entries**. The deploy build ships only live-trading code to EC2;
the analysis build carries only the big-data code on the workstation. This makes the mode a
**build-time guarantee** instead of a runtime guess, gives each mode its own (lightly
differentiated) UI, and fixes a real contradiction: the TP/SL strategy pages currently call
endpoints that the backend they're routed against doesn't even serve.

**Decisions already made (do not revisit):**
- **Split model:** one `frontend-react` package reorganized into `src/shared` · `src/deploy` ·
  `src/analysis`; two Vite entries over the shared core.
- **Visual differentiation = LIGHT:** both modes share the same `AppLayout`/`Header` chrome and
  Tailwind theme; only the **nav item set** and **one accent color token** differ per mode.
- **Strategy pages split BY JOB** (see §7).
- **Local/analysis = dev-server only** (`vite dev` on the workstation). Only the deploy build is
  containerized; the existing `npm run build → dist/index.html → nginx` pipeline stays unchanged.
- **Plan scope = strictly frontend.** Backend route gaps (below) are noted as follow-ups, not
  implemented here.

## Backend route reality (drives the strategy split)

Verified from [configure_deploy_routes](backend-deploy/src/api/mod.rs#L22-L89),
[configure_local_routes](backend-local/src/api/mod.rs#L24-L173),
[configure_core_routes](backend-core/src/api/mod.rs#L14-L54):

| Strategy capability | Served by today |
| --- | --- |
| Rule **CRUD** (list/create/update/delete), **simulate** (+cancel), **paper-result** (get/delete) | **local** only |
| **Positions** reads (by-rule / list / by-mint / by-wallet / by-id), **manual sell** (`/solana/wallet/sell`), **live-mode** toggle | **deploy** only |
| Grouped **sweeps** | **local** only |
| **activate / pause / stop**, **`/matched`** | **neither bin registers an HTTP route** (logic exists in [backend-deploy lifecycle.rs](backend-deploy/src/strategies/tpsl_sniper_1/lifecycle.rs), called internally only) |

**Backend follow-ups (NOT in this plan — tracked dependencies for full deploy control):**
- Register rule **CRUD** + **activate/pause/stop** (and optionally `/matched`) on the **deploy**
  bin so its "Live Strategies" page is fully functional. The frontend will call these paths; they
  404 against deploy until wired.

## Target `src/` structure

Move whole directories wholesale into `shared/ · deploy/ · analysis/`, keeping each dir's
internal name so per-file import churn stays minimal (see §4). New entry folders (`deploy/`,
`analysis/`) each own `main.tsx`, `App.tsx`, `nav.ts`, a per-mode store, and per-mode service files.

```
frontend-react/
├─ index.html        # UNCHANGED path; <script src> repointed to /src/deploy/main.tsx
├─ analysis.html     # NEW (dev-only entry); <script src=/src/analysis/main.tsx>
├─ vite.config.ts    # add build-gated rollupOptions.input (§6)
├─ tsconfig.json     # add @shared/@deploy/@analysis aliases, repoint legacy aliases (§4)
├─ package.json      # scripts: dev / dev:local / build (§6)
└─ src/
   ├─ index.css, vite-env.d.ts            # shared, imported by both main.tsx
   ├─ shared/
   │  ├─ components/{ui, table, tokens, token-price-chart, dashboard, layout}/
   │  │     # layout/Header.tsx + AppLayout.tsx become PARAMETERIZED (§5)
   │  │     # + strategy/{tpsl1,tpsl2}/ shared rule form + columns (§7)
   │  ├─ context/{PriceUnitContext, TimezoneContext, AppProviders}.tsx   # AppProviders parameterized (§5)
   │  ├─ hooks/{useLocalStorage,useNow,useVisiblePolling,usePriceDisplay,
   │  │         useWalletPriceDisplay,useNotificationPrefs}.ts
   │  │     # + usePolledRules, useRulePositions (shared by both strategy pages, §7)
   │  ├─ lib/, utils/, types/
   │  ├─ services/{config.ts, sse.ts (subscribe core + shared connect*), http.ts (NEW: request<T>),
   │  │            strategyApi.ts (NEW: shared rule CRUD wrappers)}
   │  ├─ store/{baseApi.ts (NEW), sharedEndpoints.ts (NEW), api.ts (NEW barrel), hooks.ts (NEW)}
   │  └─ pages/{home, dashboard, tokens, profiles/OtherProfilesPage, settings, not-found}/
   ├─ deploy/
   │  ├─ main.tsx, App.tsx, nav.ts
   │  ├─ store/{index.ts, deployEndpoints.ts}
   │  ├─ slices/syncTokenSlice.ts
   │  ├─ services/{tradesApi.ts (sync ndjson), strategyApi.ts (lifecycle+positions+sell), sse.ts}
   │  ├─ hooks/{useTradeStream, usePositionNotifications}.ts
   │  ├─ components/{wallet, transactions, LiveModeControl}/
   │  └─ pages/{tokens/SyncTokenPage, transactions/TransactionsPage,
   │            profiles/MyWalletPage, strategies/LiveStrategiesPage (NEW)}/
   └─ analysis/
      ├─ main.tsx, App.tsx, nav.ts
      ├─ store/{index.ts, analysisEndpoints.ts, strategyResultCache.ts}
      ├─ slices/swingDetectionSlice.ts
      ├─ context/BackgroundJobsContext.tsx          # analysis-only
      ├─ services/{analysisApi.ts (simulate/paper/swings/jobs/sweeps), sse.ts}
      ├─ hooks/useStreamedSweepResults.ts
      ├─ components/{analysis, sweep, strategy, tpsl1, tpsl2, dashboard-grouped}/
      └─ pages/{analysis/{AnalysisPage,SwingDetectionPage},
                strategies/{AuthoringTpsl1Page,AuthoringTpsl2Page,sweep/*}}/
```

Mode classification (established by exploration):
- **Shared:** Home, Dashboard, Tokens, Settings, OtherProfiles; `ui/ table/ tokens/
  token-price-chart/ dashboard/ layout/`; all contexts except BackgroundJobs; `lib/ utils/
  types/`; shared hooks; RTK base + shared endpoints (tokens, profiles, settings, sol price,
  creation-stats heatmap, capabilities, tokensByMints, token detail/trades); **rule CRUD + rule form**.
- **Deploy-only:** Transactions, MyWallet, SyncToken; `wallet/ transactions/`; `useTradeStream,
  usePositionNotifications`; `syncTokenSlice`; endpoints wallet holdings/prices/buy/sell, cashback,
  live-mode, tpsl positions; lifecycle (activate/pause/stop) + sell; the live-mode kill switch.
- **Analysis-only:** Analysis, SwingDetection, GroupedSweep ×2; `analysis/ sweep/ strategy/`;
  `useStreamedSweepResults`; `swingDetectionSlice, strategyResultCache`, `BackgroundJobsContext`;
  endpoints grouped-sweep + strategy simulate/paper + grouped-creation-stats.

## §4 Path-alias scheme (minimize churn)

Add three aliases to [tsconfig.json](frontend-react/tsconfig.json#L19-L30) and **repoint the legacy
bare aliases into `shared/`** so the ~100 wholesale-moved shared files need **zero** import edits
(`vite-tsconfig-paths` already resolves them — no Vite change for aliasing):

```jsonc
"@shared/*": ["./src/shared/*"], "@deploy/*": ["./src/deploy/*"], "@analysis/*": ["./src/analysis/*"],
"components/*": ["./src/shared/components/*"], "hooks/*": ["./src/shared/hooks/*"],
"utils/*": ["./src/shared/utils/*"], "lib/*": ["./src/shared/lib/*"],
"context/*": ["./src/shared/context/*"], "types": ["./src/shared/types/index.ts"],
"types/*": ["./src/shared/types/*"], "services/*": ["./src/shared/services/*"],
"store/*": ["./src/shared/store/*"]
```

Only **cross-boundary** imports change (~40–50 files, a few lines each, `tsc`-guided):
1. `store/apiSlice` hook imports → the module that now owns each hook (mitigate with a
   `@shared/store/api.ts` barrel for shared hooks; mode hooks point at `@deploy|@analysis/store/*Endpoints`).
2. `services/api` / `services/sse` consumers in mode pages → `@deploy|@analysis/services/*`.
3. Relative `../../store` (AppDispatch type in strategy pages, `strategyResultCache`) → `@analysis/store`.

## §5 RTK Query + store split

**`shared/store/baseApi.ts`** — one `createApi` shell with `reducerPath:'api'`,
`fetchBaseQuery({baseUrl: API_BASE})`, `keepUnusedDataFor:300`, `refetchOnMountOrArgChange:false`,
`endpoints:()=>({})`, and **all 9 `tagTypes`** declared up front (`Settings, LiveMode,
WalletHoldings, StrategyResult, StrategyPaper, Profiles, Cashback, GroupedSweep, TokenBatch`) —
`injectEndpoints` cannot add tag types; unused tags per mode are inert.

Three `injectEndpoints` modules attach onto `baseApi`, each bundled only in its build:
- `shared/store/sharedEndpoints.ts` — tokens, profiles, settings (+optimistic `updateSettings`
  via `baseApi.util.updateQueryData`), solPrice, creation-stats heatmap, capabilities, tokensByMints,
  token detail/trades.
- `deploy/store/deployEndpoints.ts` — wallet holdings/holding/prices, buy/sell, cashback, live-mode,
  tpsl live positions.
- `analysis/store/analysisEndpoints.ts` — grouped-sweep CRUD + combo results, strategy
  simulate/paper, grouped-creation-stats, creators/analysis page reads. (Move `StrategyRuleArg`/
  `withAnalysisRange`/`strategyResultTag` helpers here.)

**Per-mode `configureStore`** — base reducers + only that mode's slice, importing its endpoint
modules **for side-effect** (guarantees injection before the store reads them) + `setupListeners`:
- `deploy/store/index.ts`: `{ syncToken, [baseApi.reducerPath]: baseApi.reducer }` + the existing
  `syncToken/mergeSyncOutput` serializableCheck ignores; imports `sharedEndpoints` + `deployEndpoints`.
- `analysis/store/index.ts`: `{ swingDetection, ... }` (drop the syncToken ignores); imports
  `sharedEndpoints` + `analysisEndpoints`.

`strategyResultCache.ts` (analysis) must repoint `apiSlice.util.*`→`baseApi.util.*`,
`apiSlice.endpoints.X`→the injected handle from `analysisEndpoints`, and import the **analysis** store.

## §5b App shells, nav config, parameterized chrome

**`<mode>/nav.ts`** exports a `NavConfig` `{ accent, items[], showLiveModeToggle }`:
- **Deploy nav** (accent = existing `primary`/teal, `showLiveModeToggle: true`): Home · Dashboard ·
  Tokens (All + Sync) · Transactions · Profiles (My wallets + Other) · **Live Strategies** · Settings.
  Tokens stays prominent — it's the **live-ingest monitor** (live trade SSE patches).
- **Analysis nav** (accent = one new token, e.g. violet, `showLiveModeToggle: false`): Home ·
  Dashboard · Tokens (All) · Analysis (General + Swing) · Profiles (Other) · Strategies (TPSL1/TPSL2
  authoring + Grouped Sweep ×2) · Settings.

**`shared/components/layout/Header.tsx`** — drop `useCapabilities`; take `{ nav: NavConfig }` and
render `nav.items` generically. The live-mode `StatusButton` + `useGetLiveMode/SetLiveMode` are
deploy-only hooks, so accept an optional `rightSlot?: ReactNode`; deploy passes
`<LiveModeControl/>` (from `@deploy/components`), analysis passes nothing. Shared TimezoneSelect +
PriceUnitToggle + `useGetSolPriceQuery` stay. Accent: replace hardcoded `text-primary`/`bg-primary/12`
with an `accentClasses[nav.accent]` lookup in `lib/`.

**`shared/components/layout/AppLayout.tsx`** — take `{ nav, header?, beforeMain?, footer? }`. Deploy
passes `header={<LiveModeControl/>}` + `beforeMain={<NotificationMount/>}` (mounts `usePositionNotifications`);
analysis passes `footer={<BackgroundJobsIndicator/>}`.

**`<mode>/App.tsx`** — `BrowserRouter → AppProviders → RouteErrorBoundary → Suspense → <Routes>`,
static (no `useCapabilities`, no gating; only that mode's lazy routes). `AppProviders` is
parameterized so **only analysis** wraps `BackgroundJobsProvider` (keeps its analysis SSE +
`getJobsStatus` out of the deploy bundle). **`<mode>/main.tsx`** — `createRoot(...).render(
<StrictMode><Provider store={modeStore}><App/></Provider></StrictMode>)`, importing the shared CSS.

## §6 Vite / scripts / Docker (deploy pipeline preserved)

- **`index.html`** (unchanged path): only `<script src>` changes to `/src/deploy/main.tsx`. It stays
  the default Vite entry → Rollup still emits `dist/index.html` → Dockerfile `COPY --from=build
  /app/dist` and nginx `try_files $uri /index.html` are byte-for-byte unchanged.
- **`analysis.html`** (NEW, dev-only): copy of index.html pointing at `/src/analysis/main.tsx`,
  served in dev at `/analysis.html`.
- **`vite.config.ts`**: gate the analysis entry out of production builds so the deploy image never
  ships analysis code:
  ```ts
  export default defineConfig(({ command }) => ({
    // ...existing envDir/plugins/server proxy unchanged...
    build: { rollupOptions: { input:
      command === 'build'
        ? { main: 'index.html' }                                  // prod: deploy only
        : { main: 'index.html', analysis: 'analysis.html' } } },  // dev: both
  }));
  ```
- **`package.json` scripts**: `"dev": "vite --host"` (open `/` deploy, `/analysis.html` analysis),
  `"dev:local": "vite --host --open /analysis.html"`, `"build": "tsc && vite build"` **(unchanged
  command → deploy-only `dist/index.html`)`. `tsc` now type-checks both trees (both must compile).
- **Dockerfile / nginx / docker-compose: NO changes.** Analysis is never containerized.

## §7 Strategy pages (split by job; CRUD + rule form shared)

**Shared (`shared/components/strategy/{tpsl1,tpsl2}/` + `shared/hooks/` + `shared/services/strategyApi.ts`):**
`RuleFormModal` (+ build/empty/from-rule helpers, `LockGroupState`), `ruleColumns` (+`RuleRowProvider`),
`tableColumns` (`matched/position/simColumns`), `SimSummaryCard`, `TokenInspectModal`,
`usePolledRules`, `useRulePositions`, and the **rule CRUD** wrappers (list/get/create/update/delete).
SSE subscribers they share (`connectTpslRulesChanged`, `connectTpslPositionsChanged`) live in
`shared/services/sse.ts` (pure subscribers over the shared EventSource; harmless if no frames arrive).

**Analysis — `AuthoringTpsl1Page` / `AuthoringTpsl2Page`** (against local backend): rule list + CRUD
(shared form), **Simulate**, **Paper-backtest** (+ paper-test SSE), Analysis-window date range,
`SimSummaryCard`, `TokenInspectModal`, BackgroundJobs sim tracking. **Removes** activate/pause/stop,
live positions, sell, `/matched` (local doesn't serve them).

**Deploy — `LiveStrategiesPage`** (full live control, tpsl1+tpsl2 tabbed): rule list + CRUD (shared
form), **activate / pause / stop**, **live positions monitor** (`useRulePositions` + `positionColumns`),
**manual Sell/Close** (`useSellTokenMutation` + `stop`-force-close), positions summary
(`SimSummaryCard`). Lifecycle + CRUD wrappers in `deploy/services/strategyApi.ts`. **Depends on the
backend follow-up** registering CRUD + lifecycle on the deploy bin (calls 404 until then — documented).
Route: `/strategies/live`.

**Dashboard gotcha:** `DashboardPage` (shared) embeds `GroupedCreationSection`, which calls
analysis-only `getGroupedCreationStats` and imports `components/sweep/*`. Give `DashboardPage` an
optional `extraSections?: ReactNode`; move the grouped section to `@analysis/components/dashboard-grouped/`.
Analysis route renders `<DashboardPage extraSections={<GroupedCreationSection/>}/>`; deploy renders
`<DashboardPage/>` (heatmap + creation-trend only, both via shared `getCreationStats`).

## Reuse (do not rebuild)

`components/ui/*`, `components/table/DataTable`, `components/tokens/*`, `token-price-chart/*`,
`dashboard/*` heatmap, all contexts, every hook — moved into `shared/`, consumed unchanged via the
repointed bare aliases. `injectEndpoints` is the idiomatic RTK Query code-split — reuse the existing
endpoint bodies verbatim, just relocated. Nav renders through the existing `NavItem`/`NavDropdown`.

## Status (2026-06-26)

**Phases 0–6 DONE and `npm run build`-green.** Two isolated builds over a shared core ship: prod
`npm run build` emits **deploy-only** `dist/index.html` (verified: no `dist/analysis.html`, no
sweep/swing/grouped chunks); the deploy bundle has **zero** `@analysis` imports. Per-mode stores,
two Vite entries, parameterized `Header`/`AppLayout`/`AppProviders`, static per-mode `nav.ts` +
`App.tsx`, and the Dashboard grouped-section render-prop split are all in place; `useCapabilities`
gating is removed. `tokenTradeColumns` was reclassified shared (used by the shared `TokenTradeChart`).

**Deviations from the plan (intentional, documented in `@arch/frontend.md`):**

- Shared store core lives in `src/shared/store` (not a top-level `src/store`); the legacy `store/*`
  alias resolves there. Shared dispatch uses a mode-agnostic `shared/store/types.ts#AppDispatch`.
- The per-mode `services/strategyApi.ts` / `analysisApi.ts` / `sse.ts` file split was **skipped** —
  `services/api.ts` + `services/sse.ts` stay shared and side-effect-free, so tree-shaking gives the
  same per-entry bundle isolation without the churn.

**Phase 7 DEFERRED** (strategy split by job). The analysis `Tpsl{1,2}Page` still carry live-only
controls and import `useSellTokenMutation` from `@deploy` (a workstation-only bundle leak; deploy is
clean). It is **blocked on the backend follow-up below** — building deploy `LiveStrategiesPage`
(`/strategies/live`) is pointless until the deploy bin serves rule CRUD + lifecycle. **Phase 8** docs
are done for the delivered scope.

### Backend follow-up (tracked dependency, NOT done)

Register rule **CRUD + activate/pause/stop** (+ `/matched`) on the **deploy** bin
(`backend-deploy/src/api/mod.rs`); the logic exists in
`backend-deploy/src/strategies/tpsl_sniper_1/lifecycle.rs` but has no HTTP route. Until then a deploy
Live-Strategies page 404s. Once wired, finish Phase 7: promote shared tpsl parts to
`shared/components/strategy/`, build `LiveStrategiesPage` (deploy) + `AuthoringTpsl{1,2}Page`
(analysis), and drop the live-only controls (+ the `@deploy` sell import) from the analysis pages.

## Phased execution (each phase ends `npm run build`-green)

0. **Aliases + base-API shell, no moves.** Add the 3 aliases; split `apiSlice.ts` into
   `baseApi` + `sharedEndpoints` + co-located deploy/analysis injections; keep the single store
   importing all three for side-effect; re-export all hooks from a barrel so imports compile unchanged.
1. **Create `shared/`; repoint legacy aliases** at `src/shared/*`. Move the ~100 pure-shared dirs.
2. **Extract shared service helpers** (`http.ts` `request<T>`; split `sse.ts` core vs mode connect*).
3. **Carve out `analysis/`** (components/pages/slices/context/endpoints/services); repoint cross-boundary
   imports to `@analysis/*`; keep the legacy single store importing analysis endpoints temporarily.
4. **Carve out `deploy/`** (components/pages/slices/hooks/endpoints/services).
5. **Per-mode stores + two entries:** `deploy|analysis/store/index.ts`, `main.tsx`, `analysis.html`,
   repoint `index.html`, update `vite.config.ts` + scripts; delete old `src/main.tsx` + `src/store/index.ts`.
   Verify prod build emits `dist/index.html` and **NOT** `dist/analysis.html`.
6. **Parameterize Header/AppLayout/AppProviders + nav configs + per-mode `App.tsx`;** remove
   `useCapabilities` gating. Manually run `dev` (deploy) and `dev:local` (analysis).
7. **Strategy split:** promote shared tpsl parts to `shared/`; build `LiveStrategiesPage` (deploy) +
   `AuthoringTpslXPage` (analysis); split the Dashboard grouped section.
8. **Docs:** update the frontend architecture doc (`@arch/frontend.md`, referenced by CLAUDE.md —
   create if absent) + CLAUDE.md frontend command notes; record the backend follow-up (deploy
   CRUD+lifecycle routes) in the relevant `@plans/` doc.

## Riskiest gotchas

- **`injectEndpoints` ordering:** endpoint modules MUST be imported for side-effect before the store
  reads them, else `endpoints.X` is undefined at runtime. `tagTypes` can't be injected — keep all 9 on
  `baseApi`; `updateSettings.onQueryStarted` must use `baseApi.util.updateQueryData('getSettings', …)`.
- **apiSlice-split import churn (~30 files):** every `from 'store/apiSlice'` hook import repoints to its
  new owning module; `tsc` catches misses. Use a shared barrel to keep shared-page imports one-liners.
- **Context isolation:** `BackgroundJobsContext` must exist ONLY in the analysis tree (its
  `getJobsStatus` seed is analysis-only); ensure no shared component calls `useBackgroundJobs*`, or the
  deploy build crashes at runtime. `usePositionNotifications` mounts only in deploy's AppLayout slot.
- **`tsc` compiles both trees** under one `npm run build` — a type error in analysis breaks the deploy
  build (acceptable; single package).
- **Deploy strategy 404s** until the backend follow-up wires CRUD + lifecycle on the deploy bin —
  expected and documented; the LiveStrategiesPage should degrade gracefully (error states, not crash).

## Verification

- `cd frontend-react && npm run build` → green; confirm `dist/index.html` exists and
  `dist/analysis.html` does **not** (prove deploy bundle is analysis-free; spot-check the chunk list
  has no `sweep/` or `swingDetection` code).
- `npm run dev:local` → analysis shell at `/analysis.html`; run `backend-local` (port 8081); verify
  nav shows Analysis/Strategies(authoring)/Sweeps, **no** live-mode toggle, Dashboard shows the grouped
  section, no console 404s for deploy-only endpoints.
- `npm run dev` → deploy shell at `/`; run `backend-deploy`; verify nav shows Transactions/My
  wallets/Live Strategies + the LIVE/DEAD kill switch, Tokens page receives live trade SSE patches
  (live-ingest monitor), no console 404s for analysis-only endpoints. (Live-strategy CRUD/lifecycle
  calls 404 until the backend follow-up — expected.)
- Existing deploy Docker path unchanged: `docker compose build web` still produces an nginx image
  serving `dist/index.html`.
