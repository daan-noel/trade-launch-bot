# Strategy result tables → server-side pagination/sort/search/filter

**Goal:** bring every token-related table on the strategy pages to full server-side
paging/sort/search/filter, matching what the **Positions** table already does.

## Status

- ✅ **Phase 1 (numeric filters)** — DONE. `filterNumber` added to every numeric
  column in `ALL_TOKEN_COLS` (`sharedTokenColumns.tsx`), mirroring `tokenColumns.tsx`
  (lamports→SOL for `max_sol_cost`/`spendable_sol_in`). All strategy tables now
  filter enrichment columns numerically client-side.
- ✅ **Phase 2 (Paper Positions)** — DONE, **no new backend**. Extracted a shared
  `@lab/components/strategies/PaperResultSection`; its token table is now a second
  `useRulePositions` instance scoped to the paper rule (server-side via the existing
  `/positions` + `/positions/summary`, which already resolve the latest paper run).
  Removed the client-side 5,000-row array + `applyPaperDeltas`/`positionToSimResult`
  from all 3 lab pages. **Decision taken:** reuse the Positions columns/whitelist
  (paper drops the sim-only Trades count; Reason→Exit Reason).
- ✅ **Phase 3 (Matched)** / ✅ **Phase 4 (Simulated)** — DONE under a **unified
  POST + JSON contract** (see `do-phase-3-4-*` plan). All four strategy token
  tables now share one request body and page/sort/filter/search server-side with
  **numeric operators** (`>5`, `1..10`) comparing numerically on the server.

**Unified contract decision (closes Open Questions 1–3):**
- One `POST` + JSON `TableRequest` body for **all four** tables (Positions / Paper
  / Matched / Simulated), replacing the flat GET query-string. Per-column filters
  are structured `{op, val}` (`FilterOp`: contains/eq/gt/gte/lt/lte/between) so
  numeric ops compare numerically server-side (`trading_core::api::table_query` +
  the typed whitelist in `strategy_repo`). GET→POST applied to **live** positions
  too, so the shared `useRulePositions` hook + fetchers stay one code path.
- **Matched = Option A (materialize):** first POST runs the whole-`tokens` closure
  scan for the matched **mint set**, cached on `LocalState` (`MatchedCache`, TTL/GC
  like `sim_results`); later pages re-query `tokens WHERE mint = ANY(set)`. Removes
  the 5,000-row display cap.
- **Simulated (Open Question 3) — NO new table.** The finished backtest's rows are
  already fully resident (lab is single-user, workstation RAM), so we page/sort/
  filter them **in memory** (`strategies::sim_query` over the `Vec<Value>` in
  `SimResults`) — no `sim_result_tokens` table, migration, write path, or
  retention. Supersedes the plan's persist-per-token proposal below.
- Frontend converges on `toTableRequest` (serializer) + `useServerTable` (Matched/
  Simulated) + the existing `useRulePositions` (Positions/Paper, now POSTing).

**Scope:** Matched · Simulated · Paper Positions — on all lab strategy pages
(Tpsl1 / Tpsl2 / Swing1). Positions endpoint is now POST on **both** bins.

---

## 0. Current state (audited)

`DataTable` has a built-in server-side mode: pass `serverSide` + `serverTotal` +
`onQueryChange`; it emits the debounced `TableQuery` (page/pageSize/sortKeys/
search/colFilters) and renders `rows` verbatim as the current page.
[frontend-react/src/shared/components/table/DataTable.tsx](frontend-react/src/shared/components/table/DataTable.tsx)

