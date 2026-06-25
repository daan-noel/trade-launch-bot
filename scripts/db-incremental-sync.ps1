<#
.SYNOPSIS
  Append the EC2 server's newer data into the local meme_bot DB — directly,
  DB->DB, over an SSH tunnel. No CSV, no scp, no temp files.

.DESCRIPTION
  Supersedes db-incremental-csv-{dump.sh,restore.ps1} (the CSV pipeline). Instead
  of exporting CSVs and shipping them, this:

    1. Opens an SSH tunnel to the server's published Postgres host port.
    2. Attaches the server as a postgres_fdw foreign server (schema ec2_sync_src).
    3. Verifies local vs remote column parity (aborts on schema drift).
    4. Ensures local `trades` day-partitions exist for the incoming window.
    5. Per table: INSERT INTO <local> SELECT * FROM ec2_sync_src.<t>
       WHERE <ts-col> >= <local watermark>  ON CONFLICT ...
       The watermark is a literal, so postgres_fdw PUSHES IT DOWN to the server
       (remote partition pruning) — only new rows are fetched.

  Non-destructive: existing local rows (sweep results, positions, settings,
  raw_transactions, your 4.5 days of trades) are never touched. Conflicts:
    trades          UNIQUE(tx_signature, leg_index, block_time) -> DO NOTHING
    tokens          UNIQUE(mint_address)                        -> DO NOTHING
    tokens_info     UNIQUE(mint_address)                        -> DO UPDATE (newer updated_at wins)
    tokens_analysis UNIQUE(mint_address, analyzer_name)         -> DO UPDATE (newer computed_at wins)
    creator_profiles UNIQUE(wallet_address)                     -> DO UPDATE (full upsert; no monotonic ts)

  Requires: ssh + psql on PATH; local Postgres >= 16 with postgres_fdw available;
  connect as a SUPERUSER local role (CREATE EXTENSION / USER MAPPING). Stop any
  local backend writing to meme_bot first. Server creds are read from the remote
  .env automatically (POSTGRES_HOST_PORT/USER/PASSWORD/DB).

.EXAMPLE
  $env:PGPASSWORD = 'your_LOCAL_pw'
  ./scripts/db-incremental-sync.ps1 -SshTarget ubuntu@1.2.3.4
#>
param(
  [Parameter(Mandatory = $true)][string]$SshTarget,                       # user@host of the EC2 box
  [string]$SshKey          = "$PSScriptRoot/../aws-ec2-key.pem",
  [string]$RemoteDir       = '~/projects/meme-trading',                   # where the server's .env lives
  [string]$Db              = 'meme_bot',
  [string]$LocalPgHost     = 'localhost',
  [int]   $LocalPgPort     = 5432,                                        # local meme_bot port (5555 if dockerized)
  [string]$LocalPgUser     = 'postgres',
  [int]   $TunnelLocalPort = 5433,                                        # local end of the SSH tunnel (must be free)
  [int]   $RemotePgPort    = 0,                                           # 0 = auto-detect from remote .env (default 5555)
  [int]   $PartitionDays   = 40,                                          # >= server KEEP_DAYS + margin
  [switch]$NoCreatorProfiles,                                             # skip the creator_profiles full upsert
  [string]$LocalPgPassword = $env:PGPASSWORD
)
$ErrorActionPreference = 'Stop'
if (-not $LocalPgPassword) { throw "Set the LOCAL DB password: `$env:PGPASSWORD='...'  (or -LocalPgPassword)" }

# Force UTC so ensure_trades_partition creates bounds in UTC regardless of OS tz.
$env:PGOPTIONS = '-c timezone=UTC'
$localPg = @('-h', $LocalPgHost, '-p', "$LocalPgPort", '-U', $LocalPgUser, '-d', $Db, '-v', 'ON_ERROR_STOP=1')
$sshOpts = @('-i', $SshKey, '-o', 'StrictHostKeyChecking=accept-new', '-o', 'ConnectTimeout=10')

