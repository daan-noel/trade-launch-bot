# Token-table unification — one wrapper, one column SSOT, one request contract

Deep-dive reference for the "one shared method for every token table" work. **Every**
token-bearing table — Tokens page · Positions (current/history) · Paper · Matched ·
Simulated · Sweep drill-in · Trader Analysis · **Wallet Holdings** — now renders
through the single `TokenTable` wrapper over one **column grammar** + one **request
contract**, with two evaluation backends (SQL + Rust in-RAM) held at parity. The
working checklist that produced this (`token-table-features-plan.md`) has been retired;
this is the permanent record.

## `TokenTable` — the one wrapper (`components/tokens/TokenTable.tsx`)

Sits over the token-agnostic `DataTable` primitive (one-way dep `tokens/` → `table/`,
asserted by `DataTable.boundary.test.ts`) and owns the "token recipe":

1. **Append the shared token-info columns** (`appendedTokenColumns(existingKeys)`, SSOT)
   after the caller's bespoke columns — callers export only their own columns + an
   `existingKeys` set (`POSITION_KEYS`/`SIM_KEYS`); a table that owns its
   full layout passes `ALL_TOKEN_INFO_KEYS` to append nothing (Tokens page, Trader
   Analysis, Sweep drill-in, Wallet).
2. **Own the table wiring** in one of two modes:
   - **server** (`serverSide` + `serverTotal`/`onQueryChange`/`resetKey`/`loading`) —
     rows arrive backend-enriched one page at a time; the emitted `TableQuery` is handed
     to the caller's hook. Feeds Positions/Paper/Matched/Sim/Wallet/**Tokens page**.
   - **client** (default) — rows are the full already-enriched set; `DataTable`'s **own**
     client paging/sort/filter/search runs in-browser (no separate evaluator). Feeds
     tables with no backend paging endpoint: **Trader Analysis**, **Sweep drill-in**.
3. **Mint accessor** — every token-data row keys its mint under the one canonical field
   `mint_address` (SSOT across DB → wire → JS: `tokens.mint_address`, `trades.mint_address`,
   `strategy_positions.mint_address`, and every DTO/grammar key). The accessor is fixed
   internally to read `.mint_address` — there is no `mintOf` prop for a caller to pass.
   It drives the charts grid, the default `rowKey`, and the client mint-set pre-filter, and
   matches the server mint-set column key.

Two opt-in features live here so every table gets them once:
- **`mintSetFilter`** — a `<MintSetInput>` paste box (validated/deduped/capped at
  `MAX_MINT_SET` = 500). Server mode folds it into `structuredFilters` as an `in` op on
  `mint_address`; client mode applies it as a plain row pre-filter.
- **`charts`** — a toggle (persisted per `tableId`) rendering `<TokenChartsGrid>`
  (lazy-mounted, **current page only**, with `renderChartCardExtra`/`titleOf`/
  `highlightWallet` slots) below the table, fed by the table's intercepted
  `onVisibleRowsChange`. With `chartsGroupByMint`, the extra renderer also gets
  the mint's group rows. Position tables use shared `PositionChartCardExtra`;
  Trader Analysis uses `TraderChartCardExtra` (`charts` + `chartsDefaultOn` +
  wallet spotlight).

## The one request contract

Every table pages/sorts/filters/searches over the same body, `TableRequest`
(`trading_core::api::table_query` ↔ TS `services/tableRequest.ts::TableRequestBody`):
`{ pagination, sorting[], search, filters:{col→FilterSpec}, range?, trackedOnly?, … }`.
`FilterSpec = {op,val}` / `{op:'between',min,max}` / `{op:'in',val:[]}`, `FilterOp ∈
contains|eq|in|gt|gte|lt|lte|between`. `DataTable` emits raw view-state (`TableQuery`);
`toTableRequest(query, numericCols)` serializes it (numeric column → structured op,
else `{op:'contains'}`) and merges any wrapper-injected `structuredFilters`.

Locked semantics (identical across backends):
- free-text **search = mint / symbol only** (`SEARCH_FIELDS`/`SEARCH_KEYS`);
- a **numeric op on a text column** is dropped; a **null numeric field** can't satisfy a
  numeric predicate; **`contains` on a number** → equality;
- **`in`** = set membership on a text column (operand array in `val`);
- ties break by **raw (case-sensitive) `mint` ASC** so paging is stable;
- `!=` has no server op → maps to `eq`.

## The two evaluation backends (the TS twin retired)

| Backend | Where | Feeds |
| --- | --- | --- |
| **SQL** | `handlers::tokens::sql` (live) + `strategy_repo` (positions/matched) | Tokens page (live), Positions, Paper, Matched, Old-run history |
| **Rust in-RAM** | `trading_core::api::table_eval::apply_table_request` | Simulated (lab), Tokens page (lab in-RAM engine via `TokenQuery`), **Wallet Holdings** (live composed scan) |

The in-RAM evaluator is generic over a **`ColResolver`** (frontend key → `{field,kind}`);
the shared **enrichment** half of that grammar is the SSOT
`table_eval::resolve_token_enrichment_key`, which the lab Simulated resolver (`sim_query`)
and the live Holdings resolver (`portfolio.rs::holdings_resolve`) both delegate to. The
SQL backend reproduces the same op semantics via bound predicates.

