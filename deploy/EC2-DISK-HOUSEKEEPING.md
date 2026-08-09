# EC2 disk housekeeping (live box)

Ops runbook for the **deployed hunter-live EC2** (2vCPU / ~4GB RAM, single
root volume). Goal: keep the box from filling the disk again — a full disk
stops Postgres writes, kills `live-api`, and nginx returns **502** for every
`/api` request. Ingest cannot backfill the gap.

Related: command reference in [DOCKER.md](DOCKER.md).

---

## Incident pattern (2026-07-23)

| UTC time | What happened |
| --- | --- |
| ~20:47–20:56 | Containers flapping; journal under memory pressure |
| **20:58:03** | `No space left on device` (journald + rsyslog) |
| **20:58:09** | Last `trades` row written |
| 20:58 → 22:35 | Disk full → API/DB unusable → site **502** |
| **22:35–22:36** | Manual `docker compose` restart → ingest resumes |

Gap in DB: ~1h38m with zero trades/tokens. Root cause was **disk full**, not
Helius or a code bug.

Typical fillers on this box:

| Hog | Order of size | Cause |
| --- | --- | --- |
| Docker **build cache** (`containerd` snapshots) | tens of GB | repeated `docker compose build` / `up --build` **on the live EC2** |
| Unused images (forge/lab/old stacks) | tens of GB | leftover after rebuilds |
| Orphan volumes (e.g. `meme-trading_pgdata`) | several GB | old compose projects |
| Journals + syslog | a few GB | crash-looping leftover systemd units |

---

## SSH

```bash
ssh -i ~/.ssh/aws-ec2-key.pem ubuntu@54.93.174.192
```

Repo on the box is typically `~/trade-launch-bot` (or your checkout path).
Always pass `--env-file hunter/.env` to hunter compose commands.

---

## 0) Sanity check (before and after)

```bash
df -h /
docker system df
docker ps --format 'table {{.Names}}\t{{.Status}}'
```

Targets after cleanup: root well under ~80% (prefer >20–30GB free), hunter
containers `Up` / postgres `healthy`.

---

## 1) Free Docker disk (keep `hunter-pgdata`)

### Safe prune

```bash
# Build cache — usually the largest reclaim. CAP IT, don't empty it (see below).
docker builder prune -f --keep-storage 20GB

# Unused images (not referenced by a running container)
docker image prune -af

# Stopped containers / unused networks
docker container prune -f
docker network prune -f
```

> ⚠ **Never `docker builder prune -af` on a box you also build on.** The `-a`
> deletes *cache mounts* too, and `target/` lives only in a cache mount
> (`deploy/*/api.Dockerfile`) — so it throws away every dependency cargo-chef
> cooked. The next build is then a full cold compile of ~400 crates, which is
> exactly the "caching is configured but rebuilds are still slow" symptom.
> `--keep-storage` reclaims the oldest cache first and stops at the cap, which
> frees the disk without resetting the compile.
>
> `docker image prune -af` is fine to keep: it removes *unused runtime images*.
> It does also drop untagged intermediate build stages, so the `cargo install
> cargo-chef` layer recompiles (~2 min) on the next build — cheap next to a cold
> dependency tree.

### Orphan volumes — review, then remove by name

```bash
docker volume ls
docker system df -v
```

Remove only volumes you confirm are unused. **Never** delete `hunter-pgdata`
(or `forge-pgdata` if forge is still needed on this host).

```bash
# examples — only after you confirm unused:
docker volume rm meme-trading_pgdata
docker volume rm forge-pgdata
docker volume rm forge-lakedata
docker volume rm hunter_pgdata   # underscore name — NOT the same as hunter-pgdata
```

### Do not run

```bash
docker volume prune          # can wipe named volumes if unused by a container
docker compose ... down -v   # deletes the DB volume
docker system prune -a --volumes
```

Confirm:

```bash
df -h /
docker system df
docker ps
```

---

## 2) Disable zombie systemd services

Old host units (pre-docker stacks) can crash-loop forever, spam syslog/journal,
and burn CPU on the small box. Restart counters in the 100k+ range are a red flag.

List candidates:

```bash
systemctl list-units --type=service --state=failed,activating,active \
  | grep -Ei 'pump|scalper|bonk|nats|tx-publisher|analyzer'
systemctl list-unit-files | grep -Ei 'pump|scalper|bonk|analyzer'
```

Disable + stop (skip any name that is “not found”):

```bash
sudo systemctl disable --now \
  pumpswap-frontend.service \
  pumpswap-backend.service \
  scalper-dashboard.service \
  scalper-bot.service \
  bonk-frontend.service \
  bonk-backend.service \
  pumpfun-analyzer-frontend.service \
  pumpfun-analyzer-backend.service
```

Optional leftovers:

```bash
sudo systemctl disable --now nats 2>/dev/null || true
docker rm nats 2>/dev/null || true   # only if the exited nats container is unused
```

Trim log bulk after stopping the loops:

```bash
sudo journalctl --vacuum-size=200M
sudo logrotate -f /etc/logrotate.conf
# nuclear (loses local syslog history):
# sudo truncate -s 0 /var/log/syslog
```

---

## 3) Avoid filling the disk again

### Prefer on the live box

```bash
cd ~/trade-launch-bot
git pull
docker compose --env-file hunter/.env -f deploy/hunter.compose.yml \
  up -d postgres live-api live-ui
```

Pull/restart existing images. Do **not** rebuild on EC2 unless you must.

### Avoid on EC2

```bash
docker compose ... build
docker compose ... up -d --build
```

Build on the workstation (or CI), push/pull images, then `up -d` on the server.

If you must build on EC2, build **only the live services** — a bare `--build`
also builds `lab-api`, which compiles the bundled libduckdb C++ amalgamation
(tens of GB of build cache, many minutes) and has no business on this box:

```bash
docker compose --env-file hunter/.env -f deploy/hunter.compose.yml \
  up -d --build postgres live-api live-ui
```

Then cap the cache rather than emptying it — `-af` deletes the cache mounts and
guarantees the next build is a cold compile:

```bash
docker builder prune -f --keep-storage 20GB
```

### Watch disk

```bash
watch -n 30 'df -h /; echo; docker system df'
```

Simple threshold check (exit 1 if root >= 85%):

```bash
df -P / | awk 'NR==2 {gsub(/%/,"",$5); if ($5+0 >= 85) exit 1}'
```

Other constraints that bite this box:

- **No swap** — memory pressure + heavy builds amplify risk.
- Ship **hunter live only** to EC2; lab/forge builds and DuckDB stay on the workstation.
- Do not raise cache caps / retention on the server to “make analysis easier”.

---

## After cleanup — hunter healthy?

```bash
docker ps
docker logs --tail 50 hunter-live-api
docker exec hunter-postgres psql -U postgres -d hunter_bot -c \
  "SELECT max(block_time) AS last_trade FROM trades;"
df -h /
```

While ingest is live, `last_trade` should advance within the last minute or two.
If the site 502s again, check `df -h /` first — ENOSPC before anything else.
