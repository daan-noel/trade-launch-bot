# Frontend — `hunter/frontend/` (React SPA, **two apps over a shared core**)

Stack: React 19 + TS + Vite, RTK Query + Redux Toolkit, React Router 7, Tailwind 4, lightweight-charts.
Deep-dive detail: `@plans/frontend/frontend-patterns.md`, `@plans/token-analysis/*`.

## Split model (mirrors the backend two-bin split)

One `hunter/frontend` package, **three source trees** + **two Vite entries running as two dev
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
  (:5174) are independent processes; `npm run dev` runs both at once via `concurrently`. `npm run build:live`
  = live-only `dist/index.html`; `npm run build:lab` = workstation lab build. `tsc` type-checks **both**
  trees in one pass, so a lab type error fails the live build too (acceptable; single package).
- **Per-app `/api` proxy:** live proxies to the live bin (`VITE_LIVE_DEV_PROXY_TARGET`, default
  `:8130`); lab proxies to the lab bin (`VITE_LAB_DEV_PROXY_TARGET`, default `:8140`). The lab bin
  binds `LAB_PORT` (`:8140`) by default, off the live bin's `LIVE_PORT` (`:8130`), so both run side
  by side with no port override. Both default to the deploy `*_API_PORT` numbers, so local and docker
  use the same backend port.
- **Lab SPA fallback:** the lab dev server isn't served from `index.html`, so a small `configureServer`
  middleware in `vite.lab.config.ts` rewrites top-level HTML navigations to `lab.html` — a hard refresh
  on a deep route (e.g. `/strategies/sweep`) loads the lab app, not the live one. The live server uses
  Vite's default SPA fallback (`index.html`).
- **Per-mode `App.tsx` + `nav.ts`:** static route table + `NavConfig` (`{identity, items[]}`), no
  gating. `identity` (`{subtitle, badge, glyph?, pulse?}`) drives the Header logo block. Live nav
  (`liveNav`) = `Live Trading` / `LIVE` (pulsing) + Live-mode toggle; lab nav (`labNav`) =
  `Research & Backtesting` / `LAB`, no toggle.   Live money nav is collapsed to
  **Console** (`/console` — one page: the Attention/Open+Manual-trade/Waiting lanes plus the
  **History** section;
  redirects from `/floor`, `/trade`, `/ops`, `/positions`, `/live-trading`,
  `/strategies/armed`, `/strategies/monitor`, query preserved) · **Portfolio** (`/portfolio`) · **Wallet**. Rules Evidence is
  `/strategies/rules/:ruleId`. Lab flattens single-child groups (Tokens, Trader Analysis
  are leaf links). Metric panes are not a peer nav item — they live in lab Tokens detail
  (`/strategies/metric-panes` redirects to `/tokens?mint=`). The per-app **color** is NOT in the nav config — it's
  the `--color-primary` theme token, swapped per build (see "Per-app skin" below).

**Operator clarity (jobs):** Wallet = bag overview (funding/cashback + holdings; manual
trading moved to the Console); **Console = the one real-trade surface** (SSE SSOT; lanes
top-to-bottom: ⚠ Attention with per-status actions mirroring the backend close matrix,
Open ∥ Manual-trade panel (buy 202→SSE, TP/SL, sell-all-by-mint, Holding Sell ALL / 25% / 50%, persistent trade log),
collapsible Waiting, then **History**; rows carry origin dot / status+sub-chips / dead-pool ❗ /
MTM / stale-age cue; row select opens the detail modal — hero with graded PnL% /
colored exit-reason pills / ops chips (dead, banked %, parked), the same close-action
bar as the row, and a chart ∥ fills layout). Cockpit UX: sticky KPI strip, collapsible
Manual trade (`consoleManualOpen`), shared `OpenPositionStatusChips`, ←/→ modal lane
nav, Attention bulk Verify-stale / Retry-unparked, and mig `0003` backfill of legacy
`position_fills`.

The three lanes above History are the **cockpit** (live, SSE-driven, only what is still
actionable); **History is the review surface** — one URL-backed cohort (date range · rule ·
mode · status · exit reason) driving a **positions summary strip**, an exit-mix strip, a
charts deck, **and** a server-paged table over the whole `strategy_positions` population,
so all four always describe the same rows. The table's own search + per-column filters are
part of that cohort (one builder, `console/historyRequest.ts`), so filtering the table
narrows the strip and the charts with it; the strip's tiles (Closed / Open / Win% /
Migrated) filter back the other way. It replaced the old 50-row "Recent closed" lane, and it owns closed rows
outright (there is no session-local closes buffer any more — see "Live Status SSOT" below).
Detail: [review-surfaces.md](../plans/frontend/review-surfaces.md).

Portfolio = the **keep/kill review board** (default window `7d`): window spark + realized,
named decay alerts → Rules, ranked PnL bars + compact table (Rule · PnL · Return% · Exp ·
Form · N · History). The window headline carries `return_pct` beside the realized ◎, and
both re-derive from `Σ pnl / Σ closed_entry_sol` when the row set narrows (filter / decay
toggle) — never a mean of the surviving rules' percents. Bar/row click selects (`?rule=`); rule name → Rules; History link → Console.
Selecting a rule opens `PortfolioRulePositions` beneath the table — that rule's **closed**
trades for the same window, server-paged, with the per-row Charts grid and the position
inspect modal (`?pos=`). It is scoped to the population the scoreboard row aggregates
(entered + `status='End'`), so its count is the row's N and its PnL ◎ column sums to the
row's PnL. The scope is not re-derived: the panel builds a `HistoryCohort` with
`lane: 'closed'` and serializes it through Console History's one request builder
(`historyRequest`), so both surfaces mean the same thing by "closed in this window".
Changing rule / window / mode drops `?pos=` — an id from another population can't resolve.
Calendar-window closes, not
Rules Control current-run scores. Rules =
**Control** (TOTAL rollup + activate/pause + scoreboard scoped current-run / all-time) +
**Evidence** pane (run navigator, summary, positions). Home leads with the **review digest**
(7-day PnL sparkline, attention count, rule-decay alerts) and demotes the live trade feed to a
collapsed panel.
Tokens table stream toggle is **STREAM ON/OFF**
(not the header trading kill switch).

Live money nav: **Console** (`/console`) · **Portfolio** · **Wallet**.