There is **no client-side TS evaluator** — no TS table evaluator, column resolver or row
merger; every such table is server-side, so any of the three would be a second
implementation of the Rust grammar. The golden fixture `tableEval.fixtures.json` + Rust
`table_eval::conformance_shared_fixtures` are **Rust-only** (do not delete the JSON — the
Rust test `include_str!`s it).

### Wallet Holdings server-side (the last client table)

`GET /api/portfolio/holdings` stays for the Home widgets (full list). The Holdings
**table** pages via `POST /api/portfolio/holdings/query[?fresh=true]` (+ `X-Total-Count`)
and a `POST …/summary` roll-up. A short-TTL `HoldingsCache` (`HOLDINGS_TTL = 8s`) on
`DeployState` warms the composed wallet scan so paging/sort/filter cost **one** scan per
window (no new hot-path RPC); `?fresh=true` busts it after a confirmed trade. Scan-time
live marks (`value_usd`/`price_usd`/`liquidity`/`24h`) **are** server-sortable/filterable
(so dust hiding is a real `value_usd ≥ $1` filter and the summary agrees); the 20s client
price-poll still overlays fresher *display* values on the current page. `managed_by` is a
nested object → display-only.

## The one column grammar (frontend)

`sharedTokenColumns.tsx::tokenInfoColumns()` defines the ~26 enrichment columns **once**
(render/sort/search/filter logic). Consumers: `appendedTokenColumns(existingKeys)`
(overlays `defaultVisible` from `APPENDED_HIDDEN_KEYS`, drops owned keys) and the Tokens
page's `tokenColumns.tsx` (pulls each shared column by key via `tokenInfoColumnMap()`,
adding only presentation + Tokens-only columns). `numericColKeys`/`tokenNumericColKeys`
derive the numeric-filter key set (base columns + appended token-info numerics).

### Intentional by-meaning overlaps (kept by design — do NOT merge)

The unification collapsed every *accidental* same-meaning duplicate into one SSOT. A few
pairs **look** redundant but carry genuinely different data (different unit, source, or
freshness) and are kept separate on purpose. Merging any of these is a correctness bug,
not a cleanup — they are listed here so a future pass doesn't "dedupe" them.

| Pair | Column A | Column B | Why both exist |
| --- | --- | --- | --- |
| **Price** | `current_price` — **SOL**, DB enrichment (`tokens_info`), the canonical curve→pool spot (`CurrentPriceCell sol=`) | `price_usd` — **USD**, live Jupiter mark, **Wallet-only** | Different unit *and* source: A is the persisted SOL spot every table shows; B is the live USD mark that only the Wallet composes (client price-poll overlays it on the current page). Neither can substitute for the other. |
| **Created-at** | `created` — renders the DB `created_at` (our ingest's first-seen time), on every enriched table | `token_created_at` — live Jupiter-reported creation time, **Wallet-only** | Same concept, two independent sources with different provenance/precision; the Wallet shows the live one alongside marks, everything else shows the ingest one. |
| **ATH price** | `ath_price` in `tokenInfoColumns()` — shared enrichment column | `ath_price` **row-owned** on `SimulatedTokenResult` (baked per backtest run by `lab::strategies::token_enrich`) | Same key, but Simulated carries its own row-owned copy (frozen at backtest time) and suppresses the appended enrichment one via `SIM_KEYS` — so the sim row shows the run's ATH, not today's. The suppression is deliberate (see `SIM_KEYS`). |

Rule of thumb: a `*_usd` / live-mark / row-owned-snapshot column is **never** the SSOT
duplicate of its SOL / DB-enrichment namesake — the unit/source/freshness axis is the
real distinction the grammar preserves.

## Tokens-list mint-set (`in`-op on `mint`)

The Tokens page's `mintSetFilter` required the parity-guarded tokens grammar to honor a
set: `TokenQuery::from_table_request` lifts an `in` op on `mint` into a `mint_in` list
(capped at `MAX_FILTER_IN_VALUES`), honored identically by **both** engines — SQL
(`t.mint_address = ANY($n)`) and in-RAM (`TokenQuery::matches`) — with exact,
case-sensitive membership. `getTokensPage` threads `structuredFilters` into the request
body. No-DB unit tests pin the lowering + SQL; `token_repo::parity_tests` covers engine
parity.

## Outstanding (app-gated — not code)

Everything above is code-complete, typechecked, built, and unit-tested. What remains is
**runtime verification** in the running app (needs the app + Solana RPC), which no
automated check here can stand in for:
- **Wallet**: paging/sort/filter/search server-side; live value/price still tick on the
  current page; dust filter + summary bar agree; Buy/Sell/confirm + bot-managed
  interlock intact; no extra re-render on the 20s price tick.
- **Migrated tables** (Tokens page, Trader Analysis, Sweep drill-in, Positions/Matched/
  Sim/Paper): sort/filter/search/paging/col-toggle; mint-set paste narrows correctly;
  charts lazy-mount on scroll (current page only) and unmount on toggle-off.

Optional (explicitly deferred, may stay unbuilt): the mint-set "clear other filters"
affordance (needs a `DataTable` clear-filters handle) and a "N not found" note.
