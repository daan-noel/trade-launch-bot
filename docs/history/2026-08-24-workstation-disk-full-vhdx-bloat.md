# Workstation disk full - Docker vhdx bloat from analysis scratch schemas

> **History.** 2026-08-24. Windows workstation C: hit 100% (1.7 GB free), wedging the
> Docker daemon and stopping hunter ingest for ~38 hours. Root cause and the reclaim
> procedure, kept so the next occurrence is a runbook lookup rather than a diagnosis.
> The EC2 equivalent is [../../deploy/EC2-DISK-HOUSEKEEPING.md](../../deploy/EC2-DISK-HOUSEKEEPING.md).

## Symptom

`docker ps`, `docker images`, `docker system df` and `docker desktop stop` all hung and
returned nothing. `psql` still answered on 5555. Ingest had been dead since
`2026-08-22 21:45 UTC` - the newest `trades.block_time` was 1 day 14.5 h stale.

A wedged Docker CLI with a live Postgres was the tell: the daemon could not write, so
**check `df -h /c` before anything else.**

## Root cause - three layers

| Layer | Measured |
| --- | --- |
| C: free | 1.7 GB of 477 GB |
| `docker_data.vhdx` on disk | 201.6 GiB |
| Live data inside it | 142.6 GiB (of 1006.9 GiB provisioned) |
| Never returned to Windows | ~59 GiB |
| `hunter-pgdata` volume | 126.3 GB |

A WSL2 vhdx only grows. Deleting data frees blocks *inside* the disk; the host file never
shrinks on its own.

The 126 GB `hunter_bot` database was **85% ad-hoc research scratch**, not operational data
and not a retention failure:

| Schema | Tables | Size | In code |
| --- | --- | --- | --- |
| `wstudy` | 242 | 54 GB | no |
| `iv` | 152 | 44 GB | no |
| `cs` | 23 | 4.8 GB | no |
| `x3k` | 15 | 1.9 GB | no |
| `fbv` | 20 | 1.7 GB | no |
| `mstudy` | 5 | 831 MB | no |
| `wl` | 5 | 46 MB | no |
| `_timescaledb_internal` | 75 | 17 GB | **yes** - `trades` |
| `public` | 39 | 2.5 GB | **yes** |

Leftovers from the wallet studies, island search and fingerprint sweeps. A grep for
`<schema>.<table>` across `hunter/`, `forge/` and `shared/` matched none of the seven.

**Retention was never the problem.** `raw_txs` did not even register, and `trades` sat at
30 chunks / 17 GB, exactly the 30-day policy in
[../../hunter/core/migrations/0001_init.sql](../../hunter/core/migrations/0001_init.sql).
Do not go looking at retention policies again on this symptom.

## Reclaim procedure

Both halves are required. Dropping schemas alone frees space *inside* the vhdx and leaves
the host file at its high-water mark - C: actually fell to 155 MB during the drops as WAL
was written.

1. **Manifest first**, so what was dropped stays on record:
   `SELECT nspname, relname, pg_size_pretty(pg_total_relation_size(c.oid)) ... WHERE nspname IN (...)`
2. **Drop the scratch schemas.** `DROP SCHEMA ... CASCADE` is a `DROP TABLE` per table:
   space returns to the filesystem at once, no `VACUUM FULL`. Set `lock_timeout` so a
   stray lab session fails the statement instead of hanging it.
3. **`fstrim` inside the data disk** - it is not reachable from the plain distro mount
   namespace, so enter the init namespace:
   `wsl -d docker-desktop -e sh -c "nsenter -t 1 -m -u -i -n -p fstrim -v /mnt/docker-desktop-disk"`
4. **Compact**, elevated, with WSL down.

## Traps hit

- **`diskpart` un-elevated is a silent no-op** - exit code 0, no output, file unchanged.
  Check `IsInRole(Administrator)` before believing a compact succeeded.
- **Docker Desktop respawns itself.** Killing the processes and running `wsl --shutdown`
  is not enough; the supervisor restarted the `docker-desktop` distro and re-attached the
  vhdx, so `Optimize-VHD` failed `0x800700AA` (resource in use). Kill, shut down and
  compact **in one elevated script** with no window in between.
- **`LxssManager` does not exist on Windows 11** (it is `WslService`). The stop failed
  harmlessly; the compact succeeded without it once the respawn race was closed.
- Docker Desktop reclaims the trimmed blocks itself on its next start - the vhdx had
  already fallen 201.6 -> 38 GiB before `Optimize-VHD` ran, which then trimmed 26 MB more.

## Result

| | Before | After |
| --- | --- | --- |
| C: free | 155 MB | 229 GB |
| `docker_data.vhdx` | 201.6 GiB | 37.9 GiB |
| `hunter_bot` | 126 GB | 19 GB |

Postgres was hard-killed by `wsl --shutdown` and recovered clean: 42 kB of WAL redo, no
errors, `database system is ready to accept connections`. 54,434,043 trades over
`2026-07-24 .. 2026-08-22`, 813,490 tokens, 177 rules, 150 runs, 61,428 positions - all
intact.

## Still open

- **Docker's disk stays on C:.** D: has 1.6 TB free against C:'s 477 GB total. Settings ->
  Resources -> Advanced -> *Disk image location* -> `D:\DockerData` moves it and takes the
  recurrence risk off the system drive.
- **~650 MB of scratch survives in `public`** (`s64_*`, `som_*`, `w16_*`,
  `pump_graduated_tokens_old`), left alone because `public` also holds every operational
  table. `s64_mkt` is 613 MB of it.
- `Bot/target` is 49 GB on C: and was deliberately left - per
  [../../CLAUDE.md](../../CLAUDE.md) it lives in a build cache mount and deleting it costs
  a cold ~400-crate rebuild.

## Prevention

Analysis schemas are the workstation's growth term, and nothing prunes them. Drop a study
schema when its study concludes, or namespace them so one `DROP SCHEMA` retires a whole
round.
