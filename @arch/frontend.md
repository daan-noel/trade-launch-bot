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
- **Per-mode `App.tsx` + `nav.ts`:** static route table + `NavConfig` (`{identity, items[]}`), no
  gating. `identity` (`{subtitle, badge, glyph?, pulse?}`) drives the Header logo block. Live nav
  (`liveNav`) = `Live Trading` / `LIVE` (pulsing) + Live-mode toggle; lab nav (`labNav`) =
  `Research & Backtesting` / `LAB`, no toggle. The per-app **color** is NOT in the nav config — it's
  the `--color-primary` theme token, swapped per build (see "Per-app skin" below).

## Store — split `createApi` (the isolation seam)

- `shared/store/baseApi.ts` — the `createApi` **shell**: `reducerPath:'api'`, `keepUnusedDataFor:300`,
  **all 9 `tagTypes` declared up front** (`injectEndpoints` can't add tag types), `endpoints:()=>({})`.
- Three `injectEndpoints` modules attach onto it, each bundled only in its app:
  heatmap), `live/store/liveEndpoints.ts` (**portfolio** holdings/summary/positions —
  `/api/portfolio/*`, the Holdings + Home command-center data; wallet prices, buy/sell,
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

- `Header.tsx` — data-driven from `NavConfig` (`navTypes.ts`); renders `identity` (name + `badge`
  chip + `subtitle` + `glyph`) and highlights active nav with `primary` utilities (no per-mode class
  map — the old `lib/accent.ts` is deleted). Live-mode kill switch injected via `rightSlot` (live
  passes `@live/components/LiveModeControl`). Shared: SOL/USD mirror, timezone, price-unit toggle.

### Per-app skin (`src/index.css`, `index.html` / `lab.html`)

- **Mechanism:** each HTML entry tags `<html data-app="live|lab">`. `@theme` in `index.css` is the
  **live base** (neutral near-black + teal `--color-primary`); one `:root[data-app='lab']` block
  overrides the bg/panel/card/hover/border tokens (cool slate) + `--color-primary` (cyan `#06b6d4`,
  one hue-step over from live's teal — related but distinguishable).
- **Why it re-skins everything for free:** Tailwind v4 compiles `bg-bg`/`text-primary`/… to
  `var(--color-*)`, so overriding the variables re-skins every token-driven shared component with
  **no forks** (strict SSOT). The override is unlayered + higher specificity, so it wins over the
  `@theme` layer. Live is left on the base tokens (EC2-shipped app, lowest risk).
- **Purpose:** an at-a-glance "which app am I in" signal — error-prevention first (never mistake the
  lab sandbox for the live-trading cockpit), identity second.
- **Chrome that hardcoded teal** (DataTable column-hover, primary Button glow, tpsl2 sim-pill glow)
  now resolves from `var(--color-primary)` via `color-mix`, so it follows the per-app accent. Chart
  **series** colors (`token-price-chart/constants.ts` price line, range band) stay teal by design —
  semantic data-viz, not chrome.
- `AppLayout.tsx` — slots `{nav, rightSlot, beforeMain, footer}`: live passes
  `beforeMain=<NotificationMount/>` (mounts `usePositionNotifications`); lab passes
  `footer=<BackgroundJobsIndicator/>`. `AppProviders` is mode-neutral (Timezone+PriceUnit+Toast);
  **lab `App` nests `BackgroundJobsProvider` itself** (keeps its SSE out of the live build).
- **Dashboard split:** shared `DashboardPage` takes `extraSections?(ctx)` render-prop; lab
  injects `GroupedCreationSection`, so the live build never pulls `getGroupedCreationStats`.

## Pages by mode

- **Shared:** Home (minimal — lab still uses `pages/home/HomePage`), Dashboard, Tokens
  (live-ingest monitor — `token_created`/`trade_executed` SSE), OtherProfiles, Settings, NotFound.
- **Live (`@live/pages`):** **Home command center** (`home/LiveHomePage` — routed at the live
  index over the shared Home; KPI row + `home/` widgets `TopHoldingsWidget`/`LiveTradeFeed`/
  `StrategyStrip` over `/api/portfolio/{summary,holdings,positions}`), SyncToken, Transactions,
  MyWallet (**position manager**: `HoldingsSummaryBar` header + cost-basis/PnL columns +
  `managed_by` bot badge + double-sell confirm interlock; the holdings table is **server-side**
  via `POST /api/portfolio/holdings/query` + `/summary` (short-TTL scan cache) with a client
  price-poll overlaying live value/price on the current page; Home widgets still read the full
  `GET /api/portfolio/holdings`)
  (+ `InputSyncStatus`, `wallet/`, `transactions/` components; `useTradeStream`,
  `usePositionNotifications`; `syncTokenSlice`).
- **Lab (`@lab/pages`):** Analysis, SwingDetection, **TraderAnalysis** (paste a wallet →
  the **standard** full token table — the shared `tokenColumns()`, unchanged, client-side
  sort/filter/search via `DataTable` — **plus** a synced lazy charts grid below that mirrors
  the table's current sort/filter/page. The wallet-specific stats (buys / sells / last
  traded) live in each chart card's header, **not** as table columns, so nothing duplicates
  the token columns. Each `TokenTradeChart` has the wallet's buys/sells **spotlighted** among
  the tracked markers; recent-trade-first (backend order, no `defaultSort`);
  `useGetTraderTokensQuery` → `GET /api/wallets/:wallet/tokens` returns `TraderTokenRow`
  (full `TokenRecord` + `wallet_{last_trade_at,buy_count,sell_count}`), a PG read since the
  default 7d window includes today, which the lake lacks. The table→charts sync uses
  `DataTable`'s `onVisibleRowsChange` callback — fires the memoized on-screen page rows so a
  sibling view can follow), Tpsl{1,2}Page
  (authoring), Grouped Sweep ×2 (+ `analysis/`, `sweep/`, `strategy/` components;
  `useStreamedSweepResults`; `swingDetectionSlice`, `strategyResultCache`, `BackgroundJobsContext`).
  The shared `TokenTradeChart`/`TokenPriceChart` take an optional `highlightWallet` — its
  markers render larger with a gold glow+ring (`ProfileWalletInfo.isHighlighted` →
  `walletMarkersPlugin`), and a non-tracked input address gets a synthetic marker entry.
  **Tracked-wallet markers are a structural invariant:** `TokenPriceChart` defaults
  `profileWallets` to `useProfileWallets()` when the prop is omitted, so *every* token trade
  chart shows tracked-wallet markers by construction (pass an explicit list to override,
  `[]` to force none). `useProfileWallets` imports the palette/type from the chart's leaf
  files (not the barrel) to avoid an import cycle now that `TokenPriceChart` consumes it.

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
- **Token enrichment is server-side, not client-merged — for EVERY token table.** Every token-result
  table (Matched, Positions current/history, lab paper positions, Simulated, Sweep drill-in, **and, since
  Phase 4, Wallet Holdings**) receives the full `TOKEN_ENRICH_FIELDS` set **in the response body** — the
  backend attaches it from one shared `trading_core::storage::token_enrichment` SSOT — so sort/filter/
  search on enrichment columns works across the whole result set. `mergeTokenData` + the per-table
  `useGetTokensByMintsQuery` batch call are **gone** (the wallet was the last client-merged table; both
  were deleted with its server migration).
- **`TokenTable` = the ONE wrapper for every token-row table** (`components/tokens/TokenTable.tsx`).
  It owns the "token recipe" over `DataTable`: (1) append the shared token-info columns
  (`appendedTokenColumns`, so callers export only their bespoke columns + an `existingKeys` set — see
  `components/strategy/strategyColumns` `POSITION_KEYS`/`MATCHED_KEYS`/`SIM_KEYS`, each derived straight from
  its column array so keys can't drift from what's rendered; a table that owns its full layout
  passes `ALL_TOKEN_INFO_KEYS` to append nothing); (2) own the table wiring. **Two modes:** **server**
  (`serverSide` + `serverTotal`/`onQueryChange`/`resetKey`) — rows arrive backend-enriched one page at a
  time, paging/sort/filter round-trip (Positions via `RunPositionsPanel`, Paper, Matched, Sim, Wallet
  Holdings, **Tokens page**); **client** (default) — rows are the full already-enriched set and
  `DataTable`'s **own** client paging/sort/filter/search runs in-browser (NO separate evaluator — that TS
  twin retired with Wallet), used by tables with no backend paging endpoint (**Trader Analysis**,
  **Sweep drill-in**). Rows key their mint under `mint` (default) or another field via **`mintOf`** —
  which drives the charts grid, the default `rowKey`, and the client mint-set pre-filter. Two opt-in
  features live here so every token table gets them once: **`mintSetFilter`** — a `<MintSetInput>` paste
  box (server: an `in` op on `mint` folded into `structuredFilters`; client: a plain row pre-filter);
  **`charts`** — a toggle rendering `<TokenChartsGrid>` (lazy-mounted, current page only, with
  `renderChartCardExtra`/`titleOf`/`highlightWallet` slots) below the table, fed by the table's
  intercepted `onVisibleRowsChange`. `DataTable` stays token-agnostic: the dependency is one-way
  (`tokens/` → `table/`), asserted by `components/table/DataTable.boundary.test.ts`. **Every** token-row
  table now renders through `TokenTable`. (Trader Analysis keeps its always-on external `<TokenChartsGrid>`
  fed by the table's `onVisibleRowsChange` rather than the toggle, being chart-centric.)
- **One in-memory evaluator, in Rust only.** Token tables whose rows are RAM-resident on the backend (the
  lab Simulated table; the live Holdings composition) page/sort/filter through
  `trading_core::api::table_eval::apply_table_request` with a per-table `ColResolver` grammar; the shared
  enrichment half of that grammar is `resolve_token_enrichment_key` (SSOT — the Simulated and Holdings
  resolvers both delegate to it). The **TS twin** (`services/tableEval.ts` + `columnResolver` +
  `mergeTokenData`) that used to drive Wallet client-side is **deleted** — the wallet is server-side now.
  The golden fixture `tableEval.fixtures.json` and the Rust `table_eval::conformance_shared_fixtures`
  test are **kept** (now Rust-only) so the evaluator's op/sort/search/tiebreak/paging semantics stay
  pinned.
- **Shared enrichment type + strategy primitives.** The ~28 enrichment fields the backend
  `TokenEnrichment` flattens onto result rows are declared **once** in TS as
  `TokenEnrichmentFields` (`shared/types`); `RulePositionRecord`/`MatchedTokenRecord`/
  `SimulatedTokenResult` `extends` it (the all-required `TokenRecord`/`TokenDetailRecord`
  stay bespoke — their nullability differs by endpoint on purpose). Strategy-page boilerplate
  is shared under `shared/components/strategy/`: `cellFormat.ts` (the former byte-identical
  `tpsl1/2 utils.ts`), `inspectTarget.ts` (the `InspectTarget` type + `inspectFromSim`/
  `inspectFromPosition` mappers, previously copy-pasted across five pages and both modal forks).
- **One strategy-table column SSOT (`strategyColumns.tsx` in `shared/components/strategy/`).** The
  Positions / Matched / Sim tables' `positionColumns`/`matchedColumns`/`simColumns` (+ their
  `POSITION_KEYS`/`MATCHED_KEYS`/`SIM_KEYS`) + `exitReasonBadge` live here **once**. The
  **target/entry/exit** trade legs — each with **Price · Tokens · Size · Time · Tx** — are emitted by one
  `legColumns(prefix, accessors, opts)` builder (`Size` = `solOf(price, tokens)` unless a real SOL field is
  given; Tokens/Tx columns drop when their accessor is absent). This replaced the two copy-pasted
  `tpsl1/tpsl2 tableColumns.tsx` files that had **drifted** (tpsl1 had lost the whole target leg +
  tokens/size/tx; tpsl2's sim showed only price/time on entry/exit). All five strategy pages
  (lab tpsl1/tpsl2/swing1, live tpsl/swing1) and the live cross-strategy monitor
  (`live-trading/positionColumns.tsx`, entry-leg only via the same builder) now share this one source.
  The sim's exit leg still omits Tokens/Size because the sim result payload carries no `exit_token_amount`.
- **One token-info column SSOT (`tokenInfoColumns()` in `sharedTokenColumns.tsx`).** The ~26 enrichment
  columns are defined **once** (render/sort/search/filter logic); both consumers derive from it —
  `appendedTokenColumns(existingKeys)` (strategy columns, and wallet via `TokenTable`) overlays `defaultVisible` via
  `APPENDED_HIDDEN_KEYS`, and the Tokens page (`tokenColumns.tsx`) pulls each column by key through
  `tokenInfoColumnMap()`, adding only its own presentation (order + `TOKEN_COL_WIDTH` widths) and
  Tokens-only columns (identity/`token_age`/`lifetime`/fep-ratios). Per-view `defaultVisible`/width/order
  legitimately differ; the render/sort/filter facts don't. The matched tables no longer hand-roll
  `init_buy`/`cu_limit`/`cu_price` — those come from the shared `initial_buy`/`cu_limit`/`cu_price`
  columns.
- **Numeric column filters** (`>5`, `1..10`, `>=`, `!=`): every numeric column declares `filterNumber`.
  The `DataTable` emits raw filter text; the serializer (`toTableRequest` via `parseFilterSpec`) turns a
  numeric-column expression into a structured op that compares **numerically** server-side (all token
  tables, Wallet included). `!=` has no server op and maps to `eq`; the legacy `parseNumericPredicate`
  (still used by any fully client-side table) keeps the real `!=` negation.
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
