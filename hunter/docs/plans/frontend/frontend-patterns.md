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

**Which columns start visible** — three tiers, most specific wins, all resolved by the ONE
decider `defaultVisibleFor` in `DataTable.tsx`:

| Tier | Where | Use for |
| --- | --- | --- |
| 1. `defaultVisible` on the column | the column def | the column's intrinsic default |
| 2. Shared hidden-key set overlay | `TOKENS_HIDDEN_KEYS`, `SIM_HIDDEN_KEYS`, `POSITION_HIDDEN_KEYS`, `APPENDED_HIDDEN_KEYS` | a whole column *array*'s layout, when **every** table built from it agrees |
| 3. `defaultCols={{ [key]: shown }}` prop | the call site | one table's deviation, when tables sharing an array **disagree** |

Several tables are built from one shared array (`simColumns` feeds Simulate **and**
Dry-run; `appendedTokenColumns` feeds every `TokenTable`), so tier 2 alone cannot express
per-table layout — `initial_buy` is up front on Dry-run but not Simulate, `cu_price` on the
sim tables but not Evidence/Sweep/Wallet, `token_amount` on Evidence/Sweep-combo but not
the token lists. Tier 3 is the escape hatch: keep the shared array at ONE default and state
only the deviation at the call site, so a shared-array edit never silently re-lays-out a
table that never asked for it. Tier 3 applies to a table's own columns and its appended
token-info columns alike, and is the fallback for a column added *later* too (a new column
absent from `mt:table.knownCols` re-resolves through all three tiers — see `loadVisibleCols`).
Never copy the same `defaultCols` map to several call sites; if they agree, it belongs in tier 2.

**Charts-grid default** follows the same shape: `TokenTable`'s per-`tableId` charts toggle
(`mt:tablecharts:<tableId>`) starts from the call site's `chartsDefaultOn` (default off,
since each card is a per-row fetch), and the persisted choice wins once the user toggles it.
On today's tables it is on everywhere the chart IS the read, and off on `simulate-positions`
(a long result list you scan before drilling into one token).

**Hidden sort-only columns:** set `sortOnly: true` (+ `sortValue`) so the column joins multi-key sort but stays out of the Columns panel and defaults hidden. A sibling column's `renderHeader(SortCtx)` (via shared `MultiSortHeader`) calls `toggleSort(key)` for each axis — Rules/Simulate use `buildFingerprintRuleColumns`, `buildRuleParamsColumns`, and `buildCapsColumns` (concurrent / total; `0` total displays/filters as `∞` and sorts as largest).

**Row-memo performance (locked):** `TableRow` is `React.memo`'d. `DataTable` ref-stabilizes
`rowActions` / `rowDetail` / `rowClassName` / `cellGroupClassName` / `onSelect` so inline
call-site closures never bust memo. Callers must still **memoize `columns`** (and prefer a
module-level `rowKey`) — a fresh `columns` array rebuilds `visCols` and re-renders every
visible row. Client search precomputes a per-row blob (`WeakMap`); sort uses
decorate-sort-undecorate; selection-follow looks up a `Map` index; prefs/pins writes to
`localStorage` are debounced (150ms).

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
- **Vol/non-vol overlay gate:** `TokenPriceChart` draws the lines whenever *something* can
  classify — fingerprint `volume_ix_patterns` **or** just the token's creator wallet. With
  patterns the split is the engine's volume-maker vs organic; without, it degrades to
  creator + wallets they traded with vs the rest, and the toolbar tooltip says so. Only a
  token with neither disables the toggle. The per-trade **Vol badge keeps the stricter
  gate** (non-empty patterns) — it asserts a structural match, not a cohort.
  Resolve through `hooks/useFlowPatternKeys` — `useFlowPatternSource` /
  `useFlowPatternSourceForRule` / `useResolvedFlowPatternSource` (or
  `lib/flow/flowPatternKeys` for a raw pattern list). Any rule/fingerprint-scoped chart
  (`TokenTable` Charts, Floor inspect, lab inspect) still passes them, or the reader
  silently gets the weaker split.
