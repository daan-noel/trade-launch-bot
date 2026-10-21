#!/bin/sh
# Nightly disk housekeeping for the hunter-live EC2 box. Installed as
# /usr/local/bin/hunter-disk-housekeeping, run from root's crontab:
#
#   30 3 * * * /usr/local/bin/hunter-disk-housekeeping
#
# 1. Returns the heap of every compressed chunk to the OS. Compression empties
#    the chunk's own heap and indexes, but under the live write load it cannot
#    take the lock it needs to truncate them, so each compressed `trades` day
#    keeps ~2 GB of dead pages. VACUUM FULL of an empty heap takes seconds and
#    locks only that settled chunk.
# 2. Warns in syslog (`journalctl -t hunter-disk`) and exits 1 when / is at or
#    past DISK_WARN_PCT.
#
# Runbook: deploy/EC2-DISK-HOUSEKEEPING.md
set -eu

PG_CONTAINER="${PG_CONTAINER:-hunter-postgres}"
PG_DB="${PG_DB:-hunter_bot}"
MIN_RECLAIM_BYTES="${MIN_RECLAIM_BYTES:-67108864}"  # skip chunks under 64 MiB
DISK_WARN_PCT="${DISK_WARN_PCT:-85}"

psql() { docker exec -i "$PG_CONTAINER" psql -U postgres -d "$PG_DB" -At -v ON_ERROR_STOP=1 "$@"; }

# Status bit 1 = compressed. The heap of a compressed chunk holds only rows
# written after compression (status bit 8, partial), which VACUUM FULL keeps.
psql -c "
SELECT format('VACUUM (FULL) %I.%I;', c.schema_name, c.table_name)
FROM _timescaledb_catalog.chunk c
JOIN _timescaledb_catalog.hypertable h ON h.id = c.hypertable_id
WHERE h.table_name IN ('trades', 'strategy_arms', 'raw_txs')
  AND c.status & 1 = 1
  AND pg_total_relation_size(format('%I.%I', c.schema_name, c.table_name)::regclass) > $MIN_RECLAIM_BYTES
ORDER BY c.id" > /tmp/hunter-reclaim.sql

n=$(wc -l < /tmp/hunter-reclaim.sql)
if [ "$n" -gt 0 ]; then
    psql < /tmp/hunter-reclaim.sql > /dev/null
    logger -t hunter-disk "reclaimed $n compressed chunk(s)"
fi

used=$(df -P / | awk 'NR==2 {gsub(/%/, "", $5); print $5}')
if [ "$used" -ge "$DISK_WARN_PCT" ]; then
    logger -p user.warning -t hunter-disk "root filesystem at ${used}% (warn at ${DISK_WARN_PCT}%)"
    exit 1
fi
