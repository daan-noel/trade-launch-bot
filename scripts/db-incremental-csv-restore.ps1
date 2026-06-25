<#
.SYNOPSIS
  Pull incremental data from EC2 and append to local meme_bot DB. (CSV pipeline.)

  DEPRECATED (kept for fallback only). Superseded by db-incremental-sync.ps1,
  which pulls server->local directly over an SSH tunnel (postgres_fdw) -- no CSV
  export, no scp, no temp files. See db-incremental-sync.md. This script remains
  only as a fallback for hosts where a direct DB tunnel isn't possible.
  Paired with: db-incremental-csv-dump.sh.

.DESCRIPTION
  Non-destructive incremental sync -- appends only rows newer than what the
  local DB already has. Existing local data (sweep results, positions,
  settings, raw_transactions) is never touched.

  Steps:
    1. Query local DB for watermarks (max block_time, max created_at, etc.)
    2. SSH to EC2 and run db-incremental-csv-dump.sh with those watermarks
    3. Download the resulting CSV files via scp
    4. For each table: staging table + \copy CSV in + INSERT ON CONFLICT

  Conflict keys:
    trades          UNIQUE(tx_signature, leg_index, block_time) -- DO NOTHING
    tokens          UNIQUE(mint_address)                        -- DO NOTHING
    tokens_info     UNIQUE(mint_address)                        -- DO UPDATE (newer wins)
    tokens_analysis UNIQUE(mint_address, analyzer_name)         -- DO UPDATE (newer wins)
    creator_profiles UNIQUE(wallet_address)                     -- DO UPDATE

  Requires on PATH: ssh, scp (Windows OpenSSH), psql.
  Stop any local backend writing to meme_bot before running.

.EXAMPLE
  $env:PGPASSWORD = 'your_local_pw'
  powershell -ExecutionPolicy Bypass -File ./scripts/db-incremental-csv-restore.ps1 -SshTarget ubuntu@1.2.3.4

