# EC2 disk housekeeping (live box)

Disk-full runbook for the deployed hunter-live EC2. A full disk stops Postgres writes
and nginx 502s every `/api` request — **check `df -h /` first when the site 502s.**

[DOCKER.md](DOCKER.md) · retention sizing:
[raw-txs-storage.md](../hunter/docs/plans/database/raw-txs-storage.md)

```bash
ssh -i ~/.ssh/aws-ec2-key.pem ubuntu@35.158.128.131
```

## Diagnose

```bash
df -h /                                                       # target < 80%
docker system df                                              # images / volumes / build cache
sudo du -xh --max-depth=1 / | sort -rh | head -20
sudo du -xh --max-depth=1 /var/lib/docker/volumes | sort -rh  # pgdata vs eventlog
```

```bash
# Where the DB space went. pg_total_relation_size includes TOAST — pg_toast_* rows
# beside a chunk are the same bytes twice, don't sum them.
docker exec hunter-postgres psql -U postgres -d hunter_bot -c "
SELECT c.oid::regclass AS relation, pg_size_pretty(pg_total_relation_size(c.oid)) AS size
FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
WHERE c.relkind IN ('r','m','t') ORDER BY pg_total_relation_size(c.oid) DESC LIMIT 25;"
```

## Reclaim — Docker

```bash
docker builder prune -f --keep-storage 20GB   # largest reclaim; see warning
docker image prune -af
docker container prune -f
docker network prune -f
```

> ⚠ **Never `docker builder prune -af` on a box you also build on** — `-a` deletes cache
> mounts, and `target/` lives only in one, so the next build is a cold ~400-crate compile.

Orphan volumes — list, confirm unused, remove **by name**. Never `hunter-pgdata`:

```bash
docker volume ls
docker volume rm meme-trading_pgdata
```

**Do not run:** `docker volume prune` · `docker compose ... down -v` ·
`docker system prune -a --volumes` — each can delete the database.

## Reclaim — Postgres

`drop_chunks` is a `DROP TABLE` per chunk: space returns to the OS at once, no `VACUUM`.

```bash
docker exec hunter-postgres psql -U postgres -d hunter_bot -c \
  "SELECT drop_chunks('raw_txs', older_than => INTERVAL '3 days');"
```

To change a retention policy, edit `hunter/core/migrations/0001_init.sql` and apply via
[scripts/squash-catchup.sql](../scripts/squash-catchup.sql) — don't hand-edit policies on
the box or they drift from the migration.

## Reclaim — event log

Bounded by `EVENT_LOG_MAX_BYTES` in the box's `hunter/.env` (**set 536870912 = 512 MiB
here**), enforced at every segment rotation. If the directory is over that, the setting
isn't live — `docker restart` does **not** re-read `env_file`:

```bash
grep EVENT_LOG ~/trade-launch-bot/hunter/.env
docker compose --env-file hunter/.env -f deploy/hunter.compose.yml up -d live-api
```

Manual reclaim, safe with `live-api` running. Dry run, then delete all but the newest 3
segments — never the newest, that's the open one:

```bash
sudo bash -c "cd /var/lib/docker/volumes/hunter-eventlog/_data/event_log && ls -1 events-*.jsonl | sort | head -n -3"
sudo bash -c "cd /var/lib/docker/volumes/hunter-eventlog/_data/event_log && ls -1 events-*.jsonl | sort | head -n -3 | xargs -r rm -v"
```

If one file is huge, the writer holds it open — `truncate`, never `rm`:

```bash
sudo truncate -s 0 /var/lib/docker/volumes/hunter-eventlog/_data/event_log/events-$(date -u +%F).jsonl
```

## Reclaim — host logs

```bash
sudo journalctl --vacuum-size=200M
sudo apt-get clean
systemctl list-units --type=service --state=failed,activating,active \
  | grep -Ei 'pump|scalper|bonk|nats|analyzer'          # zombie pre-docker units
sudo systemctl disable --now <unit>...
```

## Prevent

Pull and restart — do **not** rebuild on EC2. If you must, name the services (a bare
`--build` also builds `lab-api`, which compiles libduckdb):

```bash
cd ~/trade-launch-bot && git pull
docker compose --env-file hunter/.env -f deploy/hunter.compose.yml up -d postgres live-api live-ui
docker compose --env-file hunter/.env -f deploy/hunter.compose.yml up -d --build postgres live-api live-ui
```

```bash
df -P / | awk 'NR==2 {gsub(/%/,"",$5); if ($5+0 >= 85) exit 1}'   # exit 1 if root >= 85%
```

## Verify

```bash
docker ps
docker logs --tail 50 hunter-live-api
docker exec hunter-postgres psql -U postgres -d hunter_bot -c \
  "SELECT max(block_time) AS last_trade FROM trades;"   # should be within a minute or two
df -h /
```
