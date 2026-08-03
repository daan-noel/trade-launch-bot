# Frontend Patterns & Implementation Detail

Deep-dive on component behavior, state management, SSE handling, and perf patterns. See [@arch/frontend.md](@arch/frontend.md) for the file-level map and pages overview.

## RTK Query — split store (`shared/store/baseApi.ts`)

The store is split across three modules (see [@arch/frontend.md](@arch/frontend.md)):
- `src/shared/store/baseApi.ts` — one `createApi` shell; all 9 `tagTypes` declared here.
- `src/shared/store/sharedEndpoints.ts` — tokens, settings, profiles, sol-price, creation-stats, etc.
- `src/live/store/liveEndpoints.ts` — wallet holdings, buy/sell, cashback, live-mode, tpsl positions.
- `src/lab/store/labEndpoints.ts` — grouped-sweep, simulate/paper, grouped-creation-stats.

Import mode-specific hooks from `@live/store/*Endpoints` or `@lab/store/*Endpoints`, never from the shared barrel, so a mode's side-effects never leak across builds.

Key behaviors:

- **`keepUnusedDataFor: 300`** (5 min) — cached query results survive route changes; no re-fetch on tab switch
- **`skipPollingIfUnfocused: true`** — polling hooks (rules, positions) pause when the tab is hidden; resumes on focus
- **Structural sharing** — RTK Query's default: if the response JSON deep-equals the previous cache entry, the returned object reference is preserved → no downstream re-renders from identity changes

**Cache invalidation:** mutations (`updateRule`, `deletePosition`, etc.) use `invalidatesTags` to selectively refetch affected queries. Avoid `invalidatesTags(['Tokens'])` after a single-mint operation — use `invalidatesTags([{ type: 'Token', id: mint }])` instead.

**Optimistic updates** (settings page, profiles page): `onQueryStarted` → `updateQueryData` → `await queryFulfilled` → if rejected, `patchResult.undo()`. This pattern is in `OtherProfilesPage` and `SettingsPage`.

## SSE — `services/sse.ts`

One shared `EventSource` for the session (`/api/stream`). All streams are multiplexed through it. The connection is established once in `AppLayout`; pages subscribe/unsubscribe via listener hooks.

**Event types dispatched:**

| SSE event | Payload | Consumer |
|---|---|---|
| `token_created` | `{ mint }` | `TokensPage` — triggers `refetchTokenCount()` |
| `trade` | `TradeEvent` | `useTradeStream()` → `TransactionsPage` buffer |
| `sol_price` | `{ price_usd }` | `PriceUnitContext` |
| `tpsl1_positions_changed` | `TpslPositionDelta` | `connectTpslPositionsChanged` → in-place row patch |
| `tpsl2_positions_changed` | `TpslPositionDelta` | same |

**`TpslPositionDelta` patch pattern:**

```typescript
// In Tpsl1Page (simplified)
useEffect(() => {
  return connectTpslPositionsChanged('tpsl1', (delta) => {
    dispatch(apiSlice.util.updateQueryData('getTpsl1Positions', ruleId, (draft) => {
      applyDelta(draft, delta);  // mutate immer draft in-place
    }));
  });
}, []);
```

No full refetch on position churn — only the changed position row is patched. This makes the strategies page stable at high trade volume (100+ position updates/min).

## `DataTable<R>` — `components/table/DataTable`

Generic, reused across all pages. Two modes:

**Client-side mode** (`data` prop): full dataset in memory; pagination/sort/filter happen in JS. Used for small result sets (sweep combos, paper positions).

**Server-side mode** (`fetchPage` prop): page state is owned by `DataTable`; each page change calls `fetchPage({ page, pageSize, sortKey, sortDir, filters })`. Used for tokens, trades (unbounded).

**Toolbar defaults:** `colFilters` and `colToggle` are **on** — every table shows Filters + Columns unless a call site opts out. Pass `tableId` so column visibility persists.

**Column visibility** is persisted per `tableId` to `localStorage` key `mt:table.cols` (a map of `tableId → Set<hidden_column_keys>`). All tables share one localStorage entry, keyed by `tableId` string. This is how column preferences survive refreshes.

