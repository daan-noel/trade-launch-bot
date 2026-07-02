# Tokens DB-Pagination Plan — remove the 25K cap, split cache strategy by bin

## Goal

Tokens tables must page the **whole** token universe (currently ~39K, expected 100K+),
not a 25K-capped in-memory snapshot. The two bins want **opposite** cache strategies:

| Bin | Box | Token-list source | In-RAM cache holds | Live updates |
| --- | --- | --- | --- | --- |
| `live` | EC2, 4 GB, RAM-constrained | **DB, paged in SQL** (`WHERE`/`ORDER BY`/`LIMIT`/`OFFSET` + `COUNT(*)`) | **tracking tokens only** (hot-path strategy eval) | SSE, tracked mints only |
| `lab` | workstation, big RAM, speed-critical | **full in-RAM snapshot** (current `build_tokens_list` engine, unchanged) | **full universe** (fast repeated analysis) | n/a (analysis-driven) |

Root cause of the cap: `run_token_list_db_refresh` (token_list_cache.rs:220) loads the DB
base via `find_list_rows(SEED_TOKEN_LIMIT=25_000, since)`; `TokenListSnapshot` is that base
overlaid with the live cache, and every `/api/tokens` page is sliced from it. So the pager's
`total` can never exceed 25K.

**Design principle: one filter/sort grammar, two execution backends.** The filter semantics
(what `f_ath_from`, `cf`, tri-flags, ix-label matching, FEP ratios, lamports→SOL mean) stay
**single-source in `trading_core`**. `lab` keeps executing them in RAM (`TokenQuery::matches` /
`sort_refs`). `live` gets a new SQL backend (`build_tokens_query_sql`) that produces the
**same** row set. Parity is enforced by tests, not by hope.

Keep `SEED_ACTIVITY_WINDOW_DAYS = 7` everywhere. It stays the live cache seed window; it does
**not** bound the new DB-paged list (the list pages the full `tokens` table by `created_at DESC`).

---

## Stage 0 — Freeze the contract (no behavior change)

The wire contract is already correct and stays fixed — frontend needs **zero** query-builder
changes:
- Request: `limit`, `offset`, `search`, `sort=col:dir,…`, `cf=key:expr;…`, `f_*` global
  filters, `tracked_only`, (lab-only) `swing_run_id` / `swing_chain_latency_ms`.
- Response: `{ total, tracked, items }` where `total` = filtered count over the universe,
  `tracked` = filtered count over the live-tracked subset.

`live` must keep emitting `tracked` so the "tracked vs all" badge keeps working. On `live` the
tracked subset is the in-RAM cache (small), so `tracked` is computed in RAM against the cache —
NOT via SQL. Only `total` + the `items` page come from SQL.

---

## Stage 1 — SQL filter/sort backend in `trading_core` (the bulk of the work)

New module `trading_core::api::handlers::tokens::sql` (sibling of `tokens.rs`), reusing the
existing `PaginationParams` / `TokenQuery` parse layer verbatim (no new parse grammar — same
`from_params`, same `parse_sort_levels`, same `parse_col_filters`).

### 1a. Query builder — `build_where_and_order(q: &TokenQuery) -> (String, Vec<SqlArg>)`

Emit a parameterized `WHERE` + `ORDER BY` fragment over `tokens t LEFT JOIN tokens_info i`.
Column→SQL-expression map, matching the SELECT projection already in `find_list_rows`:

