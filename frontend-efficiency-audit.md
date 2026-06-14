# Frontend Efficiency Audit

_Scope: `frontend-react/src` (124 files). Focus: data flow, re-fetching, re-rendering on hot paths (SOL/USD rate tick, live-trade SSE stream, large tables, chart pointer events)._

## TL;DR

The frontend is **already well-tuned**. The core infrastructure — single shared `EventSource` (`services/sse.ts`), `DataTable` row memoization + CSS-only column hover, `PriceUnitContext` + `usePriceDisplay` (rate folded out in SOL mode), `useNow` shared visibility-gated clock, memoized `priceCells`, RTK Query 5-min cache with `skipPollingIfUnfocused` — is correct and deliberate. The Tokens page and the chart formatters are exemplary.

Findings are few and concentrated. The one **High** item is a genuine hot-path cost that touches every timestamp cell in the app.

| # | Severity | File | Category | One-liner |
|---|---|---|---|---|
| 1 | **High** | `utils/date.ts` | expensive-computation | `new Intl.DateTimeFormat` built on every cell render (per row, 4×/sec on live tables) |
| 2 | Medium | `hooks/useTradeStream.ts` | subscription-churn | Stream parse + 500-row flush/re-render not paused while tab hidden |
| 3 | Low | `components/table/DateCell.tsx` | re-render | `DateCell` not `memo`-wrapped → re-formats whenever its row’s index shifts |
| 4 | Low | `components/table/DataTable.tsx` | expensive-computation | Client mode re-filters up to 500 rows every 250 ms flush even with no active filter |
| 5 | Low | `components/transactions/tradeColumns.tsx` | re-render | Live tables rebuild columns + re-render whole grid on each USD-rate tick (USD mode only) |

---

## 1. HIGH — `Intl.DateTimeFormat` constructed per timestamp-cell render

