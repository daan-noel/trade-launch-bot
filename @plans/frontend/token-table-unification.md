# Token-table unification — column SSOT + shared evaluators

Deep-dive reference for the "one shared method for every token table" work (see the
top-level `token-tables-unification-plan.md` for the phase-by-phase history). Seven
token-bearing tables — Tokens page · Positions · Paper · Old-run history · Matched ·
Simulated · Wallet Holdings — now share **one column grammar** and **one request
contract**, with three evaluation backends held at parity.

## The one request contract

Every table pages/sorts/filters/searches over the same body, `TableRequest`
(`trading_core::api::table_query` ↔ TS `services/tableRequest.ts::TableRequestBody`):
`{ pagination, sorting[], search, filters:{col→FilterSpec}, range?, trackedOnly?, … }`.
`FilterSpec = {op, val}` / `{op:'between',min,max}`, `FilterOp ∈
contains|eq|gt|gte|lt|lte|between`. The `DataTable` emits raw view-state
(`TableQuery`); `toTableRequest(query, numericCols)` serializes it — a numeric column
(one declaring `filterNumber`, set via `numericColKeys`) turns `>5`/`1..10` into a
structured op; everything else is `{op:'contains'}`.

Locked semantics (identical across all backends):
- free-text **search = mint / symbol only** (Phase 6; `SEARCH_FIELDS`/`SEARCH_KEYS`);
- a **numeric op on a text column** is dropped (not a constraint);
- a **null numeric field** can't satisfy a numeric predicate;
- **`contains` on a number** degrades to equality;
- ties break by **raw (case-sensitive) `mint` ASC** so paging is stable;
- `!=` has no server op → maps to `eq` (the legacy client `parseNumericPredicate`
  keeps a real `!=` for any table still filtering purely in-browser).

## The three evaluation backends

| Backend | Where | Feeds |
| --- | --- | --- |
| **SQL** | `handlers::tokens::sql` (live) + `strategy_repo` (positions/matched) | Tokens page (live), Positions, Paper, Matched, Old-run history |
| **Rust in-RAM** | `trading_core::api::table_eval::apply_table_request` | Simulated (lab; finished-backtest rows already resident), Tokens page (lab in-RAM engine via `TokenQuery`) |
| **TS in-RAM** | `services/tableEval.ts::applyTableRequest` | Wallet Holdings (client-side on-chain scan) |

The two in-RAM evaluators are structurally the same (`search → filter → sort → page`
over resident rows), generic over a **`ColResolver`** that maps a frontend column key
to `{field/accessor, kind}` — the grammar stays next to each table's row shape, the
machinery lives once. The SQL backend reproduces the same op semantics via
`push_filter_predicate` (bound predicates, illegal pairings dropped).

### Rust ↔ TS conformance (drift guard)

`tableEval.fixtures.json` (rows + cases + expected `mint` orderings) is consumed by
**both** `table_eval::conformance_shared_fixtures` (Rust, `include_str!`) and
`tableEval.conformance.test.ts` (vitest, `npm test`). Fixture text values are
lowercase ASCII so TS `localeCompare` and Rust lowercased-bytewise text sort agree.
Add a case there whenever either evaluator's semantics change.

## The one column grammar (frontend)

`sharedTokenColumns.tsx::tokenInfoColumns()` defines the ~26 enrichment columns
**once** — the render/sort/search/filter logic (the facts that can drift). Consumers:
- `appendedTokenColumns(existingKeys)` — strategy + wallet tables; overlays
  `defaultVisible` from `APPENDED_HIDDEN_KEYS`, drops keys the table already owns.
- `tokenColumns.tsx` (Tokens page) — pulls each shared column by key via
  `tokenInfoColumnMap()` and adds **only presentation** (bespoke order +
  `TOKEN_COL_WIDTH` widths) plus Tokens-only columns (identity, `token_age`,
  `lifetime`, `ath_fep_ratio`, `current_fep_ratio`).

`defaultVisible` / width / order legitimately differ per view and stay with the
consumer; the render/sort/filter facts are shared. Matched tables no longer hand-roll
`init_buy`/`cu_limit`/`cu_price` — those render from the shared columns.

For a **client** table (Wallet), `columnResolver(columns)` builds the `ColResolver`
straight from the `ColumnDef[]`: numeric iff the column declares `filterNumber`; sort
via `sortValue` with the same `compareSort` the client `DataTable` used (so numeric
columns keep sorting numerically even when they aren't numeric-*filterable*, e.g. the
live-only `value_usd`/`price_usd`/`liquidity`). No SQL builder is involved, so those
live-only columns need no `client_only` flag — the whole evaluation is client-resident.

## Deferred

- **Wallet enrichment source.** `mergeTokenData` + `useGetTokensByMintsQuery` still
  join token-DB enrichment onto the RPC-scanned holdings client-side. Retiring them
  ("attach enrichment the same way the other tables receive it") requires the
  `getWalletHoldings` `live` endpoint to `LEFT JOIN tokens_info` — a backend decision,
  not yet made. The evaluator/serverSide conversion above does not depend on it.