function Use-LocalPw { $env:PGPASSWORD = $LocalPgPassword }
function Invoke-LocalSqlFile([string]$sql) {
  $f = [System.IO.Path]::GetTempFileName() -replace '\.tmp$', '.sql'
  [System.IO.File]::WriteAllText($f, $sql, [System.Text.Encoding]::UTF8)
  try { & psql @localPg -f $f; if ($LASTEXITCODE -ne 0) { throw "psql failed (see above)" } }
  finally { Remove-Item $f -ErrorAction SilentlyContinue }
}
function Get-LocalScalar([string]$sql) {
  Use-LocalPw
  $r = (& psql @localPg -tAc $sql).Trim()
  if ($LASTEXITCODE -ne 0) { throw "psql query failed: $sql" }
  return $r
}

# ---- 0. Lock down the .pem so Windows OpenSSH accepts it (idempotent) --------
if (Test-Path $SshKey) {
  try {
    icacls $SshKey /reset            | Out-Null
    icacls $SshKey /inheritance:r    | Out-Null
    icacls $SshKey /grant:r "$($env:USERNAME):(R)" | Out-Null
  } catch { Write-Warning "Could not tighten key ACLs ($_). If ssh rejects the key, fix perms manually." }
}

# ---- 1. Read server DB creds from its .env ----------------------------------
Write-Host "Reading server DB config from $SshTarget ..."
$remoteEnv = (ssh @sshOpts $SshTarget "cd $RemoteDir 2>/dev/null && grep -E '^(POSTGRES_HOST_PORT|POSTGRES_PASSWORD|POSTGRES_USER|POSTGRES_DB)=' .env") -join "`n"
if ($LASTEXITCODE -ne 0 -or -not $remoteEnv) { throw "Could not read $RemoteDir/.env on $SshTarget" }
$renv = @{}
foreach ($line in ($remoteEnv -split "`n")) {
  if ($line -match '^\s*([A-Z_]+)=(.*)$') { $renv[$Matches[1]] = $Matches[2].Trim().Trim('"').Trim("'") }
}
$remotePw   = $renv['POSTGRES_PASSWORD']; if (-not $remotePw) { throw "POSTGRES_PASSWORD not found in remote .env" }
$remoteUser = if ($renv['POSTGRES_USER']) { $renv['POSTGRES_USER'] } else { 'postgres' }
$remoteDb   = if ($renv['POSTGRES_DB'])   { $renv['POSTGRES_DB'] }   else { 'meme_bot' }
if ($RemotePgPort -le 0) { $RemotePgPort = if ($renv['POSTGRES_HOST_PORT']) { [int]$renv['POSTGRES_HOST_PORT'] } else { 5555 } }
Write-Host "  server postgres: 127.0.0.1:$RemotePgPort (db=$remoteDb user=$remoteUser)"

# ---- 2. Open the SSH tunnel (background) -------------------------------------
$fwd = "127.0.0.1:${TunnelLocalPort}:127.0.0.1:${RemotePgPort}"
Write-Host "Opening tunnel  local:$TunnelLocalPort  ->  $SshTarget : $RemotePgPort ..."
$tunnelArgs = $sshOpts + @('-o', 'ExitOnForwardFailure=yes', '-o', 'ServerAliveInterval=30', '-N', '-L', $fwd, $SshTarget)
$tunnel = Start-Process ssh -ArgumentList $tunnelArgs -PassThru -WindowStyle Hidden

