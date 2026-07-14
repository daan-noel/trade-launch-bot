# forge frontend — architecture map

The operator dashboard SPA at `forge/frontend/`. A **Vite + React 18 + TypeScript**
app: **React Router 7** routing, **Redux Toolkit + RTK Query** for all server state,
**Tailwind v4** (`@tailwindcss/vite`, theme tokens in `src/index.css`), and
**lightweight-charts v5** for the token price chart. It talks only to `forge-live`'s
HTTP API (same-origin in prod; `/api` proxied in dev). The old single
`App.tsx`/`api.ts` is gone.

Path aliases (`tsconfig` + `vite-tsconfig-paths`): `@app/*`→`src/app`,
`@shared/*`→`src/shared`, `@features/*`→`src/features`.

## Architecture

- **Entry** (`src/main.tsx`): mounts `<Provider store>` + `<RouterProvider router>`
  in `StrictMode`.
- **Store** (`src/app/store.ts`): a single RTK-Query cache (`baseApi`) is the only
  reducer + middleware; `setupListeners` wires focus/reconnect so `refetchOnFocus`
  / `skipPollingIfUnfocused` work. Endpoints are attached by side-effect import of
  `endpoints.ts`.
- **Routing** (`src/app/router.tsx`): one `createBrowserRouter` tree; `AppShell` is
  the layout route, feature pages are its children via `<Outlet>`.
- **Shell** (`src/app/AppShell.tsx`): fixed left sidebar nav (`NavLink`), header with
  live SOL/USD badge (`quoteAssets` 60s poll) + the ingest pause/resume toggle.
- **Data layer**: every server read/write is an RTK-Query endpoint in
  `src/shared/store/endpoints.ts` (injected into `baseApi`). Reads are feed-derived
  (cheap); a handful of explicit mutations do the only on-chain RPC work
  (`refreshPositions`, `refreshWalletBalances`). Tag-based cache invalidation keeps
  surfaces coherent.
- **Push** (`src/shared/services/sse.ts`): ONE shared `EventSource` on `/api/stream`
  multiplexes all push frames; pages subscribe per event type and filter by mint
  client-side. SSE supplements (not replaces) polling — it patches/invalidates the
  RTK cache so a backgrounded gap self-heals on the fallback poll.
- **Auth**: the browser bundle never holds the bearer token. The Vite dev proxy
  (`vite.config.ts`) injects `Authorization: Bearer $API_AUTH_TOKEN` server-side on
  `/api`; in prod nginx does the same. Mutating routes are fail-closed on the backend.

## Layout map

| Path | Responsibility |
| --- | --- |
| `src/main.tsx` | React root; Provider + RouterProvider |
| `src/index.css` | Tailwind import + `@theme` design tokens (dark operator palette) |
| `src/app/store.ts` | `configureStore` (baseApi reducer/middleware) + `setupListeners` |
| `src/app/router.tsx` | `createBrowserRouter` route tree |
| `src/app/AppShell.tsx` | Sidebar nav + header (SOL price, ingest toggle) + `<Outlet>` |
| `src/shared/store/baseApi.ts` | RTK-Query cache shell, tag types, `apiErrorMessage` helper |
| `src/shared/store/endpoints.ts` | ALL API endpoints (injected) + generated hooks |
| `src/shared/services/sse.ts` | Shared `EventSource` multiplexer + typed `connect*Stream` helpers |
| `src/shared/types.ts` | Backend DTO/enum TypeScript mirrors (~44 exports) |
| `src/shared/hooks/useNow.ts` | App-wide throttled "now" clock (ported from hunter) for age cells |
| `src/shared/lib/format.ts` | SOL/lamports, USD, sig/mint, IPFS, explorer-link, base64 formatters |
| `src/shared/lib/cn.ts` | `cn()` clsx alias (for ported components) |
| `src/shared/lib/storage.ts` | Typed localStorage (chart prefs) |
| `src/shared/components/ui/` | Design-system ui-kit (see below) |
| `src/shared/components/IngestToggle.tsx` | Ingest pause/resume badge; SSE-patches `ingestStatus` cache |
| `src/shared/components/IxLayoutEditor.tsx` | Authors `DecoStep[]` ix layout; mirrors backend `IxLayout::validate` |
| `src/shared/components/PriceChart.tsx` | Forge adapter mapping `TradePriced`→chart `ChartTrade`; SOL/USD + price/MC toggles |
| `src/shared/components/tokenChart/` | Ported hunter token price chart (swing/chain removed) |

