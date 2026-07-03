# Token-tables unification & correctness — todo plan

Audit outcome for the 7 token-bearing tables (Tokens page · Positions · Paper · Old-run history ·
Matched · Simulated · Wallet Holdings). Goal: **one shared method** (contract + column grammar) for
every token table, no by-meaning column duplication, and two live pagination bugs fixed.

## Status — updated 2026-07-03 (Phases 1, 6, 2, 4 landed this session)

Legend: `[x]` done · `[~]` partial · `[ ]` open.

### DONE & VERIFIED this session (all backend; committed together)

- **Phase 1 — correctness bugs.** Simulated `mint` ASC tiebreak (`sim_query.rs`); Tokens default order
  unified to `created_at DESC, mint_address DESC` on both engines via one `newest_first` comparator
  (`token_list_cache.rs`) + `find_list_rows` ORDER BY; parity fixtures extended with a shared-`created_at`
  pair. See Phase 1 section (all `[x]`).
- **Phase 6 — search = mint/symbol everywhere.** `strategy_repo` positions/matched, `sql.rs::search_clause`,
  and in-RAM `search_match` all narrowed; `float8::text` drift + dead `rust_float_text` helper removed.
- **Phase 2 — full-merge column registry (chose (a)).** One `build_registry()` SSOT in `tokens.rs`;
  each `ColumnSpec` holds SQL (`sql_num`/`sql_text`/`sql_sort`) + in-RAM (`ram_num`/`ram_text`/`ram_sort`).
  Six match-arm fns + `NUMERIC_COLS`/`SORTABLE_COLS` are now registry lookups. **SQL↔in-RAM drift is now
  structurally impossible.** `grammar_parity_tests` rewritten to registry-invariant + frozen-key-set;
  `sql.rs` header updated.
- **Phase 7 (item 5 only):** fixed the stale `token_enrichment.rs` "mirrors 1:1" doc.

Verification run: `cargo test -p trading_core` 241 pass; registry + `sql` + `sim_query` tests pass; the
DB value-parity matrix (`token_repo::parity_tests`) passes against local DB (port 5555, `DATABASE_URL`
from `.env`); `cargo check -p live` + `-p lab` clean. No frontend code changed.

### DONE & VERIFIED this session — Phase 4 (backend; non-breaking)

