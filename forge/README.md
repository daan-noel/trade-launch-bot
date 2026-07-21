# forge

A Solana launch + trading + analytics platform. Starts with pump.fun token
creation and grows into a multi-launchpad (incl. USDC-quoted), multi-wallet,
multi-RPC strategy + analytics ecosystem.

This repo is the **data-infrastructure foundation**: a schema designed
*generalized for multi-venue + non-SOL quote from day one*, learning from (not
copying) the sibling `hunter` repo's data layer. Design decisions and rationale:
[`docs/decisions.md`](docs/decisions.md). Phases and task checklist:
[`docs/roadmap-plan.md`](docs/roadmap-plan.md).

**Crate map ground truth:** [`docs/arch/architecture.md`](docs/arch/architecture.md)
(and root `Bot/Cargo.toml` `members`). Prefer that over older crate names.

## Layout

One product folder inside the **`Bot/` monorepo** (single Cargo workspace at
`Bot/Cargo.toml`, shared with `hunter/`). Bins are `forge-live` / `forge-lab`
(not workspace `default-members` — build/run with `-p`).

```text
Bot/                          monorepo root: [workspace] + Cargo.lock
├── shared/                   standalone drop-ins used by BOTH products
│   ├── executor/{core,pumpfun}   write stack (dep key pump-trader)
│   ├── ingest/{core,pumpfun}     read stack (dep key ingest-laserstream)
│   └── http-auth                 fail-closed bearer gate
├── hunter/                   sibling product
└── forge/
    ├── docker-compose.yml    local Postgres + TimescaleDB (5556)
    ├── .env.example          copy to .env
    ├── migrations/
    ├── core/                 forge-core (platform_core) — data layer
    ├── orchestrator/         forge-orchestrator — Operation/Plan gate
    ├── launcher/             forge-launcher — create/dev-buy/bundle/wallet-pool
    ├── live/                 forge-live bin (+ ingest/ host adapter module)
    ├── lab/                  forge-lab bin (+ lake/ scaffold module)
    └── frontend/             operator SPA
```

**Folded modules (not crates):** `ingest-host` → `forge/live/src/ingest/`;
`lake` → `forge/lab/src/lake/` (schema constants only today — Parquet/DuckDB not
wired yet).

## Reuse (shared crates)

| Crate (dep key) | Role | Used by |
| --- | --- | --- |
| `pump-trader` (`executor-pumpfun`) | execution: build/sign/submit/confirm | `forge-launcher` |
| `ingest-laserstream` (`ingest-pumpfun`) | Helius gRPC transport + decode | `forge-live` ingest host |
| `http-auth` | bearer gate on mutating routes | `forge-live` / `forge-lab` |

`trading_core` is **not** reused — it encodes the SOL/pump domain being redesigned.
Canonical pump.fun IDLs: `shared/executor/pumpfun/idl/`.

## Dep partition

`forge-live` (EC2) and `forge-lab` (workstation) link **disjoint** dep graphs.

| Piece | `forge-live` | `forge-lab` |
| --- | --- | --- |
| ingest host + `ingest-laserstream` | ✓ | ✗ |
| launcher + `pump-trader` | ✓ | ✗ |
| lake analytics (DuckDB/arrow/parquet) | ✗ | scaffold only (no those deps yet) |
| `forge-core` / `http-auth` | ✓ | ✓ |

```sh
cargo tree -p forge-live   # must show NO duckdb / arrow / parquet
cargo tree -p forge-lab    # must show NO pump-trader / ingest-laserstream / tonic
```

## Dev loop

```sh
docker compose up -d              # local Postgres + TimescaleDB (port 5556)
sqlx migrate run
cargo run -p forge-live             # LIVE: ingest + launch + HTTP :8230
cargo run -p forge-lab              # LAB: PG reads + HTTP :8240 (no keys/gRPC)
```

Ports: DB **5556**, live **8230**, lab **8240**.
