#!/usr/bin/env bash
# db-incremental-csv-dump.sh — Incremental data export from the deployed meme_bot DB.
# Run ON the EC2 box.
#
# DEPRECATED (kept for fallback only). The CSV dump→scp→load pipeline this belongs
# to is superseded by scripts/db-incremental-sync.ps1, which pulls server→local
# directly over an SSH tunnel (no CSV, no scp). See scripts/db-incremental-sync.md.
# Paired with: db-incremental-csv-restore.ps1.
#
# Exports only rows newer than the provided watermark timestamps, as CSV files:
#   trades          — block_time  > TRADES_SINCE
#   tokens          — created_at  > TOKENS_SINCE
#   tokens_info     — updated_at  > TOKENS_INFO_SINCE
#   tokens_analysis — computed_at > ANALYSIS_SINCE
#
# Usage (env vars preferred to avoid shell quoting issues with spaces):
#   TRADES_SINCE="2026-06-20 12:00:00+00" \
#   TOKENS_SINCE="2026-06-20 00:00:00+00" \
#   TOKENS_INFO_SINCE="2026-06-20 00:00:00+00" \
#   ANALYSIS_SINCE="2026-06-20 00:00:00+00" \
#   ./scripts/db-incremental-csv-dump.sh
#
# Or via flags:
#   ./scripts/db-incremental-csv-dump.sh \
#     --trades-since "2026-06-20 12:00:00+00" \
#     --tokens-since "2026-06-20 00:00:00+00" \
#     --tokens-info-since "2026-06-20 00:00:00+00" \
#     --analysis-since "2026-06-20 00:00:00+00"
#
# Output: snapshots/incremental/<UTC-timestamp>/ with one CSV per table.
#         Last line of stdout is the output directory path (parsed by restore script).
set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_DIR"
set -a; [ -f .env ] && . ./.env; set +a
PGUSER="${POSTGRES_USER:-postgres}"
PGDB="${POSTGRES_DB:-meme_bot}"

# Defaults — can be overridden by env vars or flags below.
TRADES_SINCE="${TRADES_SINCE:-1970-01-01 00:00:00+00}"
TOKENS_SINCE="${TOKENS_SINCE:-1970-01-01 00:00:00+00}"
TOKENS_INFO_SINCE="${TOKENS_INFO_SINCE:-1970-01-01 00:00:00+00}"
ANALYSIS_SINCE="${ANALYSIS_SINCE:-1970-01-01 00:00:00+00}"

while [[ $# -gt 0 ]]; do
  case $1 in
    --trades-since)      TRADES_SINCE="$2";      shift 2 ;;
    --tokens-since)      TOKENS_SINCE="$2";      shift 2 ;;
    --tokens-info-since) TOKENS_INFO_SINCE="$2"; shift 2 ;;
    --analysis-since)    ANALYSIS_SINCE="$2";    shift 2 ;;
    *) echo "Unknown arg: $1"; exit 1 ;;
  esac
done

OUT_DIR="$REPO_DIR/snapshots/incremental/$(date -u +%Y%m%d_%H%M%SZ)"
mkdir -p "$OUT_DIR"

# Run a COPY TO STDOUT query and redirect to a file.
# psql prints "COPY N" to stderr; only the CSV rows go to stdout → the file.
run_copy() {
  local label="$1"
  local query="$2"
  local file="$3"
  echo "  Dumping ${label}…"
  docker compose exec -T -e PGPASSWORD="${POSTGRES_PASSWORD:-}" postgres \
    psql -U "$PGUSER" -d "$PGDB" \
      -c "COPY (${query}) TO STDOUT WITH (FORMAT CSV, HEADER)" \
    > "$file"
  local sz
  sz=$(du -h "$file" | cut -f1)
  echo "    → ${sz}  (${file##*/})"
}

echo "Incremental dump → $OUT_DIR"
echo "  trades   since: $TRADES_SINCE"
echo "  tokens   since: $TOKENS_SINCE"
echo "  tok_info since: $TOKENS_INFO_SINCE"
echo "  analysis since: $ANALYSIS_SINCE"
echo ""

run_copy "trades" \
  "SELECT * FROM trades WHERE block_time > '${TRADES_SINCE}'::timestamptz ORDER BY block_time" \
  "$OUT_DIR/trades.csv"

run_copy "tokens" \
  "SELECT * FROM tokens WHERE created_at > '${TOKENS_SINCE}'::timestamptz ORDER BY created_at" \
  "$OUT_DIR/tokens.csv"

run_copy "tokens_info" \
  "SELECT * FROM tokens_info WHERE updated_at > '${TOKENS_INFO_SINCE}'::timestamptz" \
  "$OUT_DIR/tokens_info.csv"

run_copy "tokens_analysis" \
  "SELECT * FROM tokens_analysis WHERE computed_at > '${ANALYSIS_SINCE}'::timestamptz" \
  "$OUT_DIR/tokens_analysis.csv"

echo ""
echo "Done. Path: $OUT_DIR"
