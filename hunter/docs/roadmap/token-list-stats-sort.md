# Token list: sorts and filters on `tokens_info` columns join every row

`/api/tokens` pages from Postgres ([token-list-backend.md](../plans/frontend/token-list-backend.md)).
A sort or filter on a `tokens_info` column (volume, price, market cap, trade count, dead,
migrated, ...) joins all 1.83M `tokens` rows to `tokens_info` before the `LIMIT`: 1.1-2.2 s
through the lab handler (deferred join, vacuumed tables, 1 parallel worker). Every other
request is 0.5 s or less.

`tokens_info` carries no index but its primary key, and stays that way in the shared schema:
live upserts every touched mint's row on each ~150 ms ingest flush, so an index on a stats
column is an index write per row there, and the `LEFT JOIN` cannot walk an index on its
nullable side anyway (the ~57k tokens with no `tokens_info` row must interleave by the
`mint_address` tiebreak).

## Options (decision open)

1. **More parallel workers on the workstation Postgres.** `POSTGRES_MAX_PARALLEL_PER_GATHER`
   is 1 (the 2 vCPU box's value). At 4, in `psql`, a `volume` page measures 0.95 s -> 0.62 s
   warm and a `dead = no` count 0.89 s -> 0.47 s. Needs the key in `hunter/.env` + `.env.example` and a
   Postgres container recreate.
2. **Lab-only `tokens_info` indexes + a `tokens_info`-driven top-N query** (a `UNION ALL` of
   info rows walked by index and info-less tokens, merged on the sort key). Sub-100 ms for
   the indexed columns, at the cost of one index per sortable column and a second page-query
   shape held at parity with the first.

## Done when

A `volume desc` page and a `dead = no` page on the full universe each return under 0.5 s warm
through the lab handler.