**Live Status SSOT:** `live/slices/liveStatusSlice` + `useLiveStatusBootstrap` (mounted
in live `App`) — REST snapshot on mount / SSE reconnect / tab visible / `sse_resync`:
armed + open positions **only**. Closed rows are deliberately not held here: a terminal
frame just deletes the row from `open`, and Console History reads it back from the DB off
that same frame (with the exit fill the frame doesn't carry).
In-place patch on `strategy_position_update` / `strategy_armed_changed`. Snapshot drops
armed rows that collide with open `(rule, mint)` (Waiting must not stick after buy).
Terminal position SSE is emitted **before** the sink drops registry meta so
`position_id` / `trade_mode` stay populated; the slice ignores nil/empty ids.
A position that opens mid-session is hydrated by **deltas alone** — the snapshot's
triggers are all session edges — so every `strategy_position_update` carries the whole
entry snapshot (`entry_price` + `entry_sol` + `entry_time`), sourced from `PositionMeta`
rather than the frame's fill (only the first-entry `Holding` frame has one). A missing
`entry_time` is invisible in the row and silently drops the chart's entry marker, which
needs time **and** price together (`buildEventMarkers`). The snapshot is not authoritative
over a delta here: `record_entry_fill` is spawned, so a snapshot racing that write returns
the entry columns empty and must carry the prior values forward.
Ops, Rules live counts, Home open KPI, and StrategyStrip read this store only (no
parallel Maps). Legacy `LiveTradingPage` / `MonitorPage` are gone — `/positions` and
`/strategies/armed` redirect to Ops.

**Armed leave-Waiting:** engine Enter does not emit `ArmedChanged(Disarmed)`; the
position sink clears `ArmedRegistry` + emits `disarmed`/`entered` on `BuySubmitted` /
`Holding`. `GET /api/strategies/armed` also filters out mints with unsettled positions.

**Live trading notify SSOT (one EventSource, writers):**

| Domain | Push | Client sink |
| --- | --- | --- |
| On-chain trades | `trade_executed` (includes `tx_index`/`leg_index`/reserves/`fee_sol`/`instruction_labels`) | Tokens table row patch; chart `getTokenTrades` append via `watchTokenTradesMint` + `liveTradeToTradeRecord`; `useMintTradeStream` for mint-filtered feeds |
| Strategy inventory | `strategy_position_update` / `strategy_armed_changed` | `liveStatusSlice` only |
| Portfolio money | bag-changing position events + `trade_executed` for `mine` wallets | `usePortfolioRealtime` invalidates `WalletHoldings` **and** fans out via `onPortfolioBagRefresh` (Wallet imperative table — no second SSE filter) |
| Display marks | `trade_executed` tip (SOL spot → USD) | `useWalletMarksLive` patches Home holdings + Jupiter price cache; Wallet page tips page rows locally. Jupiter oracle (liquidity/24h/cold) refetches on mount / bag refresh / tab focus — no interval |

A pushed frame carries **every field the REST row it patches into carries**, because a
live append is invisible: `liveTradeToTradeRecord` writes into the same
`getTokenTrades` cache the chart reads, so a dropped field is not a blank cell but a
*different answer*. `instruction_labels` is the sharp one — the chart's vol/non-vol
overlay classifies from them client-side (`lib/flow/classifyFlow`), and a label-less
row both counts as non-vol and fails to tag its wallet, so the cumulative pair diverges
from that trade onward and only heals on a refetch. For the same reason
`tokenTradesLive` is an `onSseReopen` consumer: a gap (reconnect or `sse_resync`)
refetches each watched mint's history and merges the appended tail back over it, since
a dropped frame leaves a hole a cumulative series reads as real flow.

Mount points in live `App` `NotificationMount`: `useLiveStatusBootstrap`,
`usePortfolioRealtime`, `useWalletMarksLive`, `useTokenTradesLiveBootstrap`,
`usePositionNotifications`. Tokens STREAM fallback poll defaults to 90s.
Rule Evidence (embedded on Rules Control + `/strategies/rules/:ruleId`) reloads history only on
open/close edges (not ExitPending). Rules scoreboard columns (`PnL` / `Return%` / `Exp` /
`Win%` / `W/L` / `N`) come from `GET /api/strategy-rules?score_scope=current|all`
(`current` = latest run both modes; `all` = real all-time / paper latest). `Exp` is
client-derived expectancy (`PnL / closed`). `Return%` is the server's capital-weighted
`return_pct`; the **TOTAL** tile re-derives it as `Σ total_pnl_sol / Σ closed_entry_sol`
(that denominator is shipped for exactly this) and refuses a single figure when both
modes are on screen — real ◎ and paper ◎ are not one currency, the same rule the PnL
tile already followed. Never roll it up by trade count: see
[docs/plans/strategies/pnl-percent-definition.md](../plans/strategies/pnl-percent-definition.md). Evidence run chips use
`GET /api/strategy-rules/{id}/runs`; positions use `scope=current|run|all`.
**Both apps serve all four routes** (lab off the synced mirror), so the lab Rules page is
the same cockpit — scoreboard + TOTAL rollup + Evidence — over the traded results. The one
live-only piece is `ruleLiveCounts` (SSE engine bags): omit it and `RulesView` drops the
live columns and the TOTAL falls back to the DB open count.
See [rules-cockpit-ux.md](../plans/frontend/rules-cockpit-ux.md).

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
  map — highlighting is `primary`-utility driven, never a per-mode accent module). Live-mode kill switch injected via `rightSlot` (live
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
  `beforeMain=<NotificationMount/>` (mounts `usePositionNotifications`, which toasts
  on `strategy_position_update` **and** `strategy_armed_changed` per Settings prefs;
  desktop path uses `/sw-notifications.js` + `showDesktopNotify` — mint-first
  title, REAL/PAPER · rule body, full-bleed status tile + geometric mark
  (no hero image), Console / Trade actions; tag quietly replaces lifecycle;
  sound on Holding + failures (`renotify` only ExitStuck / ExitUnconfirmed);
  Web Lock claim so multi-tab does not double-fire; click → `opsNotifyHref`);
  lab passes `footer=<BackgroundJobsIndicator/>`. `AppProviders` is mode-neutral
  (Timezone+PriceUnit+Toast);
  **lab `App` nests `BackgroundJobsProvider` itself** (keeps its SSE out of the live build).
  **Route Suspense lives inside `AppLayout` around `<Outlet />`** (not around `Routes`) so a
  lazy page chunk keeps the header/nav mounted; fallback is `SuspenseFallback` → `LoadingState`.
- **Chart code-split:** `lightweight-charts` (~177 kB raw / 57 kB gz) is not pulled into the
  app-root or route/table chunks up front — it must stay reachable only through a `lazy()`
  boundary, so it downloads when a chart actually mounts. Call sites use
  `LazyTokenTradeChart` / `LazyLabTokenInspect(Modal)` / `LazyTokenChartsGrid` /
  `LazyFloorMintChart` (live Console + `FloorPositionDetail`; Console manual-trade
  passes `chrome="compact"` so the toolbar collapses behind a Tools toggle; the
  candle/range selection is **controlled** — a host wires `useBarTradesSelection` and
  renders the trades table itself, see below) /
  `LazyLivePositionInspectModal` (live Rules + Rule Analyze) / `LazyFlowPreviewChart` (lab
  Flow Discovery); all lazy Suspense fallbacks share `LoadingState` (`page` / `panel` /
  `inline`). Lab Creation Stats owns `GroupedCreationSection` (lab-only page — no live
  `extraSections` inject).
  - **`token-price-chart/constants.ts` may import `lightweight-charts` as a TYPE only.**
    Its colors / storage key / prefs are read by ~20 non-chart modules (PnL widgets,
    `TimezoneContext`, `useProfileWallets`, `utils/date`), several of them in the eager
    root graph — one value import there (`ColorType`, `CrosshairMode`, `LineSeries`,
    `CandlestickSeries`) put the entire charting library in the app-root vendor chunk of
    **both** apps. Real enums and series constructors live in
    `token-price-chart/chartOptions.ts` (`createChartOptions`, `SERIES_BY_STYLE`), which
    only the lazily-loaded charts import. Verify after any chart-module change: no
    `modulepreload` in `dist/index.html`/`dist/lab.html` should resolve to a chunk that
    statically imports `lightweight-charts`.
- **Clicking a candle lists that bar's trades — one implementation.** The chart emits the
  pick only (`onBarClick` / `onRangeChange`); `useBarTradesSelection` holds it (bar and
  range are mutually exclusive), `token-price-chart/barTrades.ts` (`tradesInBar` /
  `tradesInRange`) is the ONE matcher keying trades exactly as the chart bars them, and
  `BarTradesPanel` renders the table (entry/exit row tint from `eventMarkers`, own-wallet
  accent). `TokenTradeChart` puts it under its chart and can yield it to an outside pick
  via `externalSelection`; `FloorPositionDetail` uses `MintBarTradesPanel` — same RTK Query
  cache the chart filled, so no extra request — placed **below** the chart ∥ fills grid,
  which is the only place with the width for a table. A host outside `token-price-chart`
  deep-imports (`.../barTrades`, `.../types`), never the barrel, which would drag
  `lightweight-charts` into a statically-mounted chunk. Detail:
  [@plans/token-analysis/token-history-chart-functionalities.md](../plans/token-analysis/token-history-chart-functionalities.md) §6a.

## Pages by mode

- **Shared:** Tokens (live-ingest monitor — `token_created`/`trade_executed` SSE
  patches table rows; detail/chart `TokenTradeChart` appends the same frames into
  RTK `getTokenTrades` via `useWatchTokenTradesLive`; scroll-into-view on select;
  table stream toggle labeled **STREAM ON/OFF** so it is not confused with the
  header trading kill switch; slim page-owned `TokensFilterBar` (Created /
  Dead / Migrated) above the table — not inside `DataTable`; `?mint=`
  deep-links selection), Profiles, Settings (`pages/settings/` — 2-col
  content-sized Trading / Notifications / Tracking / Reliability grid
  (`items-start`, no viewport stretch); page-level Saved/error feedback;
  notification Critical/All/Custom presets via `CRITICAL_NOTIFY_STATUSES`),
  NotFound.
- **Live (`@live/pages`):** **Home command center** (`home/LiveHomePage` — KPI tiles
  deep-link to Wallet / Ops / Rules; widgets `TopHoldingsWidget`/`LiveTradeFeed`/
  `StrategyStrip`; portfolio tags stay fresh via `usePortfolioRealtime`), SyncToken
  (`/tokens/sync`, legacy `/token/sync` redirects),
  MyWallet (**bag overview** — Funding (SOL + USDC) + trading KPIs + meme-positions
  table; row select opens detail + live `LazyTokenTradeChart` below (ingest
  `trade_executed` → RTK trades cache); cash not selectable; Console link; row-triggered
  buy dialog only — the old free-text header modals are gone; table
  reloads on position/our-wallet SSE),
  **Console** (`console/ConsolePage` — see "Operator clarity" above; `?mint=` prefills
  the manual-trade panel, `?position=` focuses a row's detail modal. Every position /
  Waiting modal carries a **`RuleConditionStrip`** under the fact strip: one chip per
  authored condition — `metric op threshold` with the value behind it — fed by
  `LiveRuleConditions` off `.../positions/{id}/metrics`. Open rows poll it at 1 s
  (**not** SSE: that bus already carries a frame per ingested trade and sheds under
  load); a closed row comes back `source: "replay"`, which stops the polling and
  captions the strip `○ reconstructed at exit` with an entry/exit switch, because a
  reconstruction must not read as engine truth. Chips are tone-neutral on purpose — a
  satisfied *entry* condition is why we are in and a satisfied *exit* one is why we are
  leaving, so one green would mean opposite things; satisfied is emphasized and `✓`-marked
  instead. TP/SL keep their threshold (`take profit >= 40`), a gated trail shows
  `arms +N%`, a skipped trail is dashed, an inactive ladder stage is dimmed. Backend
  contract + the parity traps: [strategies.md](strategies.md) *Rule readout*),
  Rules/Fingerprints
  (+ `InputSyncStatus`, `wallet/` components; `usePositionNotifications`; `syncTokenSlice`).
- **Lab (`@lab/pages`):** **Research home** (`LabHomePage` — shortcuts + recent sweeps
  deep-linked with `?run=` + running jobs), Creation Stats (heatmap/trend/grouped
  metric toggle carries **3 outcome + 3 trade metrics** in one payload — `count`,
  `migrate_rate`/`dead_rate` (rates), `trades`/`trades_per_day` (magnitudes, share-
  of-max shading), `trades_per_token` (a ratio backed by the mean `trades_avg`,
  log-scale contrast stretch — see `creationStats.ts`'s `MetricKind`); the metric
  toggle never refetches. `GroupedCreationSection` additionally offers **Rank
  by: Tokens / Trades / Trades per token** (`rank_by`, default unchanged =
  token count) so a small elite fingerprint group can out-rank a big group of
  mediocre launches. Drill-down opens
  `LazyLabTokenInspectModal` via `inspectFromMint` — same chart + metric panes as
  Tokens), Tokens (detail = chart +
  metric panes via `LabTokenInspect`), **TraderAnalysis**, Rules (authoring + dry-run
  **+ Evidence over the traded real/paper positions from the synced mirror**, where a
  fill opens the metric panes — see the Rule Evidence bullet below)/Fingerprints/
  **Flow discovery**/Simulate (sim table demotes live Active/Mode into a muted rule
  subtitle)/Replay, and the generic Grouped Sweep (sticky Run › Group › Combo
  breadcrumb; Simple = configure→promote; Full drill = combo/token inspect via
  `SweepTokenInspectModal` with metric panes; `sweep/` + `strategy/` components,
  `useStreamedSweepResults`, `BackgroundJobsContext`).
  Shared page chrome: `PageHeader` / `EmptyState` / theme-token `InlineAlert`.
  **Metric panes** (lab Tokens detail; old `/strategies/metric-panes` redirects): `LabTokenInspect`
  stacks `TokenTradeChart` above registry-driven `MetricPanes`. Shared wall-clock
  crosshair / visible range (`TokenPriceChart.onCrosshairTimeChange` /
  `onVisibleTimeRangeChange`); selecting a rule auto-loads its metrics/windows,
  overlays thresholds, and paints first metric entry/exit fires as `eventMarkers`
  with `role: 'signal'` and spaced `name op value` labels (e.g. `stall > 3`) —
  visually distinct from backend fill markers (`role: 'fill'`, green/red arrows +
  price lines). Entry fire skips rows where exit metrics already hold — same
  `can_enter` gate as the engine.
  Values are **readout-first**: a sticky strip lists every selected metric's
  crosshair/latest number; each pane has a large value rail + sparkline min/max
  and labeled thresholds (shape is secondary). Series from
  `GET /api/tokens/{mint}/metric-series` (optional `fingerprint_id`, per-event
  `price`). Helpers in `lib/strategy/metricPanes.ts`.
  **Rows are per *event*, not per trade** — the backend folds over the engine's
  `TICK_MS` grid, because the time-decaying metrics only advance on a tick and a
  trade-only series draws the fire marker late (it once drew an exit 70 s off).
  The panes must therefore declare what they'll evaluate: `windows` covers the
  trailing metrics and `metricClockHorizons(params)` supplies the `time`/`stall`
  ceilings that size the backend's sparse grid. A response can come back
  `truncated` (row ceiling) — render the coverage notice, never a silent partial.
  The shared `TokenTradeChart`/`TokenPriceChart` take an optional `highlightWallet` — its
  markers render larger with a gold glow+ring (`ProfileWalletInfo.isHighlighted` →
  `walletMarkersPlugin`), and a non-tracked input address gets a synthetic marker entry.
  **Tracked-wallet markers are a structural invariant:** `TokenPriceChart` defaults
  `profileWallets` to `useProfileWallets()` when the prop is omitted, so *every* token trade
  chart shows tracked-wallet markers by construction (pass an explicit list to override,
  `[]` to force none). `useProfileWallets` imports the palette/type from the chart's leaf
  files (not the barrel) to avoid an import cycle now that `TokenPriceChart` consumes it.

## Rule authoring — registry-driven (`lib/strategy/`, strategy redesign)

The named strategies (tpsl1/tpsl2/swing_1) and the hand-written `lib/params/` spec
engine are **gone**. One generic engine authors every rule = *fingerprint reference +
metric conditions*, and the whole UI renders from ONE payload
(`GET /api/meta/strategy-registry`) so a metric added in Rust appears everywhere on the
next load (no per-metric frontend work).

- `lib/strategy/registry.ts` — types mirroring the registry payload (`operators`,
  `groups[]` → metrics w/ unit/eq-tolerance/monotonic + strict params) + the cached
  `useStrategyRegistry()` hook (RTK Query, 1 h). `unitSuffix`/`findGroup`/`findMetric`.
  New engine groups (e.g. `m_flow_lifetime`) appear in rule/sweep pickers from this
  payload alone; `strategyHelp.ts` `GROUP_HELP` / `METRIC_HELP` supplies the ⓘ copy.
- `lib/strategy/grammar.ts` — the condition grammar (`">10, <=30"` → `{operator,value}`
  list; `1..10` → `>=1 AND <=10`), wrapping the shared compound `numericFilter` parser.
- `lib/strategy/ruleParams.ts` — the ONE generic `params` JSONB ⇄ form serializer
  (registry-guided strict/metric split; includes `scale_out: ExitStage[]`);
  `validate.ts` mirrors backend §5 validation (incl. scale-out caps).
- `components/strategy/` — `ConditionInput` (grammar input + chips + red-underline),
  `ConditionBuilder` (entry/exit columns; exit-only mode for scale-out stages; the
  per-row `⏻` **parks** a condition — kept, still validated, but folded into
  `params.disabled` instead of the live side, so the engine never compiles it.
  `lib/strategy/ruleConditionRows.ts` owns the row↔bag fold: `enabled` on the row,
  `rowsToSides` → `{entry, exit, disabled}`, live/parked keyed separately in the
  duplicate + `arm_above_pct`-orphan checks, and `parkedSideWarnings` for the case
  that matters — parking a side's LAST condition rewrites the rule silently
  (empty entry ⇒ buys on the fingerprint alone; empty exit ⇒ TP/SL/death only).
  **Scale-out stages pass `allowToggle={false}`**: a stage's `conditions` have no
  `disabled` bag of their own — park the whole stage instead, below),
  `ScaleOutBuilder` (ordered partial-exit ladder; each stage carries the same `⏻`
  park toggle — `enabled` on the draft, `draftsToStages(drafts, enabled)` splits the
  live ladder from `params.disabled.scale_out`, and every budget question — stage
  count, sell-% sum, remainder — is asked of the LIVE stages only, so parking a stage
  frees its slot and its share of the bag. Un-parking an explicit stage re-inserts it
  before a live remainder, the ladder's one ordering rule. **The lab sweep config form
  passes `allowToggle={false}`**: a run stores a bare `ExitStage[]` with no bag, so a
  parked stage there would vanish on reload), `RuleEditor` (builder + JSON tab + a
  `renderDryRun` slot; edit mode locks `trade_mode` behind a padlock unlock),
  `FingerprintPicker`/`FingerprintForm` (registry-driven
  `metric_config` section + `VolumeIxPatternsEditor` for `m_flow_split.volume_ix_patterns`
  — add-row / remove-row / **Delete all** footer, the last confirming via the shared
  `clearPrompt` also used by the flow-discovery cart),
  `RulesView`/`FingerprintsView` (shared list+editor, mounted by both apps'
  `RulesPage`/`FingerprintsPage`; cross-page selection via `?rule=` / `?fp=`
  (`useSelectionSearchParam` + `lib/strategy/nav.ts` — same-tab Router `Link`,
  Ctrl/middle-click still opens a new tab). Rules support soft-archive via
  `is_enabled` (Enable/Disable endpoints; Disabled hidden by default on Rules +
  Simulate, orthogonal to Active/Idle) and **tags** — a tri-state chip bar
  (`RuleTagFilter` + `TagChip`, off → include → exclude; includes OR, excludes
  hide) over the `tags` column, backed by `useTagFilter` (`?tags=`/`?notags=` in
  the URL, sticky per app in `localStorage`), authored in the editor via
  `RuleTagsInput`. The column itself is `buildRuleTagsColumn` — shared with
  Simulate, where the same chip bar narrows what "Simulate Filtered" targets.
  Tags are presentational only and orthogonal to `is_enabled`:
  chip colour is hashed from the label (`lib/strategy/tags.ts` → the shared
  `chipColorsFromHue`), and the canonical tag grammar lives server-side, NOT here
  ([rule-tags.md](@plans/strategies/rule-tags.md)).
  Beside the tag chips sits the **trade-mode scope** — `RuleModeFilter`
  (All / PAPER / REAL + counts over shared `ModeToggle`) over `useModeFilter`
  (`?mode=` in the URL, sticky per board: Rules, Rules Control and Simulate each
  keep their own key).   Ops surfaces (Console / History / Portfolio) use
  `ModeToggle` directly (`layout="ops"`). Datetime windows use
  `DateTimeRangePicker`; single civil days use `DatePicker`; other exclusive
  non-mode filters (score scope) use `ToggleGroup`; panel swaps use `Tabs`;
  paper/real pills use `ModeBadge` / `modeBadgeVariant` — see
  [ui-controls.md](@plans/frontend/ui-controls.md).
  It is the same view-filter shape as tags and composes with them: each control's
  chip counts come from the set narrowed by the *other* one, so a count never
  collapses to its own selection. Because both pages derive everything
  downstream from the filtered set, scoping also makes the Rules scoreboard
  tiles mode-pure and narrows what Simulate's bulk-run buttons target — and the
  Total PnL tile spells out its `real … / paper …` split rather than presenting
  one blended figure whenever both modes have traded rules on screen.
  Every rule row carries a mode rail (`ruleRowClass` — the ONE `rowClassName`
  for all three boards, composing the rail with the soft-archive dimming), so a
  real-money rule is identifiable regardless of sort, filter, or scroll. The
  rail is deliberately a background **gradient**, not a background colour: the
  `DataTable` merges `rowClassName` last through tailwind-merge, where a
  `bg-<color>` would collapse the row's selection and pin washes; a row-level
  `box-shadow` is not an option either under `border-collapse: collapse`. Locked
  by `lib/strategy/mode.test.ts`. Fingerprints "Used by" → Rules;
  Rules/Simulate fingerprint cells → Fingerprints; lab Rules → Simulate
  (`linkToSimulate`); Simulate rule name → Rules. Sweep Used-by / matched fp,
  Flow Discovery seed/target badges, and live Armed rule names also deep-link),
  `RuleParamsSummary` (`ruleParamsCell` — TP/SL + in/out metric chips; used by Rules,
  Simulate, and the generic sweep tables),
  `FingerprintParamsSummary` (`fingerprintParamsCell` — set match-axis chips + bucket;
  used by Rules, Simulate, and `FingerprintPicker`).
- Lab **Flow discovery** (`/strategies/flow-discovery`, `FlowDiscoveryPage`) — corpus
  window + optional **Scope by saved fingerprint** (sends `fingerprint_id`; engine
  match SSOT fills the corpus) or manual `FingerprintGroupPicker` → ranked
  ix-structure table → toggle draft patterns → Apply (`PUT` / create-bind). UI split:
  `flowDiscoverySuggest` / `StructureTable` / `DraftPatternsCart` / `TokenPreviewPanel`
  under `lab/components/flow/`. Three independent bulk-selects, each paired with what
  explains it: *Auto-select suggested* (`Auto` column — bot-likelihood composite),
  *Launch shapes · group* (`Launch%` column shows creation-slot **purity**, but the
  button takes every table row *present* in some member token's creation slot) and
  *Launch shapes · this token* (only while a token is picked in `TokenPreviewPanel` —
  applies that token's own `first_slot_ix_labels`, which bypasses the table entirely:
  uncapped, unfloored, per token). See `plans/strategies/metrics-reference.md` for why
  the group answer cannot stand in for the per-token one. Job kind `discovery` in `BackgroundJobsContext`
  (SSE `flow_discovery_*`, mutual exclusion with sweeps).
- The lab `RulesPage` injects `@lab/components/strategy/DryRunPanel` via `renderDryRun`
  (inline draft → `POST /api/strategies/simulate` → funnel summary + trades table),
  boundary-clean. Finished dry-run trades share Simulate's chart path: `useRowOverlay`
  entry/exit markers, mint-grouped episode overlays (`useSimMintEpisodeOverlay`), and
  row/chart select → `LabTokenInspectModal` with metric panes pinned to the live draft
  via `ruleOverride` (params + fingerprint). Shared episode-marker fetch lives in
  `@lab/hooks/useSimMintEpisodeOverlay`.
  Lab `SimulatePage` (`/strategies/simulate`) runs saved rules over the full lake and
  shows the `SimulatedSummary` rollup as separate DataTable columns (Mode, Entered /
  Closed / Win % / Avg PnL / Total PnL, plus a Run status) so sort/search/filter work
  per field. On load it hydrates *every* rule's resident sim summary in **one**
  `POST /api/strategies/simulate/summaries` round-trip (for the columns); a finished
  run still refreshes via the single-run `…/result/summary` path. The per-token
  detail below is **selection-gated** — only the selected rule's
  `RuleSimPositionsPanel` renders one table — the run's per-token outcomes including
  matched-but-never-entered `NoEntry` rows (`POST /api/strategies/simulate/{run_id}/result`,
  `simColumns` — same full-slice contract as the sweep combo drill-in). **Show/Hide
  not fired** injects `exit_reason != NoEntry` (server-side) so Charts can compare
  both or focus on fired only; badge shows `N · K fired` / `K / N` like the sweep
  drill-in. A bare `K` with no NoEntry rows means a stale pre-padding result — re-run
  Simulate. There is no separate Matched candidate tab — Positions already is the
  full matched slice.

## Grouped sweep — generic engine (`strategies/sweep/`, redesign FE5)

ONE page (`/strategies/sweep`, `GenericSweepPage`→`GenericSweepView`) replaced the three
per-strategy sweep pages. Reuses the kept streaming/persistence infra
(`useStreamedSweepResults`, the `getGroupedSweep*` / `startGroupedSweep` RTK endpoints,
`SelectedSweepHistory`, `FingerprintGroupPicker`) with `strategy_id = "generic"`.

- `sweep/genericAxes.ts` — the registry-driven axis model: `AxisSpec[]`
  (`{side, group, metric, operator, values[, window]}`), value parse (comma list +
  `lo..hi step s` ranges), per-row validation, combo-count. Distinct windows on the
  same (side, group) are allowed — they assemble into one `GroupConditions` instance
  per `window_size_sec` (the engine's multi-window-per-group model), so there is no
  cross-row window-conflict check. Unit-tested.
- `sweep/GenericAxisBuilder.tsx` — axis-row UI + projected-combo badge; `GenericSweepConfigForm`
  wraps it with corpus/method/caps + `FingerprintGroupPicker`, emitting `{axes:[...]}`.
  When axes reference `m_flow_*`, the form requires `volume_ix_patterns` (corpus-wide
  for the run) and sends them on start.
- `sweep/genericSweepColumns.tsx` — combo/group columns; the swept `params` is a
  `RuleParams` blob rendered via shared `ruleParamsCell` (not one flat column per knob).
- `[Promote…]` on any group/combo → `POST …/promote` (fingerprint find-or-created;
  copies run `volume_ix_patterns` into `metric_config`) →
  `PromoteRuleModal` opens the shared `RuleEditor` pre-filled (id-less draft → create)
  with the lab dry-run panel. Save is refused when an identity-identical rule already
  exists (`matchRuleIdentity` FE pre-check + backend `RuleError::Duplicate` 409 — same
  gate as Rules / Simulate create). Replaced the copy-blob path.

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
- **Tokens page = same contract (`POST /api/tokens`).** `getTokensPage` (`sharedEndpoints.ts`) serializes
  DataTable view-state via `toTableRequest`; page-owned quick filters (`TokensFilterBar` →
  `quickFiltersToSpecs`) and mint-set arrive already folded into `structuredFilters` (structured wins
  on key collision with per-column text). The Tokens-only `trackedOnly` rider sits alongside. Backend
  lowers each `FilterSpec` onto its internal panel/per-column representation
  (`TokenQuery::from_table_request`), so the LIVE (Postgres) and LAB (in-RAM) engines stay identical
  (DB parity test). The old mega Advanced-filters panel and bespoke `f_*`/`cf` query builders are gone.
- **`DataTable` stays domain-agnostic.** Extensibility is via `searchPlaceholder`,
  `toolbarLeading`/`toolbarTrailing`, and `ColumnDef.filterOptions` / `filterOptionValue` (enum
  filter-row selects). Token/History chrome lives in wrappers or page bars (`TokenTable`,
  `TokensFilterBar`, `HistoryFilterBar`) — never token vocabulary inside `components/table/`.
- **Rule Evidence = server-side paged + summary, shared by BOTH apps** (`shared/components/strategy/RuleAnalyzePanel`
  via `useServerTable` +
  `fetchRulePositionsPage` / `fetchRulePositionsSummary`): `POST …/rules/{id}/positions[?scope=current|run|all|history]`
  (+ `run_seq` when `scope=run`) and `…/summary`; run chips from `GET …/strategy-rules/{id}/runs`;
  shared **`PositionSummarySection`** (hero + exit mix + focus chips + a `▾ Charts` collapse over
  the chart cards: Equity | Return / Hold mix | Wall clock / Timing (daily calendar + dow×hour
  heatmap) / Hold vs PnL — see [position-summary.md](../plans/frontend/position-summary.md)).
  Full-cohort chart series is paged client-side from the same positions endpoint (not the table page);
  focus lenses stack on top of column filters without writing into the DataTable filter row.
  Embedded under Rules Control when a row is selected (`key={ruleId}` remount). Default Evidence
  scope follows Control (`current` / `all`); no auto-flip to History. Activate/Pause/Stop live on
  Control Execute **and** the Evidence header. Open inventory manage is **Ops** (Live Status SSOT),
  not this table. See [rules-cockpit-ux.md](../plans/frontend/rules-cockpit-ux.md).
  The panel itself is app-agnostic; each app injects its own wiring via props:
  - **live** (`@live/pages/strategies/RulesPage` → `LiveRuleEvidence`, and the standalone
    `RuleAnalyzePage`) passes `liveUpdates` (SSE on the same `rule_id` triggers `reload()`) +
    `liveOpenCount` from `selectOpenByRule` + `renderInspect` → `LivePositionInspectModal`
    (Floor chart + fills ledger; no metric panes on this bin). That component **is** Console
    History's row modal too — History renders `LazyLivePositionInspectModal` with `rule={null}`
    + a resolved `ruleName`, so the `RulePositionRecord` → `FloorDetailFacts` mapping and the
    modal header exist once. The header itself is `PositionModalTitle` in
    `@live/components/floor/openPositionStatus`, shared with the Console's open-row modal.
    Vol/non-vol overlay SSOT: `hooks/useFlowPatternKeys` (+ `useFlowPatternKeysForRule` /
    `useResolvedFlowPatternKeys`) and `lib/flow/flowPatternKeys` resolve fingerprint
    `volume_ix_patterns` → `flowPatternKeys`. Wired into Evidence `TokenTable` charts,
    `LivePositionInspectModal`, Console History/open/waiting detail, fingerprint matched-tokens,
    and sweep combo charts (run patterns; omit/empty ⇒ toolbar disabled).
    The open-count selector lives in that leaf so a status tick re-renders the panel, not `RulesView`.
  - **lab** (`@lab/components/strategy/LabRuleEvidence`) passes `notice` + `renderInspect` + `scoreScope`
    (Evidence default follows the list's scoreboard scope) and serves
    the same endpoints off the synced mirror (`lab/.../live_positions.rs`) — clicking a position opens
    `LabTokenInspectModal` with `ruleOverride` pinned to the rule that traded and `positionEntry` on the
    real fill, so a live fill reads against the metric panes. **No `liveUpdates`/`liveOpenCount`**: that
    box has no ingest or SSE, so the rows are a snapshot as of the last `db-incremental-sync.ps1`.
    Lifecycle buttons still render there, but they write the local mirror only — the live engine never
    sees it and the next sync overwrites the row (server wins).
- **Simulated = server-side via `useServerTable`** (lab-only). A lean page+total+summary hook
  (no SSE-delta patching / settle-poll — these results are static once computed) drives the table
  over `fetchEngineSimPage` (POST, `{tokens}` body + `X-Total-Count`). There is no separate
  **Matched** table: matched-but-not-entered tokens fold into this one as `NoEntry` rows, and
  "Matched" survives only as the `n_matched` summary stat (the candidate pool Entries is drawn
  from). **Simulated** pages the finished backtest's rows from the lab disk cache (`$SWEEP_LAKE_DIR/sim-results/`, hydrated into a
  one-rule RAM working set), with a matching `POST /simulate/result/summary` aggregate
  (`toSummaryBody`) for its card; unfiltered column hydrate uses meta only
  (`POST /simulate/summaries`). `reload()` refetches on the `simulation_finished` SSE. Simulate
  and Sweep combo drill-in mount the same **`PositionSummarySection`** as Evidence (Hold mix +
  Wall clock as deck ChartCards; PnL% distribution / hold scatter / equity / timing calendar +
  heatmap as sibling cards; stacked focus chips). Simulate bins via
  `POST …/result/time-summary?wall_field=&wall_grain=&hold_scheme=&tz=` (`tz` = the app
  timezone: the wall bins are CIVIL buckets and must floor in the same zone the calendar +
  heatmap do — absent ⇒ UTC) (base = table filters; linked =
  mint-filtered refetch with base grain/scheme locked) and pages a full-cohort chart series for
  distribution/scatter/equity; sweep folds client-side from `ComboTokenResult` rows. Focus lenses
  (including Temporal mint sets) narrow summary + charts + table together. Default wall field is
  **`exit_time`** = the decision instant (sold, or bought while open) — the same stamp equity /
  calendar / heatmap bin on, so every dated chart agrees on a position's civil day;
  `entry_time` / `created_at` stay on the Wall toolbar.
- **Token enrichment is server-side, not client-merged — for EVERY token table.** Every token-result
  table (Matched, Positions current/history, lab paper positions, Simulated, Sweep drill-in, **and, since
  Phase 4, Wallet Holdings**) receives the full `TOKEN_ENRICH_FIELDS` set **in the response body** — the
  backend attaches it from one shared `trading_core::storage::token_enrichment` SSOT — so sort/filter/
  search on enrichment columns works across the whole result set. `mergeTokenData` + the per-table
  `useGetTokensByMintsQuery` batch call do not exist — there is no client-side merge path,
  and re-adding one reintroduces search/sort that only sees the current page.
- **`TokenTable` = the ONE wrapper for every token-row table** (`components/tokens/TokenTable.tsx`).
  It owns the "token recipe" over `DataTable`: (1) append the shared token-info columns
  (`appendedTokenColumns`, so callers export only their bespoke columns + an `existingKeys` set — see
  `components/strategy/strategyColumns` `POSITION_KEYS`/`SIM_KEYS`, each derived straight from
  its column array so keys can't drift from what's rendered; a table that owns its full layout
  passes `ALL_TOKEN_INFO_KEYS` to append nothing); (2) own the table wiring. **Two modes:** **server**
  (`serverSide` + `serverTotal`/`onQueryChange`/`resetKey`) — rows arrive backend-enriched one page at a
  time, paging/sort/filter round-trip (Positions via `RunPositionsPanel`, Paper, Matched, Sim, Wallet
  Holdings, **Tokens page**); **client** (default) — rows are the full already-enriched set and
  `DataTable`'s **own** client paging/sort/filter/search runs in-browser (NO separate evaluator — that TS
  twin retired with Wallet), used by tables with no backend paging endpoint (**Trader Analysis**,
  **Sweep drill-in**). Every token-data row keys its mint under the one canonical field `mint_address`
  (SSOT across DB → wire → JS), so the mint accessor is fixed internally — there is no
  `mintOf` prop; it drives the charts grid, the default `rowKey`, and the client mint-set pre-filter. Two opt-in
  features live here so every token table gets them once: **`mintSetFilter`** — a `<MintSetInput>` paste
  box (server: an `in` op on `mint_address` folded into `structuredFilters`; client: a plain row pre-filter);
  **`charts`** — a toggle rendering `<TokenChartsGrid>` (lazy-mounted, current page only, with
  `renderChartCardExtra`/`titleOf`/`highlightWallet` slots) below the table, fed by the table's
  intercepted `onVisibleRowsChange`. With `groupByMint`, `renderChartCardExtra` also receives the
  mint's group rows so headers can fold re-entries. With `onSelect` wired, a chart **card header**
  click selects that mint (same toggle contract as a row — opens inspect); the chart canvas itself
  stays interactive (pan/zoom/bar-select). Strategy-result tables also pass **`useRowOverlay`** — a
  per-row hook (`ChartOverlayHook`) resolving the same **entry/exit markers + swing legs** the row's
  inspect modal shows, so the inline charts match the modal. It's built from the shared
  `inspectTarget` helpers (`markerRowOverlay` for tpsl entry/exit; `carriedSwingRowOverlay` for
  `live` swing1 positions whose legs ride the row) or `@lab/hooks/useSwing1DetectOverlay`'s
  `makeSwing1DetectRowOverlay` (lab swing1 positions/matched/sim + grouped-sweep combos, which
  re-run `swing1-detect` per card keyed off the section's rule/combo params — sim rows carry their
  legs and skip the fetch). A scale-out draws **one exit arrow per leg**, never one at
  `exit_price` — that column is the SOL-weighted average across legs, a price nothing filled
  at. Legs reach the chart through `InspectTarget.exitLegs`, fed from `exit_legs` on the row
  (`inspectFromPosition` / `inspectFromSim` share one wire shape and one mapper, so a modeled
  and a traded ladder render identically); the backend attaches them per **page**, only for a
  real ladder (>= 2 legs), so a single-leg close still falls through to `exit_*` unchanged.
  `FloorPositionDetailWithFills` re-derives them from the `position_fills` ledger instead — the
  only source with the legs of a position still laddering, kept live by `useLivePositionFills`.
  The chart toolbar's Events toggle covers the whole overlay, arrows *and* dashed fill-price
  lines; on a ladder each line is titled by its share (`Exit 70%`) so N legs don't read as one
  exit drawn N times. Position tables (Evidence / Simulate / Dry-run / Sweep) share
  **`PositionChartCardExtra`** (hold · exit · PnL% · size · entry/exit price; multi-episode fold
  when `chartsGroupByMint`). Trader Analysis uses `TraderChartCardExtra` (wallet buys/sells/hold/
  vol). `DataTable` stays token-agnostic: the dependency is one-way (`tokens/` → `table/`),
  asserted by `components/table/DataTable.boundary.test.ts`. **Every** token-row table now renders
  through `TokenTable`. Trader Analysis row / chart-card select opens `LazyLabTokenInspectModal`
  via `inspectFromMint`.
- **Trader Analysis wallet PnL analytics (`lab/components/analysis/`).** The per-mint rows returned by
  `/api/wallets/:wallet/tokens` (`WalletTokenRow` in `lab/api/handlers/wallets.rs`, backed by
  `strategies::kernel::wallet_mint_pnl` — an avg-cost reconstruction over that wallet's in-window trades on
  the mint, with gross **and** fee-adjusted-net realized PnL plus mark-to-market unrealized PnL off
  `current_price`) land on `TraderTokenRow` as `wallet_*` fields. `TraderAnalysisPage` feeds the table's
  full **filtered** cohort (via `TokenTable`'s `onFilteredRowsChange`, not just the visible page; pinned
  when focus activates — same pin pattern as Sweep drill-in) into `<WalletAnalyticsPanel>`: summary KPIs
  + Open/Closed/Win/Loss toggles, a focus-chip strip (`PositionFocusChips` over
  `lib/strategy/positionFocus` via `walletFocus.ts`), and a collapsible multi-chart deck mirroring Console
  History / Position Summary — Equity path · Return shape · Hold vs PnL · Ranked by PnL · Timing (daily
  calendar + dow×hour heatmap). Chart clicks stack focus lenses (`heat` / `day` / `week` / `pct` / `pos` /
  `band` / `status` / `outcome`); timing charts keep the parent cohort with a selection ring, other charts
  + the token table refold on the focused slice. All chart data is pure/DB-free —
  `lab/components/analysis/walletPnlStats.ts` derives every shape from the current `TraderTokenRow[]`
  cohort, unit-tested in `walletPnlStats.test.ts` / `walletFocus.test.ts` — so filtering the table
  live-updates every chart without a refetch. Every figure is a per-mint aggregate, not a per-episode
  ledger: a wallet that re-entered a mint several times in the window collapses to one row (see the doc
  comment on `kernel::wallet_mint_pnl`).
- **One in-memory evaluator, in Rust only.** Token tables whose rows are RAM-resident on the backend (the
  lab Simulated table; the live Holdings composition) page/sort/filter through
  `trading_core::api::table_eval::apply_table_request` with a per-table `ColResolver` grammar; the shared
  enrichment half of that grammar is `resolve_token_enrichment_key` (SSOT — the Simulated and Holdings
  resolvers both delegate to it). There is **no TS twin** of the evaluator — every such table
  is server-side, so a client-side column resolver or row merger is always a duplicate.
  The golden fixture `tableEval.fixtures.json` and the Rust `table_eval::conformance_shared_fixtures`
  test are **kept** (now Rust-only) so the evaluator's op/sort/search/tiebreak/paging semantics stay
  pinned.
- **Shared enrichment type + strategy primitives.** The ~28 enrichment fields the backend
  `TokenEnrichment` flattens onto result rows are declared **once** in TS as
  `TokenEnrichmentFields` (`shared/types`); `RulePositionRecord`/`SimulatedTokenResult` `extends` it (the all-required `TokenRecord`/`TokenDetailRecord`
  stay bespoke — their nullability differs by endpoint on purpose). Strategy-page boilerplate
  is shared under `shared/components/strategy/`: `cellFormat.ts` (formatters + instruction-label
  parsing, one copy for every strategy family), `inspectTarget.ts` (the `InspectTarget` type + `inspectFromSim`/
  `inspectFromPosition` mappers — one copy for five pages and both modal forks).
- **One strategy-table column SSOT (`strategyColumns.tsx` in `shared/components/strategy/`).** The
  Positions / Sim tables' `positionColumns`/`simColumns` (+ their
  `POSITION_KEYS`/`SIM_KEYS`) + `exitReasonBadge` live here **once**. The
  **target/entry/exit** trade legs — each with **Price · Tokens · Size · Time · Tx** — are emitted by one
  `legColumns(prefix, accessors, opts)` builder (`Size` = `solOf(price, tokens)` unless a real SOL field is
  given; Tokens/Tx columns drop when their accessor is absent). One builder is the point: a
  per-family copy of these column defs **drifts** — one family loses the whole target leg +
  tokens/size/tx, another's sim shows only price/time on entry/exit. Every strategy page shares
  this one source.
  The sim's exit leg still omits Tokens/Size because the sim result payload carries no `exit_token_amount`.
- **One token-info column SSOT (`tokenInfoColumns()` in `sharedTokenColumns.tsx`).** The ~26 enrichment
  columns are defined **once** (render/sort/search/filter logic); both consumers derive from it —
  `appendedTokenColumns(existingKeys)` (strategy columns, and wallet via `TokenTable`) overlays `defaultVisible` via
  `APPENDED_HIDDEN_KEYS`, and the Tokens page (`tokenColumns.tsx`) pulls each column by key through
  `tokenInfoColumnMap()`, adding only its own presentation (order + `TOKEN_COL_WIDTH` widths) and
  Tokens-only columns (identity/`token_age`/`lifetime`/fep-ratios). Per-view `defaultVisible`/width/order
  legitimately differ; the render/sort/filter facts don't. The matched tables must not hand-roll
  `init_buy`/`cu_limit`/`cu_price` — those come from the shared `initial_buy`/`cu_limit`/`cu_price`
  columns.
- **Numeric column filters** (`>5`, `1..10`, `>=`, `!=`): every numeric column declares `filterNumber`
  in the column's **displayed** units (percent points for Win%/Open%, SOL↔USD for PriceUnit amount
  cells, SOL for lamports enrichment). The `DataTable` emits raw filter text; the serializer
  (`toTableRequest` via `parseFilterSpec`) turns a numeric-column expression into a structured op.
  PriceUnit amount columns also set `filterAmount: 'sol'|'usd'` so `toTableRequest` converts the typed
  operand back to storage before the server compare (`lib/priceUnitSnapshot`). `!=` has no server op
  and maps to `eq`; the legacy `parseNumericPredicate` (still used by any fully client-side table)
  keeps the real `!=` negation.
- **One PnL-analytics SSOT (`shared/components/analytics/`).** The folds live in `pnlSeries.ts`
  over ONE neutral atom, `PnlPoint` (`{key, timeMs, pnlSol, pnlPct, label, groupId?, isOpen?}`):
  `buildEquityCurve` (+ running peak → `maxDrawdownSol`), `pnlDistributionBuckets`,
  `buildPnlHeatCells`, `buildDailyPnl`, `buildHoldScatterPoints`, `rankByValue`, and
  **`foldPnlDeck`** (one cohort walk → curve + buckets + heat + daily + sparkline value arrays +
  per-group trends incl. the decay verdict — used by Console History, Portfolio, Home digest so
  those surfaces never re-walk the same closes; it is the ONLY entry point, the standalone
  per-group folds it superseded are gone). Civil day/dow/hour share one cached
  `Intl.DateTimeFormat` per timezone (`civilPartsInTz`). Renderers: `EquityCurveChart` (the ONLY
  one pulling `lightweight-charts` — lazy-load it; `fitContent` only when the series identity
  changes), `PnlDistribution`, `PnlHeatmap`, `PnlCalendar`, `HoldPnlScatter` (log hold × PnL%;
  brush drag is DOM-ref based so it does not remount markers; canvas above ~250 points),
  `RankedPnlBars`, `PnlSparkline` (inline SVG, cheap enough per table row). Callers map their own
  row type into `PnlPoint` and every chart derives from the same points, so an equity curve can't
  disagree with the histogram beside it. Promoted out of `@lab/components/analysis` (a sanctioned
  lab→shared move — the live app may never import `@lab`); the `Wallet*` components there are now
  thin adapters that map `TraderTokenRow` → `PnlPoint` and render the shared pair, and
  `walletPnlStats.ts` keeps only the wallet-specific summary/scatter.
- Memoized column defs/price formatters; cells read context directly. localStorage via `lib/storage`
  (`mt:` namespace); column visibility in one `mt:table.cols` map keyed by `tableId`.
- **Durable UI prefs have one gate:** `lib/storage.ts` (`STORAGE_KEYS` + `ACCORDION_IDS` +
  `migrateLegacyStorage`) with `hooks/useLocalStorage` (`useLocalStorage` / `useStoredField`) and
  `hooks/useUiPrefs` (`useAccordionOpen` / `useUiToggle`) on top. Related prefs group into blobs
  (`mt:ui.accordion`, `mt:ui.toggles`, `mt:table.*`, `mt:page.creationStats`) rather than one flat
  key each; a raw `localStorage.*` in a component fails `lib/storageGate.test.ts`. Key table,
  persist-vs-not policy, and how to retire a key:
  [../plans/frontend/frontend-patterns.md](../plans/frontend/frontend-patterns.md) § localStorage.

## Known follow-ups (NOT yet done)

- **Cosmetic deviation:** shared store core lives in `src/shared/store` but the legacy `store/*`
  alias still resolves there; the per-mode `services/` file split was skipped (tree-shaking over
  one shared `services/api.ts` achieves the same bundle isolation since the helpers are
  side-effect-free).