- **Phase 4 — generalized in-memory evaluator.** Extracted `sim_query.rs`'s filter/sort/search/page
  logic into a reusable, grammar-agnostic `trading_core::api::table_eval::apply_table_request(rows,
  &TableRequest, resolve)` over `serde_json::Value` rows. `sim_query.rs` now owns only its `resolve`
  whitelist (frontend key → JSON field + `ColKind`) and is a thin adapter. Same op semantics as the SQL
  path (numeric-op-on-text drop, null-numeric exclude, contains-on-number → eq, mint/symbol search, raw
  `mint` ASC tiebreak). Generalized the sort to apply **all** resolved sort keys in order + the mint
  tail (was first-key-only — identical for today's single-key UI). Verified: `cargo test -p trading_core`
  (5 new `table_eval` tests) + `cargo test -p lab` (9 `sim_query` tests, all unchanged/green);
  `cargo check -p live` + `-p lab` + `-p trading_core` clean. The module is the shared home the Phase-8
  Rust↔TS conformance test targets, and the reference for Phase 5's TS evaluator.

### DONE & VERIFIED this session — Phase 3 (backend + frontend; BREAKING wire change)

- **Phase 3 — `/api/tokens` folded onto the unified `POST TableRequest`.** Faithful full fold (user
  choice): the global `TokenFilters` panel + DataTable per-column filters now share ONE `filters:
  {col → FilterSpec}` map, keyed by backend column key. Backend keeps the two proven eval engines
  untouched — `from_table_request` lowers each `FilterSpec` back onto the internal `f`/`col_filters`
  representation (the inverse of the frontend serializer), so zero behavior loss (DB parity green).
  Runtime-verified against the lab bin (every fold path). See Phase 3 section (all `[x]`) for the full
  per-field mapping.

### DONE this session — Phase 7 items 1–3 (frontend column-dedup; BUILD-verified, LIVE-eyeball pending)

- **Phase 7 items 1–3 — token-info columns collapsed to ONE source.** The ~26 enrichment columns now
  live in a single `tokenInfoColumns()` SSOT in `sharedTokenColumns.tsx` (render/sort/search/filter
  logic — the facts that can drift). Both consumers derive from it: `appendedTokenColumns` overlays
  `defaultVisible` (via `APPENDED_HIDDEN_KEYS`) for strategy tables; `tokenColumns.tsx` pulls each
  shared column by key through `tokenInfoColumnMap()` and applies only Tokens-page presentation (bespoke
  order, per-column widths via `TOKEN_COL_WIDTH`, plus its own identity/`token_age`/`lifetime`/fep-ratio
  columns). `defaultVisible`/width/order stay per-view (legitimate) — everything else is shared. Label
  drift fixed (`trade_count` is now "Token Trades" on the Tokens page too); `IxCountCell` (copy-on-click)
  moved to the shared module so strategy tables gain it. **Item 2:** the hand `init_buy`/`cu_limit`/
  `cu_price` columns deleted from tpsl1+tpsl2 `matchedColumns`; they now render from the shared
  `initial_buy`/`cu_limit`/`cu_price` enrichment columns (label/format/group change: "Init Buy (SOL)"
  group `params` → "Init Buy" AmountCell group `initial`; CU cols → `compute`). **Item 3:** `MATCHED_KEYS`
  cleaned in both (dropped the now-meaningless `init_buy`/`initial_buy`/`cu_limit`/`cu_price` suppression);
  the remaining identity keys are documentation no-ops (not enrichment keys). `POSITION_KEYS`/`SIM_KEYS`
  stay per-strategy — they gate genuinely different bespoke columns (SIM's own `ath_price` suppression is
  real and preserved). Verified: `npx tsc` clean (both trees) + `npm run build` clean.
  **STILL REQUIRED:** live visual check — `npm run dev` and eyeball the Tokens page + tpsl1/tpsl2
  Matched/Positions tables (I can't drive a browser here). Expected visible deltas: "Trades"→"Token
  Trades" header on Tokens page; matched Init Buy/CU columns relabelled + regrouped.

### OPEN — resume here

- **Phase 8**: Rust↔TS evaluator conformance test (`table_eval` ↔ `tableEval.ts`, shared fixtures);
  update `@arch/frontend.md` + `@arch/database.md` + `@plans/…`.
- **Phase 5 bullet 3** (deferred): retire `mergeTokenData`/`useGetTokensByMintsQuery` — needs a
  `getWalletHoldings` backend enrichment-join decision first (see Phase 5 section).

## Decisions locked (do not re-litigate)

- **One shared method** for *all* token tables, including Wallet Holdings and the Tokens page.
- **Free-text search = `mint` / `symbol` only**, everywhere. Remove `name`. Tokens-page global search
  narrows to mint/symbol too (this also **deletes the numeric-search formatting-drift bug** and drops
  creator-wallet search on the Tokens page — accepted).
- **Tokens default order = `created_at DESC, mint_address DESC` on BOTH engines** (deterministic /
  stable). Do **not** go created_at-only.
- **Simulated sort gets a `mint` tiebreaker.**
- **Evaluator split:** Simulated stays server-side (Rust, data off the wire); Wallet gets a small **TS**
  evaluator mirroring the same column-grammar spec. Wallet's live USD/liquidity columns are client-only
  and cannot be server-paged.

---

## Phase 1 — Contained correctness bug fixes (do first, ship independently) — DONE

- [x] **Simulated pagination stability.** `sim_query.rs::query` now sorts by the resolved primary key
      then a raw-`mint` ASC tail (`mint_raw`), matching the SQL tables' `t.mint_address ASC` tiebreak
      (case-sensitive base58). Runs even with no sort column, so the default view is stable too.
      Tests: `equal_sort_key_breaks_ties_by_mint_asc`, `no_sort_column_still_orders_by_mint`.
- [x] **Tokens default-order divergence.** `token_list_cache.rs` now has a single `newest_first`
      comparator (`created_at DESC, mint_address DESC`) used by BOTH `TokenListSnapshot::build` and the
      `merged_filtered` two-pointer merge, matching the SQL default exactly. `find_list_rows` (the DB
      base source) now `ORDER BY t.created_at DESC, t.mint_address DESC` so the two pre-sorted halves
      stay mergeable and the LIMIT boundary is deterministic. Test:
      `same_created_at_breaks_ties_by_mint_desc`.
- [x] **Mint-direction parity confirmed.** Default order = `mint DESC` on both engines; explicit-sort
      tiebreak = `mint ASC` on both (SQL `build_order` vs in-RAM `sort_refs` — already agreed).
- [x] **Parity fixtures extended for ORDERING.** `token_repo::parity_tests` gains PARITYf/PARITYg
      sharing a `created_at`; `in_ram_mints` now pre-sorts `created_at DESC, mint DESC`; an explicit
      assertion pins g→f adjacency under default order (catches both engines dropping the tiebreak
      identically). Ran green against local DB.
- [x] `cargo test -p lab` (sim) ✓, `cargo check -p live` + `-p lab` ✓, `cargo check -p trading_core` ✓.

## Phase 2 — Shared column-grammar registry (backend SSOT — highest leverage)

- [x] **Strategy-table whitelist → enrichment SSOT.** `strategy_repo` token sort/filter now delegates to
      `enrich_sort_sql`/`enrich_filter_sql` (commit `705809c`). Matched + Positions token columns share
      one whitelist.
- [x] **Market-cap formula deduped.** Single `MARKET_CAP_SQL` const in `token_enrichment.rs`, referenced
      by `sql.rs`, `tokens.rs`, `enrich_*` (staged) + `market_cap_ssot_tests`.
- [x] **Token-list grammar fully merged into ONE registry — chose (a).** `tokens.rs` now has a single
      `build_registry()` → `HashMap<key, ColumnSpec>` SSOT. Each `ColumnSpec` carries the SQL side
      (`sql_num`/`sql_text`/`sql_sort`) AND the in-RAM `TokenSummary` accessors (`ram_num`/`ram_text`/
      `ram_sort`). The old parallel pair — `NUMERIC_COLS`, `SORTABLE_COLS`, and the six match-arm
      functions (`sort_sql_expr`, `col_filter_number_sql`, `col_filter_text_sql`, `sort_key`,
      `col_filter_number`, `col_filter_text`) — are now thin registry lookups; `is_numeric_col` /
      `is_sortable_key` derive from it. Drift is now impossible by construction (not just guarded).
      Verified: registry-invariant tests (both engines defined per capability; frozen numeric/sortable
      key sets) + the DB value-parity matrix (`token_repo::parity_tests`) all green; `sql.rs` header
      updated to describe the registry SSOT.
- [x] **Fold in `sim_query.rs::resolve`** — done in **Phase 4**. Simulated reads JSON backtest-result
      rows (different shape + key universe: `entry_price`/`pnl_pct`/`exit_reason` plus the enrichment
      subset), so it doesn't share the `TokenSummary`-typed registry directly; the clean unification is
      the generalized Rust evaluator (`table_eval::apply_table_request`) — Simulated keeps only its own
      `resolve` grammar and delegates all evaluation to the shared, engine-agnostic machinery.
- [x] **sortable/filterable encoded as flags (single source).** The registry structure IS the encoding:
      `sql_sort.is_some()` ⇒ sortable, `sql_num.is_some()` ⇒ numeric-grammar-filterable; every other
      column stays substring-filterable via `sql_text`. The sortable-but-not-numeric cols (`created`,
      `mayhem_mode`, `cashback`, `migrated`, `dead`, `ath_timestamp`, `last_trade`, `ath_price`,
      `current_price`, …) are now one explicit entry each — no divergent lists left to reconcile.

## Phase 3 — One wire contract: migrate Tokens page onto `TableRequest` — DONE (faithful full fold)

**Per-field lowering** (`TokenQuery::from_table_request`, the inverse of the frontend
`tokenFiltersToSpecs` / `toTableRequest`): each wire `FilterSpec` (keyed by backend column key) routes to
the internal rep the two proven engines already consume —
- identity `symbol/name/mint/creator/create_tx` → `f[key]` single-field substring (contains).
- dates `created/last_trade/ath_timestamp` → `f[{key}_from/_to]` (between→both, gte/gt→from, lte/lt→to);
  a `contains` on a date col stays a per-column substring.
- `lifetime` → `f[life_min/life_max]` (minutes; dead-only stale-guard preserved).
- `ix_labels` → `f[ix_label]` (JSON ordered-exact vs text-substring grammar).
- flags `migrated/dead/mayhem_mode/cashback` → `f[…]="yes"/"no"` on eq; a non-tri value stays substring.
- every `is_numeric_col` key → `col_filters` raw predicate (between→`lo..hi`, gt→`>v`, …, contains→substring).
- unknown keys dropped. The numeric `col_filters` path already reproduces the panel's `opt_f64`/`range_f64`
  null-handling via the registry `sql_num`/`ram_num`, so no new eval code was needed.

- [x] Backend: `/api/tokens` is now `POST web::Json<TableRequest>` on BOTH bins (`live`/`lab` `list.rs`;
      routes flipped GET→POST in each `api/mod.rs`). `PaginationParams`/`from_params`/`parse_sort_levels`/
      `parse_col_filters` deleted; new `TokenQuery::from_table_request` **lowers** each `FilterSpec` back
      onto the internal `f` panel map / `col_filters` predicates the two proven eval engines (`matches`,
      `sql.rs`) already consume — so evaluation code is untouched and faithful by construction. The
      global `TokenFilters` panel folds into the same `filters: {col → FilterSpec}` map as the DataTable
      per-column filters (identity/date/lifetime/ix-label/flag → panel map; numeric → per-column
      predicate). Tokens-only `trackedOnly`/`swingRunId`/`swingChainLatencyMs` added to `TableRequest`;
      pagination keeps the 50k envelope (NOT `Page::bounds`' 1000). `ath_price`/`current_price` made
      numeric-filterable (frozen numeric-key test updated).
- [x] Frontend: `getTokensPage` now POSTs the `TableRequest` body via `toTableRequest` (DataTable
      view-state) merged with `tokenFiltersToSpecs` (the global panel → per-column `FilterSpec`, tz-
      normalized dates, panel-wins on collision) + the Tokens-only extras. Bespoke `f_*`/`cf`
      `URLSearchParams` builder deleted; dead `getTokens`/`useGetTokensQuery`/`TokensArgs` removed.
      `TokensPage.tsx`/`SwingDetectionPage.tsx` unchanged (same hook args).
- [x] SQL(live)/in-RAM(lab) split kept behind the contract, both fed by the Phase-2 registry. Verified:
      `trading_core` tokens tests (39) + DB `token_repo::parity_tests` green (SQL≡in-RAM across the
      filter/sort matrix); `cargo check -p live`/`-p lab` clean; `npm run build` clean.

## Phase 4 — Generalized in-memory evaluator (Rust) for Simulated — DONE

- [x] Extracted `sim_query.rs` filter/sort/search/page logic into a reusable
      `trading_core::api::table_eval::apply_table_request(rows, &TableRequest, resolve)` that embodies the
      registry `kind`/op semantics (Contains/Eq/Gt/Gte/Lt/Lte/Between; numeric-op-on-text → drop;
      Contains-on-number → eq; null numeric → exclude; mint/symbol search). Generic over the column
      grammar via a `ColResolver` (blanket-impl'd for `Fn(&str) -> Option<(&'static str, ColKind)>`), so
      the whitelist stays next to each table's row shape. Simulated calls it (thin adapter keeping only
      its `resolve`). Behavior identical to the SQL path — all 9 pre-existing `sim_query` tests pass
      unchanged; 5 new `table_eval` tests cover the generic machinery.
- [x] Folded the Phase-1 Simulated tiebreak into the evaluator, generalized to apply **all** resolved
      sort keys in order then the raw `mint` ASC tail (was first-key-only; identical for today's
      single-key UI, but the SSOT is now the general form the TS evaluator will mirror).

## Phase 5 — Wallet Holdings onto the shared method (client-executed TS evaluator) — 1/2/4 DONE, 3 DEFERRED

- [x] **TS evaluator `applyTableRequest(rows, body, resolve)`** written at `services/tableEval.ts`,
      mirroring the Rust `table_eval` op/sort/search semantics (mint/symbol search, numeric-op-on-text
      drop, null-numeric exclude, contains-on-number → eq, all resolved sort keys in order + raw-`mint`
      ASC tiebreak, `Page::bounds` paging). Generic over a `ColResolver`; `columnResolver(columns)`
      derives the grammar straight from a table's `ColumnDef[]` (numeric iff `filterNumber`; sort via
      `sortValue` with the same `compareSort` the client `DataTable` used) — so it's the single client
      evaluator, reusable for any future client-resident token table, with **zero sort regression**.
- [x] **Wallet Holdings converted to `DataTable` `serverSide` mode fed by the evaluator, locally.**
      `MyWalletPage` holds the emitted `TableQuery`, serializes it via `toTableRequest`, and evaluates
      in-browser (`applyTableRequest(rows, …, columnResolver(columns))`) → `{items,total}` passed as
      `rows`/`serverTotal`. Behavior deltas (all intended, matching the unified grammar the other tables
      already use): free-text search narrows to **mint/symbol only** (was all-columns); a plain-number
      filter in a numeric column now means `eq` (was substring); `!=` maps to `eq`.
- [x] **Item 4 — live-only columns:** handled uniformly by `columnResolver` (value/price/liquidity/…
      sort + filter client-side). No `client_only` flag added: the whole Wallet evaluation is
      client-resident, so there is **no SQL builder to exclude them from** — a flag would be dead
      metadata. (Revisit only if these columns are ever fed to `toTableRequest` for a *server* table.)
- [ ] **DEFERRED — retire `mergeTokenData` + `useGetTokensByMintsQuery`.** These are the Wallet's
      enrichment DATA source (RPC balances × token-DB batch, merged client-side). Deleting them strips
      creator/ATH/volume/mcap/CU/ix data from the Wallet unless the `getWalletHoldings` backend endpoint
      starts LEFT JOIN-ing `tokens_info` (a `live`-bin handler/repo change — "the same way the other
      tables receive it" = server-joined). That's a separate decision + backend change; the evaluator
      + serverSide conversion above don't require it and don't regress. `mergeTokenData` kept for now.

## Phase 6 — Search = mint/symbol everywhere — DONE

- [x] `strategy_repo.rs`: dropped `t.name ILIKE` from both the positions search (`push_position_where`)
      and matched search (`push_token_where`) → now `sp.mint`/`t.mint_address` + `t.symbol` only. Doc
      comments updated.
- [x] Tokens-page global search: `sql.rs::search_clause` narrowed to `t.mint_address` + `t.symbol`
      (deleted the date/numeric/`name`/`creator_wallet` ORs and the now-unused `rust_float_text` helper);
      the in-RAM `search_match` collapsed to the same two-field check. Removes the `float8::text` vs Rust
      `to_string` numeric-substring drift. Test `global_search_is_mint_and_symbol_only`; DB parity's
      "global search AB" case still green. (Simulated already mint/symbol.)

## Phase 7 — Frontend column-duplication cleanup (by meaning) — items 1–3 DONE, 4 DEFERRED

- [x] **Collapsed the two hand-maintained copies into ONE `tokenInfoColumns()` SSOT** in
      `sharedTokenColumns.tsx`. `ALL_TOKEN_COLS` is gone; `appendedTokenColumns` derives from the SSOT +
      overlays `defaultVisible` (`APPENDED_HIDDEN_KEYS`). `tokenColumns.tsx` consumes the shared columns
      by key via `tokenInfoColumnMap()`, applying only Tokens-page presentation (order + `TOKEN_COL_WIDTH`
      widths) and keeping its own `symbol`/`name`/`mint`/`token_age`/`lifetime`/`ath_fep_ratio`/
      `current_fep_ratio`. Label drift fixed (`trade_count` → "Token Trades" everywhere); `IxCountCell`
      moved to the shared module. `npx tsc` + `npm run build` clean. **Live eyeball pending** (`npm run dev`).
- [x] **Removed the `init_buy`/`cu_limit`/`cu_price` hand-column aliases** from tpsl1+tpsl2
      `matchedColumns` — they now render from the shared `initial_buy`/`cu_limit`/`cu_price` enrichment
      columns (label/format/group changes as expected; verify visually).
- [x] **Reconciled `MATCHED_KEYS`** in both tpsl bins — dropped the now-dead `init_buy`/`initial_buy`/
      `cu_limit`/`cu_price` suppression; remaining identity keys are documentation no-ops. `POSITION_KEYS`/
      `SIM_KEYS` intentionally stay per-strategy (they gate genuinely different bespoke columns; SIM's
      real `ath_price` suppression preserved). The `mint` divergence noted earlier was a no-op, unchanged.
- [ ] **DEFERRED (doc).** Document the by-meaning overlaps left by design (`current_price` SOL/DB vs
      `price_usd` USD/live in Wallet — the latter arrives in Phase 5; `created`/`token_created_at`;
      `ath_price` row-owned + enrichment). Best written alongside the Phase 5 Wallet work.
- [x] **Fixed stale doc:** `token_enrichment.rs` header now says the JSON `TokenEnrichment` flatten
      mirrors `TOKEN_ENRICH_FIELDS` 1:1 while the `TokenEnrichmentRow` is a **superset** carrying the
      row-owned `symbol`/`created_at`/`ath_price`.

## Phase 8 — Tests / CI / docs

- [x] Non-DB CI grammar guard exists (`grammar_parity_tests` in tokens.rs — key coverage) and the DB
      value-parity test now auto-runs with `DATABASE_URL`. NOTE: coverage is **key-set only** — extend
      it to sort/tiebreak/ordering equivalence once the Phase-2 registry lands (see Phase 1 fixture
      todo, which covers the default-order ordering case).
- [ ] Add a Rust↔TS evaluator conformance check (shared fixture set, same expected ordering) so the two
      in-memory evaluators (Simulated in Rust, Wallet in TS) can't drift.
- [ ] Update docs per CLAUDE.md "Definition of done":
      - `@arch/frontend.md` (unified contract now covers Tokens + Wallet; `mergeTokenData` removed)
      - `@arch/database.md` (search = mint/symbol; shared registry as filter/sort SSOT)
      - `@plans/…` deep-dive for the registry + evaluator design.

## Definition of done (per CLAUDE.md)

- [ ] `cargo check -p live` + `cargo check -p lab` + `-p trading_core` clean; clippy on touched code;
      tests where logic changed.
- [ ] `npm run build` clean (tsc checks both trees); no extra re-render on SOL/USD tick or live-trade
      stream.
- [ ] No secrets in code; stayed in owning crates; docs updated across all affected tiers.
