# Lake — analysis cold tier (forge-lab)

The lake is the Parquet/DuckDB cold tier for sweeps and backtests. It is the
**in-crate `forge/lab/src/lake/` module** of the `forge-lab` bin — **no longer a
separate `lake` crate** (the workspace `CLAUDE.md`/README crate table is stale on
this point; `forge/lab/Cargo.toml` is ground truth). It runs on the **workstation
only** and never ships to EC2.

> Status: **scaffold**. Only the column single-source (`schema.rs`) exists. The
> Parquet export writer, the DuckDB reader, and the sweep/backtest engines are
> **absent** — the previous `lake-export` subcommand and a `run_export` stub were
> both removed rather than left as no-ops. The stack deps
> (`duckdb`/`arrow`/`parquet`/`rayon`) are **not yet added** to `forge-lab`.

## Architecture

Data flow (only the first hop exists today):

```
EC2 PG (hot buffer: raw_txs 7d, trades 30d)
   → DB sync → local PG mirror (workstation)
      → [PLANNED] forge-lab -- lake-export → sealed-day Parquet
         → [PLANNED] DuckDB name-based reader → sweeps / backtests / simulate
```

`forge-lab` is the ANALYSIS composition root: no signing keys, no gRPC, no
executor/ingest/tonic/solana stack. It boots the local PG mirror and serves a thin
read-only HTTP surface (`src/http.rs`). Sweeps/backtests are meant to read the
**lake** (sealed Parquet), not PG — but neither the reader nor those engines are
implemented, so today every `forge-lab` HTTP read goes straight to the local PG
mirror via `platform-core` repos.

The load-bearing invariant is the **dep partition**: `forge-lab` must not link
`pump-trader`/`ingest-laserstream`/`tonic`/solana; the lake stack
(`duckdb`/`arrow`/`parquet`/`rayon`) must live only here and never reach the
2vCPU/4GB EC2 box (where `forge-live` runs and must not pull `duckdb`/`arrow`/
`parquet`).

## Module map

| File | Responsibility |
| --- | --- |
| `forge/lab/src/lake/mod.rs` | Module root + doc-of-record for the cold tier. Declares `pub mod schema` only. Comments mark the export/reader/parity pipeline as a later phase; the `run_export` stub was deleted, not stubbed to `Ok(0)`. |
| `forge/lab/src/lake/schema.rs` | **Column SSOT** for the sealed-day Parquet schema. `mod trades` holds one `&str` const per column + an ordered `COLUMNS: &[&str]`. Writer + reader (when they land) MUST import these — never string-literal a column name. `#![allow(dead_code)]` because there is no consumer yet. A guard test pins uniqueness, presence of the generalized quote/base + reserve pair, and `len == 15`. |
| `forge/lab/src/main.rs` | ANALYSIS composition root. Loads `Settings`, connects the local PG mirror, serves HTTP. **Rejects any CLI arg** (`"forge-lab takes no subcommands"`) — the `lake-export` subcommand is gone. Fail-closed bearer gate (`http-auth`), token OPTIONAL here (reads pass, lab moves no SOL). Binds `HOST`/`PORT` → falls back to `LAB_HOST`/`LAB_PORT` = `127.0.0.1:8240`. |
| `forge/lab/src/http.rs` | Thin read surface over the local mirror (NOT the lake). Routes: `GET /health`, `/api/quote_assets`, `/api/launchpads`, `/api/tokens/{mint}/overview`, `/api/tokens/{mint}/trades`. Own route config, disjoint from `forge-live`. Errors mapped to generic 500 (detail logged, never returned). |

### Parquet `trades` columns (schema.rs SSOT, ordered)

`mint_address`, `wallet_ref` (interned `wallet_dict` id, not a managed-wallet UUID),
`launchpad_id`, `market_kind`, `quote_asset_id`, `trade_type`, `amount_quote`,
`amount_base`, `reserve_quote`, `reserve_base`, `slot`, `tx_index`, `leg_index`,
`block_time`, `tx_signature` (15 total). Generalized quote/base amounts + a
venue-neutral reserve pair + interned `launchpad_id`/`quote_asset_id` — a new
launchpad/quote asset is a data row, never a new column.

## Key rules

- **Not a crate — a module.** `forge/lab/src/lake/`, part of the `forge-lab` bin.
  The workspace CLAUDE.md/README "6 crates" table listing a `lake` lib crate is
  stale; trust `forge/lab/Cargo.toml`.
- **Workstation only, never EC2.** The lake stack (`duckdb`/`arrow`/`parquet`/
  `rayon`) is confined to `forge-lab`. `forge-live` must never pull it (verify
  `cargo tree -p forge-live` shows no `duckdb`).
- **Dep partition both ways.** `forge-lab` must not link
  `pump-trader`/`ingest-laserstream`/`tonic`/solana (verify `cargo tree -p
  forge-lab`). Today `forge-lab` deps are only `platform-core`, tokio, actix,
  sqlx, http-auth, tracing.
- **Column SSOT.** All Parquet column names come from `lake::schema::trades`; the
  guard test pins the ordered set so writer and reader can't drift.
- **Sealed-days-only (planned).** The design is sealed-day Parquet + a PG
  fresh-tail union for recent tokens, with a PG-vs-Parquet parity test. None of
  this is implemented yet — only documented in the module headers.
- **Analysis is offline.** EC2 PG is a hot rolling buffer; analysis runs off a DB
  sync to a local mirror, then `lake-export` → Parquet. No DuckDB/export cron on
  the server.

## Not yet present (verified absent in source)

- `lake-export` subcommand / Parquet export writer — removed; `main.rs` rejects
  all args.
- DuckDB name-based reader — not written.
- Sweep / backtest / simulate engines — not written; `forge-lab` HTTP reads hit
  local PG directly.
- `duckdb`/`arrow`/`parquet`/`rayon` deps — not in `forge/lab/Cargo.toml` (the
  only workspace mention is a "keep it out" comment in `forge/live/Cargo.toml`).