**File:** [utils/date.ts:11](frontend-react/src/utils/date.ts#L11), [:69](frontend-react/src/utils/date.ts#L69), [:102](frontend-react/src/utils/date.ts#L102)
**Category:** expensive-computation

`formatInstantParts`, `formatIsoCompact`, and `formatTimestampMsCompact` each construct a **fresh `Intl.DateTimeFormat`** every call. `Intl.DateTimeFormat` is one of the most expensive standard-library constructors (full locale + timezone data resolution, ~0.1–1 ms each), and here it’s built with identical `(timeZone, options)` arguments every time.

The call chain makes this per-row, per-tick on the hottest surface in the app:

- `formatIsoLines` → `formatInstantParts` is called by [DateCell.tsx:8](frontend-react/src/components/table/DateCell.tsx#L8) on **every render**.
- The live-trade tables ([DashboardPage](frontend-react/src/pages/dashboard/DashboardPage.tsx), [TransactionsPage](frontend-react/src/pages/transactions/TransactionsPage.tsx)) render a `DateCell` per row ([tradeColumns.tsx:110](frontend-react/src/components/transactions/tradeColumns.tsx#L110)).
- `useTradeStream` flushes a new array every 250 ms ([useTradeStream.ts:29](frontend-react/src/hooks/useTradeStream.ts#L29)), **prepending** the batch. That shifts every visible row’s `index` prop ([DataTable.tsx:517](frontend-react/src/components/table/DataTable.tsx#L517)), so `TableRow`’s `memo` cannot skip — **every visible Time cell re-runs and rebuilds a formatter**.

At 10–25 visible rows and up to 4 flushes/sec that’s **~40–100 `Intl.DateTimeFormat` constructions per second**, all on identical arguments. The same waste recurs (less hot) on every other `DateCell`/compact-timestamp table on each poll/refetch.

**The fix is already demonstrated in this codebase:** [chartTimezone.ts:121](frontend-react/src/components/token-price-chart/chartTimezone.ts#L121) `createChartTimeFormatters` builds its formatters **once** and reuses them across all crosshair/tick callbacks. `date.ts` just doesn’t follow that pattern.

**Suggested fix:** module-level formatter cache.
```ts
const dtfCache = new Map<string, Intl.DateTimeFormat>();
function getDtf(timeZone: string, opts: Intl.DateTimeFormatOptions, key: string) {
  let f = dtfCache.get(key);
  if (!f) { f = new Intl.DateTimeFormat('en-US', { timeZone, ...opts }); dtfCache.set(key, f); }
  return f;
}
```
Key by `` `${timeZone}|${withFractionalSeconds}` `` (timezone choices are a tiny finite set). Apply to all three construction sites.

---

## 2. MEDIUM — `useTradeStream` is not visibility-gated

**File:** [hooks/useTradeStream.ts:21-42](frontend-react/src/hooks/useTradeStream.ts#L21-L42)
**Category:** subscription-churn

The SSE subscription and the flush timer keep running while `document.hidden`. Every incoming frame is still `JSON.parse`d ([:37](frontend-react/src/hooks/useTradeStream.ts#L37)) and the `setTimeout` flush still fires (browser-throttled to ~1/sec in background tabs), so a hidden Dashboard/Transactions tab performs roughly one full update per second indefinitely:

- rebuild of the 500-element events array (`batch.reverse().concat(prev)`),
- a page re-render,
- `DataTable`’s `processed` useMemo re-running its filter pass over all rows ([DataTable.tsx:240-272](frontend-react/src/components/table/DataTable.tsx#L240-L272)),
- a full visible-page row re-render including the per-cell formatter cost from finding #1.

This deviates from the codebase’s own established visibility-gating: `useNow` pauses when hidden ([useNow.ts:40](frontend-react/src/hooks/useNow.ts#L40)) and RTK Query polls use `skipPollingIfUnfocused`. It’s the one hot stream that doesn’t honor it.

**Suggested fix:** while `document.hidden`, stop scheduling flushes (keep buffering or cap the buffer); on `visibilitychange → visible`, flush once and resume. Mirror the `useNow` visibility handler.

---

## 3. LOW — `DateCell` is not memoized

**File:** [components/table/DateCell.tsx:5](frontend-react/src/components/table/DateCell.tsx#L5)
**Category:** re-render

Because live-trade flushes shift each row’s `index`, the memoized `TableRow` re-renders even for rows whose underlying trade object is unchanged, and `DateCell` re-runs `formatIsoLines` each time. Wrapping `DateCell` in `React.memo` (its only prop is `iso`) lets an unchanged timestamp skip formatting entirely when the row re-renders solely because the feed shifted indices. Pairs naturally with finding #1 (cache) — together they take the live-table timestamp column close to zero cost.

---

## 4. LOW — Client-side `processed` re-filters the full buffer every flush

**File:** [components/table/DataTable.tsx:240-272](frontend-react/src/components/table/DataTable.tsx#L240-L272)
**Category:** expensive-computation

In client mode the `processed` useMemo depends on `rows`, so each 250 ms live flush re-runs the filter pass (and allocates a new array) over up to 500 rows — even on Dashboard/Transactions where no search/column-filter/sort is active. The per-row predicate is cheap, but it’s 500 iterations + an array allocation 4×/sec for no result change.

**Suggested fix:** short-circuit when client mode has no active search, no `activeColFilters`, and no `sortCol` — return `rows` directly (mirrors the existing `serverSide` short-circuit at [:243](frontend-react/src/components/table/DataTable.tsx#L243)).

---

## 5. LOW — Live-trade columns rebuild on every USD-rate tick (USD mode only)

**File:** [components/transactions/tradeColumns.tsx:9](frontend-react/src/components/transactions/tradeColumns.tsx#L9), consumed at [DashboardPage.tsx:10](frontend-react/src/pages/dashboard/DashboardPage.tsx#L10) / [TransactionsPage.tsx:11](frontend-react/src/pages/transactions/TransactionsPage.tsx#L11)
**Category:** re-render

`tradeColumns(price)` bakes the formatter into each `render`, and the pages do `useMemo(() => tradeColumns(price), [price])`. In **USD mode**, every SOL/USD-rate tick hands back a fresh `price` object → new `columns` identity → the entire live table re-renders. In SOL mode the rate is folded out of `usePriceDisplay` ([usePriceDisplay.ts:13](frontend-react/src/hooks/usePriceDisplay.ts#L13)), so this is a no-op — which is why it’s low severity.

The token table already solved this: it uses the memoized, context-reading `priceCells` ([priceCells.tsx](frontend-react/src/components/tokens/priceCells.tsx)) so only the price cells re-render on a rate tick, not the whole grid. The live-trade columns could adopt the same `PriceCell`/`AmountCell` components to localize USD-mode rate re-renders.

---

## Areas reviewed and found efficient (no action)

- **`services/sse.ts`** — single shared `EventSource`, lazy open / ref-counted close, per-type fan-out. Correct.
- **`context/PriceUnitContext.tsx`** — `useReducer` + memoized value; settings fetch deduped via RTK cache; `usdRate` kept local.
- **`hooks/usePriceDisplay.ts`** — rate folded out of the memo key in SOL mode (the key anti-re-render trick).
- **`components/tokens/priceCells.tsx`** — memoized cells read context directly; columns stay referentially stable across rate ticks.
- **`hooks/useNow.ts`** — one interval at the coarsest needed granularity, visibility-gated, `useSyncExternalStore` bail-out.
- **`components/table/DataTable.tsx`** — memoized `TableRow`, CSS-only column hover, debounced inputs, server-side mode short-circuit, careful page-reset deps. (Only the two minor items above.)
- **`components/token-price-chart/chartTimezone.ts`** — formatters built once per timezone and reused across crosshair/tick callbacks. This is the reference pattern.
- **Tokens page** — server-side pagination, SSE-triggered refetch, memoized columns. Confirmed as the known-good reference.

---

_Note: this audit was completed by direct file review. An earlier multi-agent run was interrupted mid-pass; its one completed reviewer independently surfaced findings #1 and #2, which were then re-verified here against the source._
