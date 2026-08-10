# Incremental DB Sync — `db-incremental-sync.ps1`

Pull the EC2 server's **newer** data and append it into your local `hunter_bot` DB —
directly **DB → DB** over an SSH tunnel. No CSV files, no `scp`, no temp files.

Safe to run repeatedly. **Non-destructive for market data**: your local sweep results,
settings, `raw_txs`, and existing trades are never touched — only new rows are added
(and a few metadata rows refreshed).

> **Strategy tables are mirrored (server wins), non-destructively.** `fingerprints`,
> `strategy_rules`, `strategy_runs`, `strategy_run_metrics`, and `strategy_positions`
> are copied **full-table each run** and upserted (`DO UPDATE`): new server rows are
> added and changed ones refreshed, but **no local row is ever deleted**. So the lab
> keeps its accumulated history — a run/position the server has since deleted or aged
> out of its rolling window **survives locally**, and rows you author directly in the
> lab UI survive too. The trade-off: server-side deletes are **not** propagated (a rule
> deleted / "clear paper results" / a reaped position on the live box lingers on the
> lab until you remove it manually). Because upserts are server-wins, a lab-authored
> row sharing a UUID with the server's would still be overwritten (UUIDs collide with
> ≈0 probability). (Server-side deletes are never tombstoned back; a leftover
> `_ec2_sync_seen_ids` table from an older sync scheme is dropped on every run.)
>
> Run from the project root in PowerShell.

---

## TL;DR

```powershell
$env:PGPASSWORD = 'your_LOCAL_postgres_password'
./scripts/db-incremental-sync.ps1
```

Current-day analysis (DB sync + lake snapshot in one shot):

```powershell
./scripts/db-incremental-sync.ps1 -IncludeToday -ExportLake
```

`-SshTarget` (defaults to `ubuntu@35.158.128.131`) and `-LocalPgPort` (defaults to
`5555`, the dockerized local DB) are now baked in — pass `-LocalPgPort 5432` if you
run a native local Postgres instead.

---

## Prerequisites

- `aws-ec2-key.pem` is at `~/.ssh/aws-ec2-key.pem` (never in the repo). `-SshKey` probes
  `<repo>/../aws-ec2-key.pem` first and falls back to that, so neither path needs passing.
- `ssh` and `psql` are on `PATH` (verified: psql 18.4, OpenSSH 10.2).
- Local Postgres **≥ 16**, and you connect as a **superuser** role
  (the script runs `CREATE EXTENSION postgres_fdw` and `CREATE USER MAPPING`).
