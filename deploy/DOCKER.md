# Docker commands

A command reference for building, running, and operating the Docker stacks in
this repo. Run everything from the **repo root** (`c:\Users\User\Documents\Bot`) —
the build context is the cargo workspace at the root.

---

## The two compose files

There are two families (**hunter** = meme-coin trading, **forge** = launch platform).
Each family has **one merged compose file** that runs its live tier and lab tier
together against a single shared Postgres:

| Compose file | Runs | Env file |
| --- | --- | --- |
| `deploy/hunter.compose.yml` | hunter live **+** lab | `hunter/.env` |
| `deploy/forge.compose.yml` | forge live **+** lab (lab is API-only) | `forge/.env` |

> Within a family the live and lab tiers **share** the `<family>-postgres`
> container and `<family>-pgdata` volume — that is the whole point of the merged
> file (two Postgres postmasters can't mount one PGDATA). hunter and forge use
> different names/ports, so the two families **can** run together on one host.

The per-tier Dockerfiles and nginx templates still live under
`deploy/hunter-live/`, `deploy/hunter-lab/`, `deploy/forge-live/`, and
`deploy/forge-lab/` (the build context is the repo root, so their paths are
unchanged — only the compose files moved up into `deploy/`).

---

## Command flags

| Flag | Meaning |
| --- | --- |
| `-f <file>` | Which compose file to use (always required — we have two). |
| `--env-file <file>` | Load ports/secrets from `hunter/.env` or `forge/.env`. |
| `-d` | Detached: run in the background. Omit to watch logs in the foreground. |
| `--build` | Rebuild images from source first (use after code changes). |
| `run --rm <svc> <cmd>` | Run a one-off command in a throwaway container, then remove it. |
| `down -v` | Also delete volumes — ⚠️ **wipes the database, lake, and event log.** |

---

## Bring a stack up (build + start, background)

```bash
docker compose --env-file hunter/.env -f deploy/hunter.compose.yml up -d --build
docker compose --env-file forge/.env  -f deploy/forge.compose.yml  up -d --build
```

- `--build` recompiles images. Drop it to just (re)start existing images.
- `-d` runs in the background. Drop it to stream logs live in the foreground.
- **Updating after a code change = the same `up -d --build` command.** Compose
  rebuilds only what changed and recreates just those containers.

---

## Stop a stack

```bash
# Stop + remove containers/network, KEEP the database + lake volumes
docker compose -f deploy/hunter.compose.yml down

# Stop but keep the containers (faster restart)
docker compose -f deploy/hunter.compose.yml stop
```

Swap in `deploy/forge.compose.yml` for the forge family.

> ⚠️ Add `-v` (`down -v`) only if you want to **delete the database and lake** and
> start clean.

---

## Status and logs

```bash
# Containers in this stack
docker compose -f deploy/hunter.compose.yml ps

# All containers on the host
docker ps

# Follow logs for the whole stack
docker compose -f deploy/hunter.compose.yml logs -f

# Follow logs for one service (postgres | live-api | live-ui | lab-api | lab-ui)
docker compose -f deploy/hunter.compose.yml logs -f live-api
```

---

## Restart / rebuild one service

```bash
docker compose -f deploy/hunter.compose.yml restart live-api
docker compose --env-file hunter/.env -f deploy/hunter.compose.yml up -d --build live-api
```

---

## Batch job: lake export

`run --rm` runs a one-off command in a throwaway container. The lab bin differs
per family (`hunter-lab` for hunter, `forge-lab` for forge):

```bash
# hunter
docker compose --env-file hunter/.env -f deploy/hunter.compose.yml run --rm lab-api hunter-lab lake-export

# forge
docker compose --env-file forge/.env  -f deploy/forge.compose.yml  run --rm lab-api forge-lab lake-export
```

---

## Access the database

Default DB host ports: **hunter = 5555**, **forge = 5556** (override in `.env`).

```bash
# From the host
psql postgres://postgres:<password>@localhost:5555/hunter_bot        # hunter
psql postgres://postgres:<password>@localhost:5556/forge_bot         # forge

# Inside the running container
docker compose -f deploy/hunter.compose.yml exec postgres psql -U postgres -d hunter_bot
docker compose -f deploy/forge.compose.yml  exec postgres psql -U postgres -d forge_bot
```

Open a shell inside any service:

```bash
docker compose -f deploy/hunter.compose.yml exec live-api sh
```

---

## Default host ports

Set in `.env` (scheme `8·F·C·P`); the values below are the defaults.

| Service | hunter | forge |
| --- | --- | --- |
| Postgres | `5555` | `5556` |
| live UI (http / https) | `8110` / `8111` | `8210` / `8211` |
| live API | *(internal only, via nginx)* | *(internal only, via nginx)* |
| lab API | `8140` | `8240` |
| lab UI (http / https) | `8120` / `8121` | *(none — forge lab is API-only)* |

---

## Housekeeping

For the live EC2 disk-full / 502 runbook (build-cache prune, orphan volumes,
zombie systemd units), see [EC2-DISK-HOUSEKEEPING.md](EC2-DISK-HOUSEKEEPING.md).

```bash
# Remove stopped containers + dangling images (keeps named volumes)
docker system prune

# List volumes (your data: hunter-pgdata, hunter-lakedata, hunter-eventlog,
# forge-pgdata, forge-lakedata). hunter-eventlog holds the strategy event log that
# boot recovery replays to re-arm — deleting it costs the armed state, not history.

docker volume ls
```

---

## Quick reference card

```bash
# Up (hunter), background, rebuild
docker compose --env-file hunter/.env -f deploy/hunter.compose.yml up -d --build

# Logs (one service)
docker compose -f deploy/hunter.compose.yml logs -f live-api

# Status
docker compose -f deploy/hunter.compose.yml ps

# Stop (keep data)
docker compose -f deploy/hunter.compose.yml down

# Lake export
docker compose --env-file hunter/.env -f deploy/hunter.compose.yml run --rm lab-api hunter-lab lake-export

# DB shell
docker compose -f deploy/hunter.compose.yml exec postgres psql -U postgres -d hunter_bot
```

For forge: swap `hunter`→`forge`, `hunter_bot`→`forge_bot`, `hunter-lab`→`forge-lab`,
the compose file `deploy/hunter.compose.yml`→`deploy/forge.compose.yml`, and ports
`5555`/`81xx`→`5556`/`82xx`.