- **Never split a `FlowPatternSource`.** The keys and the `fingerprintId` answer different
  questions — "classify with what" and "edit which row" — so pass `flowPatternKeys` and
  `flowFingerprintId` together down the whole chain (host → `TokenTradeChart` /
  `TokenChartsGrid` → `BarTradesPanel`). Dropping the id does not degrade gracefully: the
  Vol badge's write target then has to be guessed from the pattern set, and an unconfigured
  fingerprint's set is empty, which matches every other unconfigured row. The badge goes
  dead when several match and edits an unrelated rule's fingerprint when one does.
- **Pointer x -> chart coordinate goes through `paneCoords`.** lightweight-charts lays the
  container out as `[left axis][pane][right axis]` and every time-scale coordinate is
  measured from the PANE, so a bare `clientX - rect.left` is off by
  `chart.priceScale('left').width()` whenever the left (flow) scale is visible. Use
  `paneX` / `barTimeAtClientX` (`components/token-price-chart/paneCoords`) for any
  pointer-driven conversion — drag-to-select range, future hit-tests. Coordinates that
  arrive from the library itself (`MouseEventParams.point`, `timeToCoordinate`, primitive
  renderers) are already pane-relative and need no adjustment; `dualPriceScaleSync`'s
  axis-gutter hit-test deliberately stays in container space.

## BackgroundJobsContext — `context/BackgroundJobsContext.tsx`

App-wide registry for long-running jobs (sweep runs, simulation, swing-detection-all). Two split contexts (same pattern as PriceUnitContext):
- `BackgroundJobsActionsContext` — `{ register, deregister, cancel }` — stable
- `BackgroundJobsStateContext` — `{ jobs }` — ticks when job list changes

`BackgroundJobsIndicator` in the header subscribes to state; individual launch buttons subscribe to actions only.

## Sweep — `components/sweep/` and `GenericSweepView.tsx`

`GenericSweepView` is the sweep UI; `GenericSweepPage` passes the `strategyId` prop plus the param key list + axes definitions.

**Config form** (`SweepConfigForm`): param ranges are per-axis (`AxesSpec` from backend). The frontend declares which params to show and their display names in `components/sweep/genericAxes.ts`. Adding a new param = add an entry to the axes file + ensure the backend `AxesSpec` includes it.

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
  Match-count chip is **lazy** (`onRequestMatchCount` on hover / `openMatches`) via
  `useFingerprintMatches` — no `POST …/grouped/tokens` on every page mount with a
  persisted seed id.
- **`FingerprintGroupPicker`** — shared group-by + value filters. Clears filters in
  **one** parent update (`onClearFilters`). When scoped: Creation Stats passes
  `disabled` (manual group-by/filters dropped entirely); Flow / Sweep pass
  `filtersDisabled` (engine match replaces filters; group-by can still split).
  High-churn filter text uses `useLocalStorage(..., { debounceMs: 400 })` so React
  state stays live while disk/broadcast writes are coalesced.

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

## The query string is shared state — patch it, never rebuild it

A page's filters, cohort, selection and deep link all live in ONE `URLSearchParams`, and
several hooks write it independently (Console: the page's `position`/`mint`,
`useHistoryCohort`'s `h*` keys, the History scroll cleanup). Two rules follow:

- **Delete/set only the keys you own.** A fresh `new URLSearchParams()` drops every key
  another hook owns — on the Console that silently resets every filter the moment a row
  modal closes.
- **Take the functional form** `setParams(prev => …, { replace: true })`, so two writes in
  one tick cannot drop each other's keys (and the handler stays stable for memoized
  children).

Building from empty is correct **only** in an href builder (`lib/strategy/nav.ts`), which
constructs a link rather than mutating the live URL.

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

Four rules, all enforced by tests (`lib/storage.test.ts`, `lib/storageGate.test.ts`):

1. **One gate.** Every durable pref goes through `lib/storage` or `hooks/useLocalStorage`.
   A raw `localStorage.*` in a component fails `storageGate.test.ts`. The one allowlisted
   exception is `live/lib/desktopNotify.ts` (an ephemeral cross-tab claim stamp, not a
   pref). `sessionStorage` is used only for the one-shot discovery → sweep seed.
2. **One prefix.** Everything durable is `mt:`. There is no `hunter.*` namespace.
3. **Registry = truth.** Every key is in `STORAGE_KEYS`; accordion ids are in
   `ACCORDION_IDS`. A key that nothing reads is deleted in the PR that finds it.
