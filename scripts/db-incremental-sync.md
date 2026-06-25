# Incremental DB Sync — `db-incremental-sync.ps1`

Pull the EC2 server's **newer** data and append it into your local `meme_bot` DB —
directly **DB → DB** over an SSH tunnel. No CSV files, no `scp`, no temp files.

Safe to run repeatedly. **Non-destructive**: your local sweep results, positions,
settings, `raw_transactions`, and existing trades are never touched — only new
rows are added (and a few metadata rows refreshed).

> Run from the project root in PowerShell.

---

## TL;DR

```powershell
$env:PGPASSWORD = 'your_LOCAL_postgres_password'
./scripts/db-incremental-sync.ps1 -SshTarget ubuntu@54.93.174.192
```

If your **local** `meme_bot` runs in Docker (port `5555`) instead of a native
install (`5432`), add `-LocalPgPort 5555`.

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
5. **Schema-parity guard** — compares local vs. server columns for all 5 tables and **aborts on drift** before moving any data.
6. **Ensures local `trades` day-partitions** exist for the incoming window.
7. **Computes local watermarks** (`MAX(block_time)`, `MAX(created_at)`, …).
8. **Upserts each table** with the watermark as a pushed-down literal (see table below).
9. **Detaches** — drops the foreign server (removing the server password from the local catalog) and kills the tunnel (in a `finally`, so it's cleaned up even on error).

### Conflict handling

| Table | Conflict key | Action |
| --- | --- | --- |
| `trades` | `(tx_signature, leg_index, block_time)` | DO NOTHING |
| `tokens` | `mint_address` | DO NOTHING |
| `tokens_info` | `mint_address` | DO UPDATE — newer `updated_at` wins |
| `tokens_analysis` | `(mint_address, analyzer_name)` | DO UPDATE — newer `computed_at` wins |
| `creator_profiles` | `wallet_address` | Full upsert (no monotonic timestamp; skip with `-NoCreatorProfiles`) |

---

## Parameters

| Param | Default | Notes |
| --- | --- | --- |
| `-SshTarget` | *(required)* | e.g. `ubuntu@54.93.174.192` |
| `-SshKey` | `../aws-ec2-key.pem` | Path to the EC2 private key |
| `-RemoteDir` | `~/projects/meme-trading` | Where the server's `.env` lives |
| `-Db` | `meme_bot` | Local + remote DB name |
| `-LocalPgHost` | `localhost` | |
| `-LocalPgPort` | `5432` | Use `5555` if your local DB is the dockerized one |
| `-LocalPgUser` | `postgres` | Must be a superuser role |
| `-TunnelLocalPort` | `5433` | Local end of the SSH tunnel (must be free) |
| `-RemotePgPort` | `0` (auto) | `0` = read `POSTGRES_HOST_PORT` from server `.env` (fallback `5555`) |
| `-PartitionDays` | `40` | Days of `trades` partitions to pre-create (≥ server `KEEP_DAYS` + margin) |
| `-NoCreatorProfiles` | *(off)* | Skip the `creator_profiles` full upsert |
| `-LocalPgPassword` | `$env:PGPASSWORD` | Local DB password |

---

## Avoid the key passphrase prompt

The `.pem` is passphrase-protected, so SSH prompts on each run. Cache it once per
PowerShell session:

```powershell
Start-Service ssh-agent
ssh-add "F:\pumpfun\meme-trading\aws-ec2-key.pem"
```

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

The run is safe to re-execute at any point — watermarks + `ON CONFLICT` make it idempotent.

---

## Which DB script do I want?

| Goal | Script |
| --- | --- |
| **Append** the server's new data, keep everything local | **`db-incremental-sync.ps1`** ← this one |
| **Full refresh** — replace local tokens/trades/etc. with the server snapshot | `db-snapshot-restore.ps1` (+ `db-snapshot-dump.sh` on the server) |
| Legacy CSV append (fallback only, no direct tunnel) | `db-incremental-csv-restore.ps1` (+ `db-incremental-csv-dump.sh`) |

---

## Server reference

- Host: `ubuntu@54.93.174.192`
- Project: `/home/ubuntu/projects/meme-trading`
- Postgres: published on the host's `POSTGRES_HOST_PORT` (default `5555`), reached via the SSH tunnel
