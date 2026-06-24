# TimescaleDB Adoption Plan

## Context

Goal: reduce DB size/disk, improve query speed, simplify partition ops — on both local analysis box and EC2 (2vCPU/4GB). Starting from an **empty DB**, so no in-place migration needed. Hypertables replace native `PARTITION BY RANGE` from day one.

Three decisions that simplify everything vs. earlier analysis:
- **ON CONFLICT DO NOTHING** (drop the reserve-refresh upsert) → compressed-chunk hazard eliminated
- **Empty DB** → no `create_hypertable()` migration on existing partitioned tables; `maintenance.rs` can be deleted
- **Watchdog can be remade** → ingest error detection is redesignable around the new structure

---

## Why it's worth doing

| Problem today | TimescaleDB fix |
| --- | --- |
| Working set outgrows 1GB `shared_buffers` → disk thrash → pool exhaustion → watchdog restart | Columnar compression shrinks older chunks ~10–20×; they stop competing for cache |
| `maintenance.rs` partition loop (create/drop every 6h) | `add_retention_policy()` + `add_compression_policy()` replace it declaratively |
| Client-side OHLC bucketing in browser | Continuous aggregates for server-side candles (Phase 2) |
| Manual `EXTRACT`/`date_bin` in creation_stats_repo.rs | `time_bucket_gapfill()` + caggs |
| 7-day rolling retention limits local sweep corpora | Long retention cheap once compressed |

**RAM**: TimescaleDB adds ~5–10 MB shared library + ~52 MB burst during background compression jobs (2 workers max). Compressed chunks return hundreds of MB to `shared_buffers`. Net positive on the 4GB box.

**CPU**: Background compression fires once per day per chunk — periodic, seconds-long burst. Tunable via `timescaledb.max_background_workers = 2`. Zero steady-state overhead during ingest.

---

## What changes in the ingest path

**[trade_repo.rs](backend/src/storage/repositories/trade_repo.rs)** — two places:
- `insert()` line 184: `ON CONFLICT … DO UPDATE SET …` → `ON CONFLICT DO NOTHING`
- `insert_many()` line 265: same change

Safe because reserves in the first write are the correct on-chain values. With `DO NOTHING`, gRPC reconnect re-delivery is silently skipped — correct behavior, simpler, and compressed chunks are never touched by writes.

**[maintenance.rs](backend/src/ingest_laserstream/maintenance.rs)** — **delete entire file**. Retention and compression are handled by Timescale policies. Remove the `run_partition_maintenance` task from [main.rs](backend/src/main.rs)'s `tokio::select!`.

**Watchdog** — simplify or rebuild. The `IngestHeartbeat` + OS-thread force-exit was partly defensive against partition-not-found errors and backlog wedges. With hypertables auto-managing chunks, re-evaluate what failure modes remain and instrument only what's still needed.

---

## Schema (migrations rewrite)

Drop the existing `PARTITION BY RANGE` baseline and replace with hypertables. Key pattern:

```sql
CREATE EXTENSION IF NOT EXISTS timescaledb;

CREATE TABLE trades (
    id                      UUID             NOT NULL DEFAULT uuid_generate_v4(),
    mint_address            TEXT             NOT NULL,
    wallet_address          TEXT             NOT NULL,
    trade_type              TEXT             NOT NULL CHECK (trade_type IN ('buy', 'sell')),
    sol_amount              DOUBLE PRECISION NOT NULL,
    token_amount            DOUBLE PRECISION NOT NULL,
    price_per_token         DOUBLE PRECISION NOT NULL,
    tx_signature            TEXT             NOT NULL,
    slot                    BIGINT           NOT NULL,
    block_time              TIMESTAMPTZ      NOT NULL,
    virtual_sol_reserves    DOUBLE PRECISION,
    virtual_token_reserves  DOUBLE PRECISION,
    real_sol_reserves       DOUBLE PRECISION,
    real_token_reserves     DOUBLE PRECISION,
    ix_type                 TEXT             NOT NULL DEFAULT 'Unknown',
    ix_labels               JSONB            NOT NULL DEFAULT '[]',
    leg_index               INTEGER          NOT NULL DEFAULT 0,
    received_at             TIMESTAMPTZ      NOT NULL,
    venue                   TEXT             NOT NULL DEFAULT 'curve' CHECK (venue IN ('curve', 'amm')),
    PRIMARY KEY (block_time, id)
);

SELECT create_hypertable('trades', 'block_time', chunk_time_interval => INTERVAL '1 day');

ALTER TABLE trades SET (
    timescaledb.compress,
    timescaledb.compress_segmentby = 'mint_address',
    timescaledb.compress_orderby   = 'block_time DESC'
);
SELECT add_compression_policy('trades', compress_after => INTERVAL '2 days');
SELECT add_retention_policy('trades', drop_after => INTERVAL '7 days');

-- Indexes (same as before; cascade to all chunks automatically)
CREATE UNIQUE INDEX idx_trades_tx_leg    ON trades (tx_signature, leg_index, block_time);
CREATE INDEX idx_trades_mint             ON trades (mint_address);
CREATE INDEX idx_trades_mint_time        ON trades (mint_address, block_time DESC);
CREATE INDEX idx_trades_mint_venue_slot  ON trades (mint_address, venue, slot DESC);
-- Note: BRIN on block_time (migration 0006) is NOT needed — Timescale chunk
-- exclusion replaces it for range scans on the partition key.
```

