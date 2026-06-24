# Postgres Performance Plan

**Root cause:** DbWriter wedges under the trade firehose on a 2vCPU/4GB box — Postgres is disk-IO-bound and `synchronous_commit=on` fsyncs ~40×/sec.

**Evidence (prod 2026-06-24):**

- `buffers_backend=22407` >> `buffers_checkpoint=8323` → buffer starvation (indexes spilling to disk)
- 3 requested checkpoints in 75 min → WAL fills 2GB `max_wal_size` every ~25 min
- 5 of 10 `trades` indexes had 0 scans/day (migration 0004 already fixed this)

## Constraints

1. **No infra spend** — 2vCPU/4GB, Postgres + backend share it. Bigger box is off the table.
2. **Sweeps/backtests run LOCAL only** — the deployed server's batch pool does no work.
3. **Analysis is dump→local** — deployed DB is a thin rolling ingest buffer; all analysis on local.

---

## Tier 1 — Free, highest-impact

### [ ] 1a. Turn off `ingest.persist_raw` — biggest single lever, no deploy

Toggle in the Settings UI. Kills the fat Helius-blob writes (`raw_transactions`), roughly halving write IO and WAL volume. Sweeps use `trades`+`tokens` only — nothing lost for analysis.

### [ ] 1b. `synchronous_commit = off`

Safe here: watchdog kills the **backend**, Postgres stays up, so no committed trades are lost on restart. Only risk: power-loss drops <1 WAL buffer — feed is replayable.

**Where:** `docker-compose.yml` postgres service or `postgresql.conf`.

### [ ] 1c. Raise flush batch size and interval

`backend/src/ingest_laserstream/db_writer.rs` lines 27–28:

```rust
const BATCH_MAX: usize = 1000;      // was 256
const FLUSH_INTERVAL_MS: u64 = 150; // was 25
```

### [ ] 1d. Cut connection counts — free RAM becomes page cache

Each open connection is a ~26 MB backend competing with the 256 MB buffer pool.

```
POSTGRES_MAX_CONNECTIONS=80   # docker-compose.yml
DB_MAX_CONNECTIONS=12         # .env  (hot pool — ingest writes)
DB_API_MAX_CONNECTIONS=8      # .env  (dashboard reads)
DB_BATCH_MAX_CONNECTIONS=2    # .env  (no batch work on server)
```

### [ ] 1e. Set `KEEP_DAYS = 7`

`backend/src/ingest_laserstream/maintenance.rs` line 18. 7 days covers the daily dump + safety margin. Smaller live table → indexes fit in the 256 MB buffer pool → less disk thrash.

Also set `SEED_ACTIVITY_WINDOW_DAYS` (`tuning.rs:89`) ≤ `KEEP_DAYS` so the cold-start seed can't scan beyond what's on disk.

### [ ] 1f. Raise `max_wal_size = 4GB`

`docker-compose.yml` postgres command. Spreads checkpoints time-based instead of WAL-fill spikes. With 1a (raw off) WAL fills slower — 4GB is plenty without risking the disk.

---

## Tier 1b — Const-var tuning (2vCPU/4GB sizing)

### Group A — DB write fan-out

| Const | File | Now | Set | Why |
| --- | --- | --- | --- | --- |
| `BATCH_MAX` | `db_writer.rs:27` | 256 | **1000** | Fewer, larger inserts = fewer fsyncs (= 1c) |
| `FLUSH_INTERVAL_MS` | `db_writer.rs:28` | 25 | **150** | ~6× fewer fsyncs/sec (= 1c) |
| `METRIC_WRITE_CONCURRENCY` | `db_writer.rs:32` | 16 | **6** | 16 concurrent upserts oversubscribes 2 cores + the 12-conn hot pool |

### Group B — In-memory cache RAM