4. **Few blobs, not many flat keys.** Group related prefs into one object and give each
   control a *field*; a new pref should cost a field, not a key. Shape changes **merge
   with defaults** at the reader — never a `_v2` key, never a migration framework.

### Hooks

| Hook | Use for |
|---|---|
| `useLocalStorage(key, initial, { debounceMs })` | one whole key (a form draft, a standalone pref). High-churn filter text passes `debounceMs: 400` |
| `useStoredField(key, field, initial, opts)` | one **field** of a shared blob. The write is read-modify-write against storage, so sibling fields (including debounced ones) can never clobber each other |
| `useAccordionOpen(ACCORDION_IDS.x, defaultOpen?)` | a collapsible panel's open state (`mt:ui.accordion`) |
| `useUiToggle('hideDust', false)` | an app-wide show/hide switch (`mt:ui.toggles`) |

All of them broadcast, so every mounted reader of the same key/field stays in sync within
the tab, and follow a `storage` event across tabs.

### Key map

| Key | Content |
|---|---|
| `mt:app.timezone` / `mt:app.priceUnit` | timezone; `'SOL'` or `'USD'` |
| `mt:chart.prefs` · `mt:notifications` · `mt:swing.criteria` | chart toolbar; notification prefs; swing criteria |
| `mt:ui.accordion` | `{ [accordionId]: boolean }` — every collapsible chrome panel (`ACCORDION_IDS`) |
| `mt:ui.toggles` | `{ showDisabledRules, hideDust, consoleWaitingOpen }` |
| `mt:ui.pnlDistDensity` · `mt:ui.metricPanes` | distribution bin density; metric-pane selection |
| `mt:table.{cols,knownCols,prefs,pins,charts}` | per-`tableId` maps: visible columns, the column set at write time, sort/pageSize/pinsCollapsed/filtersOpen, pinned rows + snapshots, charts-grid on/off |
| `mt:page.creationStats` | Creation Stats page **and** its grouped section (one blob, one field per control) |
| `mt:tokens.filters` / `mt:tokens.live` | Tokens page quick filters; live-stream toggle |
| `mt:form.flowDiscovery` / `mt:form.metricDiscovery` / `mt:form.ruleSearch` / `mt:form.familySearch` / `mt:form.replay` / `mt:form.traderAnalysis` / `mt:sweep.config*` | form drafts — **one blob per page**, every input of that form in it |
| `mt:simulate.runPrefs` | Simulate run parameters: created window + fill/cost model (what a run scans and how it prices) |
| `mt:flow.previewChart.prefs` | FlowPreviewChart toolbar |
| `mt:sweep.sel.*` · `mt:sweep.showNotFired` · `mt:simulate.showNotFired` | run selection; the two **deliberately separate** not-fired toggles (a sweep row is a combo token, a sim row a position — Simulate and Dry-run *do* share theirs) |
| `mt:filter.tags.<pageId>` / `mt:filter.mode.<pageId>` | URL-mirrored view filters; **the URL stays authoritative**, storage only restores the last-used value when the URL carries no param |
| `mt:console.tradeLog` | the manual-trade log — data, not a view pref (positions table stays the source of truth) |

### Persist vs do-not-persist

Persist **view preferences**: collapse state, column visibility/sort/page size/pins,
show-hide toggles, chart toolbar prefs, and form drafts — including the run knobs that
decide what a query covers (date range, look-back, row cap, fill/cost model), which
otherwise reset to a *different* scope than the numbers on screen were read under. Do
**not** persist modal/popover open, focus chips or row selection, in-flight busy flags,
results, or a cohort filter the URL already owns (Console History, Portfolio) — nor the
target of a one-shot operation (the mint on Sync Token, a create-form's fields).

A form persists as **one blob per page** with every input in it, and each value has
exactly one writer: the page's draft, or the URL, never both. A picker split across two
homes restores two different windows on refresh. An accordion whose default follows the data (`defaultOpen={runs.length === 0}`)
must stay unpersisted — otherwise the first visit's data shape sticks forever.

### Retiring a key

Add it to the move tables in `storage.ts` (`LEGACY_JSON_MOVES` / `LEGACY_STRING_MOVES` /
`LEGACY_ACCORDION_MOVES`) so `migrateLegacyStorage` folds the user's value into its new
home on the next load, then purges the old key. The move only writes when the destination
is still empty, so it is safe to re-run.