The **Positions** table is the reference implementation, uniform across all 5 pages:
- Hook: [useRulePositions](frontend-react/src/shared/hooks/useRulePositions.ts) owns the page + whole-run summary, keeps rows live via SSE deltas, aborts stale fetches.
- API: `fetchRulePositionsPage` reads `X-Total-Count` for the pager. [api.ts:63](frontend-react/src/shared/services/api.ts#L63)
- Backend: `PositionQuery` + `position_sort_sql`/`position_filter_sql` whitelist, `find_positions_by_run_paged` + `count_positions_by_run`, LEFT-JOIN `tokens` + `tokens_info` so token-enrichment columns sort/filter/search server-side too. [strategy_repo.rs:320-1012](trading_core/src/storage/repositories/strategy_repo.rs#L320-L1012)

The three remaining tables were **client-side** — full array, paged in-browser.
Their backends return plain `Vec`s with **no** paging/sort/filter/count API:

| Table | Endpoint | Data source | Cap | Difficulty |
|---|---|---|---|---|
| Paper Positions | `…/paper-result` | persisted `strategy_positions` (latest paper run) | 5,000 | **Low** — reuse existing paged repo → **DONE (Phase 2)** |
| Matched | `…/matched` | whole-`tokens` scan, entry predicate is a **Rust closure** | 5,000 | **High** — predicate isn't SQL |
| Simulated | `…/simulate` → `…/result` | async backtest, result stashed **in-memory** as one JSON blob | — | **High** — no queryable store |

---

## Phase 1 — Numeric filters on enrichment columns ✅ DONE

**Problem:** the strategy tables use `ALL_TOKEN_COLS` in
`sharedTokenColumns.tsx`, which defined **no `filterNumber`** — so `>5`, `1..10`
on price/mcap/volume/trade_count/… did a literal substring match. The Tokens
page's `tokenColumns.tsx` already had `filterNumber` on all of these.

**Done:** added `filterNumber` to each numeric column in `ALL_TOKEN_COLS`, copying
the accessors from `tokenColumns.tsx` (lamports→SOL `/1e9` on
`max_sol_cost`/`spendable_sol_in`; `first_slot_buy_sol`/`first_slot_sell_sol` are
already SOL in the frontend `TokenRecord`, no divide). Boolean flag columns left as
text-only (0/1) to match `tokenColumns.tsx`.

**Server-side note (still open):** in `serverSide` mode `DataTable` short-circuits
`activeColFilters` to empty and sends the raw filter text to the backend as an
`ILIKE '%text%'` substring on the `::text`-cast column. So `>5` typed on a
server-side column still substring-matches, not numeric-compares. Making numeric
*operators* work server-side needs the wire `filter` format + `position_filter_sql`
(and any new resolvers) to parse `>`/`<`/`..` and emit real numeric predicates —
**Open Question 1**.

---

## Phase 2 — Paper Positions → server-side ✅ DONE (no new backend)

**Insight that made it low-effort:** the `paper-result` endpoint already called
`find_positions_by_run_paged` (the SAME function the Positions table uses), and the
lab `…/positions` + `…/positions/summary` endpoints already resolve a rule's latest
paper run and are fully server-side. So no new backend was needed.

**What shipped:**
1. New shared component `frontend-react/src/lab/components/strategies/PaperResultSection.tsx`
   (was inlined byte-identically in all 3 lab pages). Presentational: takes the
   server-side page + total + summary as props, renders run-meta chrome + Clear +
   `SimSummaryCard` (server `summary`) + a `serverSide` `DataTable` of
   `positionColumns`.
2. Each lab page (Tpsl1/Tpsl2/Swing1) now runs a **second** `useRulePositions`
   instance scoped to `paperResult?.ruleId` (its own `paperPosQuery`), feeding that
   component. The two hook instances (selection-driven + paper) never conflict
   (different rule ids) and each aborts its own stale fetches.
3. Removed the client-side path: the 5,000-row token array, `positionToSimResult`,
   `applyPaperDeltas`, and the bespoke paper-delta SSE effect. The hook's own
   visible-row SSE patching + summary refresh replaces it.
4. `onSelectPaperToken` now resolves a position by **id** (`inspectFromPosition`),
   since the table emits id-keyed position rows instead of mint-keyed sim rows.

**Column decision (user-confirmed):** reuse Positions columns/whitelist. The paper
table therefore loses the sim-only **Trades** count column; **Reason** → **Exit
Reason**, **Holding** derived from the position. All sort/filter/search go through
the existing position whitelist (incl. token-enrichment via the tokens/tokens_info
join).

---

## Phase 3 — Matched → server-side (high effort, NOT started)

**Blocker:** the entry predicate is a Rust closure over `TokenRecord`
(`token_matches_buy_rule` + `!is_mayhem_mode`), evaluated in
`collect_matching_tokens`. Not expressible as SQL today, so we can't just add
`LIMIT/OFFSET/ORDER BY`.

Options (pick one — see Open Questions):
- **A. Materialize per request.** Run the scan once, cache the matched mint set
  (keyed by rule_id + range), then page/sort/filter over a JOIN of that mint set
  against tokens + tokens_info in SQL. Removes the 5,000 cap for display; first
  request still does the full scan.
- **B. Translate fingerprint criteria to SQL.** Compile the configured fingerprint
  into a `WHERE` clause so the scan *is* the paged query. Most work, best result
  (true streaming pagination, no cap). Fingerprint is optional/vacuous-true → then
  it's "all non-mayhem tokens in range", trivially SQL.
- **C. Keep client-side, raise/remove cap.** Cheapest; not truly server-side.

Wire contract becomes `{ tokens, total }` + `X-Total-Count` accepting
`limit/offset/sort/q/filter`, reusing a `position_sort_sql`-style whitelist for the
enrichment columns.

---

## Phase 4 — Simulated → server-side (high effort, NOT started)

**Blocker:** the sim result is a computed backtest stashed **in-memory** as one JSON
blob in `sim_results`, collected once via the result endpoint. No queryable store.

**Approach:** persist per-token results into a table
(`sim_result_tokens(rule_id, run_id, mint, entry_*, exit_*, pnl_*, …)`), then
page/sort/filter/search server-side with a whitelist + LEFT-JOIN to tokens_info for
enrichment. Summary card reads a whole-run aggregate. Existing "start job → SSE
finished → collect" flow stays; "collect" becomes "first page fetch".

**Cost/scale caution (CLAUDE.md):** new write path — every sim writes N rows.
Justify IO; add retention (ephemeral analysis artifacts). Lab-only, never ships to
EC2. Consider a hypertable with short retention or an unlogged/temp table.

---

## Shared frontend refactor (future, spans 3–4)

`useRulePositions`'s core (owns a page + total + summary, emits `TableQuery`, aborts
stale fetches, SSE-patches visible rows, settle-gated poll) is exactly what
matched/sim need. Extract a generic `useServerTable` so all tables share it; each
supplies `fetchPage(query) → { items, total }`, `fetchSummary?`, `sseChannel?`.
(Phase 2 reused `useRulePositions` directly rather than extracting — extract when
Phase 3/4 need a non-position row shape.)

---

## Definition of done (per CLAUDE.md)

- `cargo check -p lab` + `cargo check -p trading_core` clean; clippy on touched code;
  repo tests where logic changed (mirror the positions `parity_tests` idea for any
  new whitelist).
- `npm run build` clean; no extra re-render on SOL/USD tick or live-trade stream.
- Every new whitelist resolver drops non-whitelisted keys; needles bind as params +
  LIKE-escaped.
- New write path (Phase 4) carries retention + IO justification.
- Docs: `@arch/frontend.md` (done for 1+2), `@arch/strategies.md` + `@arch/database.md`
  for new endpoints/tables (phases 3–4).

---

## Open questions (need answers before phases 3–4)

1. **Numeric operators server-side?** Do we want `>5`/`1..10` to compare
   numerically on server-side tables, or is substring-on-text acceptable there?
   (Phase 1 fixed client-side either way.) If yes, the `filter` wire format + all
   filter resolvers must parse operators — bigger, shared change.
2. **Matched strategy — A, B, or C?** Depends on real-world matched counts.
3. **Sim persistence — durable table vs temp/unlogged?** Do sim results need to
   survive a lab restart, or are they always re-run on demand?
