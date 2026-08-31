# Database Patterns & Operational Notes

Layout: migrations at `hunter/core/migrations/` (core) and `hunter/lab/migrations/` (lab),
repos at `hunter/core/src/storage/repositories/`, ingest at `shared/ingest/pumpfun/src/`.

Deep-dive on pool rationale, migration conventions, sqlx patterns, and query guardrails. See [@arch/database.md](@arch/database.md) for the schema overview and repo table. See [@plans/database/db-pool-routing.md](@plans/database/db-pool-routing.md) for routing logic.

## Pool Rationale — why three isolated pools

A single pool under mixed workload creates latency spikes: a slow sweep query (full `trades` scan) holds a connection that a StrategyRunner exit confirmation needs. Three pools make starvation impossible by construction:

| Pool | Max conn | Timeout | Users |
|---|---|---|---|
| `hot` (64) | none | none | DbWriter (ingest), StrategyRunner (exits, position writes), maintenance, token-list seed |
| `api` (32) | 8s `statement_timeout` | 8s | All HTTP handlers via `AppState.db` |
| `batch` (16) | none | none | Sweeps, backtests, token_sync backfill via `AppState.batch_db` |

**Statement timeout on `api`:** the 8s timeout kills any query that would make a user-facing request hang. It is not on `hot` because DbWriter flush must never be interrupted. It is not on `batch` because sweeps run long scans by design.

**EC2 constraint:** each open Postgres connection costs ~26 MB RAM. At 64+32+16=112 connections peak, that's ~2.9 GB out of 4 GB total (server + Postgres share). Don't raise the limits without shrinking something else. Pool sizes are tuned in `.env` (`DATABASE_HOT_MAX_CONNECTIONS`, `DATABASE_API_MAX_CONNECTIONS`, `DATABASE_BATCH_MAX_CONNECTIONS`).

## Migration Conventions

File naming: `<crate>/migrations/00NN_descriptive-slug.sql`. `0001_init.sql` is the consolidated baseline — the entire chain squashed into one end-state file, so it creates exactly what running the chain on a fresh database would leave. New migrations are additive only.

**Squashing again later is not free.** The ledger keys on `(version, SHA-384 of the file bytes)`, so folding `00NN_*.sql` back into `0001_init.sql` both changes version 1's checksum and orphans versions 2..N — the runner refuses to boot on either. Every already-migrated database must be reconciled once with `scripts/consolidate-migration-ledgers.ps1` (ledger-only). When you squash: fold each later migration into the logical place in the end-state DDL, keep its rationale as a comment there, and **drop the pure data backfills** (they only rewrote pre-existing rows, so they are no-ops on a fresh DB) — note in the header which ones were dropped and why. Then verify by applying the squashed file to a scratch database and diffing `information_schema.columns` + `pg_constraint` + `pg_indexes` against a real, incrementally-migrated one; the diff must be empty.

**The precondition that makes a squash dangerous: every folded-in migration must already be applied on every database you will reconcile.** The reconcile script rewrites the *ledger* and never the *schema* — it stamps "version 1 applied" on the assumption that everything the squashed file creates is already there. A migration that had not yet run on that box therefore never runs at all: sqlx sees version 1 as done and skips the file forever, the column silently stays missing, and the bin fails at **query time, not boot time**. This is easy to hit precisely because a pending migration is the normal state between shipping the file and redeploying the server.