## Pages

| Page | Route | Purpose |
| --- | --- | --- |
| `features/dashboard/DashboardPage.tsx` | `/` | Overview: pool status tiles, low-pool banner, ingest state, recent launches |
| `features/launch/LaunchConsolePage.tsx` | `/launch` | Fund-for-launch + execute launch; template/dev-wallet pick, requirement preview, live launch-status via SSE |
| `features/launches/LaunchesPage.tsx` | `/launches` | Paged enriched launched-token list (`LaunchListRow`, PAGE=100) |
| `features/launches/TokenDetailPage.tsx` | `/tokens/:mint` | Token detail: overview, price chart, live trades (SSE, throttled refetch), positions, manage/ladder/volume panels |
| `features/wallets/WalletPoolPage.tsx` | `/wallets` | Wallet pool: list/generate, fund, transfer, sweep/consolidate, key export; SSE-driven refresh |
| `features/templates/LaunchTemplatesPage.tsx` | `/templates` | CRUD launch templates (buy-variant legs, ix layout, metadata link) |
| `features/metadata/MetadataTemplatesPage.tsx` | `/metadata` | CRUD metadata templates (name/symbol/socials + image pin) — token-identity SSOT authoring |

Token-detail sub-panels (children of `TokenDetailPage`, not routes):

| Component | Purpose |
| --- | --- |
| `features/launches/ManagePanel.tsx` | Post-launch sell/buy/consolidate: preview → execute across a wallet group + action history |
| `features/launches/LadderPanel.tsx` | Arm take-profit sell ladders (sell X% at MC/price threshold) |
| `features/launches/VolumePanel.tsx` | Author + control volume-making bots (jittered buys on interval, SOL-budget capped) |
| `features/templates/legForm.ts` | Human↔base-unit leg-row conversion; `VARIANTS` / `BUY_VARIANTS` (4 pump.fun buy encodings) |

## RTK-Query API layer

`baseApi` (`baseApi.ts`) is a single `createApi` with `baseUrl: ''`,
`keepUnusedDataFor: 120`, and all tag types declared up front (Bootstrap, Templates,
MetadataTemplates, Wallets, Launches, Ingest, Dimensions, Positions, ManageActions,
Ladders, Volume). `endpoints.ts` injects every endpoint and re-exports the generated
hooks. Endpoint groups:

| Group | Endpoints (→ route) |
| --- | --- |
| Composite / dimensions | `bootstrap` (/api/bootstrap), `launchpads`, `quoteAssets` |
| Ingest | `ingestStatus` (GET), `setIngest` (PUT /api/ingest) |
| Launch templates | `templates`, `createTemplate`, `updateTemplate`, `deleteTemplate` (/api/launch_templates) |
| Metadata templates | `metadataTemplates`, `create/update/deleteMetadataTemplate` (/api/metadata_templates) |
| Wallet pool | `walletPool`, `generateWallets`, `refreshWalletBalances`, `fundPool`, `fundForLaunch`, `transferSol`, `sweepWallets`, `consolidateWallets`, `exportWalletKey` (X-Export-Secret header, no cache) |
| Launches | `launches` (paged), `executeLaunch`, `launch`, `launchStatus`, `launchRequirement`, `executeBundle` (/api/bundles/{id}/execute) |
| Token detail | `tokenOverview`, `tokenTrades`, `tokenPositions`, `refreshPositions` (only RPC holdings reconcile) |
| Management | `managePreview` (POST, no cache), `manageExecute`, `manageActions` |
| Sell ladders | `ladders`, `armLadder`, `cancelLadder` |
| Volume bots | `volumeBots`, `startVolumeBot`, `pause/resume/stopVolumeBot` |