.EXAMPLE
  # Re-apply an already-downloaded batch without re-dumping from EC2
  powershell -ExecutionPolicy Bypass -File ./scripts/db-incremental-csv-restore.ps1 `
    -SshTarget ubuntu@1.2.3.4 -LocalBatchDir 'snapshots/incremental/20260625_120000Z'
#>
param(
  [Parameter(Mandatory = $true)][string]$SshTarget,
  [string]$SshKey        = 'F:\pumpfun\meme-trading\aws-ec2-key.pem',
  [string]$RemoteDir     = '~/projects/meme-trading',
  [string]$LocalDir      = "$PSScriptRoot/../snapshots/incremental",
  [string]$LocalBatchDir = '',
  [string]$Database      = 'meme_bot',
  [string]$PgUser        = 'postgres',
  [string]$PgHost        = '',
  [int]   $PgPort        = 5432,
  [int]   $PartitionDays = 60,
  [string]$PgPassword    = $env:PGPASSWORD
)
$ErrorActionPreference = 'Stop'
if ($PgPassword) { $env:PGPASSWORD = $PgPassword }
# Force UTC so ensure_trades_partition creates bounds in UTC regardless of local OS timezone.
$env:PGOPTIONS        = '-c timezone=UTC'
$env:PGCLIENTENCODING = 'UTF8'

$sshOpts = if ($SshKey) { @('-i', $SshKey) } else { @() }

$pgHostArgs = if ($PgHost) { @('-h', $PgHost) } else { @() }
$pg = @('-U', $PgUser) + $pgHostArgs + @('-p', "$PgPort", '-d', $Database, '-v', 'ON_ERROR_STOP=1')

function Run-Sql([string]$sql) {
  & psql @pg -c $sql | Out-Null
  if ($LASTEXITCODE -ne 0) { throw "psql failed: $sql" }
}

function Query-Sql([string]$sql) {
  $r = (& psql @pg -tAc $sql).Trim()
  if ($LASTEXITCODE -ne 0) { throw "psql query failed: $sql" }
  return $r
}

# Write a string to a temp .sql file with CRLF endings (needed for \copy meta-cmd).
function Write-TempSql([string]$content) {
  $f = [System.IO.Path]::GetTempFileName() -replace '\.tmp$', '.sql'
  [System.IO.File]::WriteAllText($f, $content, [System.Text.Encoding]::ASCII)
  return $f
}

# ---- 1. Watermarks -----------------------------------------------------------
if (-not $LocalBatchDir) {
  Write-Host "Querying local watermarks..."
  $tradesWm   = Query-Sql "SELECT COALESCE(MAX(block_time), '1970-01-01 00:00:00+00')::text FROM trades"
  $tokensWm   = Query-Sql "SELECT COALESCE(MAX(created_at), '1970-01-01 00:00:00+00')::text FROM tokens"
  $tinfoWm    = Query-Sql "SELECT COALESCE(MAX(updated_at),  '1970-01-01 00:00:00+00')::text FROM tokens_info"
  $analysisWm = Query-Sql "SELECT COALESCE(MAX(computed_at), '1970-01-01 00:00:00+00')::text FROM tokens_analysis"

  Write-Host "  trades   since: $tradesWm"
  Write-Host "  tokens   since: $tokensWm"
  Write-Host "  tok_info since: $tinfoWm"
  Write-Host "  analysis since: $analysisWm"

  # ---- 2. Remote dump --------------------------------------------------------
  Write-Host ""
  Write-Host "Running incremental dump on $SshTarget..."

  $remoteCmd = "cd $RemoteDir && TRADES_SINCE='$tradesWm' TOKENS_SINCE='$tokensWm' TOKENS_INFO_SINCE='$tinfoWm' ANALYSIS_SINCE='$analysisWm' bash scripts/db-incremental-csv-dump.sh"
  $remoteOutput = (ssh @sshOpts $SshTarget $remoteCmd) -join "`n"
  if ($LASTEXITCODE -ne 0) { throw "Remote dump failed" }

  $pathLine = ($remoteOutput -split "`n" | Where-Object { $_ -match 'Done\. Path:' } | Select-Object -Last 1)
  if ($pathLine -match 'Path:\s*(.+)') { $remotePath = $Matches[1].Trim() }
  else { throw "Could not parse remote output dir from: $remoteOutput" }
  Write-Host "Remote batch: $remotePath"

  # ---- 3. Download -----------------------------------------------------------
  New-Item -ItemType Directory -Force -Path $LocalDir | Out-Null
  scp @sshOpts -r "${SshTarget}:$remotePath" $LocalDir
  if ($LASTEXITCODE -ne 0) { throw "scp failed" }
  $LocalBatchDir = Join-Path $LocalDir (Split-Path $remotePath -Leaf)
  Write-Host "Downloaded -> $LocalBatchDir"
}

# ---- 4. Ensure trades partitions (single connection, DO block) --------------
Write-Host ""
Write-Host "Ensuring trades partitions ($PartitionDays days)..."
$today = (Get-Date).ToUniversalTime().Date
$startDate = $today.AddDays(-$PartitionDays).ToString('yyyy-MM-dd')
$endDate   = $today.AddDays(2).ToString('yyyy-MM-dd')
$partSql = "DO `$`$ DECLARE d date := '$startDate'; BEGIN WHILE d <= '$endDate'::date LOOP BEGIN PERFORM ensure_trades_partition(d); EXCEPTION WHEN OTHERS THEN NULL; END; d := d + 1; END LOOP; END `$`$;"
$partFile = [System.IO.Path]::GetTempFileName() -replace '\.tmp$', '.sql'
[System.IO.File]::WriteAllText($partFile, $partSql, [System.Text.Encoding]::ASCII)
try {
  & psql @pg -f $partFile
  if ($LASTEXITCODE -ne 0) { throw "partition ensure failed" }
} finally {
  Remove-Item $partFile -ErrorAction SilentlyContinue
}

# ---- 5. Apply one CSV via staging table -------------------------------------
function Apply-Csv {
  param(
    [string]$Label,
    [string]$CsvPath,
    [string]$Staging,
    [string]$Target,
    [string]$Like,
    [string]$Conflict
  )
  if (-not (Test-Path $CsvPath)) { Write-Host "  $Label -- CSV not found, skipping"; return }
  $lineCount = (Get-Content $CsvPath | Measure-Object -Line).Lines
  if ($lineCount -le 1) { Write-Host "  $Label -- empty (no new rows)"; return }
  Write-Host "  $Label -- $($lineCount - 1) rows..."

  $csvFwd = $CsvPath -replace '\\', '/'

  Run-Sql "DROP TABLE IF EXISTS $Staging"
  Run-Sql "CREATE TABLE $Staging (LIKE $Like)"

  $copySql = "\copy $Staging FROM '$csvFwd' WITH (FORMAT CSV, HEADER)`r`n"
  $tmpFile = Write-TempSql $copySql
  try {
    & psql @pg -f $tmpFile
    if ($LASTEXITCODE -ne 0) { throw "psql \copy failed for $Label" }
  } finally {
    Remove-Item $tmpFile -ErrorAction SilentlyContinue
  }

  Run-Sql "INSERT INTO $Target SELECT * FROM $Staging $Conflict"
  Run-Sql "DROP TABLE $Staging"
}

