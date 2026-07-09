# Analysis workflow (lab + lake)

The **analysis** workload runs entirely on the workstation (`lab` bin + `lake`
crate). It never ships to EC2 — DuckDB / arrow / parquet / rayon and the sweep
engine stay here (verify: `cargo tree -p live` shows none of them). EC2's only
role in the cold tier is being the upstream PG source that gets synced down.

## The pipeline

```text
EC2 `live` Postgres            workstation
(rolling buffer:               ┌──────────────────────────────────────────────┐
 raw_txs 7d, trades 30d)       │ local PG mirror (long retention)             │
        │                      │      │                                       │
        │  db-incremental-sync │      │  cargo run -p lab -- lake-export      │
        └─────────────────────▶│      ▼                                       │
                               │  sealed-day Parquet lake ($SWEEP_LAKE_DIR)   │
                               │      │                                       │
                               │      ▼  DuckDB name-based reader             │
                               │  sweeps · backtests · simulate · analytics   │
                               └──────────────────────────────────────────────┘
```

1. **Sync** the server's rolling buffer down to the local mirror:

   ```powershell
   ./scripts/db-incremental-sync.ps1 -ServerDatabaseUrl <server-conn> `
       -LocalDatabaseUrl $env:DATABASE_URL
   ```

   Incremental: dimensions + identity tables upsert (server wins), `trades` /
   `raw_txs` append by a `block_time` watermark, and `wallet_dict` is a
   **non-destructive merge** — local-only ids the server has aged out are
   preserved (the lab retains trade history longer than the server's window; a
   truncate+replace would re-orphan old trades). See the script header.

2. **Export** sealed days from the local mirror to the Parquet lake:

   ```powershell
   PORT=8092 cargo run -p lab -- lake-export --include-today
   ```

   Sealed-days-only, so run it on a cadence (e.g. nightly + `--include-today`)
   or recent-token reads truncate. Column names are single-sourced in
   `crates/lake/src/schema.rs` (`lake::schema::trades::COLUMNS`) with a guard
   test — the writer and the DuckDB reader both consume that list, so they can't
   drift.

3. **Read**: sweeps / backtests / simulate live in `lab` and read the lake (+ a
   PG fresh-tail union for tokens newer than the last export). Single-rule
   simulate reads Parquet, not PG.

## Running the lab HTTP surface

```powershell
# lab defaults to :8092 (live uses :8091); pair with a local PG mirror.
LAB_PORT=8092 cargo run -p lab
curl http://127.0.0.1:8092/health          # {"status":"ok","service":"lab"}
curl http://127.0.0.1:8092/api/quote_assets
```

## Status

`lake` currently ships only the **schema seam** (`schema.rs` + guard test) and a
`run_export` stub — the DuckDB/parquet writer + reader + parity test fill in a
later phase, once the live feed is populating `trades`. The `lab` bin's HTTP
surface reads the local mirror today; sweep/backtest/simulate endpoints attach as
the lake fills. The sync script is schema-correct; its SSH-tunnel / FDW-server
wiring is stubbed until the EC2 box exists (dry-run it between two local DBs
first).
