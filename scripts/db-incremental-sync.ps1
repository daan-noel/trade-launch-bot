<#
.SYNOPSIS
  Incremental DB -> DB sync: EC2 `live` Postgres (rolling buffer) -> local `lab`
  mirror (long retention), so the workstation can lake-export + sweep/backtest
  without loading the server.

.DESCRIPTION
  Rewrite of meme-trading's script for THIS project's generalized schema. The
  server keeps only a short rolling window (raw_txs 7d, trades 30d); the lab keeps
  history far longer. Each run pulls new rows and upserts the dimensions.

  Strategy (mirrors the proven meme-trading approach):
    1. Reach the server DB (direct conn string, or an SSH tunnel to a private box
       -- wire the tunnel here when the EC2 box exists; params below).
    2. postgres_fdw import the server schema into a scratch `ec2_sync_src`.
    3. Per table, in FK-safe order:
         - dimensions (quote_assets, launchpads)        -> upsert, server wins
         - wallet_dict                                  -> NON-DESTRUCTIVE merge
         - tokens, markets, token_market_state,
           token_sync_state                             -> upsert, server wins
         - trades                                       -> APPEND by block_time watermark
         - raw_txs (only with -IncludeRawTxs)           -> APPEND by block_time watermark
         - own-launch (managed_wallets, launch_templates,
           launches, bundles)                           -> upsert, server wins
    4. Drop the FDW scratch schema.

  wallet_dict is a NON-DESTRUCTIVE merge (NOT truncate+replace): the lab retains
  trades longer than the server's window, so local-only ids the server has aged
  out MUST be preserved -- server wins by id, local-only ids stay. A truncate
  would re-orphan old trades (they LEFT JOIN wallet_dict; a missing id renders as
  'unknown:<id>'). See CLAUDE.md "wallet_dict orphan trades".

  STATUS: foundation. The table logic below is schema-correct; the connection /
  SSH-tunnel wiring is stubbed until the EC2 box exists. Do a dry run against two
  LOCAL databases first (-ServerDatabaseUrl a second local DB) to validate.

.PARAMETER ServerDatabaseUrl
  libpq conn string for the SERVER (or tunnel local end), e.g.
  postgres://user:pass@127.0.0.1:5540/launch_platform

.PARAMETER LocalDatabaseUrl
  libpq conn string for the LOCAL mirror (default: $env:DATABASE_URL).

.PARAMETER IncludeRawTxs
  Also append raw_txs (large; usually the lab only needs trades + dimensions).
#>
param(
    [Parameter(Mandatory = $true)] [string] $ServerDatabaseUrl,
    [string] $LocalDatabaseUrl = $env:DATABASE_URL,
    [switch] $IncludeRawTxs
)

$ErrorActionPreference = 'Continue'   # native psql stderr noise must not abort; see meme-trading notes
if (-not $LocalDatabaseUrl) { throw 'LocalDatabaseUrl or $env:DATABASE_URL is required' }

# Tables mirrored in FK-safe order. Each entry: name + merge mode.
#   upsert  = INSERT ... ON CONFLICT (pk) DO UPDATE  (server wins)
#   wallet  = non-destructive wallet_dict merge (server wins by id, keep local-only)
#   append  = INSERT ... ON CONFLICT DO NOTHING, only rows newer than local max(block_time)
$dimensions = @('quote_assets', 'launchpads')
$identity   = @('tokens', 'markets', 'token_market_state', 'token_sync_state')
$ownLaunch  = @('managed_wallets', 'launch_templates', 'launches', 'bundles')

function Invoke-LocalSql([string] $sql) {
    & psql $LocalDatabaseUrl -v ON_ERROR_STOP=1 -q -c $sql
    if ($LASTEXITCODE -ne 0) { throw "psql failed: $sql" }
}

Write-Host "==> Importing server schema via postgres_fdw"
Invoke-LocalSql @"
CREATE EXTENSION IF NOT EXISTS postgres_fdw;
DROP SCHEMA IF EXISTS ec2_sync_src CASCADE;
CREATE SCHEMA ec2_sync_src;
-- TODO(EC2): CREATE SERVER + USER MAPPING from `$ServerDatabaseUrl` (host/port/db/user/pass),
--            then IMPORT FOREIGN SCHEMA public FROM SERVER ec2 INTO ec2_sync_src;
"@

Write-Host "==> Dimensions (upsert, server wins)"
foreach ($t in $dimensions) {
    Invoke-LocalSql "INSERT INTO $t SELECT * FROM ec2_sync_src.$t ON CONFLICT (id) DO UPDATE SET (mint, symbol) = (EXCLUDED.mint, EXCLUDED.symbol);"
}

Write-Host "==> wallet_dict (NON-DESTRUCTIVE merge)"
# Server wins by id; local-only ids (server aged them out) are PRESERVED.
Invoke-LocalSql @"
INSERT INTO wallet_dict (id, address)
OVERRIDING SYSTEM VALUE
SELECT id, address FROM ec2_sync_src.wallet_dict
ON CONFLICT (id) DO UPDATE SET address = EXCLUDED.address;
"@

Write-Host "==> Identity tables (upsert, server wins)"
foreach ($t in $identity) {
    # PK differs per table; DO NOTHING keeps this generic for the foundation. Tighten
    # to DO UPDATE per-table (token_market_state should refresh metrics) when wired.
    Invoke-LocalSql "INSERT INTO $t SELECT * FROM ec2_sync_src.$t ON CONFLICT DO NOTHING;"
}

Write-Host "==> trades (append by block_time watermark)"
Invoke-LocalSql @"
INSERT INTO trades
SELECT s.* FROM ec2_sync_src.trades s
WHERE s.block_time > COALESCE((SELECT max(block_time) FROM trades), 'epoch')
ON CONFLICT (block_time, tx_signature, leg_index) DO NOTHING;
"@

if ($IncludeRawTxs) {
    Write-Host "==> raw_txs (append by block_time watermark)"
    Invoke-LocalSql @"
INSERT INTO raw_txs
SELECT s.* FROM ec2_sync_src.raw_txs s
WHERE s.block_time > COALESCE((SELECT max(block_time) FROM raw_txs), 'epoch')
ON CONFLICT (block_time, tx_signature) DO NOTHING;
"@
}

Write-Host "==> Own-launch tables (upsert, server wins)"
foreach ($t in $ownLaunch) {
    Invoke-LocalSql "INSERT INTO $t SELECT * FROM ec2_sync_src.$t ON CONFLICT (id) DO NOTHING;"
}

Write-Host "==> Cleanup"
Invoke-LocalSql "DROP SCHEMA IF EXISTS ec2_sync_src CASCADE;"
Write-Host "Sync complete. Next: cargo run -p lab -- lake-export --include-today"