**Hidden sort-only columns:** set `sortOnly: true` (+ `sortValue`) so the column joins multi-key sort but stays out of the Columns panel and defaults hidden. A sibling column's `renderHeader(SortCtx)` (via shared `MultiSortHeader`) calls `toggleSort(key)` for each axis — Rules/Simulate use `buildFingerprintRuleColumns`, `buildRuleParamsColumns`, and `buildCapsColumns` (concurrent / total; `0` total displays/filters as `∞` and sorts as largest).

## `useTradeStream` — `hooks/useTradeStream.ts`

Buffers incoming SSE `trade` events. Max buffer size: 500 (older events are dropped from the front). The buffer is a `useRef` (not state) to avoid re-renders on every trade; a debounced `setState` at 100ms flushes the ref into React state for rendering. This means `TransactionsPage` re-renders at most 10×/sec regardless of trade volume.

## PriceUnitContext — `context/PriceUnitContext.tsx`

Provides SOL/USD toggle + current SOL price. Listens to `sol_price` SSE events.

**Split to avoid re-renders:** `PriceUnitProvider` splits into two contexts:
- `PriceUnitActionsContext` — `{ toggleUnit, setUnit }` — stable object, never changes reference
- `PriceUnitStateContext` — `{ unit, solPrice }` — ticks on every SOL price update

Cells that only need `unit` (to format labels) import `PriceUnitActionsContext`. Cells that display live USD values import `PriceUnitStateContext`. This prevents label-only cells from re-rendering on every 60s price tick.

**Filter/sort units:** amount cells convert for display, but filters must match what the user
sees. Column defs set `filterAmount: 'sol'|'usd'` and `filterNumber` via
`amountInDisplayUnit` (`lib/priceUnitSnapshot`, mirrored from the provider).
`toTableRequest({ amountCols })` converts typed operands back to storage before the
server compare. Percent columns that render `×100` (Win %, Open %) use a local
`displayUnits: (n) => n * 100` on `filterNumber` — same pattern as Simulate's `simMetric`.

## Route Suspense + chart code-split

- **Suspense boundary:** `AppLayout` wraps only `<Outlet />` (not the whole `Routes` tree).
  Lazy route chunks show `SuspenseFallback` → `LoadingState` (`page`) in the main pane;
  header/nav stay mounted.
- **Lazy-chunk placeholder SSOT:** every Suspense fallback for a `lazy()` import uses
  `LoadingState` (`components/ui/LoadingState`) — `page` for routes, `panel` for chart /
  inspect shells, `inline` for compact embeds. `LazyLabTokenInspectModal` paints the modal
  chrome immediately with a `panel` placeholder inside (never a blank `null` fallback).
- **`lightweight-charts` deferral:** never static-import `TokenTradeChart` / inspect modals from a
  route or from `TokenTable`. Use `LazyTokenTradeChart`, `LazyLabTokenInspect(Modal)`, and
  `TokenTable`'s `LazyTokenChartsGrid` (Charts toggle). Creation-stats trend charts
  are lazy inside the page/section so the control shell paints first.

## BackgroundJobsContext — `context/BackgroundJobsContext.tsx`

App-wide registry for long-running jobs (sweep runs, simulation, swing-detection-all). Two split contexts (same pattern as PriceUnitContext):
- `BackgroundJobsActionsContext` — `{ register, deregister, cancel }` — stable
- `BackgroundJobsStateContext` — `{ jobs }` — ticks when job list changes

`BackgroundJobsIndicator` in the header subscribes to state; individual launch buttons subscribe to actions only.

## `strategyResultCache.ts` — imperative start→wait→fetch

Used for sim (`POST .../simulate`) and swing-detection-all (`POST .../detect-swings-batch`):

1. **POST** → receive `{ job_id }` (202 Accepted)
2. **Subscribe** to SSE for `job_complete` events with matching `job_id`
3. **On event:** `GET .../jobs/{job_id}/result` → store in slice
4. **Timeout:** after 5min, mark job as timed-out; user can still manually refetch