try {
  # ---- 3. Wait for the tunnel + verify end-to-end with remote creds ----------
  $ok = $false
  foreach ($i in 1..40) {
    if ($tunnel.HasExited) { throw "SSH tunnel exited early (key perms? host? port $RemotePgPort not published on the server?)" }
    if ((Test-NetConnection -ComputerName 127.0.0.1 -Port $TunnelLocalPort -WarningAction SilentlyContinue).TcpTestSucceeded) { $ok = $true; break }
    Start-Sleep -Milliseconds 500
  }
  if (-not $ok) { throw "Tunnel port $TunnelLocalPort never opened" }

  $env:PGPASSWORD = $remotePw
  & psql -h 127.0.0.1 -p $TunnelLocalPort -U $remoteUser -d $remoteDb -v ON_ERROR_STOP=1 -tAc 'SELECT 1' | Out-Null
  if ($LASTEXITCODE -ne 0) { throw "Could not reach server Postgres through the tunnel (creds/port?)" }
  Use-LocalPw
  Write-Host "  tunnel up + server Postgres reachable."

  # ---- 4. Attach the server as a foreign server (fresh each run) -------------
  Write-Host "Attaching server via postgres_fdw ..."
  $pwEsc = $remotePw -replace "'", "''"
  Invoke-LocalSqlFile @"
CREATE EXTENSION IF NOT EXISTS postgres_fdw;
DROP SERVER IF EXISTS ec2_sync CASCADE;
CREATE SERVER ec2_sync FOREIGN DATA WRAPPER postgres_fdw
  OPTIONS (host '127.0.0.1', port '$TunnelLocalPort', dbname '$remoteDb', fetch_size '50000');
CREATE USER MAPPING FOR CURRENT_USER SERVER ec2_sync
  OPTIONS (user '$remoteUser', password '$pwEsc');
DROP SCHEMA IF EXISTS ec2_sync_src CASCADE;
CREATE SCHEMA ec2_sync_src;
IMPORT FOREIGN SCHEMA public
  LIMIT TO (trades, tokens, tokens_info, tokens_analysis, creator_profiles)
  FROM SERVER ec2_sync INTO ec2_sync_src;
"@

  # ---- 5. Schema-parity guard (abort before moving any data) -----------------
  Invoke-LocalSqlFile @'
DO $$
DECLARE t text;
BEGIN
  FOREACH t IN ARRAY ARRAY['trades','tokens','tokens_info','tokens_analysis','creator_profiles'] LOOP
    IF (SELECT string_agg(column_name, ',' ORDER BY ordinal_position)
          FROM information_schema.columns WHERE table_schema='public' AND table_name=t)
       IS DISTINCT FROM
       (SELECT string_agg(column_name, ',' ORDER BY ordinal_position)
          FROM information_schema.columns WHERE table_schema='ec2_sync_src' AND table_name=t)
    THEN RAISE EXCEPTION 'Column mismatch local vs server for table %; aborting (schema drift)', t;
    END IF;
  END LOOP;
END $$;
'@

  # ---- 6. Ensure local trades partitions cover the incoming window -----------
  Write-Host "Ensuring trades partitions ($PartitionDays days) ..."
  $today = (Get-Date).ToUniversalTime().Date
  $start = $today.AddDays(-$PartitionDays).ToString('yyyy-MM-dd')
  $end   = $today.AddDays(2).ToString('yyyy-MM-dd')
  Invoke-LocalSqlFile "DO `$`$ DECLARE d date := '$start'; BEGIN WHILE d <= '$end'::date LOOP PERFORM ensure_trades_partition(d); d := d + 1; END LOOP; END `$`$;"

  # ---- 7. Local watermarks ---------------------------------------------------
  Write-Host "Local watermarks:"
  $tradesWm   = Get-LocalScalar "SELECT COALESCE(MAX(block_time), '1970-01-01 00:00:00+00')::text FROM trades"
  $tokensWm   = Get-LocalScalar "SELECT COALESCE(MAX(created_at), '1970-01-01 00:00:00+00')::text FROM tokens"
  $tinfoWm    = Get-LocalScalar "SELECT COALESCE(MAX(updated_at), '1970-01-01 00:00:00+00')::text FROM tokens_info"
  $analysisWm = Get-LocalScalar "SELECT COALESCE(MAX(computed_at), '1970-01-01 00:00:00+00')::text FROM tokens_analysis"
  Write-Host "  trades >=   $tradesWm"
  Write-Host "  tokens >=   $tokensWm"
  Write-Host "  tok_info >= $tinfoWm"
  Write-Host "  analysis >= $analysisWm"

  # ---- 8. Upserts (predicate pushed to server; psql prints each row count) ----
  Write-Host "Appending new rows ..."
  $creatorSql = if ($NoCreatorProfiles) { "\echo 'creator_profiles: skipped'" } else { @"
\echo '-- creator_profiles'
INSERT INTO creator_profiles SELECT * FROM ec2_sync_src.creator_profiles
ON CONFLICT (wallet_address) DO UPDATE SET
  tokens_created = EXCLUDED.tokens_created, total_volume_sol = EXCLUDED.total_volume_sol,
  suspiciousness_score = EXCLUDED.suspiciousness_score, wash_trade_score = EXCLUDED.wash_trade_score,
  last_analyzed_at = EXCLUDED.last_analyzed_at, indicators = EXCLUDED.indicators;
"@ }

  Invoke-LocalSqlFile @"
\echo '-- trades'
INSERT INTO trades SELECT * FROM ec2_sync_src.trades
WHERE block_time >= '$tradesWm'::timestamptz
ON CONFLICT (tx_signature, leg_index, block_time) DO NOTHING;

\echo '-- tokens'
INSERT INTO tokens SELECT * FROM ec2_sync_src.tokens
WHERE created_at >= '$tokensWm'::timestamptz
ON CONFLICT (mint_address) DO NOTHING;

\echo '-- tokens_info'
INSERT INTO tokens_info SELECT * FROM ec2_sync_src.tokens_info
WHERE updated_at >= '$tinfoWm'::timestamptz
ON CONFLICT (mint_address) DO UPDATE SET
  ath_price = EXCLUDED.ath_price, ath_timestamp = EXCLUDED.ath_timestamp, age = EXCLUDED.age,
  volume = EXCLUDED.volume, market_cap = EXCLUDED.market_cap, trade_count = EXCLUDED.trade_count,
  last_trade_at = EXCLUDED.last_trade_at, current_price = EXCLUDED.current_price,
  is_dead = EXCLUDED.is_dead, is_migrated = EXCLUDED.is_migrated,
  last_synced_at = EXCLUDED.last_synced_at, last_synced_curve_sig = EXCLUDED.last_synced_curve_sig,
  last_synced_amm_sig = EXCLUDED.last_synced_amm_sig, last_synced_curve_slot = EXCLUDED.last_synced_curve_slot,
  last_synced_amm_slot = EXCLUDED.last_synced_amm_slot, lifetime_secs = EXCLUDED.lifetime_secs,
  updated_at = EXCLUDED.updated_at
WHERE EXCLUDED.updated_at >= tokens_info.updated_at;

\echo '-- tokens_analysis'
INSERT INTO tokens_analysis SELECT * FROM ec2_sync_src.tokens_analysis
WHERE computed_at >= '$analysisWm'::timestamptz
ON CONFLICT (mint_address, analyzer_name) DO UPDATE SET
  score = EXCLUDED.score, indicators = EXCLUDED.indicators, computed_at = EXCLUDED.computed_at
WHERE EXCLUDED.computed_at >= tokens_analysis.computed_at;

$creatorSql
"@

  # ---- 9. Detach (drops the stored server password from the local catalog) ---
  Invoke-LocalSqlFile "DROP SERVER IF EXISTS ec2_sync CASCADE; DROP SCHEMA IF EXISTS ec2_sync_src CASCADE;"
  Write-Host ""
  Write-Host "Incremental sync complete (server credentials removed from local catalog)."
}
finally {
  if ($tunnel -and -not $tunnel.HasExited) { Stop-Process -Id $tunnel.Id -Force -ErrorAction SilentlyContinue }
}
