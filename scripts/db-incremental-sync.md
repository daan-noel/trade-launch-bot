# Incremental DB Sync — `db-incremental-sync.ps1`

Pull the EC2 server's **newer** data and append it into your local `meme_bot` DB —
directly **DB → DB** over an SSH tunnel. No CSV files, no `scp`, no temp files.

Safe to run repeatedly. **Non-destructive**: your local sweep results, settings,
`raw_txs`, and existing trades are never touched — only new rows are added (and a
few metadata rows refreshed).

> **Strategy tables are mirrored (server wins).** `strategy_rules`, `strategy_runs`,
> `strategy_run_metrics`, and `strategy_positions` are copied **full-table each run**
> and upserted (`DO UPDATE`) so the LIVE box's real **and** paper positions are
> viewable on the lab. Because it's server-wins, a lab-authored rule/run sharing a
> UUID with the server's would be overwritten (UUIDs collide with ≈0 probability).
>
> Run from the project root in PowerShell.

---

## TL;DR

```powershell
$env:PGPASSWORD = 'your_LOCAL_postgres_password'
./scripts/db-incremental-sync.ps1
```

`-SshTarget` (defaults to `ubuntu@54.93.174.192`) and `-LocalPgPort` (defaults to
`5555`, the dockerized local DB) are now baked in — pass `-LocalPgPort 5432` if you
run a native local Postgres instead.

---

## Prerequisites

- `aws-ec2-key.pem` is at `F:\pumpfun\meme-trading\aws-ec2-key.pem` (gitignored — never commit it).
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
| Counts every CSV line into memory, dozens of `psql` spawns, `scp` round-trip | One streamed pull per table; nothing written to disk |
| CSV escaping / quoting / encoding + PowerShell pipe corruption | Native Postgres types over `postgres_fdw` — zero escaping |
| Pulls a dump regardless, parses watermarks from output | Watermark is a literal → **pushed down to the server**, which prunes to the recent `trades` partitions and sends only new rows |

Only genuinely-new rows cross the wire, which is gentle on the IO/RAM-constrained
EC2 box (per the data-scale guardrails in `CLAUDE.md`).

---

## What it does (step by step)

1. **Locks down `aws-ec2-key.pem`** (idempotent `icacls`) so Windows OpenSSH accepts it.
2. **Reads server DB creds** (`POSTGRES_HOST_PORT/USER/PASSWORD/DB`) from the server's `.env` over SSH.
3. **Opens an SSH tunnel** `localhost:5433 → server:<POSTGRES_HOST_PORT>` (background) and waits until it's reachable.
4. **Attaches the server** as a `postgres_fdw` foreign server (schema `ec2_sync_src`), rebuilt fresh each run.
5. **Schema-parity guard** — compares local vs. server columns for every synced table and **aborts on drift** before moving any data.
6. **Computes local watermarks** (`MAX(block_time)`, `MAX(created_at)`, …) and the sealed-day cutoff (midnight UTC today). TimescaleDB auto-creates destination chunks on insert — no partition-ensure step.
7. **Upserts each market-data table** with the watermark as a pushed-down literal (see table below); hypertables pull only sealed days `[watermark, cutoff)`.
7b. **Mirrors the strategy tables** (`strategy_rules` → `strategy_runs` → `strategy_run_metrics` → `strategy_positions`, FK-safe order) **full-table, server wins** — no watermark (tiny vs. `trades`), `DO UPDATE` refreshes changed rows so status/exit-fill updates propagate. The lab reads these mirrored rows for both the positions table/summary **and** the rules-table counters (open/pending/total/win/loss): the lab has no runtime cache, so its `list_*_rules` handlers compute those counters in SQL via `StrategyRepo::rule_counters_for_latest_paper_runs` (latest paper run per rule) instead of a cache read. Without this sync the lab shows all-zero counters.
8. **Syncs `_sqlx_migrations`** from the server so the local backend doesn't re-apply migrations.
9. **Detaches** — drops the foreign server (removing the server password from the local catalog) and kills the tunnel (in a `finally`, so it's cleaned up even on error).

### Conflict handling

| Table | Conflict key | Action |
| --- | --- | --- |
| `wallet_dict` | `id` / `address` | DO NOTHING (immutable; ids mirrored verbatim) |
| `tokens` | `mint_address` | DO NOTHING |
| `tokens_info` | `mint_address` | DO UPDATE — newer `updated_at` wins |
| `token_sync_state` | `(mint_address, venue)` | DO UPDATE — newer `last_synced_at` wins |
| `trades` | `(block_time, tx_signature, leg_index)` | DO NOTHING (append-only) |
| `raw_txs` | `(block_time, tx_signature)` | DO NOTHING (opt-in via `-IncludeRawTxs`) |
| `strategy_rules` | `id` | DO UPDATE — server wins (full-table) |
| `strategy_runs` | `id` | DO UPDATE — server wins (full-table) |
| `strategy_run_metrics` | `run_id` | DO UPDATE — server wins (full-table) |
| `strategy_positions` | `id` | DO UPDATE — server wins (full-table; real + paper) |

---

## Parameters

| Param | Default | Notes |
| --- | --- | --- |
| `-SshTarget` | `ubuntu@54.93.174.192` | user@host of the EC2 box |
| `-SshKey` | `../aws-ec2-key.pem` | Path to the EC2 private key |
| `-RemoteDir` | `~/projects/meme-trading` | Where the server's `.env` lives |
| `-Database` | `meme_bot` | Local + remote DB name |
| `-LocalPgHost` | `localhost` | |
| `-LocalPgPort` | `5555` | Dockerized local DB; use `5432` for a native local Postgres |
| `-LocalPgUser` | `postgres` | Must be a superuser role |
| `-TunnelLocalPort` | `5433` | Local end of the SSH tunnel (must be free) |
| `-FdwTunnelHost` | `host.docker.internal` | How the **local** Postgres reaches the tunnel. Dockerized DB → `host.docker.internal`; native local Postgres → `127.0.0.1` |
| `-RemotePgPort` | `0` (auto) | `0` = read `POSTGRES_HOST_PORT` from server `.env` (fallback `5555`) |
| `-IncludeRawTxs` | *(off)* | Also sync the heavy `raw_txs` BYTEA feed |
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
ssh-add "F:\pumpfun\meme-trading\aws-ec2-key.pem"   # enter passphrase once
```

After that the key stays loaded across sessions and the script never prompts.

---

## Troubleshooting

- **"SSH tunnel exited early"** — wrong host, key rejected, or the server's Postgres
  host port isn't published. Confirm `POSTGRES_HOST_PORT` in the server's `.env` and
  that you can `ssh -i aws-ec2-key.pem ubuntu@54.93.174.192` manually.
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

The run is safe to re-execute at any point — watermarks + `ON CONFLICT` make it idempotent.

---

## Server reference

- Host: `ubuntu@54.93.174.192`
- Project: `/home/ubuntu/projects/meme-trading`
- Postgres: published on the host's `POSTGRES_HOST_PORT` (default `5555`), reached via the SSH tunnel