Same pattern for `raw_transactions` (hypertable on `received_at`, same compression + retention).

---

## docker-compose.yml change

Swap image and add two lines. All existing tuning (`shared_buffers`, `synchronous_commit`, pool sizes, etc.) stays unchanged.

```yaml
postgres:
  image: timescale/timescaledb-ha:pg16   # was: postgres:16
  command:
    - "postgres"
    - "-c"
    - "shared_preload_libraries=timescaledb"
    - "-c"
    - "timescaledb.max_background_workers=2"
    # ... all existing -c flags unchanged ...
```

---

## Continuous aggregates — Phase 2 (optional)

Once hypertables are stable, add server-side candles:

```sql
CREATE MATERIALIZED VIEW trades_ohlc_1m
WITH (timescaledb.continuous) AS
SELECT
    time_bucket('1 minute', block_time) AS bucket,
    mint_address,
    first(price_per_token, block_time)  AS open,
    max(price_per_token)                AS high,
    min(price_per_token)                AS low,
    last(price_per_token, block_time)   AS close,
    sum(sol_amount)                     AS volume_sol
FROM trades
GROUP BY bucket, mint_address;

SELECT add_continuous_aggregate_policy('trades_ohlc_1m',
    start_offset      => INTERVAL '3 days',
    end_offset        => INTERVAL '1 minute',
    schedule_interval => INTERVAL '1 minute');
```

Enables a new API endpoint serving pre-aggregated candles; frontend chart in
`frontend-react/src/components/token-price-chart/` can then fetch from server
instead of bucketing raw trades client-side.

---

## Phased rollout

### Phase 1 — Local (zero server risk)
1. Swap to `timescale/timescaledb-ha:pg16` locally in docker-compose
2. Fresh DB (empty)
3. Rewrite migrations: hypertables + compression + retention policies
4. `trade_repo.rs`: `DO UPDATE` → `DO NOTHING` (lines 184 and 265)
5. Delete `maintenance.rs`; remove its task from `main.rs` `tokio::select!`
6. `cargo check --bin backend` clean; run integration tests
7. Validate: ingest dedup, compression ratio, query latency vs. before

### Phase 2 — EC2 (after Phase 1 stable for a week)
1. Backup `.env` → `.env.backup` (CLAUDE.md rule)
2. `docker compose down` → swap image → `docker compose up -d`
3. Fresh DB (user confirmed OK to start empty)
4. Deploy backend with `DO NOTHING` change
5. Monitor for one week: ingest p99, CPU during bg-worker compression bursts, RAM (expect improvement), watchdog silence

### Phase 3 — Server-side candles (optional, after Phase 2 stable)
- Add `trades_ohlc_1m` cagg + API endpoint
- Refactor frontend chart to fetch candles from server

---

## Files touched

| File | Change |
| --- | --- |
| [docker-compose.yml](docker-compose.yml) | Image → `timescale/timescaledb-ha:pg16`; add `shared_preload_libraries`, `max_background_workers` |
| `backend/migrations/0001_init.sql` | Rewrite: hypertables instead of `PARTITION BY RANGE`; drop SQL partition functions |
| `backend/migrations/0002–0006_*.sql` | Review each; most absorbed into new baseline or deleted |
| [trade_repo.rs](backend/src/storage/repositories/trade_repo.rs) | `ON CONFLICT DO UPDATE` → `ON CONFLICT DO NOTHING` (lines 184, 265) |
| [maintenance.rs](backend/src/ingest_laserstream/maintenance.rs) | **Delete** |
| [main.rs](backend/src/main.rs) | Remove `run_partition_maintenance` from `tokio::select!` |
| [db_writer.rs](backend/src/ingest_laserstream/db_writer.rs) | Review/simplify watchdog logic |
| `db-snapshot-restore.ps1` | Provision extension + hypertables + policies on restore |
| `@arch/database.md` | Update schema docs; remove partition-function section |
| `@plans/database/*` | Update performance plan docs to reflect Timescale ops model |

---

## Verification

```powershell
# Typecheck
cargo check --bin backend

# Integration tests (needs DATABASE_URL pointing at new Timescale DB)
cargo test --bin backend -- --ignored

# In psql — confirm hypertables + compression working
SELECT * FROM timescaledb_information.hypertables;
SELECT * FROM chunk_compression_stats('trades');
SELECT * FROM hypertable_compression_stats('trades');

# Force compress chunks older than 2 days to inspect ratio immediately
SELECT compress_chunk(c)
FROM show_chunks('trades', older_than => INTERVAL '2 days') c;

SELECT * FROM hypertable_compression_stats('trades');
```
