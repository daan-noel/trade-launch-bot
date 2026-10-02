# Token-list backend: one SQL page per request

Deep-dive for the `/api/tokens` rule in [CLAUDE.md](../../../CLAUDE.md). Both bins serve the
list from ONE core handler (`trading_core::api::handlers::tokens::list`), so `live` and `lab`
page the same rows. Related: `@arch/frontend.md`, `@arch/database.md`.

## Wire contract

`/api/tokens` is **`POST` `TableRequest`** — the unified strategy-table body. The global
filter panel + per-column filters fold into ONE `filters:{col→FilterSpec}` map, lowered by
`TokenQuery::from_table_request`. `POST /api/tokens/mints` takes the same body and returns
only the matched `mint_address` set (the Swing page's "run over every filtered token").

## Full list — one page from Postgres

`handlers::tokens::sql::build_where_and_order` compiles filter/sort/search to a parameterized
`WHERE` + `ORDER BY`; the handler then runs `TokenRepo::count_list` (the pager's `total`) and
`TokenRepo::find_list_page` (`LIMIT`/`OFFSET`) concurrently. A request reads one page and one
count — never the whole universe, never a RAM copy of it.

`TokenRepo::LIST_FROM` joins `tokens_info` and the per-mint `last_synced_at` as `LEFT JOIN`s on
a unique key, so Postgres drops a join a query never references: the unfiltered count scans
`tokens` alone. A `LATERAL` join there blocks that removal.

What keeps a page fast at 1.83M tokens:

- **Deferred join** (`find_list_page`): a `page` CTE picks the page's mints carrying only
  the columns its `WHERE`/`ORDER BY` read; the full projection is built for those rows
  alone. Projecting first drags the wide `jsonb` columns of every row through the sort.
- **Search indexes** (lab migration `0007`): trigram GIN on `LOWER(symbol)` and
  `LOWER(mint_address)`, the exact expressions `search_clause` emits (a guard test pins
  them). Lab-only, so the live ingest path pays no index write.
- **Visibility map**: `db-incremental-sync.ps1` runs `VACUUM (ANALYZE) tokens,
  tokens_info` after each pull, so the index-only scans skip the heap.

Measured through the lab handler on the workstation (256 MB `shared_buffers`, 1 parallel
worker), warm / first request:

| Request | Time |
| --- | --- |
| Default page (newest first), page 1 or 500 | 0.2 s / 1.0 s |
| Search (`pepe`, or a full mint) | 0.02-0.04 s / 0.1 s |
| Sort on a `tokens` column (`symbol`) | 0.5 s / 0.6 s |
| Sort on a `tokens_info` column (`volume`) | 1.1 s / 1.3 s |
| Filter + sort on `tokens_info` columns (`dead = no`, `volume`) | 1.9 s / 2.2 s |

A sort or filter on a `tokens_info` column still joins every row (no index there). Open
work: [token-list-stats-sort.md](../../roadmap/token-list-stats-sort.md).

## Tracked-only — the in-RAM cache

`tracked_only=true` (the "tracked" badge) filters/sorts/pages the live `TokenCache` snapshot
(`state::token_list_cache`) in RAM, no DB round-trip. `tracked` in every response is that
subset's filtered count. `lab` has no ingest, so its cache is empty and `tracked` is 0.

## Seed cap is not a list cap

`SEED_TRACKING_LIMIT` is the **live tracking-cache seed** cap only — never the list cap.

## Parity guards

The SQL and in-RAM engines (the tracked view) are held at parity by
`token_repo::parity_tests` — page order, `count_list`, and the `find_list_mints` set, per case
(not `--ignored`: auto-runs when `DATABASE_URL` is set, self-skips otherwise) — plus a
**no-DB** column-key guard `handlers::tokens::grammar_parity_tests` that runs on every
`cargo test`.
