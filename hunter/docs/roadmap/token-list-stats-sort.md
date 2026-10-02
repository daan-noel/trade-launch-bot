# Token list: sorts and filters on `tokens_info` columns cost a full scan

`/api/tokens` pages from Postgres ([token-list-backend.md](../plans/frontend/token-list-backend.md)).
A sort or filter on a `tokens_info` column (volume, price, market cap, trade count, dead,
migrated, ...) joins all 1.83M `tokens` rows to `tokens_info` before the `LIMIT`: 1.6-2.6 s
warm and 6-11 s cold in `psql`, about 8 s through the handler. That is at the `api` pool's
8 s `statement_timeout`, so such a request can return 500.

`tokens_info` carries no index but its primary key. Its rows are rewritten on every trade
batch, so each index added there is a cost on the live ingest write path.

## Options (decision open)

1. **Index + `tokens_info`-driven top-N.** An index per sortable `tokens_info` column, and a
   page query that walks it and joins `tokens` per row. `LEFT JOIN` semantics need care: the
   ~57k tokens with no `tokens_info` row all share the sort key (NULL / 0) and must interleave
   by the `mint_address` tiebreak. Needs the write-path cost measured on the live box first.
2. **Lab-only longer timeout.** Route the lab's list reads through a pool with a higher
   ceiling; fixes the 500s on the workstation, leaves the 2-11 s wait.
3. **More cache on the workstation Postgres.** `shared_buffers` is 256 MB against ~2.2 GB of
   `tokens` + `tokens_info` + indexes; a larger cache cuts the cold-path cost only.

## Done when

A `volume desc` page and a `dead = no` count on the full universe each return under 1 s warm
on the workstation, with the live ingest write latency unchanged.
