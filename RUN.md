# Running the project (local dev)

How to run everything **from source on your workstation** — no Docker. Two
families (**hunter** = meme trading, **forge** = launch platform), each with a
**live** tier and a **lab** (analysis) tier.

- Run all `cargo` commands from the **repo root** (`c:\Users\User\Documents\Bot`).
- Run all `npm` commands from that family's **frontend folder** (`hunter/frontend`
  or `forge/frontend`).
- Each family needs its own Postgres + a synced `.env` (see [Prerequisites](#prerequisites)).
- **Merged** = run live + lab together (both backends + all frontend apps).
  **Split** = run just one tier on its own.
- For the **Docker / compose** runs (deploy stacks, merged & split), see
  [`deploy/DOCKER.md`](deploy/DOCKER.md) — this doc is source-only local dev.

---

## Ports at a glance (local dev defaults)

| | hunter | forge |
| --- | --- | --- |
| live backend | `:8130` | `:8230` |
| lab backend | `:8140` | `:8240` |
| live frontend (vite) | `:5173` → proxies `/api` to `:8130` | `:5175` → proxies `/api` to `:8230` |
| lab frontend (vite) | `:5174` → proxies `/api` to `:8140` | *(none — forge lab is API-only)* |
| Postgres | `:5555` | `:5556` |

> The backend ports now MATCH the Docker `*_API_PORT`s in `deploy/DOCKER.md`, so a
> given backend answers on the same number whether you run it from source or in
> Docker. The **frontend** dev ports (5173/5174/5175) stay Vite-dev-only — the
> Docker UI is nginx on 81xx/82xx, a different server.

---

## Prerequisites

1. **Postgres running** for the family you're working on, and a `DATABASE_URL`
   in that family's `.env` pointing at it. Easiest is to run just the DB from the
   deploy stack:

   ```powershell
   # hunter DB on :5555
   docker compose --env-file hunter/.env -f deploy/hunter.compose.yml up -d postgres
   # forge DB on :5556
   docker compose --env-file forge/.env  -f deploy/forge.compose.yml  up -d postgres
   ```

2. **`.env` synced** (gitignored; copy from `.env.example` and fill real values):

   ```powershell
   Copy-Item hunter/.env.example hunter/.env   # then fill in real values
   Copy-Item forge/.env.example  forge/.env
   ```

   `hunter/live` additionally needs `HELIUS_RPC_URL` (gRPC feed) + keys; `lab`
   needs **no** keys / **no** gRPC.

   The bins pick their local-dev bind port from `.env`: live reads `LIVE_PORT`
   (8130), lab reads `LAB_PORT` (8140) — the SAME numbers as the deploy
   `*_API_PORT`s, so local and Docker use one backend port — and live/lab default
   to different ports so you never pass an inline `PORT=…`. (Docker overrides both
   with the injected `PORT`.)

3. **Frontend deps installed** (once per frontend folder):

   ```powershell
   cd hunter/frontend; npm install
   cd forge/frontend;  npm install
   ```

---

## hunter

### Merged (live + lab)

Run each in its own terminal.

```powershell
# 1) live backend  → :8130   (needs Postgres + Helius gRPC + keys)
cargo run -p hunter-live

# 2) lab backend   → :8140   (needs Postgres only)
cargo run -p hunter-lab

# 3) both frontends → live :5173, lab :5174
cd hunter/frontend; npm run dev
```

Open `http://localhost:5173` (live) and `http://localhost:5174` (lab).

### Split — live only

```powershell
cargo run -p hunter-live                      # backend  → :8130
cd hunter/frontend; npm run dev:live          # frontend → :5173
```

### Split — lab only

```powershell
cargo run -p hunter-lab                        # backend  → :8140 (LAB_PORT)
cd hunter/frontend; npm run dev:lab            # frontend → :5174
```

### hunter extras

```powershell
cargo run -p hunter-lab -- lake-export        # export sealed PG days → Parquet lake ($SWEEP_LAKE_DIR)
cargo run -p hunter-lab -- lake-export --include-today   # include today's (unsealed) tokens
cargo run -p hunter-lab -- reroll-run <run-uuid>...      # recompute a FINISHED run's strategy_run_metrics
cargo run -p hunter-live -- probe <ladder|fanout|simulate-sell|holdings> [args]
```

`reroll-run` is the manual lever for a finalized run whose membership changed after
the fact (a position reattributed between runs, a straggler that settled while the
process was down). It folds through the same `exact_run_metrics` kernel a live
finalize uses — never recompute run metrics in SQL. It refuses a still-`Running`
run, because "a `strategy_run_metrics` row exists" is what the run navigator reads
as "this run is over" (`--force` overrides).

---

## forge

forge's **lab is API-only** — there is no lab frontend. The single forge frontend
(`:5175`) proxies to the **live** backend (`:8230`).

### Merged (live + lab)

```powershell
# 1) live backend → :8230   (needs Postgres + keys)
cargo run -p forge-live

# 2) lab backend  → :8240   (needs Postgres only; API-only, no UI)
cargo run -p forge-lab

# 3) frontend → :5175 (proxies /api to the live backend :8230)
cd forge/frontend; npm run dev
```

Open `http://localhost:5175`. Hit the lab API directly on `http://localhost:8240`.

### Split — live only

```powershell
cargo run -p forge-live                       # backend  → :8230
cd forge/frontend; npm run dev                # frontend → :5175
```

### Split — lab only

```powershell
cargo run -p forge-lab                        # API only → :8240  (no frontend)
```

### forge extras

```powershell
cargo run -p forge-lab -- lake-export         # export sealed PG days → Parquet lake
```

---

## Typecheck / test / build

```powershell
# Backend (per crate — stay in the owning crate)
cargo check -p hunter-live      # or hunter-lab / hunter-core / forge-live / forge-lab / forge-core
cargo test  -p hunter-live      # unit tests; add `-- --ignored` for DB/gRPC integration tests

# Frontend (from the family's frontend folder)
cd hunter/frontend; npm run build:live   # tsc + vite build (ship this to prod)
cd hunter/frontend; npm run build:lab    # workstation-only lab bundle (never deployed)
cd forge/frontend;  npm run build        # single forge bundle
```

---

## Notes

- **Bare `cargo run` at the root** builds only the hunter bins (`default-members`).
  Always target a crate with `-p <name>`.
- **Per-bin ports live in `.env`** (`LIVE_PORT`/`LAB_PORT` for hunter,
  `LIVE_PORT`/`LAB_PORT` for forge) — change them there, no inline override.
  Docker's injected `PORT` still wins over both.
- Override any vite proxy target without editing config via env vars:
  `VITE_LIVE_DEV_PROXY_TARGET` / `VITE_LAB_DEV_PROXY_TARGET` (hunter),
  `VITE_LIVE_PROXY` (forge).
- To run hunter **and** forge at the same time, use separate terminals — their
  ports (81xx/5555 vs 82xx/5556) don't collide.