- Local backend (`cargo run -p backend`) is **stopped** while syncing.
- `$env:PGPASSWORD` set to your **local** DB password (the server password is read
  automatically from the server's `.env`).

The `postgres_fdw` extension ships with standard Postgres — no install step. The
script enables it on first run.

---

## Why this approach (vs. the old CSV pipeline)

The old `db-incremental-csv-*` scripts exported CSVs on the server, `scp`'d them
down, and `\copy`'d them in. That had three problems this design removes:

| Old CSV pipeline | This script |
| --- | --- |
| Counts every CSV line into memory, dozens of `psql` spawns, `scp` round-trip | Streamed FDW pulls; hypertables in hour chunks; nothing written to disk |
| CSV escaping / quoting / encoding + PowerShell pipe corruption | Native Postgres types over `postgres_fdw` — zero escaping |
| Pulls a dump regardless, parses watermarks from output | Watermark is a literal → **pushed down to the server**, which prunes to the recent `trades` partitions and sends only new rows |

Only genuinely-new rows cross the wire, which is gentle on the IO/RAM-constrained
EC2 box (per the data-scale guardrails in `CLAUDE.md`). Large `trades` / `raw_txs`
windows are pulled in **fixed-hour chunks** (default 6h) with FDW `fetch_size`
10k and automatic retries — a single full-window INSERT was observed to drop the
remote connection (`FETCH … FROM c1` / “server closed the connection”) on cold
local DBs.

---

## What it does (step by step)

1. **Locks down `aws-ec2-key.pem`** (idempotent `icacls`) so Windows OpenSSH accepts it.
2. **Reads server DB creds** (`DB_PORT/USER/PASSWORD/DB`) from the server's `.env` over SSH.
3. **Opens an SSH tunnel** `localhost:5433 → server:<DB_PORT>` (background, SSH keepalives) and waits until it's reachable.
4. **Attaches the server** as a `postgres_fdw` foreign server (schema `ec2_sync_src`), rebuilt fresh each run (`fetch_size` + TCP keepalives).
5. **Schema-parity guard** — compares local vs. server columns for every synced table and **aborts on drift** before moving any data.
6. **Computes local watermarks** (`MAX(block_time)`, `MAX(created_at)`, …) and the sealed-day cutoff (midnight UTC today). A **cold** local table (epoch watermark) clamps to the remote `MIN(block_time)` so FDW never scans from 1970. TimescaleDB auto-creates destination chunks on insert — no partition-ensure step.
7. **Upserts each market-data table** with the watermark as a pushed-down literal (see table below); hypertables pull `[watermark, cutoff)` in **hour chunks** (each chunk commits independently; transient FDW drops are retried with a fresh FDW attach). **`wallet_dict` is the exception** — it is **non-destructively merged** each run (one transaction), not watermark-incremental. `lab` never mints its own ids, there is **no FK** on `trades.wallet_id`, and a missing dict row makes the trade-history reads render the wallet as `unknown:<id>` (LEFT-join fallback in `trade_repo.rs`) rather than drop the trade. The merge pulls the server dict into a temp table, drops local rows whose address the server reassigned to a different id (server wins), then UPSERTs every server row by id — while **preserving local-only ids the server has aged out**, because the lab retains trade history longer than the server's 7-day window. Two shapes that do **not** work here: `id > MAX(local id) ON CONFLICT DO NOTHING` silently skips colliding server rows (a `wallet_dict` re-mint on the server then loses ~98k ids locally → 58% of trades invisible), and a naive `TRUNCATE`+full-replace fixes recent days but re-orphans the oldest retained days on every run.
7a. **Integrity** — a HARD completeness guard runs *inside* the merge transaction: every **server** id must be present locally afterward, else the sync aborts (catches a real mirror regression). A separate post-trades **report** just logs the residual orphans — trades whose `wallet_id` is absent from *both* dicts (wallets the server re-minted/aged out; they render as `unknown:<id>` and age out of lab retention). The residual is informational and does not fail the sync.
7b. **Mirrors the strategy tables** (`fingerprints` → `strategy_rules` → `strategy_runs` → `strategy_run_metrics` → `strategy_positions`, FK-safe order) **full-table, server wins, non-destructive** — no watermark (tiny vs. `trades`). For each table: `INSERT ... ON CONFLICT DO UPDATE` so new server rows are added and status/exit-fill changes propagate — but **no local row is deleted**, so the lab keeps its accumulated history (rows the server deleted/aged out) and its own lab-authored rows (the lab UI's create/update/delete-rule handlers write straight to the local DB for local backtest/paper authoring). Server-side deletes are deliberately **not** propagated — a rule/position deleted on the live box lingers on the lab until removed manually (the trade-off for retaining old local data). The one exception is a single **constraint-conflict resolver** on `strategy_runs`: it also has `UNIQUE(rule_id, mode, run_seq)`, so a divergent local run sharing that triple under a different id is dropped first (server wins) or the insert would abort — this fires only on a genuine collision, never on age. (A leftover `_ec2_sync_seen_ids` table from an older tombstone-delete scheme is dropped on every run.) The lab reads these mirrored rows for both the positions table/summary **and** the rules-table counters (open/pending/total/win/loss): the lab has no runtime cache, so its `list_*_rules` handlers compute those counters in SQL via `StrategyRepo::rule_counters_for_latest_paper_runs` (latest paper run per rule) instead of a cache read. Without this sync the lab shows all-zero counters.
8. **Syncs `_sqlx_migrations`** from the server so the local backend doesn't re-apply migrations.
9. **Detaches** — drops the foreign server (removing the server password from the local catalog) and kills the tunnel (in a `finally`, so it's cleaned up even on error).
10. **Optional lake export** (`-ExportLake`) — after detach, runs
    `cargo run -p hunter-lab -- lake-export` (adds `--include-today` when
    `-IncludeToday` was set) so hop-1 sync and hop-2 Parquet seal are one command
    for current-day simulate/sweep.

### Conflict handling

| Table | Conflict key | Action |
| --- | --- | --- |
| `wallet_dict` | `id` / `address` | **MERGE** — UPSERT server rows by id (server wins), drop local rows the server reassigned, **keep local-only old ids** (self-healing) |
| `tokens` | `mint_address` | DO NOTHING |
| `tokens_info` | `mint_address` | DO UPDATE — newer `updated_at` wins |
| `token_sync_state` | `(mint_address, venue)` | DO UPDATE — newer `last_synced_at` wins |
| `trades` | `(block_time, tx_signature, leg_index)` | DO NOTHING (append-only; includes `ix_labels`) |
| `raw_txs` | `(block_time, tx_signature)` | DO NOTHING (opt-in via `-IncludeRawTxs`) |
| `fingerprints` | `id` | DO UPDATE — server wins, non-destructive (full-table; local rows kept; required before `strategy_rules`) |
| `strategy_rules` | `id` | DO UPDATE — server wins, non-destructive (full-table; local rows kept) |
| `strategy_runs` | `id` | DO UPDATE — server wins, non-destructive (full-table; local rows kept) |
| `strategy_run_metrics` | `run_id` | DO UPDATE — server wins, non-destructive (full-table; local rows kept) |
| `strategy_positions` | `id` | DO UPDATE — server wins, non-destructive (full-table; real + paper; local rows kept) |

---

## Parameters

| Param | Default | Notes |
| --- | --- | --- |
| `-SshTarget` | `ubuntu@35.158.128.131` | user@host of the EC2 box |
| `-SshKey` | first of `../aws-ec2-key.pem`, `~/.ssh/aws-ec2-key.pem` | Path to the EC2 private key |
| `-RemoteDir` | `~/trade-launch-bot/hunter` | Where the server's `.env` lives |
| `-Database` | `hunter_bot` | Local + remote DB name |
| `-LocalPgHost` | `localhost` | |
| `-LocalPgPort` | `5555` | Dockerized local DB; use `5432` for a native local Postgres |
| `-LocalPgUser` | `postgres` | Must be a superuser role |
| `-TunnelLocalPort` | `5433` | Local end of the SSH tunnel (must be free) |
| `-FdwTunnelHost` | `host.docker.internal` | How the **local** Postgres reaches the tunnel. Dockerized DB → `host.docker.internal`; native local Postgres → `127.0.0.1` |
| `-RemotePgPort` | `0` (auto) | `0` = read `DB_PORT` from server `.env` (fallback `5555`) |
| `-IncludeRawTxs` | *(off)* | Also sync the heavy `raw_txs` BYTEA feed |
| `-IncludeToday` | *(off)* | Also pull today's still-open chunk (partial day) |
| `-ExportLake` | *(off)* | After sync, run `hunter-lab lake-export` (passes `--include-today` when `-IncludeToday`) |
| `-FdwFetchSize` | `10000` | `postgres_fdw` fetch batch size (lower = gentler on EC2 RAM) |
| `-HypertableChunkHours` | `6` | `trades` / `raw_txs` pull window size in hours |
| `-ChunkRetries` | `4` | Retries per chunk on transient FDW/tunnel drops |
| `-LocalPgPassword` | `$env:PGPASSWORD` | Local DB password |

---

## The passphrase prompt

The `.pem` is passphrase-protected. By default you'll be prompted **twice** per run
(once to read the server config, once to open the tunnel) — that's expected. The
tunnel prompt appears inline in the console; just type the passphrase.

### Optional: zero prompts via ssh-agent

Cache the key in the agent so every `ssh` runs non-interactively. The `ssh-agent`
service ships **Disabled** on Windows, so enable it once from an **elevated**
PowerShell:

```powershell
Set-Service ssh-agent -StartupType Manual   # one-time, needs admin
Start-Service ssh-agent
ssh-add "$HOME\.ssh\aws-ec2-key.pem"   # enter passphrase once
```

After that the key stays loaded across sessions and the script never prompts.

---

## Troubleshooting

- **"SSH tunnel exited early"** — wrong host, key rejected, or the server's Postgres
  host port isn't published. Confirm `DB_PORT` in the server's `.env` and
  that you can `ssh -i aws-ec2-key.pem ubuntu@35.158.128.131` manually.
- **"Tunnel port 5433 never opened"** — port `5433` is busy locally; pass a free
  `-TunnelLocalPort`.
- **"Could not reach server Postgres through the tunnel"** — server creds mismatch,
  or Postgres isn't up on the server.
- **"Column mismatch local vs server"** — local and server schemas diverged (run
  pending migrations on the lagging side). The guard refuses to load mismatched data.
- **`CREATE EXTENSION`/`USER MAPPING` permission denied** — connect as a Postgres
  superuser (`-LocalPgUser postgres`).
- **Can't connect to local DB** — wrong `-LocalPgPort` (native `5432` vs docker `5555`).
- **`could not connect to server "ec2_sync" ... port 5433 ... Connection refused`** — your local
  Postgres is dockerized and `postgres_fdw` (which connects from *inside* the container) can't see
  the host's tunnel on `127.0.0.1`. Fixed by the defaults: the tunnel binds `0.0.0.0` and
  `-FdwTunnelHost` is `host.docker.internal`. If you switched to a native local Postgres, pass
  `-FdwTunnelHost 127.0.0.1`. (Windows Firewall may prompt once to allow `ssh` to bind `0.0.0.0`.)
- **`server closed the connection` / `FETCH … FROM c1` during trades** — the remote (or local)
  Postgres dropped mid-FDW cursor, usually under RAM pressure on a large window (cold local DB
  / empty watermarks). The script now chunks hypertables (default 6h), clamps cold watermarks
  to the remote `MIN`, and retries transient drops. Re-run is safe (watermarks + `ON CONFLICT`).
  If a single chunk still fails, pass `-HypertableChunkHours 3` (or `1`) and/or
  `-FdwFetchSize 5000`.

The run is safe to re-execute at any point — watermarks + `ON CONFLICT` make it idempotent.

---

## Server reference

- Host: `ubuntu@35.158.128.131`
- Project: `/home/ubuntu/projects/hunter`
- Postgres: published on the host's `DB_PORT` (default `5555`), reached via the SSH tunnel