# ---- 6. Apply tables --------------------------------------------------------
Write-Host ""
Write-Host "Applying incremental data..."

Apply-Csv -Label 'trades' `
  -CsvPath (Join-Path $LocalBatchDir 'trades.csv') `
  -Staging '_inc_trades' -Target 'trades' -Like 'trades' `
  -Conflict 'ON CONFLICT (tx_signature, leg_index, block_time) DO NOTHING'

Apply-Csv -Label 'tokens' `
  -CsvPath (Join-Path $LocalBatchDir 'tokens.csv') `
  -Staging '_inc_tokens' -Target 'tokens' -Like 'tokens' `
  -Conflict 'ON CONFLICT (mint_address) DO NOTHING'

$tinfoConflict = "ON CONFLICT (mint_address) DO UPDATE SET " +
  "ath_price = EXCLUDED.ath_price, ath_timestamp = EXCLUDED.ath_timestamp, " +
  "age = EXCLUDED.age, volume = EXCLUDED.volume, market_cap = EXCLUDED.market_cap, " +
  "trade_count = EXCLUDED.trade_count, last_trade_at = EXCLUDED.last_trade_at, " +
  "current_price = EXCLUDED.current_price, is_dead = EXCLUDED.is_dead, " +
  "is_migrated = EXCLUDED.is_migrated, last_synced_at = EXCLUDED.last_synced_at, " +
  "last_synced_curve_sig = EXCLUDED.last_synced_curve_sig, " +
  "last_synced_amm_sig = EXCLUDED.last_synced_amm_sig, " +
  "last_synced_curve_slot = EXCLUDED.last_synced_curve_slot, " +
  "last_synced_amm_slot = EXCLUDED.last_synced_amm_slot, " +
  "lifetime_secs = EXCLUDED.lifetime_secs, updated_at = EXCLUDED.updated_at " +
  "WHERE EXCLUDED.updated_at >= tokens_info.updated_at"

Apply-Csv -Label 'tokens_info' `
  -CsvPath (Join-Path $LocalBatchDir 'tokens_info.csv') `
  -Staging '_inc_tokens_info' -Target 'tokens_info' -Like 'tokens_info' `
  -Conflict $tinfoConflict

$analysisConflict = "ON CONFLICT (mint_address, analyzer_name) DO UPDATE SET " +
  "score = EXCLUDED.score, indicators = EXCLUDED.indicators, " +
  "computed_at = EXCLUDED.computed_at " +
  "WHERE EXCLUDED.computed_at >= tokens_analysis.computed_at"

Apply-Csv -Label 'tokens_analysis' `
  -CsvPath (Join-Path $LocalBatchDir 'tokens_analysis.csv') `
  -Staging '_inc_tokens_analysis' -Target 'tokens_analysis' -Like 'tokens_analysis' `
  -Conflict $analysisConflict

$creatorsConflict = "ON CONFLICT (wallet_address) DO UPDATE SET " +
  "tokens_created = EXCLUDED.tokens_created, " +
  "total_volume_sol = EXCLUDED.total_volume_sol, " +
  "suspiciousness_score = EXCLUDED.suspiciousness_score, " +
  "wash_trade_score = EXCLUDED.wash_trade_score, " +
  "last_analyzed_at = EXCLUDED.last_analyzed_at, " +
  "indicators = EXCLUDED.indicators"

Apply-Csv -Label 'creator_profiles' `
  -CsvPath (Join-Path $LocalBatchDir 'creator_profiles.csv') `
  -Staging '_inc_creator_profiles' -Target 'creator_profiles' -Like 'creator_profiles' `
  -Conflict $creatorsConflict

Write-Host ""
Write-Host "Incremental sync complete. Local batch: $LocalBatchDir"