So the deploy order is **catch-up → reconcile → redeploy**, per database, EC2 first (`db-incremental-sync.ps1` copies the server's `_sqlx_migrations` rows into the local mirror, so a stale server ledger re-inserts versions you just cleaned):

```bash
psql <url> -f scripts/squash-catchup.sql                                   # schema to end state
pwsh scripts/consolidate-migration-ledgers.ps1 -DatabaseUrl <url> -Apply   # then the ledger
```

`scripts/squash-catchup.sql` carries the folded DDL in idempotent form (`ADD COLUMN IF NOT EXISTS` / `CREATE OR REPLACE`), so it is safe on a database that is already current and safe to re-run. Rewrite it as part of the next squash rather than accumulating generations in it — it is scratch tooling for one migration event, not a second schema SSOT.

**A migration column reaches the lab only if the sync names it.** `db-incremental-sync.ps1` copies row-by-row with a named column list (never `SELECT *` — local and server column ORDER diverges as each side runs `ALTER TABLE ADD COLUMN`), so a list kept by hand rots the moment a migration lands: the new column is simply not copied and reads NULL on the lab, with no error anywhere. The lists therefore come from the local catalog at run time, taken after the parity guard has proven local and server hold the same column set. Two consequences: adding a column to a synced table needs no edit to that script, and the server must be redeployed onto the same core-migration state as the lab before a sync — otherwise the shapes differ and the guard aborts the run by table name. A column added while `ON CONFLICT ... DO NOTHING` governs its table (`trades`, `raw_txs`) does NOT backfill on a later run: rows already local stay NULL for it, and the watermark never revisits the window. Both need `-RepairFrom <ts> [-RepairTo <ts>]`, which re-walks the given window with server-wins upserts — it is also the only way to fill rows a local ingest outage missed. It costs the whole window re-transferred and is bounded by the server's ~8-day retention, so give it one window at a time.

Runner: `sqlx::migrate!("./migrations")` in `storage/postgres.rs::connect()`. Migrations run at every startup on the `hot` pool. `sqlx_migrations` table tracks applied migrations.

**Idempotency rule:** migrations must be safe to inspect but not safe to re-apply (sqlx tracks applied state). Never use `DROP TABLE` in a migration without extreme care. Prefer `ADD COLUMN IF NOT EXISTS`, `CREATE INDEX IF NOT EXISTS`, `CREATE TABLE IF NOT EXISTS` for columns/indexes.

**No hand-rolled time indexes or partition maintenance.** Hot tables are TimescaleDB
hypertables: chunk exclusion does the time-range pruning (so no BRIN on `block_time`), and
declarative `add_compression_policy` / `add_retention_policy` jobs do the aging (so no
partition-creation task). Both are declared in `0001_init.sql` — see the pattern below.

## sqlx Patterns

**`query_as!` macro vs `query!`:** use `query_as!` when the result type is a named struct. `query!` for single-value or unnamed result shapes. Never use `query_builder` for hot paths — prefer static SQL strings with `$N` params.

**Bind-param ceiling:** sqlx 0.6 has a hard ceiling of 65 535 bind parameters per query (Postgres wire protocol limit). Bulk inserts must chunk:

```rust
let chunk_size = 65_535 / BINDS_PER_ROW;
for chunk in rows.chunks(chunk_size) {
    // build INSERT ... VALUES ($1,$2,...) for this chunk
}
```

This is not enforced by the library — it will silently produce a malformed query or panic at runtime. Every `insert_many` in the repos is chunked this way.

**Keyset pagination:** `find_page_before(cursor_id, limit)` pattern (not OFFSET) for large tables. OFFSET scans scale O(n) with page depth; keyset is O(log n) via index seek. All `/api/tokens` list endpoints use this pattern.

**`mint = ANY($1::text[])`:** efficient multi-mint lookup using Postgres array containment. Chunked by 500 mints per call (stays well under the bind-param ceiling).

**ON CONFLICT strategies:**
- `tokens`: `ON CONFLICT (mint_address) DO NOTHING` — first-write wins; `token_sync` backfills don't overwrite live ingest data
- `trades`: `ON CONFLICT (tx_signature, leg_index, block_time) DO NOTHING` — idempotent; gRPC feed can deliver duplicates on reconnect
- `tokens_info`: `ON CONFLICT (mint_address) DO UPDATE SET ...` — last-write wins for metrics (always newer)
- `wallets`: `ON CONFLICT (address) DO UPDATE SET last_seen_at = EXCLUDED.last_seen_at`

## Bulk-insert Size Ceilings (current binds-per-row counts)

| Table | Binds/row | Max chunk |
|---|---|---|
| `trades` | 17 | 3855 rows |
| `tokens` | 14 | 4681 rows |
| `raw_txs` | 6 | 8000 rows (`RawTxRepo::INSERT_CHUNK_ROWS`) |
| `wallets` | 3 | 21 845 rows |

If columns are added to these tables, recalculate `chunk_size = floor(65535 / binds_per_row)` and update the corresponding `insert_many`.

## Retention & Compression Pattern

New high-volume tables (> 1M rows/week projected) must follow this pattern, all declared in
the migration itself — there is no maintenance task to register with:

1. `SELECT create_hypertable('my_table', by_range('block_time', INTERVAL '1 day'))` — the
   time dimension must be **timestamptz**, so the policies below can use native `now()`.
2. `ALTER TABLE … SET (timescaledb.compress, timescaledb.compress_segmentby = '<the
   high-selectivity filter column>', timescaledb.compress_orderby = '<the scan order>')`.
3. `SELECT add_compression_policy('my_table', compress_after => …)` — the window MUST
   exceed how far back any backfill/sync writes, or those inserts land in a compressed chunk.
4. `SELECT add_retention_policy('my_table', drop_after => …)`.
5. Index only what the **uncompressed** recent chunks need; historical chunks are served by
   the `segmentby`/`orderby` metadata, and a redundant time index just costs writes.

Tables that don't need time-based aging (strategy rules, positions, sweep results) use standard B-tree PKs + targeted indexes. Sweep results use the `retention.rs` compaction strategy (retain per-metric extremes, ~660 rows/group) instead of time-based deletion.

## Sweep Result Retention

Retention is applied **write-time**, in the grouped-sweep handler, before results are
persisted — the table never grows unboundedly:

1. `sweep::retention::retained_combo_ids(metrics, best_combo_id, cfg)` — select:
   best_by_score, best_by_win_rate, best_by_expectancy, best_by_pnl, global_best_combo
   → union → ~660 ids max
2. Only those combos' rows are written for the group; the rest are never inserted.

`retained_combo_ids` is the SSOT for "which combos survive"; the same pure filter would
select existing and future data identically. (The old one-time backfill-compaction probe —
`vacuum_full_results` / `fetch_combo_metrics_for_group` / `delete_combos_except` and the
never-wired `compact-sweeps` subcommand — write-time retention makes one moot.)