Result is stored in `simulationResultSlice` / `swingResultSlice` and retrieved via `selectSimResult(ruleId)` selectors. Pages access results via these selectors, not by polling the API.

## Sweep — `components/sweep/` and `GroupedSweepView.tsx`

`GroupedSweepView` is the generic sweep UI. Two thin wrappers (`Tpsl1GroupedSweepPage`, `Tpsl2GroupedSweepPage`) pass the `strategyId` prop and the strategy-specific param key list + axes definitions.

**Config form** (`SweepConfigForm`): param ranges are per-axis (`AxesSpec` from backend). The frontend declares which params to show and their display names in `sweep/axes.ts` (one file per strategy). Adding a new param = add an entry to the axes file + ensure the backend `AxesSpec` includes it.

**FingerprintGroupPicker**: shared between the sweep config form (filter corpus) and the dashboard (`GroupedCreationSection`). Renders a multi-select of known fingerprint field values; selection is serialized to URL query params so the group filter survives refresh.

**`buildGroupColumns` / `buildSweepColumns`**: column factories called once per render with the current `strategyId`. Memoized with `useMemo` — column defs are stable objects so `DataTable` doesn't re-sort unnecessarily.

## Fingerprint scope control — `SearchableSelect` + `FingerprintScopeControl`

Flow Discovery, Grouped Sweep (`GenericSweepConfigForm`), and the Creation Stats
dashboard (`GroupedCreationSection`) all let the user "scope by a saved fingerprint":
pick one from a dropdown and the page's corpus collapses to a single "ALL" group of
tokens the engine matcher (`hunter_engine::fingerprint::matches`) says match it —
manual group-by / value filters are then ignored (both client-side and, for
Creation Stats, server-side — see below).

- **`shared/components/ui/SearchableSelect.tsx`** — generic type-to-filter combobox
  primitive (case-insensitive substring match, ↑/↓/Enter/Esc, click-outside-to-close,
  a clear button). Not fingerprint-specific; reuse for any long option list that needs
  search-as-you-type instead of a native `<select>` (which has no filtering).
- **`shared/components/strategy/FingerprintScopeControl.tsx`** — wraps
  `SearchableSelect` with the fingerprint-picking UX: selected-fingerprint badge + link
  (`fingerprintsHref`) + axis-params summary (`fingerprintParamsCell`), or a manual-mode
  hint when nothing's picked. The three pages share this component; only the help copy
  (`tip`, `scopedDescription`, `manualHint`) differs per page, passed in by the caller.

**Creation Stats parity note:** unlike Flow Discovery/Sweep (which load a corpus into
memory and run the engine matcher directly), Creation Stats is SQL-only. Its backend
(`creation_stats_repo::grouped_scoped` / `build_grouped_tokens_where_scoped`) reproduces
the engine matcher as SQL predicates instead: exact equality for discrete fields,
bucket-index equality (same `bucket_index`/`BUCKET_EPS` math as `hunter_engine::grouping`)
for continuous SOL fields at the fingerprint's own `bucket_size_amount`, and `jsonb`
structural equality (`SqlArg::Json`) for `ix_labels` to preserve order/duplicates like the
in-memory matcher. The frontend sends `fingerprint_id` and omits every other grouping/
filter param when scoped (`labEndpoints.ts`'s `getGroupedCreationStats` /
`getGroupedCreationTokens` query builders branch on it).

## Strategy cross-page selection — `?rule=` / `?fp=`

Same deep-link shape as Tokens `?mint=` and Sweep `?run=`: selection in the URL so
same-tab navigation (and Ctrl/middle-click new tabs) land with the row selected.

| Param | Page | Helper |
|---|---|---|
| `?rule=<id>` | `/strategies/rules`, `/strategies/simulate` | `rulesHref` / `simulateHref` |
| `?fp=<id>` | `/strategies/fingerprints` | `fingerprintsHref(id)` |

- `lib/strategy/nav.ts` — path + href builders (SSOT for cross-links). Simulate is
  lab-only — never link to it from the live app.
