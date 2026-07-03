# Frontend — `frontend-react/` (React SPA, **two apps over a shared core**)

Stack: React 19 + TS + Vite, RTK Query + Redux Toolkit, React Router 7, Tailwind 4, lightweight-charts.
Deep-dive detail: `@plans/frontend/frontend-patterns.md`, `@plans/modes/frontend-split-plan.md` (the split itself), `@plans/token-analysis/*`.

## Split model (mirrors the backend two-bin split)

One `frontend-react` package, **three source trees** + **two Vite entries running as two dev
servers** — the mode is a **build-time guarantee**, not a runtime `useCapabilities` guess:

| Tree | Alias | Bundled by |
| --- | --- | --- |
| `src/shared/` | `@shared/*` (+ legacy bare aliases `components/ hooks/ utils/ lib/ context/ types/ services/ store/` all repointed here) | both |
| `src/live/` | `@live/*` | live (LIVE/EC2) app only |
| `src/lab/` | `@lab/*` | lab (workstation) app only |
| `src/pages/` | `pages/*` | shared pages (Home, Dashboard, Tokens, OtherProfiles, Settings, NotFound) |

- **Entries:** `index.html → /src/live/main.tsx` (the default; Rollup emits `dist/index.html` →
  Docker/nginx unchanged). `lab.html → /src/lab/main.tsx` (**dev-only**, never built for prod).
- **Vite configs (one factory, two apps):** `vite.config.base.ts` exports `makeConfig({port, entry,
  proxyTargetEnvKey, proxyTargetDefault, spaFallback})`; `vite.live.config.ts` (port 5173, `index.html`)
  and `vite.lab.config.ts` (port 5174, `lab.html`, `spaFallback`) are thin wrappers. Each config has a
  **single `rollupOptions.input`**, so an app's build can never pull the other app's HTML/code. **Prod
  builds run the live config only** → `dist/index.html`, no `lab.html`, no sweep/swing/grouped chunks
  (the EC2 image is lab-free).
- **Two dev servers, run separately or concurrently:** `npm run dev:live` (:5173) and `npm run dev:lab`
  (:5174) are independent processes; `npm run dev` runs both at once via `concurrently`. `npm run build`
  = live-only `dist/index.html`; `npm run build:lab` = workstation lab build. `tsc` type-checks **both**
  trees in one pass, so a lab type error fails the live build too (acceptable; single package).
- **Per-app `/api` proxy:** live proxies to the live bin (`VITE_LIVE_DEV_PROXY_TARGET`, default
  `:8081`); lab proxies to the lab bin (`VITE_LAB_DEV_PROXY_TARGET`, default `:8082`). To run both
  side by side, start the lab bin off the live port: `PORT=8082 cargo run -p lab`.
- **Lab SPA fallback:** the lab dev server isn't served from `index.html`, so a small `configureServer`
  middleware in `vite.lab.config.ts` rewrites top-level HTML navigations to `lab.html` — a hard refresh
  on a deep route (e.g. `/strategies/tpsl1`) loads the lab app, not the live one. The live server uses
  Vite's default SPA fallback (`index.html`).
- **Per-mode `App.tsx` + `nav.ts`:** static route table + `NavConfig` (`{accent, items[]}`), no
  gating. Live nav (`liveNav`) = teal accent + Live-mode toggle; lab nav (`labNav`) = violet accent, no toggle.

## Store — split `createApi` (the isolation seam)

