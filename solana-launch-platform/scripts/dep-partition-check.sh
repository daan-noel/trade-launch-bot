#!/usr/bin/env bash
# Dep-partition guard (roadmap Phase 2b). Fails if live pulls lake deps or lab
# pulls live-side signing/ingest crates.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

echo "==> cargo check --workspace"
cargo check --workspace

echo "==> slp-live must NOT pull duckdb / arrow / parquet"
if cargo tree -p slp-live 2>/dev/null | grep -E 'duckdb|arrow|parquet'; then
  echo "FAIL: slp-live dep partition violated (lake stack leaked into slp-live)" >&2
  exit 1
fi

echo "==> slp-lab must NOT pull pump-trader / ingest-laserstream / tonic"
if cargo tree -p slp-lab 2>/dev/null | grep -E 'pump-trader|ingest-laserstream|tonic'; then
  echo "FAIL: slp-lab dep partition violated (live stack leaked into slp-lab)" >&2
  exit 1
fi

echo "OK: dep partition holds"