- `hooks/useSelectionSearchParam(param)` — bidirectional `selectedKey` ↔ search param
  (`replace: true` on user select; URL seeds on load / back-forward).
- Fingerprints "Used by" → `rulesHref`; Rules (lab `linkToSimulate`) → `simulateHref`;
  Simulate rule name → `rulesHref`; fingerprint cells → `fingerprintsHref`.
- Sweep group Used-by chips → `rulesHref`; matched fingerprint → `fingerprintsHref`. The
  chip's **best** badge is `ruleParamsJsonEqual(rule.params, group.best_params)` — that
  comparator canonicalizes key order **and** the order of every set-like array (a group's
  window instances, a metric's DNF arms / AND atoms), because an editor round-trip re-emits
  window instances sorted by window. `scale_out` is the one array kept positional (the
  ladder executes in authored order). Never compare rule `params` with a bare
  `JSON.stringify`.
- Flow Discovery seed/target badges → `fingerprintsHref`.
- Live Armed rule columns → `rulesHref`.

## Memo & Render Discipline

- **Column defs** created with `useMemo(() => buildColumns(...), [deps])` — never inline in JSX
- **Price formatter** created with `useMemo(() => createFormatter(unit, solPrice), [unit, solPrice])` — shared across cells via context
- **Row identity** in `DataTable`: rows must have a stable `id` field; `DataTable` uses it as React `key` — without it, position changes cause full row remount
- **No anonymous objects in JSX props** — `style={{ color: 'red' }}` inline creates a new object each render; extract to a const or use Tailwind classes
- **`useCallback` on event handlers passed to memoized children** — otherwise the child re-renders on every parent render

## Signed value tone — `lib/signedTone.ts`

Glanceable green/red for PnL-like numbers. SSOT:

- `signedToneClass(v)` → Tailwind class (`>0` green, `<0` red, `0` mid, null/NaN dim)
- `signedStatTone(v)` → `StatTile` tone names
- `formatSigned` / `formatSignedPct` → `+`-prefixed display strings

`lib/strategy/runSummary.goodBad(v)` (pivot `0`) delegates here so sweep/sim/live summaries
match wallet/home. Non-zero pivots (win-rate 0.5, profit-factor 1) keep threshold semantics.
Do **not** reintroduce local `pnlClass` / `v > 0 ? 'text-green' : …` ternaries for signed PnL.

## UI chrome SSOT — page header / empty / alert / signal grade

| Primitive | Where | Use for |
|---|---|---|
| `PageHeader` | `components/ui/PageHeader` | Title + one-line job + optional actions (homes = `size="page"`, tools = default) |
| `EmptyState` | `components/ui/EmptyState` | Dashed empty panel + optional single CTA — prefer over ad-hoc dashed boxes |
| `InlineAlert` | `components/ui/Modal` | Error / success / warning strips — **theme tokens only** (`text-green` / `text-warning` / `text-red`) |
| `signalGradeClass` | `lib/signedTone` | 0..1 bot/wash-like grades (Flow Discovery) — same warning→danger ladder as theme |
| `inspectFromMint` | `components/strategy/inspectTarget` | Lab mint-only inspect (Creation Stats) → `LazyLabTokenInspectModal` (chart + metric panes) |

Lab Evidence must surface the sync-snapshot caveat via `InlineAlert variant="warning"`, not dim microcopy.

## localStorage — `lib/storage.ts`

All localStorage access goes through `lib/storage` wrapper. Keys are namespaced `mt:`:

| Key | Content |
|---|---|
| `mt:table.cols` | `{ [tableId]: string[] }` — hidden column keys per table |
| `mt:price-unit` | `'SOL'` or `'USD'` |
| `mt:timezone` | `'local'` or `'UTC'` |
| `mt:dashboard.grouped.fingerprintId` | Creation Stats' saved-fingerprint scope (`GroupedCreationSection`) |

Direct `localStorage.getItem/setItem` calls outside `lib/storage` are a code smell — they bypass SSR safety guards and miss the namespace prefix.
