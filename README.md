# solana-launch-platform

A Solana launch + trading + analytics platform. Starts with pump.fun token
creation and grows into a multi-launchpad (incl. USDC-quoted), multi-wallet,
multi-RPC strategy + analytics ecosystem.

This repo is the **data-infrastructure foundation**: a schema designed
*generalized for multi-venue + non-SOL quote from day one*, learning from (not
copying) the sibling `meme-trading` repo's data layer. See
[`and-about-the-instructions-shimmying-shore.md`](../and-about-the-instructions-shimmying-shore.md)
for the full design.

## Layout

```text
solana-launch-platform/
├── Cargo.toml            workspace + path deps → ../meme-trading/*
├── docker-compose.yml    local Postgres + TimescaleDB (host port 5556)
├── .env.example          copy to .env
├── idl/                  pump.fun IDLs (copied SSOT: pfee / pump_amm / pump_bonding_curve)
├── migrations/           0001_init.sql (Phase 2)
└── crates/
    ├── platform-core (lib)  data layer: config, models, storage, repositories, venue trait
    ├── ingest-host   (lib)  borrowed ingest → PG trades          [LIVE only]
    ├── launcher      (lib)  create/dev-buy/bundle via pump-trader [LIVE only]
    ├── lake          (lib)  Parquet/DuckDB cold tier             [LAB only]
    ├── live          (bin)  ingest + launcher + trading + HTTP → EC2
    └── lab           (bin)  lake + sweeps + backtests + analytics → workstation
```

## Reuse (borrowed crates)

Two standalone crates from `../meme-trading` are referenced by **path dep** during
co-development (switch to a pinned `git` rev once stable):

| Crate | Role | Used by |
| --- | --- | --- |
| `pump-trader` | execution: build/sign/submit/confirm/retry; pump.fun venue adapter | `launcher` |
| `ingest-laserstream` | Helius gRPC transport + curve/AMM decoders + watchdog | `ingest-host` |

`trading_core` is **not** reused — it encodes the SOL/pump domain being redesigned.
Only tiny pure SSOT files were lifted by copy: the pump.fun IDLs (`idl/`) and the
`lamports↔sol` conversion (generalized to quote/base decimals in
`platform-core::units`).

## Dep partition (enforced from commit 1)

`live` (EC2) and `lab` (workstation) link **disjoint** dep graphs — a resource
partition, not a naming preference. EC2 is 2vCPU/4GB: DuckDB/arrow/parquet/rayon
must never ship there; signing keys + gRPC never ship to the workstation.

| Crate | `live` | `lab` |
| --- | --- | --- |
| `ingest-host` (+ `ingest-laserstream`) | ✓ | ✗ |
| `launcher` (+ `pump-trader`) | ✓ | ✗ |
| `lake` (+ `duckdb`/`arrow`/`parquet`/`rayon`) | ✗ | ✓ |
| `platform-core` | ✓ | ✓ |

Verify the partition (resolution only, no compile):

```sh
cargo tree -p live   # must show NO duckdb / arrow / parquet
cargo tree -p lab    # must show NO pump-trader / ingest-laserstream / tonic
```

## Dev loop

```sh
docker compose up -d              # local Postgres + TimescaleDB (port 5556)
# sqlx migrate run                # Phase 2+ (0001_init.sql)
cargo run -p live                 # LIVE box: ingest + launch + trade + HTTP
cargo run -p lab                  # ANALYSIS box: lake + sweeps (no gRPC, no keys)
```

- **Data flow (live):** Helius gRPC → `ingest-laserstream` → `ingest-host` → PG
  `raw_txs`/`trades` (7-day `raw_txs` / 30-day `trades` retention on EC2).
- **Data flow (analysis):** EC2 PG → DB sync → local PG → `lab -- lake-export` →
  sealed-day Parquet → DuckDB reads/sweeps.
- **Reuse flow:** edit `../meme-trading/pump-trader`, both projects see it (path dep).

## Tests

`cargo test` is green with no DB — DB-gated tests self-skip. To run the
generalization proof (a USDC-quoted + a SOL-quoted token through the same
views, USD-comparable), point it at a **throwaway** database (it runs migrations
and mutates seed rows):

```sh
createdb launch_platform_test   # or: psql -c 'CREATE DATABASE launch_platform_test'
PLATFORM_TEST_DATABASE_URL=postgres://postgres:pass@localhost:5556/launch_platform_test \
  cargo test -p platform-core --test generality -- --nocapture
```
