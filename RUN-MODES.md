# Run Modes — Develop & Deploy

Two ways to run this project, mapped to the two compose files. Everything is
driven by a single `.env` (copy from `.env.example`; never commit it).

| Mode | File | Services | Where |
| --- | --- | --- | --- |
| **Develop** | `docker-compose.dev.yml` | postgres + **live** + **lab** | Workstation |
| **Deploy** | `docker-compose.yml` | postgres + **live** + nginx (`web`) | EC2 |

Why `lab` is dev-only: it pulls arrow/parquet/rayon + a bundled libduckdb
(compiled from C++). It must **never** ship to EC2 (2 vCPU / 4 GB). The EC2 box
runs `live` (ingest + strategies + trader + API) and nginx only.
See CLAUDE.md → "Deployed server".

---

## Develop mode (both `live` + `lab`)

Local stack over **one shared Postgres** (same `pgdata` volume as prod compose,
so data is shared on this host). No nginx/TLS — the two SPA dev servers run
natively and proxy `/api` to the dockerized backends.

```powershell
# --- Backends (docker) ---
docker compose -f docker-compose.dev.yml up -d --build      # postgres + live + lab
docker compose -f docker-compose.dev.yml up -d postgres live # live only
docker compose -f docker-compose.dev.yml up -d postgres lab  # lab only

docker compose -f docker-compose.dev.yml logs -f lab         # follow a service
docker compose -f docker-compose.dev.yml down                # stop (data persists)
docker compose -f docker-compose.dev.yml down -v             # stop + WIPE volumes

# Lake export (batch, one-off container):
docker compose -f docker-compose.dev.yml run --rm lab lab lake-export
```

Published ports (override via `LIVE_HOST_PORT` / `LAB_HOST_PORT` in `.env`):
`live` → `:8081`, `lab` → `:8082`, postgres → `:5555` (`POSTGRES_HOST_PORT`).

```powershell
# --- Frontend (native, separate terminal) ---
cd frontend-react
npm run dev          # both apps:  live :5173, lab :5174
npm run dev:live     # live only  (:5173 -> proxies /api to :8081)
npm run dev:lab      # lab only   (:5174 -> proxies /api to :8082)
```

### Native (no docker) alternative
Needs a Postgres reachable at `DATABASE_URL`. Run bins directly:

```powershell
cargo run -p live                 # needs Postgres + Helius gRPC
cargo run -p lab                  # needs Postgres only (NO keys / NO gRPC)
$env:PORT=8082; cargo run -p lab  # run lab beside live (live keeps 8081)
cargo run -p lab -- lake-export   # export sealed days -> Parquet lake
```

---

## Deploy mode (only `live`) — EC2

Flow: **SSH in → `git pull` → `docker compose up -d --build`**. The box builds
the image itself (cargo-chef caches the dep tree, so your-code-only rebuilds are
~1 min). `docker-compose.yml` is the prod stack: postgres + `live` (service named
`backend`) + nginx (`web`, the only published service, ports 80/443).

### First-time setup on the box
```bash
git clone <repo-url> meme-trading
cd meme-trading
cp .env.example .env
nano .env          # fill REAL values — see "Required .env" below
```

### Every deploy (update)
```bash
ssh <user>@<ec2-host>
cd meme-trading
git pull
docker compose up -d --build        # rebuild + restart changed services
docker compose logs -f backend      # confirm live ingest/trader came up clean
```

### Operating the box
```bash
docker compose ps                   # status
docker compose logs -f backend      # live bin logs
docker compose restart backend      # restart just the backend
docker compose down                 # stop all (pgdata volume persists)
docker compose up -d                # start without rebuild
```

> `init: true` + tini means `docker stop`/redeploy forwards SIGTERM and the bin
> exits promptly instead of being SIGKILL'd mid-trade. Safe to redeploy live.

### Required `.env` on EC2 (deploy-critical)
Compose **overrides** `HOST`/`PORT`/`DATABASE_URL` to point `live` at the
postgres service, so you mainly need real secrets + config:

- `HELIUS_API_KEY`, `HELIUS_RPC_URL`, `HELIUS_LASERSTREAM_URL` (regional, nearest box), `HELIUS_FAST_SENDER_URL`
- `WALLET_PRIVATE_KEY`, `NONCE_ACCOUNTS`
- `POSTGRES_USER` / `POSTGRES_PASSWORD` / `POSTGRES_DB`
- `API_AUTH_TOKEN` — **fail-closed**; backend refuses to start without it. nginx
  injects it as the upstream `Authorization: Bearer` for proxied `/api`.
  Generate: `head -c 32 /dev/urandom | base64`

Do **not** raise `MAX_TRADES_RETAINED`, `SEED_TOKEN_LIMIT`, pool sizes, or cache
TTLs on the box — connection counts and RAM are load-bearing (CLAUDE.md).

---

## Data: server → local (for sweeps/backtests)
Sweeps/backtests run **locally only** (EC2 keeps just a ~7-day rolling buffer).
Pull a fresh corpus from the box over an SSH tunnel:

```powershell
./scripts/db-incremental-sync.ps1   # incremental EC2 DB -> local DB
```
