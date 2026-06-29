# Database Patterns & Operational Notes

> **NOTE:** File paths below that reference `backend/` are stale — the old monorepo crate was renamed.
> The current layout is: migrations at `trading_core/migrations/`, repos at
> `trading_core/src/storage/repositories/`, ingest at `ingest-laserstream/src/`. The
> `maintenance.rs` partition loop is **deleted** — TimescaleDB retention/compression policies
> replaced it (see [timescaledb-plan.md](../../timescaledb-plan.md)). `PARTITION BY RANGE` is
> replaced by hypertables. BRIN on `block_time` is replaced by Timescale chunk exclusion.

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

File naming: `backend/migrations/00NN_descriptive-slug.sql`. `0001_init.sql` is the consolidated baseline — it contains every table that existed at the point the migration history was squashed. New migrations are additive only.

Runner: `sqlx::migrate!("./migrations")` in `storage/postgres.rs::connect()`. Migrations run at every startup on the `hot` pool. `sqlx_migrations` table tracks applied migrations.

**Idempotency rule:** migrations must be safe to inspect but not safe to re-apply (sqlx tracks applied state). Never use `DROP TABLE` in a migration without extreme care. Prefer `ADD COLUMN IF NOT EXISTS`, `CREATE INDEX IF NOT EXISTS`, `CREATE TABLE IF NOT EXISTS` for columns/indexes.

**BRIN indexes on `trades`:** block_time is append-only and monotonically increasing — a BRIN index (`pages_per_range = 32`) is 10–100× smaller than a B-tree for time-range scans on partitioned tables. Migration `0006_brin_trades_block_time.sql` adds this.

**Partition maintenance:** `ingest_laserstream/maintenance.rs` creates future partitions (today + 2 days ahead) and drops past-retention partitions. It runs every 6 hours as a tokio task. A new partitioned table must register its `table_name` + `KEEP_DAYS` in `maintenance.rs`; there is no auto-discovery.

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

## Retention & Partition Pattern

New high-volume tables (> 1M rows/week projected) must follow this pattern:

1. Partition by time range (daily for trade-scale; weekly for lower-volume): `PARTITION BY RANGE (created_at)`
2. Register in `maintenance.rs::MANAGED_TABLES`: `{ table: "my_table", keep_days: 30 }`
3. Create `my_table_YYYY_MM_DD` partitions for at least today + 2 future days at migration time
4. Indexes go on the partition template, not the parent table

Tables that don't need time-based partitioning (strategy rules, positions, sweep results) use standard B-tree PKs + targeted indexes. Sweep results use the `retention.rs` compaction strategy (retain per-metric extremes, ~660 rows/group) instead of time-based deletion.

## Sweep Result Compaction

`grouped_sweep_repo.rs::vacuum_full_results()` runs after each group completes:

1. `fetch_combo_metrics_for_group(group_id)` — load all result rows for this group
2. `retained_combo_ids(metrics)` — select: best_by_score, best_by_win_rate, best_by_expectancy, best_by_pnl, global_best_combo → union → ~660 ids max
3. `delete_combos_except(group_id, retained_ids)` — one DELETE ... WHERE combo_id NOT IN (...)
4. Result: only ~660 rows remain per group regardless of how many combos were swept

This runs write-time so the table never grows unboundedly. It also runs at startup for `running` runs that were interrupted (`reconcile_orphaned_runs`).