- `shared/store/baseApi.ts` — the `createApi` **shell**: `reducerPath:'api'`, `keepUnusedDataFor:300`,
  **all 9 `tagTypes` declared up front** (`injectEndpoints` can't add tag types), `endpoints:()=>({})`.
- Three `injectEndpoints` modules attach onto it, each bundled only in its app:
  `shared/store/sharedEndpoints.ts` (tokens, profiles, settings+optimistic, solPrice, creation
  heatmap), `live/store/liveEndpoints.ts` (wallet holdings/prices, buy/sell,
  cashback, **live-mode**), `lab/store/labEndpoints.ts` (grouped sweeps, strategy
  simulate/paper, grouped creation stats, creators/analysis).
- `shared/store/apiSlice.ts` is a **barrel re-exporting SHARED endpoints only** (+ `apiErrorMessage`,
  and `baseApi` as `apiSlice` for `util.invalidateTags`). Mode hooks are imported **directly** from
  `@live|@lab/store/*Endpoints` so neither mode's `injectEndpoints` **side effect** leaks into
  the other bundle. Typed `.endpoints.X` / `util.updateQueryData('X')` callers import the owning
  typed api (`sharedApi` / `liveApi` / `labApi`).
- **Per-mode `configureStore`** (`live/store/index.ts`, `lab/store/index.ts`): base reducer +
  only that mode's slice (`syncToken` / `swingDetection`), importing its endpoint modules
  **for side-effect** before the store reads them, + `setupListeners`. Each exports its own
  `RootState`/`AppDispatch`. Shared components import a **mode-agnostic** `AppDispatch` from
  `shared/store/types.ts` (`ThunkDispatch<any,…>`), assignable to either store.

## Parameterized chrome — `shared/components/layout/`

- `Header.tsx` — data-driven from `NavConfig` (`navTypes.ts`); accent via `lib/accent.ts`
  (`accentClasses[teal|violet]`). Live-mode kill switch injected via `rightSlot` (live passes
  `@live/components/LiveModeControl`). Shared: SOL/USD mirror, timezone, price-unit toggle.
- `AppLayout.tsx` — slots `{nav, rightSlot, beforeMain, footer}`: live passes
  `beforeMain=<NotificationMount/>` (mounts `usePositionNotifications`); lab passes
  `footer=<BackgroundJobsIndicator/>`. `AppProviders` is mode-neutral (Timezone+PriceUnit+Toast);
  **lab `App` nests `BackgroundJobsProvider` itself** (keeps its SSE out of the live build).
- **Dashboard split:** shared `DashboardPage` takes `extraSections?(ctx)` render-prop; lab
  injects `GroupedCreationSection`, so the live build never pulls `getGroupedCreationStats`.

## Pages by mode

- **Shared:** Home, Dashboard, Tokens (live-ingest monitor — `token_created`/`trade_executed` SSE),
  OtherProfiles, Settings, NotFound.
- **Live (`@live/pages`):** SyncToken, Transactions, MyWallet (+ `InputSyncStatus`, `wallet/`,
  `transactions/` components; `useTradeStream`, `usePositionNotifications`; `syncTokenSlice`).
- **Lab (`@lab/pages`):** Analysis, SwingDetection, Tpsl{1,2}Page (authoring), Grouped
  Sweep ×2 (+ `analysis/`, `sweep/`, `strategy/` components; `useStreamedSweepResults`;
  `swingDetectionSlice`, `strategyResultCache`, `BackgroundJobsContext`).

## Rule forms + copy/paste params — `lib/params/` (one engine, one spec/strategy)

All 3 strategies (tpsl1, tpsl2, swing_1) share ONE rule-form + copy/paste path. The
canonical key everywhere — form state, clipboard blob, create/update payloads — is
the backend **column** (`p_exit_take_profit`, `buy_amount_sol`, `trade_mode`, …), so
there are no camelCase/axis/prefix translators.

- `lib/params/types.ts` — `ParamField` (column · group · section · kind · required ·
  `comboKey`? · `detectKey`? · presentation) and `StrategySpec` (fields + ordered
  accordion `sections`).
- `lib/params/engine.ts` — generic, spec-driven: `emptyForm` / `formFromRule` /
  `serializeRule(Json)` / `serializeCombo(Json)` (maps a sweep combo's bare key →
  column via `comboKey`) / `parseBlob` / `applyBlob` (live ⇒ sizing-only) /
  `buildCreatePayload` (blank→null, or 0 via `createBlankZero`) / `buildUpdatePayload`
  (locked section sends no keys; blank→0). No React imports — pure + unit-testable.
- `lib/params/specs/{tpsl1,tpsl2,swing1}.ts` — standalone hand-written specs; `index.ts`
  re-exports + `getSpec` + the swing1 **detect** adapter (`blobToDetectParams` /
  `detectParamsToBlob`, spec-derived via `detectKey`).
- `components/strategy/SpecRuleForm.tsx` — ONE inline (non-modal) form: Mode+Name row,
  `PasteParamsSection`, then one collapsible `Accordion` per `spec.sections`. Same chrome
  for every strategy; lock groups + live-freeze read off the spec. Rendered in-page by
  Tpsl{1,2}Page / Swing1Page (live + lab) — the old `tpsl{1,2}/RuleFormModal.tsx` +
  `Swing1RuleAccordion.tsx` + `lib/ruleParams.ts` are gone.
- Combo ⎘ (`sweep/GroupedSweepView`) and the swing1 detect ⎘/paste all go through the
  same engine, so a blob copied anywhere pastes anywhere (blob format unchanged: `p_*`,
  version 1; cross-strategy paste rejected by `PasteParamsSection`).

## Services / hooks (shared, tree-shaken per entry)

- `services/http.ts` — shared `request<T>` fetch wrapper (throws backend `{error}`; 204→undefined).
- `services/api.ts` (imperative helpers) + `services/sse.ts` (single shared `EventSource`,
  `connect*` subscribers) stay shared and side-effect-free, so each entry tree-shakes the calls it
  doesn't use.

## Perf patterns (unchanged)

- Single SSE `EventSource` multiplexes all streams; positions/rules = delta patch + visibility-gated
  fallback poll. RTK Query structural sharing + 5min `keepUnusedDataFor` + `skipPollingIfUnfocused`.
- **Unified server-side table contract (POST + JSON).** Every server-side token table — the four
  strategy tables (Positions / Paper / Matched / Simulated) **and the Tokens page** — pages/sorts/
  filters/searches over **one** request body: `TableRequest`
  (`{ pagination, sorting, search, filters, range?, trackedOnly?, swingRunId?, swingChainLatencyMs? }`),
  serialized by `services/tableRequest.ts` (`toTableRequest(query, numericCols, {range?})`). Per-column
  filters are structured `{op, val}` (`FilterOp` = contains/eq/gt/gte/lt/lte/between): for a **numeric**
  column, `parseFilterSpec` turns `>5` / `1..10` into `{op:'gt',val:5}` / `{op:'between',…}`, so numeric
  operators compare **numerically server-side**. `numericColKeys(columns)` derives the numeric-key set
  from a column list. All tables read the run-wide `total` off the response. Positions is POST on
  **both** bins (live + lab) so the shared hook/fetchers are one code path.
- **Tokens page = same contract (`POST /api/tokens`).** `getTokensPage` (`sharedEndpoints.ts`) folds the
  DataTable view-state (`toTableRequest`) AND the global `TokenFilters` panel (`tokenFiltersToSpecs`,
  `filters.ts`) into ONE `filters: {col → FilterSpec}` map keyed by backend column key (panel-wins on
  collision), tz-normalizing the datetime pickers; the Tokens-only `trackedOnly`/`swingRunId`/
  `swingChainLatencyMs` ride alongside. Backend lowers each `FilterSpec` back onto its internal panel/
  per-column representation (`TokenQuery::from_table_request`), so the LIVE (Postgres) and LAB (in-RAM)
  engines are unchanged and identical (DB parity test). The old bespoke `f_*`/`cf` `URLSearchParams`
  builder and the dead simple `getTokens` GET endpoint were removed.
- **Positions/Paper = server-side paged, summary decoupled** (`useRulePositions`): the positions
  `DataTable` runs in `serverSide` mode; the hook serializes its `TableQuery` (+ `numericCols`) into
  the POST body, fetches one page, and reads the total off `X-Total-Count`; live SSE deltas patch only
  rows *already on the page*. The **Positions Summary** panel renders a **separate** server-computed
  aggregate (`/rules/{id}/positions/summary`, GET) over the *whole* run. All five strategy pages (live
  `TpslPage`/`Swing1Page` + the 3 lab pages) share this path; the Paper Test section is a second
  `useRulePositions` instance scoped to the paper rule.
- **Matched/Simulated = server-side via `useServerTable`** (lab-only). A lean page+total+summary hook
  (no SSE-delta patching / settle-poll — these results are static once computed) drives the two tables
  over `fetchMatchedPage` / `fetchSimulatedPage` (POST, `{tokens}` body + `X-Total-Count`). **Matched**
  materializes server-side: the first POST scans the whole `tokens` table for the matched mint set,
  caches it, and pages the DB restricted to it (no 5,000-row cap). **Simulated** pages the finished
  backtest's rows **in memory** on the server (already resident — lab is single-user), with a whole-run
  `/simulate/result/summary` aggregate for its card; `reload()` refetches on the `simulation_finished`
  SSE (collect → fetch-first-page).
- **Token enrichment is server-side, not client-merged.** Every token-result table (Matched,
  Positions current/history, lab paper positions, Simulated, Sweep drill-in) receives the full
  `TOKEN_ENRICH_FIELDS` set **in the response body** — the backend attaches it from one shared
  `trading_core::storage::token_enrichment` SSOT — so sort/filter/search on enrichment columns works
  across the whole result set. `mergeTokenData(rows, tokenMap)` + the per-table
  `useGetTokensByMintsQuery` batch call were **removed** from those tables; `mergeTokenData` survives
  **only** for **Wallet Holdings** (`MyWalletPage`), which has no server pagination (a full client-side
  on-chain-scan dataset, so a client merge there isn't a workaround for missing server sort).
- **Shared enrichment type + strategy primitives.** The ~28 enrichment fields the backend
  `TokenEnrichment` flattens onto result rows are declared **once** in TS as
  `TokenEnrichmentFields` (`shared/types`); `RulePositionRecord`/`MatchedTokenRecord`/
  `SimulatedTokenResult` `extends` it (the all-required `TokenRecord`/`TokenDetailRecord`
  stay bespoke — their nullability differs by endpoint on purpose). Strategy-page boilerplate
  is shared under `shared/components/strategy/`: `cellFormat.ts` (the former byte-identical
  `tpsl1/2 utils.ts`), `inspectTarget.ts` (the `InspectTarget` type + `inspectFromSim`/
  `inspectFromPosition` mappers, previously copy-pasted across five pages and both modal forks).
- **Numeric column filters** (`>5`, `1..10`, `>=`, `!=`) on the shared token-enrichment columns:
  `ALL_TOKEN_COLS` in `sharedTokenColumns.tsx` declares `filterNumber` on every numeric column
  (mirrors the Tokens-page `tokenColumns.tsx`). The `DataTable` emits raw filter text; the serializer
  (`toTableRequest` via `parseFilterSpec`) turns a numeric-column expression into a structured op that
  compares **numerically server-side** (no longer client-only / `ILIKE`-substring). `!=` has no server
  op and maps to `eq`; the legacy `parseNumericPredicate` (still used by any fully client-side table)
  keeps the real `!=` negation.
- Memoized column defs/price formatters; cells read context directly. localStorage via `lib/storage`
  (`mt:` namespace); column visibility in one `mt:table.cols` map keyed by `tableId`.

## Known follow-ups (NOT yet done)

- **Strategy split by job (plan §7):** the lab `Tpsl{1,2}Page` still carry live-only controls
  (activate/pause/stop, manual sell) that the **lab** backend doesn't serve — so the lab
  build still imports `useSellTokenMutation` from `@live/store/liveEndpoints` (a workstation-only
  leak; the prod live bundle is clean). Splitting into a live `LiveStrategiesPage` (`/strategies/live`,
  full live control) + lab `AuthoringTpsl{1,2}Page` (CRUD + simulate + paper only) is pending —
  it **depends on the backend follow-up** below.
- **Backend follow-up:** register rule **CRUD + activate/pause/stop** (and `/matched`) on the
  **live** bin; until then a live Live-Strategies page would 404. See
  `live/src/strategies/tpsl_sniper_1/lifecycle.rs` (logic exists, no HTTP route).
- **No enforced boundary:** a few `src/shared/*` files do real value imports from `@lab` (e.g.
  `dashboard/GroupedCreationSection.tsx` → `labEndpoints`); they stay out of the live bundle only
  because no live route reaches them (tree-shaking),
  not because anything forbids it. An ESLint `no-restricted-imports` guard banning `shared/`+`live/`
  from importing `@lab` would make the seam enforced rather than incidental.
- **Cosmetic deviation:** shared store core lives in `src/shared/store` but the legacy `store/*`
  alias still resolves there; the `live/services/strategyApi.ts` / `lab/services/labApi.ts`
  file-level split was skipped (tree-shaking over one shared `services/api.ts` achieves the same
  bundle isolation since the helpers are side-effect-free).
