#!/usr/bin/env bash
# Dep-partition guard (roadmap Phase 2b). Fails if live pulls lake deps or lab
# pulls live-side signing/ingest crates.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

echo "==> cargo check --workspace"
cargo check --workspace

echo "==> forge-live must NOT pull duckdb / arrow / parquet"
if cargo tree -p forge-live 2>/dev/null | grep -E 'duckdb|arrow|parquet'; then
  echo "FAIL: forge-live dep partition violated (lake stack leaked into forge-live)" >&2
  exit 1
fi

echo "==> forge-lab must NOT pull pump-trader / ingest-laserstream / tonic"
if cargo tree -p forge-lab 2>/dev/null | grep -E 'pump-trader|ingest-laserstream|tonic'; then
  echo "FAIL: forge-lab dep partition violated (live stack leaked into forge-lab)" >&2
  exit 1
fi

echo "OK: dep partition holds"
