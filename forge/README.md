# forge

A Solana launch + trading + analytics platform. Starts with pump.fun token
creation and grows into a multi-launchpad (incl. USDC-quoted), multi-wallet,
multi-RPC strategy + analytics ecosystem.

This repo is the **data-infrastructure foundation**: a schema designed
*generalized for multi-venue + non-SOL quote from day one*, learning from (not
copying) the sibling `hunter` repo's data layer. Design decisions and rationale:
[`docs/decisions.md`](docs/decisions.md). Phases and task checklist:
[`docs/roadmap-plan.md`](docs/roadmap-plan.md).

## Layout

Now one product folder inside the **`Bot/` monorepo** (single Cargo workspace at
`Bot/Cargo.toml`, shared with `hunter/`). The bins are `forge-live` / `forge-lab`
(hunter owns `hunter-live`/`hunter-lab`); they are not workspace `default-members`, so target
them with `-p forge-live` / `-p forge-lab`.

```text
Bot/                          monorepo root: [workspace] + Cargo.lock, resolver "1"
├── shared/                   standalone drop-in crates used by BOTH products
│   ├── pump-trader/          (was ../hunter/pump-trader)
│   └── ingest-laserstream/   (was ../hunter/ingest-laserstream)
├── hunter/             sibling product (live/lab)
└── forge/
    ├── docker-compose.yml    local Postgres + TimescaleDB (5556) + forge-live service
    ├── .env.example          copy to .env
    ├── migrations/           0001_init.sql
    └── crates/
        ├── platform-core (lib)  data layer: config, models, storage, repositories, venue trait
        ├── ingest-host   (lib)  borrowed ingest → PG trades              [forge-live only]
        ├── launcher      (lib)  create/dev-buy/bundle via pump-trader    [forge-live only]
        ├── lake          (lib)  Parquet/DuckDB cold tier                 [forge-lab only]
        ├── forge-live      (bin)  ingest + launcher + trading + HTTP → EC2
        └── forge-lab       (bin)  lake + sweeps + backtests + analytics → workstation
```

## Reuse (shared crates)

Two standalone crates live in the monorepo's **`shared/`** home and are consumed as
intra-workspace deps (`{ workspace = true }`) by both products:

| Crate | Role | Used by |
| --- | --- | --- |
| `pump-trader` | execution: build/sign/submit/confirm/retry; pump.fun venue adapter | `launcher` |
| `ingest-laserstream` | Helius gRPC transport + curve/AMM decoders + watchdog | `ingest-host` |

`trading_core` is **not** reused — it encodes the SOL/pump domain being redesigned.
The pump.fun IDLs are the single canonical copy at `shared/executor/pumpfun/idl/`
(alongside the fetch scripts and hand-written decoders); forge no longer keeps its own
copy. The only pure SSOT lifted by copy is the `lamports↔sol` conversion (generalized to
quote/base decimals in `platform-core::units`).

## Dep partition (enforced from commit 1)

`forge-live` (EC2) and `forge-lab` (workstation) link **disjoint** dep graphs — a resource
partition, not a naming preference. EC2 is 2vCPU/4GB: DuckDB/arrow/parquet/rayon
must never ship there; signing keys + gRPC never ship to the workstation.

| Crate | `forge-live` | `forge-lab` |
| --- | --- | --- |
| `ingest-host` (+ `ingest-laserstream`) | ✓ | ✗ |
| `launcher` (+ `pump-trader`) | ✓ | ✗ |
| `lake` (+ `duckdb`/`arrow`/`parquet`/`rayon`) | ✗ | ✓ |
| `platform-core` | ✓ | ✓ |

Verify the partition (resolution only, no compile):

```sh
cargo tree -p forge-live   # must show NO duckdb / arrow / parquet
cargo tree -p forge-lab    # must show NO pump-trader / ingest-laserstream / tonic
```

## Dev loop

```sh
docker compose up -d              # local Postgres + TimescaleDB (port 5556)
# sqlx migrate run                # Phase 2+ (0001_init.sql)
cargo run -p forge-live             # LIVE box: ingest + launch + trade + HTTP
cargo run -p forge-lab              # ANALYSIS box: lake + sweeps (no gRPC, no keys)
```

- **Data flow (live):** Helius gRPC → `ingest-laserstream` → `ingest-host` → PG
  `raw_txs`/`trades` (7-day `raw_txs` / 30-day `trades` retention on EC2).
- **Data flow (analysis):** EC2 PG → DB sync → local PG → `lab -- lake-export` →
  sealed-day Parquet → DuckDB reads/sweeps.
- **Reuse flow:** edit `shared/pump-trader`, both products see it (intra-workspace dep).

## Tests

`cargo test` is green with no DB — DB-gated tests self-skip. To run the
generalization proof (a USDC-quoted + a SOL-quoted token through the same
views, USD-comparable), point it at a **throwaway** database (it runs migrations
and mutates seed rows):

```sh
createdb forge_bot_test   # or: psql -c 'CREATE DATABASE forge_bot_test'
PLATFORM_TEST_DATABASE_URL=postgres://postgres:pass@localhost:5556/forge_bot_test \
  cargo test -p platform-core --test generality -- --nocapture
```
