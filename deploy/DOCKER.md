# Docker commands

Run from the **repo root** (`c:\Users\User\Documents\Bot`) — the build context is the
cargo workspace there.

For forge, swap throughout: `hunter`→`forge`, `hunter_bot`→`forge_bot`,
`hunter-lab`→`forge-lab`, `deploy/hunter.compose.yml`→`deploy/forge.compose.yml`,
ports `5555`/`81xx`→`5556`/`82xx`.

| Compose file | Runs | Env file |
| --- | --- | --- |
| `deploy/hunter.compose.yml` | hunter live **+** lab | `hunter/.env` |
| `deploy/forge.compose.yml` | forge live **+** lab (API-only) | `forge/.env` |

| Flag | Meaning |
| --- | --- |
| `-f <file>` | Which compose file (always required — there are two). |
| `--env-file <file>` | Load ports/secrets. |
| `-d` | Detached. Omit to stream logs. |
| `--build` | Rebuild images from source first. |
| `run --rm <svc> <cmd>` | One-off command in a throwaway container. |
| `down -v` | Also delete volumes — ⚠️ **wipes database, lake, and event log.** |

## Up / update

```bash
docker compose --env-file hunter/.env -f deploy/hunter.compose.yml up -d --build
```

> ⚠ **On the live EC2 box, name the services** — a bare `--build` also builds `lab-api`,
> which compiles the bundled libduckdb amalgamation and must never be built there:
>
> ```bash
> docker compose --env-file hunter/.env -f deploy/hunter.compose.yml \
>   up -d --build postgres live-api live-ui
> ```

On the workstation, `export CARGO_BUILD_JOBS=8` before `up --build` (build arg defaults
to `2`, sized for the 2vCPU EC2).

## Stop

```bash
docker compose -f deploy/hunter.compose.yml down    # remove containers, KEEP volumes
docker compose -f deploy/hunter.compose.yml stop    # keep containers (faster restart)
```

## Status / logs

```bash
docker compose -f deploy/hunter.compose.yml ps
docker compose -f deploy/hunter.compose.yml logs -f            # whole stack
docker compose -f deploy/hunter.compose.yml logs -f live-api
```

Services: `postgres` · `live-api` · `live-ui` · `lab-api` · `lab-ui`.

## Restart / rebuild one service

```bash
docker compose -f deploy/hunter.compose.yml restart live-api
docker compose --env-file hunter/.env -f deploy/hunter.compose.yml up -d --build live-api
```

## Lake export

```bash
docker compose --env-file hunter/.env -f deploy/hunter.compose.yml run --rm lab-api hunter-lab lake-export
```

## Database / shell

```bash
psql postgres://postgres:<password>@localhost:5555/hunter_bot
docker compose -f deploy/hunter.compose.yml exec postgres psql -U postgres -d hunter_bot
docker compose -f deploy/hunter.compose.yml exec live-api sh
```

## Ports

Set in `.env` (scheme `8·F·C·P`); defaults below.

| Service | hunter | forge |
| --- | --- | --- |
| Postgres | `5555` | `5556` |
| live UI (http / https) | `8110` / `8111` | `8210` / `8211` |
| live API | *(internal, via nginx)* | *(internal, via nginx)* |
| lab API | `8140` | `8240` |
| lab UI (http / https) | `8120` / `8121` | *(none)* |

## Housekeeping

```bash
docker system prune    # stopped containers + dangling images; keeps named volumes
docker volume ls       # hunter-pgdata, hunter-lakedata, hunter-eventlog, forge-*
```

Disk-full / 502 runbook: [EC2-DISK-HOUSEKEEPING.md](EC2-DISK-HOUSEKEEPING.md).