| Filter / column | SQL expression |
| --- | --- |
| symbol / name / mint / creator / create_tx (`text_match`) | `LOWER(col) LIKE '%'‖LOWER($n)‖'%'` |
| created / last_trade / ath datetime ranges (`date_in_range`) | `t.created_at >= $lo AND <= $hi` (parse `f_*` via `parse_dt` → bind `timestamptz`) |
| lifetime minutes (dead-only, `lifetime_minutes`) | see **1c** — needs the stale-guard predicate |
| ath_fep / cur_fep ratios | `(i.ath_price / (t.initial_buy_sol/1e9 / NULLIF(t.initial_supply_token,0)))` — guard `initial_buy_sol>0`; null ratio ⇒ excluded when a bound is set (mirrors `opt_f64` None→false) |
| ath_price / current_price / market_cap / first_slot_* / init_buy | direct `i.*` / `t.*` numeric range; `opt_f64` semantics: NULL fails when a bound is set |
| volume / trade_count / ix_count | `range_f64` (present-or-0 style; these are non-null in practice — match `matches()` exactly) |
| max_sol_cost / spendable_sol_in | stored lamports; filter in SOL: `(col::float8/1e9)` |
| init_supply / token_amount / min_tokens_out / cu_limit / cu_price | numeric range, `opt_f64` |
| ix_label (`ix_label_matches`) | see **1d** |
| migrated / dead / mayhem / cashback (`tri_match`) | `= true` / `= false` / (unset ⇒ no clause) |
| global `search` | see **1e** |
| per-column `cf` (numeric grammar or substring) | see **1f** |

`ORDER BY`: one term per sort level in order, then `t.mint_address` as the stable tiebreak.
**Null-sorts-last must match `cmp_keys` exactly**: emit `col ASC NULLS LAST` / `col DESC NULLS
LAST` for every level (Postgres default is NULLS LAST for ASC, NULLS FIRST for DESC — so DESC
needs an explicit `NULLS LAST`). String sorts use `LOWER(col)` to match the case-insensitive
in-RAM compare. Empty sort levels ⇒ default `ORDER BY t.created_at DESC, t.mint_address DESC`
(the snapshot's implicit newest-first).

Use a bind-arg accumulator (push a `SqlArg` enum: `Str`/`F64`/`I64`/`Ts`/`Bool`) and reference
`$n` positionally so nothing is string-interpolated from user input (**no SQL injection** — the
only interpolated tokens are the fixed column expressions we choose, never `q` values).

### 1b. Paged repo methods — extend `token_repo.rs`

Two new methods that take the built fragments:

```rust
// items page — same SELECT projection as find_list_rows, with dynamic WHERE/ORDER + LIMIT/OFFSET
pub async fn find_list_page(&self, where_sql: &str, order_sql: &str,
    args: &[SqlArg], limit: i64, offset: i64) -> anyhow::Result<Vec<TokenListRow>>

// filtered total over the whole universe
pub async fn count_list(&self, where_sql: &str, args: &[SqlArg]) -> anyhow::Result<i64>
```

Bind the `SqlArg` vec in order, then `LIMIT`/`OFFSET` (page) as the trailing binds. Reuse the
exact `SELECT t.…, i.…` projection block from `find_list_rows` so `TokenListRow`/`TokenSummary`
mapping is unchanged. **No `since` floor** — the list is the whole table now (the window only
governs the live cache seed). `ORDER BY … , mint_address` keeps `LIMIT/OFFSET` deterministic
across pages.

### 1c. Lifetime filter (dead-only stale guard)

`lifetime_minutes` returns `None` (exempt) when the token traded within `LIFETIME_STALE_MS`
(1 h) of `now`. Port as: only apply the lifetime range when
`i.last_trade_at < now() - interval '1 hour'`; else the row passes (not excluded). Value =
`COALESCE(i.lifetime_secs, EXTRACT(EPOCH FROM (i.last_trade_at - t.created_at)))/60`.

### 1d. ix_label matching

`parse_ix_label_filter` has JSON-array (exact set-equality) and text (any-substring) modes over
`t.ix_labels` (jsonb). Simplest faithful port: keep the **parser in Rust**, then emit SQL:
- Text mode: `EXISTS (SELECT 1 FROM jsonb_array_elements_text(labels) e WHERE LOWER(e) LIKE …)`
  for each needle, OR'd.
- JSON mode (set-equality by count + elementwise): compare `jsonb_array_length` and an ordered
  elementwise match. If the SQL gets gnarly, fall back to fetching `ix_labels` isn't an
  option on the paged path — instead encode set-equality as
  `(SELECT array_agg(LOWER(e) ORDER BY ord) FROM jsonb_array_elements_text(labels) WITH ORDINALITY x(e,ord)) = $needles`.
  Handle the `{instructions:[…]}` object shape via `COALESCE(labels->'instructions', labels)`.

### 1e. Global search

`search_match` scans symbol/name/mint/creator/create_tx (substring) + stringified dates and
numerics. **Decision: FULL PARITY.** SQL ORs across `LOWER(text_col) LIKE '%q%'` plus the
date/number fields. To match the in-RAM formatting exactly:
- Dates use `to_rfc3339()` in Rust → emit `to_char(ts AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS+00:00')`
  and verify against Rust output in the parity test; adjust the format literal until identical.
- Numerics use Rust `f64::to_string()` / integer `to_string()`. SQL `::text` on `float8`/`int8`
  differs (trailing zeros, exponent). The parity test asserts equality on a fixture; where the
  representation diverges, cast/format in SQL to match Rust (e.g. trim, or compare via a
  normalized form). This is the cost the user accepted for byte-for-byte search parity.

The parity integration test (Stage 5) is the enforcement mechanism — global-search rows must
match between the two engines on the fixture.

### 1f. Per-column `cf` filters

Reuse `parse_numeric_predicate` (Rust) to turn each `cf` expr into `NumPred`, then emit the
matching SQL comparison against `col_filter_number`'s column expression (same expr table as
1a). Non-numeric columns (or unparseable exprs on numeric cols) → substring `LIKE` against the
column's text projection (`col_filter_text` equivalent).

