#!/usr/bin/env bash
# db-snapshot-dump.sh — DATA-ONLY snapshot of the deployed meme_bot DB. Run ON the
# EC2 box. Single-threaded + off-peak-friendly so it doesn't fight the ingest
# firehose for the 2 vCPU / slow disk.
#
# Captures the DATA of every table EXCEPT:
#   - raw_transactions (+ its daily partitions)  — the fat Helius blobs
#   - *_grouped_sweep_* (tpsl1/tpsl2)            — local owns sweep results
#   - app_settings                               — local keeps its own settings
#   - _sqlx_migrations                           — local manages its own schema
# So you get tokens, trades (every daily partition), rules, positions, wallets.
#
# It is DATA-ONLY (no schema): local already has the schema from its own migrations,
# and the restore reloads into the existing tables WITHOUT dropping anything — so
# local sweep results / settings / raw stay intact. See db-snapshot-restore.ps1.
#
# Why dump-the-DB-minus-exclusions instead of `-t tokens -t trades …`:
#   `pg_dump -t trades` is a TRAP on a partitioned table — it matches only the
#   parent relation, whose rows live in child partitions (trades_2026_06_23, …)
#   that the pattern does NOT match, so trades would come out EMPTY. Dumping the
#   whole DB pulls every partition automatically and keeps FK chains intact.
#
# --load-via-partition-root routes trades rows through the parent on restore, so
# local only needs the day-partitions to EXIST (the restore script pre-creates
# them) — it does not need the exact same partition set as the server.
#
# Usage:   ./scripts/db-snapshot-dump.sh
# Output:  ./snapshots/meme_bot_data_<UTC-timestamp>.dump  (custom/compressed)
set -euo pipefail

# This script lives in scripts/ — resolve repo root so docker-compose.yml + .env load.
REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_DIR"

# DB creds from .env (POSTGRES_USER / POSTGRES_PASSWORD / POSTGRES_DB).
set -a; [ -f .env ] && . ./.env; set +a
PGUSER="${POSTGRES_USER:-postgres}"
PGDB="${POSTGRES_DB:-meme_bot}"

# Compression. zstd:3 is the best fit for a CPU-tight box — faster AND smaller than
# gzip — and is read by any Postgres 16+ pg_restore (the postgres:16 image has it).
# If your LOCAL Postgres is older than 16, run with COMPRESS=6 for gzip instead.
COMPRESS="${COMPRESS:-zstd:3}"

OUT_DIR="$REPO_DIR/snapshots"
mkdir -p "$OUT_DIR"
OUT="$OUT_DIR/meme_bot_data_$(date -u +%Y%m%d_%H%M%SZ).dump"

echo "Dumping DATA of '$PGDB' → $OUT"
echo "  format=custom compress=$COMPRESS  (excluding raw / sweep / settings / migrations)…"

# -T (no TTY) keeps the binary custom-format stream intact through the host pipe.
docker compose exec -T -e PGPASSWORD="${POSTGRES_PASSWORD:-}" postgres \
  pg_dump -U "$PGUSER" -d "$PGDB" \
    --data-only \
    --format=custom \
    --compress="$COMPRESS" \
    --load-via-partition-root \
    --exclude-table-data='raw_transactions*' \
    --exclude-table-data='*grouped_sweep*' \
    --exclude-table-data='app_settings' \
    --exclude-table-data='_sqlx_migrations' \
  > "$OUT"

echo "Done: $OUT ($(du -h "$OUT" | cut -f1))"
