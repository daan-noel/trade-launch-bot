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

Measured at 1.83M tokens on the workstation (256 MB `shared_buffers`), warm cache:

| Request | Time |
| --- | --- |
| Default page (newest first) + unfiltered count | ~1.9 s (the count; the page is ~3 ms) |
| Search `pepe` | ~2.8 s |
| Filter on a `tokens_info` column (`dead = no`) | ~4.3 s |
| Sort on a `tokens_info` column (`volume`) | ~8 s, at the `api` pool's 8 s `statement_timeout` |

A filter or sort on a `tokens_info` column joins every row, so it costs a full scan. Open
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