### 1g. Swing-chain columns are lab-only

`swing_pairs` / `max_seq_pairs` / `chain_count` are computed from in-RAM swing runs, not DB
columns. On `live` there are no swing runs (already the documented no-data fallback). In the SQL
`ORDER BY`, a swing sort level is **dropped** (falls back to default order) — matching live's
current "sort matching rows last / no run" behavior. `lab` keeps computing them in RAM.

---

## Stage 2 — Rewire `live`'s `/api/tokens` handler to the SQL path

`live/src/api/handlers/tokens/list.rs`:
- Build `TokenQuery::from_params` (unchanged).
- `build_where_and_order(&q)` → `(where_sql, order_sql, args)`.
- `total` = `count_list(where_sql, args)`; `items` = `find_list_page(…, limit, offset)`.
- `tracked` = count the **in-RAM cache** rows that satisfy `q.matches(…)` (small set; keep the
  existing `snapshot.tracked_filtered_count` path, but sourced from the tracking-only cache).
- `tracked_only=true` ⇒ serve from the in-RAM cache via the existing `build_tokens_list` path
  (tracked subset is small and RAM-resident; no need to hit SQL). So live keeps BOTH paths:
  SQL for the full list, in-RAM for the tracked-only view.
- ETag: hash the serialized page bytes exactly as today.
- Run the SQL on the async pool (`.await`), not `web::block` (it's I/O, not CPU).

**Retire the live DB-base refresher for the list:** `run_token_list_db_refresh` +
`TokenListCache.db_base` are no longer needed to back the full list on `live` (SQL is the
universe). The live `TokenListCache` collapses to a **tracking-only** snapshot (just `live_rows`
for the `tracked` count + `tracked_only` view; drop `db_rows`/`db_base`/`set_db_base`/the
two-pointer merge on the live composition). Confirm no other live consumer reads the merged base.

## Stage 3 — `lab` keeps the RAM snapshot (minimal change)

`lab/src/api/handlers/tokens/list.rs` is unchanged in shape: it still calls `build_tokens_list`
over the full in-RAM `TokenListSnapshot` (with swing stats). The only question is whether lab's
snapshot should also drop the 25K cap so its in-RAM universe is complete:
- **Yes** — lab has big RAM and wants the whole set. Raise/remove the cap **on the lab
  composition only**: lab's `run_token_list_db_refresh` calls `find_list_rows` with a lab-local
  limit (e.g. `i64::MAX` / a new `LAB_TOKEN_LIST_LIMIT`) and, ideally, no 7-day `since` floor for
  the list base (or a wider lab window). `live` keeps `SEED_TOKEN_LIMIT` for its *tracking* seed.
- This is the one place the constant diverges by bin: **`SEED_TOKEN_LIMIT` stays the live
  tracking-seed cap; lab's list base is uncapped.** Keep the constant as-is; branch the value at
  the lab call site rather than raising the shared const (guardrail: never raise it on server).

## Stage 4 — Indexes (EC2 IO cost — justified, minimal)

The SQL paged path needs the sort/filter columns index-servable. Add to `0001_init.sql` (or a
new migration) only what the sortable/filterable columns require:
- `tokens(created_at DESC, mint_address DESC)` — the default order + tiebreak (likely exists as
  `idx_tokens_created_at`; extend to include `mint_address` for the deterministic page).
- `tokens_info(trade_count)`, `(volume)`, `(market_cap-expr)`, `(current_price)`, `(ath_price)`,
  `(last_trade_at)`, `(ath_timestamp)`, `(first_slot_buy_sol)`, `(first_slot_sell_sol)` — the
  numeric sort columns users actually sort by. Add lazily: index the columns the UI exposes as
  default sorts first; note the rest as follow-ups rather than indexing all 25.
- **`market_cap` is a computed expr** (`current_price * initial_supply_token`) across the join —
  can't be a plain index; sorting it does a sort-node. Acceptable (local-heavy; note it).
- **Deep `OFFSET`** on 100K+ rows scan-and-discards. Acceptable for now (analysis-grade, not hot
  path); note keyset/seek paging as the future upgrade if deep pages get used. Do **not** build
  keyset now.

## Stage 5 — Parity tests (the safety net)

In `trading_core` tests, assert the SQL WHERE produces the **same row set** as `TokenQuery::matches`
on a fixture:
- Unit: for a representative `PaginationParams` (each filter family — text, date range, FEP,
  lamports, tri-flag, ix-label text + json, per-col numeric grammar, multi-key sort), build the
  SQL and assert the emitted fragment + bind args match a golden expectation.
- Integration (`#[ignore]`, needs `DATABASE_URL`): seed a small `tokens`/`tokens_info` fixture,
  run both engines (in-RAM `matches`/`sort_refs` over the same fixture vs the SQL page), assert
  identical ordered mint lists across a matrix of filter/sort combos. This is where full-parity
  is *proven*, per the user's "full parity in SQL" requirement.

## Stage 6 — Frontend

**No query-builder change** (`getTokensPage` already sends the exact params). Verify:
- SSE trade patching still targets tracked mints only (already the case in `TokensPage`).
- No extra re-render on SOL/USD tick or live-trade stream (Definition of Done).
- Swing Detection page (`TOKENS_LIST_LIMIT` bulk pull) still hits `lab`'s in-RAM path — unaffected.

## Definition of done

- `cargo check -p live` + `cargo check -p lab` + `cargo check -p trading_core` clean; clippy on
  touched code; parity tests pass (unit; integration behind `--ignored`).
- `npm run build` clean; no extra re-render on tick/stream.
- Docs: update **@arch/database.md** (new paged repo methods + indexes),
  **@arch/frontend.md** only if the response contract notes change (it shouldn't), and a deep-dive
  **@plans/tokens/db-pagination.md** capturing the filter→SQL column map, the null-sort-last
  contract, the global-search deviation, and the live/lab cache split rationale.
- CLAUDE.md: note the live=DB-paged / lab=in-RAM token-list split under Data-scale guardrails.

## Open decisions to confirm before coding

1. **Global search in SQL — text columns only** (my recommendation, logged deviation) vs full
   date/numeric parity (slower, unindexable). → *recommend text-only.*
2. **lab list base cap** — uncap entirely (`i64::MAX`) vs a large `LAB_TOKEN_LIST_LIMIT`. →
   *recommend a named large constant so lab RAM stays bounded-but-huge, not literally unbounded.*
3. **ix_label JSON set-equality in SQL** — the array_agg approach vs accepting text-mode-only on
   the live SQL path (JSON-exact mode is rare). → *recommend full port; fall back to text-only if
   the SQL proves fragile.*