| Const | File | Now | Set | Why |
| --- | --- | --- | --- | --- |
| `MAX_TRADES_RETAINED` | `token_cache.rs:32` | 10_000 | **2_500** | Per-mint ring × thousands of live mints = biggest backend heap consumer. tpsl only needs a recent window. |
| `SEED_TOKEN_LIMIT` | `tuning.rs:86` | 100_000 | **25_000** | 100k-token cold-start scan is heavy; 25k covers the live working set |
| `SEED_ACTIVITY_WINDOW_DAYS` | `tuning.rs:89` | 7 | **≤ `KEEP_DAYS`** | No point seeding beyond what's on disk |
| `TOKEN_CACHE_EVICT_IDLE_SECONDS` | `tuning.rs:103` | 7200 | **2700 (45 min)** | Evict idle mints sooner → smaller resident set → more page cache |
| `TOKEN_CACHE_EVICT_INTERVAL_SECONDS` | `tuning.rs:99` | 300 | **120** | Enforce the shorter idle window more often (cheap in-memory scan) |

### Group C — Cut ingest volume at the source

| Const | File | Now | Set | Why |
| --- | --- | --- | --- | --- |
| `POOL_SUBSCRIBE_ACTIVITY_WINDOW_SECONDS` | `tuning.rs:80` | 21600 (6h) | **7200–10800 (2–3h)** | Fewer AMM pools in gRPC subscription → fewer decoded txs → less write IO |
| `TOKEN_LIST_DB_REFRESH_SECS` | `tuning.rs:106` | 60 | **120** | Ranked refresh query stalled 7s on `DataFileRead` post-restart |

### Leave alone

- **`DB_QUEUE_CAP = 16384`** (`pipeline.rs:41`) — burst absorber; shrinking it causes gRPC consumer-lag eviction → restart → token gap. With 1a on, its worst-case memory is a few MB. Keep it.
- **Sweep memory budgets** — local-only, never tune for the server.

---

## Tier 2 — Small code/config changes

### [ ] 2a. BRIN index on `block_time` (replace btree)

`block_time` is append-ordered — BRIN is a few KB vs a btree updated on every insert.

New migration `0006_brin_trades_block_time.sql`:

```sql
DROP INDEX IF EXISTS idx_trades_block_time;
CREATE INDEX idx_trades_block_time_brin ON trades USING BRIN (block_time);
```

### [ ] 2b. Coarsen token-list DB refresh

Already covered by Group C (`TOKEN_LIST_DB_REFRESH_SECS` → 120). No separate code change needed.

---

## Tier 3 — Daily dump → local

**Model:** data-only refresh. Local keeps sweep results, settings, and raw_transactions intact — only tokens/trades/rules/positions/wallets are truncated and reloaded.

### Commands

**EC2 (run on box, off-peak):**

```bash
./scripts/db-snapshot-dump.sh
```

**Local (Windows — pull from EC2 and refresh):**

```powershell
$env:PGPASSWORD='<local_pg_superuser_pw>'; ./scripts/db-snapshot-restore.ps1 -SshTarget ubuntu@<ec2-ip>
```

Stop any local backend writing to `meme_bot` before running the restore.

### What's dumped / excluded

| Included | Excluded |
| --- | --- |
| `tokens`, `tokens_info`, `tokens_analysis`, `creator_profiles` | `raw_transactions*` (fat blobs) |
| `trades` (all daily partitions) | `*grouped_sweep*` (local owns these) |
| `tpsl{1,2}_strategy_rules`, all 4 position tables, `paper_test_run` | `app_settings` (local keeps its own) |
| `wallets`, `wallet_profiles`, `wallet_profile_tags` | `_sqlx_migrations` |

### Key flags

- `--data-only` — never touches local schema
- `--load-via-partition-root` — pg_restore routes rows to whichever local day-partition exists; restore script pre-creates them with `ensure_trades_partition`
- `--compress=zstd:3` — faster + smaller than gzip on PG16+; falls back to gzip via `COMPRESS=6`
- `-j 4` on restore — parallelism on the local side, not the server (single-threaded dump spares the 2vCPU box)

---

## Order of attack

1. **Now, no deploy:** 1a (`persist_raw` off in Settings UI) — likely ends the wedge alone.
2. **Config only, no rebuild:** 1b (`synchronous_commit`) + 1d (pool counts) + 1f (`max_wal_size`).
3. **One deploy:** Tier 1b consts (Groups A/B/C) + 1e (`KEEP_DAYS=7`) + 2a (BRIN migration).
4. **Run daily:** Tier 3 dump+restore to keep local analysis fresh.

---

## Already done

- `0004_drop_unused_trades_indexes.sql` — dropped 5 never-scanned `trades` indexes. Deployed.
