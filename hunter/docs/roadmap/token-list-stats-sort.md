# Token list: sorts and filters on `tokens_info` columns join every row

`/api/tokens` pages from Postgres ([token-list-backend.md](../plans/frontend/token-list-backend.md)).
A sort or filter on a `tokens_info` column (volume, price, market cap, trade count, dead,
migrated, ...) joins all 1.83M `tokens` rows to `tokens_info` before the `LIMIT`. Through the
lab handler (deferred join, vacuumed tables, 4 parallel workers): a sort alone 0.5-0.6 s, a
filter plus a sort 1.55 s (count and page each ~0.5 s, sharing the Docker VM's 6 cores).
Every other request is 0.25 s or less.

`tokens_info` carries no index but its primary key, and stays that way in the shared schema:
live upserts each changed mint on a 30 s flush, and once more when the mint leaves the cache, so an index on a stats
column is an index write per row there, and the `LEFT JOIN` cannot walk an index on its
nullable side anyway (the ~57k tokens with no `tokens_info` row must interleave by the
`mint_address` tiebreak).

## Remaining option (decision open)

**Lab-only `tokens_info` indexes + a `tokens_info`-driven top-N query** (a `UNION ALL` of info
rows walked by index and info-less tokens, merged on the sort key). Sub-100 ms for a sort on
an indexed column. Costs: ~100-150 MB of disk per indexed column, a slower sync (every
appended or updated `tokens_info` row writes each index), a second page-query shape held at
parity with the first. It does not help a filter's `COUNT` (still a full join, ~0.5 s) or the
cross-table sorts (`market_cap`, the FEP ratios), which no single-table index serves.

## Done when

A `volume desc` page and a `dead = no` page on the full universe each return under 0.5 s warm
through the lab handler.
