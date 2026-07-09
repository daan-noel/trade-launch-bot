# Dep-partition guard (roadmap Phase 2b). Fails if live pulls lake deps or lab
# pulls live-side signing/ingest crates.
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

Write-Host "==> cargo check --workspace"
cargo check --workspace

Write-Host "==> forge-live must NOT pull duckdb / arrow / parquet"
$liveTree = cargo tree -p forge-live 2>$null
if ($liveTree -match 'duckdb|arrow|parquet') {
  Write-Error "FAIL: forge-live dep partition violated (lake stack leaked into forge-live)"
}

Write-Host "==> forge-lab must NOT pull pump-trader / ingest-laserstream / tonic"
$labTree = cargo tree -p forge-lab 2>$null
if ($labTree -match 'pump-trader|ingest-laserstream|tonic') {
  Write-Error "FAIL: forge-lab dep partition violated (live stack leaked into forge-lab)"
}

Write-Host "OK: dep partition holds"
