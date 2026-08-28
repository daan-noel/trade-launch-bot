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
  **History** and **Arms** sections (both collapsible, and collapsed means NO fetch —
  the body unmounts, since each pays for a server page plus an aggregate);
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
**Arms is History's twin on the other side of the entry decision** and reads the same way:
its own `a*` cohort, a funnel strip, a server-paged table, and a row click opening
`ArmDetailModal` (←/→ walks the page). Both tables own their modal, because the row each
opens lives on its own page rather than in the live registry the cockpit lanes read.
The Arms funnel breaks its `Unsat` tile down by the entry condition that held each
episode out (`armBlockers.tsx` renders `end_detail` for the strip, the **Blocked by**
column and the modal's verdict line, off ONE string builder); each bar is a lens on its
own `ablocked` cohort key, which composes with the reason lens rather than replacing it.
Detail: [review-surfaces.md](../plans/frontend/review-surfaces.md),
[arm-ledger.md](@plans/strategies/arm-ledger.md).

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
Both the snapshot and every `strategy_armed_changed` frame carry **`armed_at`** — the
server's instant for the episode, on the arm AND the disarm. A client that stamped its
own arrival time restarted every Waiting row's age on reconnect and disagreed with the
arm ledger about the same episode.

**Waiting (live) vs Arms (durable):** the Waiting lane reads the in-RAM registry and
loses a row the instant it disarms; the **Arms** section pages `strategy_arms` over a
date range and keeps every episode, including the ones that never traded. Two readers
of one fact on purpose — the lane must patch per-event with no round trip
([arm-ledger.md](@plans/strategies/arm-ledger.md)).

**Live trading notify SSOT (one EventSource, writers):**

| Domain | Push | Client sink |
| --- | --- | --- |
| On-chain trades | `trade_executed` (includes `tx_index`/`leg_index`/reserves/`fee_sol`/`instruction_labels`, plus the cumulative `live` stats snapshot) | Tokens table row patch; chart `getTokenTrades` append via `watchTokenTradesMint` + `liveTradeToTradeRecord`, **and** `getTokenDetail` stats patch via `applyTokenLiveStats`; `useMintTradeStream` for mint-filtered feeds |
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

**A chart's bars and its reference lines read different caches**, so both have to be
patched. Bars come from `getTokenTrades`; the ATH line, the ATH/FEP readout and the
detail stat cards come from `getTokenDetail` — a one-shot query with no poll and no
invalidating tag. Patching only the trades cache paints new highs above an ATH line
frozen at mount time, so `tokenTradesLive` writes the frame's `live` snapshot into
`getTokenDetail` too, through the one `applyTokenLiveStats` writer the Tokens grid
shares. The snapshot is cumulative and backend-authoritative — last frame wins, and a
missed frame heals on the next one (unlike the append log, which needs the resync).

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
  `arms +N%`, a skipped trail is dashed, an inactive ladder stage is dimmed.
  **Hovering the chart moves the instant**: `FloorPositionDetail` publishes the
  crosshair time through `crosshairTime.tsx` — a subscribable store, not state, so a
  per-frame move re-renders the chips and not the chart that emitted it (the lanes
  below travel the same way in reverse, both over the one `publishedStore`; the
  detail relays them because it is deliberately free of the live endpoints) — and the
  strip swaps to `○ reconstructed at hh:mm:ss` until the pointer leaves, the
  entry/exit pins still visible as what it returns to. The rows come from
  `.../positions/{id}/metric-series`, fetched **lazily on the first crosshair move**
  (one fold per modal, never per hover) and indexed with `seriesIndexAsOf` — the last
  row AT OR BEFORE the hovered instant. The chart resolves a hovered candle to the
  end of its coverage (`buildBarWallEndSec`, which answers for **every** bar, empty
  slot bars included), so the pair reads "what had the rule seen by the end of this
  candle". Two rails hold that honest: a nearest-row lookup may answer with a row
  from after the instant, and an unresolvable bar must **not** fall back to the
  pinned readout — the strip says `no reading here` instead, because a pinned value
  repeated on every gap bar reads as a metric frozen at its exit value.
  The condition that closed the position is also drawn as a **value line in its own
  pane** with its thresholds dashed across it (`ChartValueLane`), so the number a
  decision was taken on is on the chart rather than only in a chip. A two-sided
  condition draws **both** edges — with one line there is no seeing where a band stops
  holding — and a DNF of several OR arms draws none, because its arms disagree about
  where the line sits. The line stops at the last recorded point instead of carrying
  it rightward: a flat tail past coverage is the same "metric frozen at its final
  value" the chips refuse to show. On the entry side the drawn condition skips
  `m_state.time`, which sorts first on most rules and is a ramp of the x axis. An open position's hover carries the replay caption too:
  the engine keeps one instant of state, not a history, so a past instant can only
  be reconstructed. A capped series says `· past coverage` rather than repeating its
  last row silently. The strip's **timeline** toggle draws the same payload as
  bottom-pane lanes on the chart (`timeBandsPlugin`), one per condition, filled
  where it held — off by default because turning it on is what pays for the fold,
  and it shares the crosshair's cache entry so whichever comes first covers both.
  Lanes snap through each bar's wall-clock end, so they draw in **slot** mode too.
  That array is bar **ends**, not bar keys, and both edges of a span round **outward**
  to the bar the instant falls inside — rounding the end edge inward drops the bar the
  span finishes in, and the renderer then paints centre-to-centre for another half bar
  at each end, so a span reads about two bars narrower than the truth and a brief
  satisfaction disappears. Spans are painted edge to edge off `barSpacing`.
  Every number in the detail reports an engine decision taken under the
  fingerprint's saved `ix_patterns`, and that saved row is the only set any
  surface classifies from — so the vol / non-vol split a reader sums by hand and the
  exit beside it can never be classified differently. Editing from the trades table's
  Vol badge stays available (it is how a misclassified bot tx gets found) and writes
  straight to the fingerprint.
  A disarmed row never fills: the fold is skipping that req, not failing it.
  Lanes thin down rather than vanish as a ladder grows, and the empty ones matter —
  against the coverage track they read as "never fired".
  **A Waiting row gets the identical surface** off `.../armed/metric-series` — hover
  and timeline both — because "how close is it" is a question about the approach that
  one live instant cannot answer, and Waiting is the row an operator stares at
  longest. It passes the arm instant so coverage centres on why it is *still* waiting,
  and its value line is drawn from the **entry** side: nothing has exited, so the
  condition holding the row out is an entry one. With no fill the engine gate is
  `entry_satisfied && !exit_metrics_satisfied`, so the strip marks a satisfied **exit**
  chip `blocks entry` and says so on the group — otherwise every entry chip can read
  green on a row that was never enterable. **An ENDED episode** — an Arms
  ledger row in `ArmDetailModal` — passes `endedAt` and inverts the two sources: the
  live readout is skipped (the pair is out of the registry, so it is a permanent 404
  that would still cost the decision loop a round trip a second) and the pin becomes
  the fold's row at the disarm instant, captioned with that clock rather than `at
  exit`, which an episode never had. `ConditionSeriesStrip` +
  `useConditionSeriesGate` are that whole crosshair/timeline half, shared by both
  hosts — split at the gate because *whether to fetch* is decided in the strip (first
  hover, or lanes on) but must be known by the host that owns the query. Backend
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
  subtitle)/**Replay** (decision timeline left, `LabTokenInspect` right — the page's
  subject is *why* a decision came out the way it did, and fill markers alone only say
  that it did, so the chart is pinned to the rule the replay decided under. One walk of
  the focused mint's effects yields the markers, the entry fill the `m_position` panes
  anchor on, the exit reason the timeline's value line follows, and the rule id itself;
  the params come off the CURRENT rule row because that is literally what the replay
  re-decided under — `rule_to_loaded`, no snapshot), and the generic Grouped Sweep (sticky Run › Group › Combo
  breadcrumb; Simple = configure→promote; Full drill = combo/token inspect via
  `SweepTokenInspectModal` with metric panes; `sweep/` + `strategy/` components,
  `useStreamedSweepResults`, `BackgroundJobsContext`).
  Shared page chrome: `PageHeader` / `EmptyState` / theme-token `InlineAlert`.
  **Metric panes** (lab Tokens detail; old `/strategies/metric-panes` redirects): `LabTokenInspect`
  stacks `TokenTradeChart` above registry-driven `MetricPanes`. Shared wall-clock
  crosshair / visible range (`TokenPriceChart.onCrosshairTimeChange` /
  `onVisibleTimeRangeChange`); selecting a rule auto-loads its metrics/windows,
  overlays thresholds, and paints first metric entry/exit fires as `eventMarkers`
  with `role: 'signal'` and spaced `name[@Ns] op value` labels (e.g.
  `tagged_buy@2s >= 0.85`) — visually distinct from backend fill markers
  (`role: 'fill'`, green/red arrows + price lines). Entry fire skips rows where exit
  metrics already hold — same `can_enter` gate as the engine.
  **The entry marker names the condition(s) that FLIPPED at that instant**, joined by
  `+` and summarised past two (`… +2`). Entry is a conjunction, so a condition that
  was already holding decided nothing about the timing; labelling with the first
  authored one lets a monotone lifetime metric explain a fire two trailing windows
  produced, minutes after its own line crossed. Nothing flipped (the exit veto
  cleared) ⇒ the whole conjunction. The rest of the conjunction is always on the
  chart as lanes.
  **Every condition label carries its window**, from the one `conditionMetricName`
  namer shared by markers, lanes and the value line — the lifetime and windowed
  registry entries share every metric name. The label is the WHOLE span
  (`30s`, `30sl`, `30sl@1` — `formatWindowSpec`, the vocabulary of the Rust
  `event::format_metric_exit_name`), because two reqs differing only in unit or lag
  read different tape. For the same reason pane verdicts and threshold lines key on
  the **series column** (`metric` / `metric@Ns`, `metricThresholdsFor`), never on the
  metric name: a rule may constrain both twins, and the monotone one would otherwise
  paint the windowed pane. `/metric-series` computes wall-clock windows only, so a
  slot-window condition keys its own pane and finds no column — an unavailable pane,
  never the 30-second series relabelled 30 slots.
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
  The value strip's **timeline** toggle is the lab twin of the live position modal's:
  `metricConditionBands` folds the same series into one `ChartTimeBand` lane per
  authored condition (`IN`/`OUT`-tagged, filled where it held) plus the run's exit
  condition as a `ChartValueLane` with its threshold dashed across, and
  `LabTokenInspect` relays them into `TokenTradeChart`. It costs no request — the
  panes already fetched the series — but it is the **panes'** answer, so it models
  no arming gate and no ladder stage, exactly like the `signal` markers beside it:
  a lane says the condition's own reading crossed, not that the engine would have
  sold there. Lane colors, the side tag and the exit-reason→condition match
  (`parseMetricExitTarget`) are shared with the live strip, so the two timelines can
  never label the same condition differently.
  The shared `TokenTradeChart`/`TokenPriceChart` take an optional `highlightWallet` — its
  markers render at ~2.4x the base radius (with a `HIGHLIGHT_MIN_RADIUS` floor so a
  wide zoom can't shrink it into the crowd) plus a gold glow+ring
  (`ProfileWalletInfo.isHighlighted` → `walletMarkersPlugin`), take the stack row
  nearest the bar edge (`focusFirst`), and a non-tracked input address gets a synthetic
  marker entry. Marker stacks pack EDGE to EDGE from each marker's own outer extent, so
  an oversized tier pushes its neighbours out instead of overlapping them.
  `TokenTradeChart` forwards the same address to `BarTradesPanel`, which paints those
  rows gold (background + 4px left accent, outranking the entry/exit tint and the
  `myWalletAddresses` amber accent) and heads the panel with an `N of M by <addr>` chip.
  `compareWallets` (Trader Analysis "Compare with", plumbed `TokenTable` →
  `TokenChartsGrid` → `TokenTradeChart`) adds the **middle tier** between that focus and
  the crowd: `isCompared` → a square silhouette at `COMPARE_MULT` with a
  `COMPARE_MIN_RADIUS` floor and an outer ring in the marker's OWN color, and the
  comparison tier outranks the lifecycle tier on size (whose marker it is beats which leg
  it was). Class still wins the silhouette, so a compared `mine`/dev wallet keeps its
  diamond/triangle and only the ring and size carry the tier. Size and shape are
  deliberately redundant: size is the cue that survives a color-blind read, shape the one
  that survives zooming out, and the tier has to hold at both.
  While the list is non-empty every OTHER tracked wallet is flagged `dimmed` — drawn at
  `DIM_ALPHA` with no glyph. Dimming the crowd, not only enlarging the tiers above it, is
  what makes the comparison readable at a glance, and it spends no encoding on a marker
  that already uses shape, fill, border and two rings; with no comparison armed the crowd
  IS the content and stays at full strength.
  A comparison wallet's color comes from `compareWalletColor(slot, wallet)` keyed by its
  **comparison slot**, not by the tracked-wallet rotation — the rotation can hand two
  compared wallets adjacent hues, which is exactly the distinction the page exists to
  make. A `mine` wallet keeps its fixed color, the one identity the chart never recolors.
  That helper is the SSOT for the markers, for `CoTradeSummary`'s swatch (drawn square to
  match) AND for the `co_trade` column chips: the strip is the legend for those markers,
  so a second copy of the rule is a legend that can lie, and a chip reading the profile
  rotation instead gives one wallet two colors on one page. `COMPARE_MARKER_COLORS` holds
  one hue per slot the co-trade read accepts, so the helper's modulo never collides
  (`coTrade.test.ts` asserts the length against the cap).
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
  The payload is static per backend **process**, not per tab, so the hook passes
  `refetchOnMountOrArgChange: REGISTRY_STALE_SECS` (60 s) over the app-wide `false`:
  a restart that adds a metric group otherwise leaves the pickers rendering the
  previous vocabulary for an hour with no error, while authored rules already show
  the new group because `RuleParamsSummary` falls back to raw params.
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
  `disabled` bag of their own — park the whole stage instead.
  A dynamic row's window is a **span, not a number**: size + `unit` (sec / slot) +
  `lag`, authored as three controls and read through `lib/strategy/windowSpec.ts`,
  the one mirror of `hunter_engine::metrics::WindowSpec`. Both axes of a two-window
  group share the row's unit and lag, so the slice control re-spells itself
  (`slice_size_sec` ⇄ `slice_size_slots`) on a unit flip rather than leaving the
  other param behind for the backend to reject. **The instance key is the whole
  span on both axes** (`ruleRowInstanceKey`): keyed on size alone, two slot windows
  of one metric merge into one `GroupConditions` and the later row's strict bag
  silently wins — one of the two gates disappears at save. Exactly one size param is
  written, and a zero lag is omitted, so a pre-slot rule round-trips
  byte-identically),
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
  `metric_config` section + `VolumeIxPatternsEditor` for `m_flow_ix.ix_patterns`
  — add-row / remove-row / **Delete all** footer, the last confirming via the shared
  `clearPrompt` also used by the flow-discovery cart). The form auto-fills
  `Fingerprint::auto_name` from the axes (`3ix:Buy · max=1 · bkt=1`) and keeps a
  typed nickname; pickers search axis text and show the chip row in the dropdown.
  `RulesView`/`FingerprintsView` (shared list+editor, mounted by both apps'
  `RulesPage`/`FingerprintsPage`; the lab page passes `onViewMatches`, which puts a
  chart row-action on every fingerprint opening its **matched tokens** dashboard —
  day x hour creation heatmap + calendar trend + the paged token table, all from the
  fingerprint-scoped grouped endpoints (`useFingerprintMatchesFor`, the row-driven
  twin of `useFingerprintMatches`). One `CreationWindowPicker` drives charts and table, and
  a heatmap tile click narrows the table to that recurring weekly slot. The prop is
  optional because those endpoints are lab-only and `shared` cannot import `@lab`; cross-page selection via `?rule=` / `?fp=`
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
  `FingerprintParamsSummary` (`fingerprintParamsCell` — set match-axis chips, plus the
  always-shown bucket and `FlowPatternsChip`; used by Rules, Simulate, and
  `FingerprintPicker`). `IxLabelsChip` and `FlowPatternsChip` share `ContentsChip`: a
  count body over a `hashHue` ribbon of the **contents**, since neither axis is its
  count — the tooltip carries the sequences and a click copies them as JSON. An
  unconfigured pattern set stays visible as a dimmed `flow✗`, because an empty set is
  the verdict "every trade classifies organic", not a dropped criterion.
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
  the group answer cannot stand in for the per-token one.   Job kind `discovery` in `BackgroundJobsContext`
  (SSE `flow_discovery_*`, mutual exclusion with sweeps).
- Lab **Rule search** (`/strategies/rule-search`, `RuleSearchPage`) — one required
  fingerprint + datetime range + buy/fill/cost/copycat (default ON) + optional
  incumbent (compare only). Job kind `rule_search` in `BackgroundJobsContext`
  (SSE `rule_search_*`, single-flight vs sweep / flow-discovery / metric-discovery).
  The page collects the board on `rule_search_finished` (then GET) — no poll
  deadline. Board: refuse / ungated / candidate, champion vs empty-entry vs incumbent
  (authority SOL ranks, then tighter fill spread; first-in-window quoted beside
  it), `ruleParamsCell` for the champion, top archive, diagnostics (cut phases),
  Promote (`src:rule-search`) and a draft Simulate of the unsaved champion.
- Lab **Family search** (`/strategies/family-search`, `FamilySearchPage` +
  `@lab/components/family/FamilySearchBoard`) — rule search's sibling over a whole
  **fingerprint family**. Job kind `family_search` in `BackgroundJobsContext` (SSE
  `family_search_*`, single-flight against every other heavy job), same
  finished-then-GET collection with no poll deadline; each cohort's matched count
  arrives as a `family_search_notice` toast, because that count is the run's cheapest
  scope guard.
  The form is **two required fields** (fingerprint + created range) plus buy size;
  fill / cost / copycat / slots / token cap / varied axis / freshness slack /
  concurrency caps / incumbent sit in one persisted `Accordion` whose collapsed badge
  states what they currently are. Every one of them is sent from the form: a saved
  rule supplies none of them, so there is no control whose value can silently come
  from somewhere else.
  The board is ordered as the argument it makes — **verdict → portrait → execution →
  grade → evidence**. The verdict (`lab/lib/familySearchVerdict.ts`, unit-tested)
  blends nothing: it names which of six gates decided (clears execution · family ·
  rank transfer ρ · beats the ungated control · clauses hold up · freshness) and prints
  all six beside the headline with their numbers, so a reader who disagrees can see the
  deciding line. The headline ladder is ordered by what invalidates what — a refused
  cohort (the search never ran, which is not the same statement as "found nothing"),
  then a missing draft, then **fill luck** (whether the number is real at all), then a
  gate that costs money, then single-cohort, then a collapsed ρ, then thin headroom,
  and last **Fragile draft** — the D13 per-clause findings, which come last because
  they judge a draft that already cleared everything structural. `familyRobustness`
  only *counts* backend verdicts; it never re-derives one, and a finding downgrades a
  draft rather than removing it, because the backend keeps diagnostics out of
  selection. The four D13 sections each pair a table with the sentence that makes it
  actionable; `LadderRow` draws a threshold ladder as a bar strip scaled from the
  ladder's own minimum, since scaling from zero flattens exactly the differences the
  chart exists to resolve.
  Then the portrait prose (the product); the **Execution** section — cost clearance
  (the typical best available exit against one round trip, in `x`) ∥ fill spread (the
  same closes repriced at the friendliest honest fill); the grade as three cards —
  draft (held-out level) ∥ ungated control ∥ oracle capture, with an incumbent demoted
  to a dashed `display only` strip — the draft's clauses, the family table, per-alarm
  attribution with an **Asked → got** column (the authored threshold against the mean
  realized *gross* return, rendered only where the units match), the narrow re-check,
  the entry-timing table, the entry-gate ρ table, and the archive.
  **`fit_ret_pct` is dimmed and labelled `rank only` wherever it appears**: it
  produced the ordering and is negative for every candidate on the reference family
  while the winner pays +31% on the held-out cohort, so printing it as a level is the
  one mistake the fit/validate split exists to prevent. Promote (`src:family-search`)
  binds to the **run's** target fingerprint rather than the form's current pick, and
  Simulate replays the unsaved draft through `DryRunDetail`. A freshness refusal is
  caught by message and rendered with the sync command that fixes it — it is the
  likeliest first-run failure and the backend gate is fatal, not advisory.
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
  When axes reference `m_flow_*`, the form requires `ix_patterns` (corpus-wide
  for the run) and sends them on start.
- `sweep/genericSweepColumns.tsx` — combo/group columns; the swept `params` is a
  `RuleParams` blob rendered via shared `ruleParamsCell` (not one flat column per knob).
- `[Promote…]` on any group/combo → `POST …/promote` (fingerprint find-or-created;
  copies run `ix_patterns` into `metric_config`) →
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
    Vol/non-vol overlay SSOT: `hooks/useFlowPatternKeys` (`useFlowPatternSource` /
    `useFlowPatternSourceForRule` / `useResolvedFlowPatternSource`) and
    `lib/flow/flowPatternKeys` resolve a fingerprint's `ix_patterns` into a
    `FlowPatternSource` — `{ fingerprintId, keys }`. **Both halves travel together**: the keys
    say what to classify with, the id says which row an edit writes to, and a key set alone
    cannot be traced back to one (`metric_config` is not part of fingerprint identity, and
    every unconfigured row carries the same empty set). There is deliberately no keys-only
    variant. Wired into Evidence `TokenTable` charts, `LivePositionInspectModal`, Console
    History/open/waiting detail, fingerprint matched-tokens, the Creation Stats grouped
    drill-in (from the **applied** scope fingerprint, so a manual group-by drill-in has none),
    and sweep combo charts (run patterns). A charts grid takes `flowPatternKeys` +
    `flowFingerprintId` when ONE fingerprint covers every card (Portfolio's rule-scoped
    drill-in) and `useRowChartFlowPatternSource` — a per-card hook, `FlowPatternSourceHook`,
    resolved next to `useRowOverlay` — when its rows span fingerprints (Console History spans
    rules, so one set would misclassify every card from another rule). The per-card source wins
    per half independently. A grid and its rows' inspect modals read the same hook, so a card
    and its modal cannot disagree. Omit/empty is not a blank chart — the overlay falls back to
    a creator-vs-rest split and only goes dark on a token with no creator wallet either.
    Clicking a Vol badge **persists**: `useVolumePatternTarget` writes that row's ordered
    `instruction_labels` (`lib/flow/volumePatterns.togglePattern`, shared with Flow Discovery)
    straight into the target fingerprint's `metric_config`, and the engine picks it up on its
    next rules reload. There is no staging copy — it would be a second answer to "what counts
    as volume". `useFlowReasons` supplies the contagion-aware `via creator` / `via wallet`
    marker so the structural badge and the lines stop disagreeing silently. `flowReadOnly` opts
    a subtree out (the grouped-sweep drill-in reads a run snapshot) and skips the hook's
    fetches. Target precedence is `resolveVolumePatternTarget` — explicit pick > the host's
    `flowFingerprintId` > a lone pattern-set match, in that order, since a match on the SET
    can never outrank an id (an empty set matches every unconfigured row at once). A match is
    taken only when exactly one row carries the set and is flagged as inferred; picking away
    from the host is flagged as off-host, because the badges then answer for a different row
    than the chart's lines. `VolumePatternBar` renders the target, both flags and the
    active-rule count, because a write changes every rule bound to that id. Detail:
    [`@plans/token-analysis/token-history-chart-functionalities.md`](../plans/token-analysis/token-history-chart-functionalities.md) §6b.
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
  **An inspect modal draws every episode on the mint, not just the row that was clicked** —
  `hooks/useMintEpisodeMarkers` (shared; the traded twin of the lab's
  `useSimMintEpisodeOverlay`) reads `positions/mint/{mint}/episodes` off either bin and folds
  them through `buildEventMarkersForEpisodes`, so a re-entered token shows `Entry 1..N` with
  every leg of every exit. It is **mode-scoped** — paper fills are modeled and real ones are
  money, so they never share a chart — but NOT rule-scoped: the chart's subject is the token's
  whole traded history. The focused episode is substituted into the union (so it keeps the
  ledger's fresher legs) and tagged `◂`. The per-row chart-card decks stay single-episode by
  design: there a card *is* one position row, and folding them is `chartsGroupByMint`'s job.
  The chart toolbar's Events toggle covers the whole overlay, arrows *and* dashed fill-price
  lines; on a ladder each line is titled by its share (`Exit 70%`) so N legs don't read as one
  exit drawn N times. Position tables (Evidence / Simulate / Dry-run / Sweep) share
  **`PositionChartCardExtra`** (hold · exit · PnL% · size · entry/exit price; multi-episode fold
  when `chartsGroupByMint`). Trader Analysis uses `TraderChartCardExtra` (wallet buys/sells/hold/
  vol), repeating the headline wallet stats its table columns also carry so a card read on its own
  still says who did what. `DataTable` stays token-agnostic: the dependency is one-way (`tokens/` → `table/`),
  asserted by `components/table/DataTable.boundary.test.ts`. **Every** token-row table now renders
  through `TokenTable`. Trader Analysis row / chart-card select opens `LazyLabTokenInspectModal`
  via `inspectFromMint`.
- **Trader Analysis look-back window (`lab/pages/analysis/TraderAnalysisPage.tsx`).** One
  `DateTimeRangePicker` covers both shapes: the rolling day presets (1 / 3 / 7 / 14 / 30 / 60 / 90)
  and `Custom`, an exact `from → to` on the two-month calendar with time fields. The preset value
  lives in the persisted draft's `days`; `Custom` parks the sentinel there and the bounds in
  `from`/`to` as wall-clock in the **project zone**, converted at the query boundary by
  `datetimeLocalToUtcWallClock` (+`Z`) — a `lower`/`upper` bound each, so a DST-ambiguous hour stays
  inside the range. A day preset still hands the picker its resolved lower bound, so the trigger
  reads `7 days · 08/18 → now` and opening `Custom` starts from that window rather than a blank
  calendar. On the wire only the active shape goes out (`days=N`, or `from=&to=`), and the summary
  sentence under the inputs names the window the rows were read over, not just a day count. The 90d
  clamp is the backend's (`resolve_window`), which also swaps a reversed pair and, on an over-long
  span, keeps the upper bound — the page reads end-first.
- **Trader Analysis wallet columns (`lab/components/analysis/walletTokenColumns.tsx`).** The page
  splices `walletTokenColumns()` into `tokenColumns()` directly after the **identity** block, so the
  wallet's position reads before the token's own activity/price/market fields. The splice happens at
  the page, never inside `tokenColumns()`, which stays the SSOT every other token table shares. Two groups
  (`groupLabels`: Position · Bonding curve): entry / exit instants and their **token ages**
  (creation -> first buy / last sell), hold span, buy+sell leg counts, SOL in / out, avg buy/sell
  price, total PnL and PnL%, the protocol fee the reconstruction charges, an open/partial/closed
  state cell, and **entry / exit curve progress** with the gain across the hold. Curve progress is
  the pool's real SOL just **before** that leg (the wallet's own impact backed out) over
  `PUMP_GRADUATION_REAL_SOL`; >100% reads as a migrated pool. Second-order columns (`w_entry`,
  `w_exit`, `w_avg_buy`, `w_avg_sell`, `w_fee`) default hidden. Every field is the per-mint window
  grain, not one round trip — a wallet that re-entered shows its first buy, its last sell, and a
  span covering both.
- **Trader Analysis co-trade (`lab/components/analysis/coTrade.ts` + `coTradeColumns.tsx` +
  `CoTradeSummary.tsx`).** "Compare with" names up to `MAX_COMPARE_WALLETS` tracked wallets (`?with=` on
  `/api/wallets/:wallet/tokens`); the page stays **primary-shaped** and the comparison set is purely
  additive. That constant mirrors the handler's `MAX_COMPARISON_WALLETS`, which drops the excess
  silently, so the picker refuses past it, `run` slices the wire list, an over-cap draft chip strikes
  through, and `coTrade.test.ts` reads the Rust source and fails when the two numbers drift. Only the primary's mints make rows — a comparison wallet's own tokens never add any — and
  each row carries `co_traders[]`: that wallet's entry, its curve depth, and `entry_lag_slots` /
  `entry_lag_tx` signed against the primary (**negative = it entered first**). Ordering is the entry
  leg's `(slot, tx_index)`, never `block_time`, which is second-precision and ties across a whole slot.
  Columns (group `co_trade`): Also (count) · Co-traders (colored chips, entry order, each with its lag)
  · First In · Lag · Coupling. `coTrade.ts` holds every derivation pure and DB-free — `tightestCoTrader`
  (smallest |lag|, ties toward the wallet that was ahead), `firstMover`, `coTradeMix`,
  `coTradePerWallet` — unit-tested in `coTrade.test.ts`. `<CoTradeSummary>` reports the bucket mix and,
  ahead of it, **one chip per comparison wallet** carrying that wallet's own overlap count and coupled
  share: the totals count each row once on its tightest coupling, so they are the SET ceiling and one
  busy wallet can carry them alone. Read the mix, not the overlap count — two busy wallets share some
  memecoins by chance alone and that coincidence lands in `independent`, while a shared tape trigger
  concentrates in `co-slot`/`leads`/`follows`.
  A chip is also a **focus toggle**: with a wallet focused, `pickCoTrader` re-points Lag / Coupling
  (label, sort and filter keys included), the strip's totals and the "co-traded only" filter at that
  wallet alone, blank on the rows it is absent from; unfocused they answer on the row's tightest
  coupling. Without it the second and third wallet on a shared row are unsortable and unfilterable —
  visible only as chips, which reads as the page working with the first comparison wallet only.
  "Co-traded only" stays a client-side filter — no refetch — and carries a **depth**: `passesCoFilter`
  keeps the rows holding at least N of the comparison wallets, N = 1 being the union (any one of them,
  which two busy wallets satisfy by coincidence) and N = the set size the INTERSECTION, i.e. only the
  tokens the primary and EVERY named wallet were on. A focus composes with it rather than replacing it.
  `<CoTradeSummary>`'s depth ladder (`coDepthCounts`, cumulative) shows what each rung costs before it is
  picked and doubles as the control: a set whose ladder reads 987 / 180 / 12 / 0 has no four-wallet
  intersection at all, which an empty table alone would not distinguish from a broken filter.
  The **coupling badges select too** — an OR set over `CO_BUCKET_KEYS` (the four buckets plus `unordered`,
  which is an answer about the window, not a gap), matched by `matchesCoBuckets` on the same
  `coBucketKey` the counts come from, so a badge and its own filter cannot disagree. `CO_BUCKET_VARIANT`
  is the one bucket→color map the Coupling column and the badges share.
  Every strip control previews over the cohort narrowed by the OTHER controls and never by itself: a
  count that collapsed onto its own selection could not offer the switch back, and one blind to the rest
  would promise rows the click cannot deliver. Column filters stay out of the previews — the controls
  preview over the query, the totals beside them describe what is on screen. Chip and First In colors come
  from `compareWalletColor(slot)`, the same call the chart markers and the strip's swatches make, so a
  wallet reads identically in the table and on every chart. The primary stays the subject of the PnL deck, the flow lens' `excludeSelf`, and every
  `wallet_*` column.
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
- **Trader Analysis flow lens (`lab/components/analysis/FlowLensBar.tsx` +
  `useTraderFlowLens.ts`).** The page's tokens belong to no cohort, so there is no fingerprint to read
  `ix_patterns` off and the charts' vol/non-vol overlay has nothing to classify with. The lens is
  the second owner of that same fact: a named `ix_pattern_sets` row (lab-only table, CRUD at
  `/api/ix-pattern-sets`) holding `[{ group, ix_labels }]`, picked once above the grid and applied to
  every card. Keys ride the existing `flowPatternKeys` prop path; the classifier options and the
  Vol-badge write target ride `context/FlowLensContext`, which the page provides and
  `TokenPriceChart` / `TokenTradeChart` / `BarTradesPanel` consume — absent everywhere else, where the
  chart stack classifies exactly as the engine does. A lens defaults to **contagion off** (each trade
  judged by its own `ix_labels`, no forward wallet tagging) and **excludes the studied wallet**, because
  it answers "which structures surround this moment", not "who is in the volume crew". A Vol badge under
  a lens writes to the pattern set, never a fingerprint; the one path into the engine is the explicit
  **Copy to fingerprint**. Paste accepts a `{ "patterns": [...] }` study file, a `[{ tool, ix_labels }]`
  list, bare label arrays, or one JSON array per line, and reports accepted / duplicate / skipped counts
  (`lib/flow/ixPatternSets.ts`). Group chips narrow which patterns classify (view state, per set).
  Detail: [@plans/strategies/trader-flow-lens.md](@plans/strategies/trader-flow-lens.md).
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
  `hooks/useUiPrefs` (`useAccordionOpen` / `useUiToggle`) on top. A run form keeps ONE draft blob
  per page (`mt:form.*`, `mt:sweep.config`, `mt:simulate.runPrefs`) holding every input including
  its date range, so a refresh restores the exact scope the numbers were read under. Related prefs group into blobs
  (`mt:ui.accordion`, `mt:ui.toggles`, `mt:table.*`, `mt:page.creationStats`) rather than one flat
  key each; a raw `localStorage.*` in a component fails `lib/storageGate.test.ts`. Key table,
  persist-vs-not policy, and how to retire a key:
  [../plans/frontend/frontend-patterns.md](../plans/frontend/frontend-patterns.md) § localStorage.

## Known follow-ups (NOT yet done)

- **Cosmetic deviation:** shared store core lives in `src/shared/store` but the legacy `store/*`
  alias still resolves there; the per-mode `services/` file split was skipped (tree-shaking over
  one shared `services/api.ts` achieves the same bundle isolation since the helpers are
  side-effect-free).
