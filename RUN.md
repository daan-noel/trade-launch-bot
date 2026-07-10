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
| live backend | `:8081` | `:8091` |
| lab backend | `:8082` | `:8092` |
| live frontend (vite) | `:5173` → proxies `/api` to `:8081` | `:5175` → proxies `/api` to `:8091` |
| lab frontend (vite) | `:5174` → proxies `/api` to `:8082` | *(none — forge lab is API-only)* |
| Postgres | `:5555` | `:5556` |

> Note the difference from the **Docker** ports (81xx/82xx) in `deploy/DOCKER.md`.
> These are the local-from-source defaults.

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
   (8081), lab reads `LAB_PORT` (8082) — so live and lab already default to
   different ports and you never pass an inline `PORT=…`. (Docker overrides both
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
# 1) live backend  → :8081   (needs Postgres + Helius gRPC + keys)
cargo run -p hunter-live

# 2) lab backend   → :8082   (needs Postgres only)
cargo run -p hunter-lab

# 3) both frontends → live :5173, lab :5174
cd hunter/frontend; npm run dev
```

Open `http://localhost:5173` (live) and `http://localhost:5174` (lab).

### Split — live only

```powershell
cargo run -p hunter-live                      # backend  → :8081
cd hunter/frontend; npm run dev:live          # frontend → :5173
```

### Split — lab only

```powershell
cargo run -p hunter-lab                        # backend  → :8082 (LAB_PORT)
cd hunter/frontend; npm run dev:lab            # frontend → :5174
```

### hunter extras

```powershell
cargo run -p hunter-lab -- lake-export        # export sealed PG days → Parquet lake ($SWEEP_LAKE_DIR)
cargo run -p hunter-lab -- lake-export --include-today   # include today's (unsealed) tokens
cargo run -p hunter-live -- probe <ladder|fanout|simulate-sell|holdings> [args]
```

---

## forge

forge's **lab is API-only** — there is no lab frontend. The single forge frontend
(`:5175`) proxies to the **live** backend (`:8091`).

### Merged (live + lab)

```powershell
# 1) live backend → :8091   (needs Postgres + keys)
cargo run -p forge-live

# 2) lab backend  → :8092   (needs Postgres only; API-only, no UI)
cargo run -p forge-lab

# 3) frontend → :5175 (proxies /api to the live backend :8091)
cd forge/frontend; npm run dev
```

Open `http://localhost:5175`. Hit the lab API directly on `http://localhost:8092`.

### Split — live only

```powershell
cargo run -p forge-live                       # backend  → :8091
cd forge/frontend; npm run dev                # frontend → :5175
```

### Split — lab only

```powershell
cargo run -p forge-lab                        # API only → :8092  (no frontend)
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