## SSE channel

`GET /api/stream`, one shared unfiltered `EventSource`, opened lazily on first
subscriber and closed when the last leaves (stays under the HTTP/1.1 ~6-conn cap).
Typed helpers, each returning a `{ close }` handle:

| Helper | Event | Consumer / effect |
| --- | --- | --- |
| `connectTradeStream` | `trade_executed` | TokenDetail — throttled `/trades` refetch (1.5s window) |
| `connectTokenCreatedStream` | `token_created` | list pages refetch current page |
| `connectIngestStatusStream` | `ingest_status` | IngestToggle patches `ingestStatus` cache in place |
| `connectLaunchStatusStream` | `launch_status` | Launch Console refetches status on id match |
| `connectWalletPoolStream` | `wallet_pool` | Wallet Pool refetches `/api/wallet_pool` |

## Shared ui-kit (`src/shared/components/ui`)

`index.ts` re-exports: `Button`, `IconButton`, `Icon`/`IconName`, `FilterToggle`,
`Badge`/`StatusPill`/`statusTone`/`TradeTypePill`/`RolePill`/`roleColorVar`/`toneColorVar`/`Tone`,
`Field`/`InfoTip`/`Input`/`Textarea`/`Select`/`Checkbox` (`form.tsx`), `Card`/`Banner`,
`DataTable`/`Column`, `StatCard`, `AgeCell`, `AddressDisplay`, `KV`/`KVRow`. `DataTable`
memoizes rows — pages hold column arrays at module scope so identity is stable across
polls.

## Token chart (`src/shared/components/tokenChart`)

Ported from hunter's `token-price-chart/` with swing/chain-specific pieces removed;
built on `lightweight-charts`. `PriceChart.tsx` is the forge adapter (maps
`trades_priced` raw quote/base ratios to hunter's human-SOL/raw-token units so the
pump.fun constants apply unchanged, sources role-colored wallet markers from the
managed pool, owns SOL/USD + price/MC toggles). Internals: `TokenPriceChart.tsx`
(chart host), `chartBars.ts` (trade→bar aggregation + pump constants),
`ChartToolbar`/`ChartRangeSlider` (controls), `walletMarkersPlugin`/`rangeSelectPlugin`
(canvas primitives) with their tooltip components, `chartViewport`/`chartTimezone`/
`labelMetrics`/`constants`/`types`.

## Key rules

- **One RTK-Query cache is the single server-state store.** No hand-rolled fetch/
  context state; every route hits an `endpoints.ts` endpoint, and cross-surface
  coherence is via `providesTags`/`invalidatesTags`, not manual refetch wiring.
- **Endpoints attach by side-effect import** (`store.ts` imports `endpoints.ts`);
  `baseApi` declares tag types up front because `injectEndpoints` cannot add them.
- **SSE supplements polling, never replaces it.** Push frames patch/invalidate the
  cache; the (unfocus-skipping) poll remains the gap-heal fallback. One shared
  connection, client-side mint filtering.
- **Reads are feed-derived (zero RPC); on-chain reconcile is an explicit mutation**
  (`refreshPositions`, `refreshWalletBalances`) — plain queries never hit chain.
- **The bearer token never reaches the browser.** Dev proxy (`vite.config.ts`,
  Node-side) and prod nginx inject it on `/api`; secrets must never carry a `VITE_`
  prefix. Dev proxy target defaults to `http://127.0.0.1:8230` (`VITE_LIVE_PROXY`).
- **Token identity is authored only via metadata templates** (Metadata page); launch
  templates reference a `metadata_template_id`, never inline name/symbol/uri.
- **Column arrays live at module scope** to preserve identity across polls and keep
  `DataTable` row memoization effective.
- **The client `IxLayoutEditor`/`validateLayout` and `legForm` variants mirror the
  backend** (`executor_core::IxLayout`, the four pump.fun buy encodings); the backend
  remains the fail-closed source of truth.
- **`useNow` is the single app clock** — age/relative-time cells subscribe to it
  rather than owning timers; it runs at the coarsest needed granularity and only
  while the tab is visible.
