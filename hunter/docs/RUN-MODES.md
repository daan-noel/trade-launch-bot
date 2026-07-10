# Run Modes — Develop & Deploy

`hunter` is one product folder in the `Bot/` monorepo. Two Rust bins (`live`,
`lab`) over a shared core, plus a two-app Vite frontend (`hunter/frontend`:
`live` entry + `lab` entry). Everything is driven by a single **`hunter/.env`**
(copy from `hunter/.env.example`; never commit it). All host/service ports live
in that file's **`PORTS`** block (scheme `8·F·C·P` — see the comment there).

| Mode | How | Postgres | Where |
| --- | --- | --- | --- |
| **Develop** | native bins + Vite dev servers | one local PG (dockerized or native) | Workstation |
| **Deploy** | `deploy/hunter.compose.yml` (merged live + lab) | **one shared** `pgdata` for live+lab | One shared box |

Why `lab` is special: it pulls arrow/parquet/rayon + a bundled libduckdb
(compiled from C++) and runs heavy sweeps. Keep it off a small (2 vCPU / 4 GB)
live box — co-locate it only on a box sized well above the old live EC2 (it shares
the merged stack's Postgres with the latency-sensitive `live` hot path). See
CLAUDE.md → "Deployed server".

> **Always pass `--env-file hunter/.env`** to every `docker compose` command
> below. Compose reads `${VAR}` port values from the file named by `--env-file`
> (or the shell) — **not** from a service's `env_file:` block. Without it, compose
> falls back to the built-in defaults baked into the compose file and any port you
> changed in `.env` is silently ignored.

---

## Develop mode (fast local loop)

Run the bins natively and the two SPAs on Vite dev servers. You just need a
Postgres reachable at `DATABASE_URL`. Easiest is to start only the dockerized
Postgres from the merged compose (published on `DB_PORT`, default 5555):

```powershell
# Postgres only (dockerized) — or use a native local Postgres instead
docker compose --env-file hunter/.env -f deploy/hunter.compose.yml up -d postgres

# --- Backends (native, separate terminals) ---
cargo run -p live                 # needs Postgres + Helius gRPC/keys (binds LIVE_PORT :8130)
cargo run -p lab                  # binds LAB_PORT :8140 by default — runs beside live, no override
cargo run -p lab -- lake-export                  # export sealed days -> Parquet lake
cargo run -p lab -- lake-export --include-today  # also today's open day (sweep current-day data)

# --- Frontend (native, separate terminal) ---
cd hunter/frontend
npm run dev          # both apps:  live :5173, lab :5174
npm run dev:live     # live only  (:5173 -> proxies /api to the live bin)
npm run dev:lab      # lab only   (:5174 -> proxies /api to the lab bin)
```

> `lake-export` writes only **sealed** (strictly-before-today UTC) days into the
> Parquet lake, skipping any day whose immutable file already exists. Add
> `--include-today` to also snapshot today's still-open day (force-overwritten
> each run). The lake is the sole sweep corpus, so `--include-today` is the only
> way to sweep current-day data.

---

## Deploy mode — co-located (one shared box)

All tiers on one host, **sharing one Postgres** (`hunter-postgres`, volume
`pgdata`) via the single merged compose file. Services: `postgres`, `live-api`,
`live-ui`, `lab-api`, `lab-ui`. Only the two nginx UIs and `lab-api` are
published; `live-api` is reached only through `live-ui`'s nginx.

```bash
docker compose --env-file hunter/.env -f deploy/hunter.compose.yml up -d --build
docker compose --env-file hunter/.env -f deploy/hunter.compose.yml ps
docker compose --env-file hunter/.env -f deploy/hunter.compose.yml logs -f live-api
docker compose --env-file hunter/.env -f deploy/hunter.compose.yml down        # pgdata persists
```

Published (defaults, all in the `.env` PORTS block): postgres `DB_PORT` 5555 ·
live-ui `LIVE_UI_HTTP_PORT`/`LIVE_UI_HTTPS_PORT` 8110/8111 · lab-ui
`LAB_UI_HTTP_PORT`/`LAB_UI_HTTPS_PORT` 8120/8121 · lab-api `LAB_API_PORT` 8140.

Lake export as a one-off container:

```bash
docker compose --env-file hunter/.env -f deploy/hunter.compose.yml run --rm lab-api hunter-lab lake-export
```

> **Per-tier boxes.** The old split `hunter-live`/`hunter-lab` compose files were
> retired in favor of this single merged file. To put just one tier on a box,
> bring up only the services you want (compose still starts their `depends_on`):
> `... -f deploy/hunter.compose.yml up -d --build live-api live-ui` for a live-only
> box, or `... up -d --build lab-api lab-ui` for a lab-only box. Both target the
> same `hunter-postgres`, so run only one tier-subset per host unless you also
> override `DB_PORT`/UI ports.

### First-time setup on a box
```bash
git clone <repo-url> Bot && cd Bot
cp hunter/.env.example hunter/.env
nano hunter/.env      # fill REAL secrets — see "Required .env" below
```

> `init: true` + tini means `docker stop`/redeploy forwards SIGTERM and the bin
> exits promptly instead of being SIGKILL'd mid-trade. Safe to redeploy live.

### Required `.env` (deploy-critical)
Compose **overrides** `HOST`/`PORT`/`DATABASE_URL` to point the bins at the
`postgres` service, so you mainly need real secrets + config:

- `HELIUS_API_KEY`, `HELIUS_RPC_URL`, `HELIUS_LASERSTREAM_URL` (regional, nearest box), `HELIUS_FAST_SENDER_URL`
- `WALLET_PRIVATE_KEY`, `NONCE_ACCOUNTS`
- `POSTGRES_USER` / `POSTGRES_PASSWORD` / `POSTGRES_DB`
- `API_AUTH_TOKEN` — **fail-closed**; the backend refuses to start without it, and
  nginx injects it as the upstream `Authorization: Bearer` for proxied `/api`.
  Generate: `head -c 32 /dev/urandom | base64`
- The `PORTS` block (`DB_PORT`, `LIVE_API_PORT`, `LAB_API_PORT`, `LIVE_UI_*`,
  `LAB_UI_*`) — `LIVE_API_PORT`/`LAB_API_PORT` also drive the nginx upstream, so
  they must be present.

Do **not** raise `MAX_TRADES_RETAINED`, `SEED_TRACKING_LIMIT`, pool sizes, or
cache TTLs on the live box — connection counts and RAM are load-bearing (CLAUDE.md).

---

## Data: server → local (for sweeps/backtests)
Sweeps/backtests run **locally only** (the live box keeps just a ~7-day rolling
buffer). Pull a fresh corpus from the box over an SSH tunnel — the script reads
the server's `DB_PORT` from its `.env` automatically:

```powershell
./scripts/db-incremental-sync.ps1   # incremental server DB -> local DB
```
