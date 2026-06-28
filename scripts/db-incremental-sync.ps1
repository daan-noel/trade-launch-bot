<#
.SYNOPSIS
  Append the EC2 server's newer **sealed daily** data into the local meme_bot DB --
  directly, DB->DB, over an SSH tunnel. No CSV, no scp, no temp files.

.DESCRIPTION
  This is the `lab` data-pipeline hop 1 (EC2-PG -> local-PG) for the clean-rebuild
  TimescaleDB schema (live/lab remake, Phase 1/4). It supersedes the old CSV pipeline
  and the pre-rebuild version of this script (which targeted the dropped
  tokens_analysis / creator_profiles tables, the dropped ensure_trades_partition()
  function, and the old trades unique key). It:

    1. Opens an SSH tunnel to the server's published Postgres host port.
    2. Attaches the server as a postgres_fdw foreign server (schema ec2_sync_src).
    3. Verifies local vs remote column parity (aborts on schema drift).
    4. Per table: INSERT INTO <local> SELECT * FROM ec2_sync_src.<t>
       WHERE <watermark predicate>  ON CONFLICT ...
       The watermark is a literal, so postgres_fdw PUSHES IT DOWN to the server
       (remote chunk pruning) -- only new rows are fetched.

  **Sealed daily semantics.** High-volume hypertables (trades, raw_txs) are pulled
  only up to the start of *today* (UTC) -- i.e. yesterday-and-older, the days whose
  Timescale chunks are sealed (no longer receiving writes). Today stays open on the
  server and is pulled on the next run once it has sealed. Both bounds are literals,
  so the server prunes to exactly the sealed window. TimescaleDB auto-creates the
  destination chunks on insert, so there is no partition-ensure step anymore.

  Non-destructive & idempotent: existing local rows are never deleted; the dedup PKs
  + ON CONFLICT make re-running over an overlapping window a no-op. Conflict policy:
    wallet_dict      PK(id) / UNIQUE(address)                    -> DO NOTHING (immutable)
    tokens           PK(mint_address)                            -> DO NOTHING (write-once)
    tokens_info      PK(mint_address)                            -> DO UPDATE (newer updated_at wins)
    token_sync_state PK(mint_address, venue)                     -> DO UPDATE (newer last_synced_at wins)
    trades           PK(block_time, tx_signature, leg_index)     -> DO NOTHING (append-only)
    raw_txs          PK(block_time, tx_signature)                -> DO NOTHING (append-only; opt-in)

  wallet_dict ids are GENERATED ALWAYS AS IDENTITY. The local DB is a read-mirror of
  the server's market data (lab has no ingest, so it never mints its own ids), so we
  copy the server ids verbatim with OVERRIDING SYSTEM VALUE -- trades.wallet_id then
  resolves against the same dictionary on both boxes. Order matters for the FKs:
  wallet_dict + tokens first, then tokens_info / token_sync_state / trades.

  Strategy tables (strategy_rules/runs/run_metrics/positions) are NOT synced -- those
  are per-box authoring/run state, not shared market data.

  Requires: ssh + psql on PATH; local Postgres >= 16 with postgres_fdw + TimescaleDB;
  connect as a SUPERUSER local role (CREATE EXTENSION / USER MAPPING). Stop any local
  backend writing to meme_bot first. Server creds are read from the remote .env
  automatically (POSTGRES_HOST_PORT/USER/PASSWORD/DB).

.EXAMPLE
  $env:PGPASSWORD = 'your_LOCAL_pw'
  ./scripts/db-incremental-sync.ps1 -SshTarget ubuntu@1.2.3.4

.EXAMPLE
  # Also pull the short-lived raw_txs feed (BYTEA payloads, large):
  ./scripts/db-incremental-sync.ps1 -SshTarget ubuntu@1.2.3.4 -IncludeRawTxs
#>
param(
  [Parameter(Mandatory = $true)][string]$SshTarget,                       # user@host of the EC2 box
  [string]$SshKey          = "$PSScriptRoot/../aws-ec2-key.pem",
  [string]$RemoteDir       = '~/projects/meme-trading',                   # where the server's .env lives
  [string]$Database        = 'meme_bot',
  [string]$LocalPgHost     = 'localhost',
  [int]   $LocalPgPort     = 5432,                                        # local meme_bot port (5555 if dockerized)
  [string]$LocalPgUser     = 'postgres',
  [int]   $TunnelLocalPort = 5433,                                        # local end of the SSH tunnel (must be free)
  [int]   $RemotePgPort    = 0,                                           # 0 = auto-detect from remote .env (default 5555)
  [switch]$IncludeRawTxs,                                                 # also sync raw_txs (BYTEA payloads, large; off by default)
  [string]$LocalPgPassword = $env:PGPASSWORD
)
$ErrorActionPreference = 'Stop'
if (-not $LocalPgPassword) { throw "Set the LOCAL DB password: `$env:PGPASSWORD='...'  (or -LocalPgPassword)" }

# Force UTC so the sealed-day boundary (start-of-today) is computed in UTC
# regardless of the workstation's OS timezone.
$env:PGOPTIONS = '-c timezone=UTC'
$localPg = @('-h', $LocalPgHost, '-p', "$LocalPgPort", '-U', $LocalPgUser, '-d', $Database, '-v', 'ON_ERROR_STOP=1')
$sshOpts = @('-i', $SshKey, '-o', 'StrictHostKeyChecking=accept-new', '-o', 'ConnectTimeout=10')

# Tables to mirror, in FK-safe order. raw_txs is appended only with -IncludeRawTxs.
$syncTables = @('wallet_dict', 'tokens', 'tokens_info', 'token_sync_state', 'trades')
if ($IncludeRawTxs) { $syncTables += 'raw_txs' }
$importList = ($syncTables -join ', ')

$Utf8NoBom = New-Object System.Text.UTF8Encoding $false   # psql chokes on a UTF-8 BOM
function Use-LocalPw { $env:PGPASSWORD = $LocalPgPassword }
function Invoke-LocalSqlFile([string]$sql) {
  $f = [System.IO.Path]::GetTempFileName() -replace '\.tmp$', '.sql'
  [System.IO.File]::WriteAllText($f, $sql, $Utf8NoBom)
  try { & psql @localPg -f $f; if ($LASTEXITCODE -ne 0) { throw "psql failed (see above)" } }
  finally { Remove-Item $f -ErrorAction SilentlyContinue }
}
function Get-LocalScalar([string]$sql) {
  Use-LocalPw
  $r = (& psql @localPg -tAc $sql).Trim()
  if ($LASTEXITCODE -ne 0) { throw "psql query failed: $sql" }
  return $r
}
# Fast, silent TCP probe (Test-NetConnection does a slow ICMP ping + progress UI).
function Test-Port([int]$Port) {
  $c = New-Object System.Net.Sockets.TcpClient
  try {
    $iar = $c.BeginConnect('127.0.0.1', $Port, $null, $null)
    if ($iar.AsyncWaitHandle.WaitOne(800) -and $c.Connected) { $c.EndConnect($iar); return $true }
    return $false
  } catch { return $false } finally { $c.Close() }
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
Write-Host "  (if a passphrase prompt appears below, enter the key passphrase to open the tunnel)"
$tunnelArgs = $sshOpts + @('-o', 'ExitOnForwardFailure=yes', '-o', 'ServerAliveInterval=30', '-N', '-L', $fwd, $SshTarget)
# -NoNewWindow: share this console so ssh's passphrase prompt is reachable. A hidden
# window would leave the prompt invisible and the tunnel stuck forever.
$tunnel = Start-Process ssh -ArgumentList $tunnelArgs -PassThru -NoNewWindow

try {
  # ---- 3. Wait for the tunnel + verify end-to-end with remote creds ----------
  # Generous window (~2 min) so there's time to type the passphrase; breaks as
  # soon as the forwarded port accepts a connection.
  $ok = $false
  foreach ($i in 1..120) {
    if ($tunnel.HasExited) { throw "SSH tunnel exited (wrong passphrase, key perms, host, or server port $RemotePgPort not published?)" }
    if (Test-Port $TunnelLocalPort) { $ok = $true; break }
    Start-Sleep -Milliseconds 1000
  }
  if (-not $ok) { throw "Tunnel port $TunnelLocalPort never opened" }

  $env:PGPASSWORD = $remotePw
  & psql -h 127.0.0.1 -p $TunnelLocalPort -U $remoteUser -d $remoteDb -v ON_ERROR_STOP=1 -tAc 'SELECT 1' | Out-Null
  if ($LASTEXITCODE -ne 0) { throw "Could not reach server Postgres through the tunnel (creds/port?)" }
  Use-LocalPw
  Write-Host "  tunnel up + server Postgres reachable."

  # ---- 4. Attach the server as a foreign server (fresh each run) -------------
  Write-Host "Attaching server via postgres_fdw (tables: $importList) ..."
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
  LIMIT TO ($importList)
  FROM SERVER ec2_sync INTO ec2_sync_src;
"@

  # ---- 5. Schema-parity guard (abort before moving any data) -----------------
  # Build the table-array literal from $syncTables so it tracks -IncludeRawTxs.
  $parityArray = "ARRAY[" + (($syncTables | ForEach-Object { "'$_'" }) -join ',') + "]"
  Invoke-LocalSqlFile @"
DO `$`$
DECLARE t text;
BEGIN
  FOREACH t IN ARRAY $parityArray LOOP
    IF (SELECT string_agg(column_name, ',' ORDER BY ordinal_position)
          FROM information_schema.columns WHERE table_schema='public' AND table_name=t)
       IS DISTINCT FROM
       (SELECT string_agg(column_name, ',' ORDER BY ordinal_position)
          FROM information_schema.columns WHERE table_schema='ec2_sync_src' AND table_name=t)
    THEN RAISE EXCEPTION 'Column mismatch local vs server for table %; aborting (schema drift)', t;
    END IF;
  END LOOP;
END `$`$;
"@

  # ---- 6. Sealed-day boundary + local watermarks -----------------------------
  # The sealed cutoff: midnight UTC today. Hypertable pulls use [watermark, cutoff)
  # so only fully-sealed days move; today is left open for the next run.
  $sealedCutoff = Get-LocalScalar "SELECT date_trunc('day', now() AT TIME ZONE 'UTC')::text"
  Write-Host "Sealed-day cutoff (exclusive upper bound): $sealedCutoff UTC"

  Write-Host "Local watermarks:"
  $walletWm  = Get-LocalScalar "SELECT COALESCE(MAX(id), 0) FROM wallet_dict"
  $tradesWm  = Get-LocalScalar "SELECT COALESCE(MAX(block_time), '1970-01-01 00:00:00+00')::text FROM trades"
  $tokensWm  = Get-LocalScalar "SELECT COALESCE(MAX(created_at), '1970-01-01 00:00:00+00')::text FROM tokens"
  $tinfoWm   = Get-LocalScalar "SELECT COALESCE(MAX(updated_at), '1970-01-01 00:00:00+00')::text FROM tokens_info"
  $syncWm    = Get-LocalScalar "SELECT COALESCE(MAX(last_synced_at), '1970-01-01 00:00:00+00')::text FROM token_sync_state"
  Write-Host "  wallet_dict id >    $walletWm"
  Write-Host "  trades >=           $tradesWm"
  Write-Host "  tokens >=           $tokensWm"
  Write-Host "  tokens_info >=      $tinfoWm"
  Write-Host "  token_sync_state >= $syncWm"
  if ($IncludeRawTxs) {
    $rawWm = Get-LocalScalar "SELECT COALESCE(MAX(block_time), '1970-01-01 00:00:00+00')::text FROM raw_txs"
    Write-Host "  raw_txs >=          $rawWm"
  }

  # ---- 7. Upserts (predicate pushed to server; psql prints each row count) ----
  # Order: wallet_dict + tokens first (referenced), then dependents + the
  # sealed-window hypertable pulls. TimescaleDB routes inserts to chunks (creating
  # them as needed) -- no partition-ensure step.
  Write-Host "Appending new rows ..."

  $rawTxsSql = if ($IncludeRawTxs) { @"
\echo '-- raw_txs (sealed days only)'
INSERT INTO raw_txs SELECT * FROM ec2_sync_src.raw_txs
WHERE block_time >= '$rawWm'::timestamptz AND block_time < '$sealedCutoff'::timestamptz
ON CONFLICT (block_time, tx_signature) DO NOTHING;
"@ } else { "\echo 'raw_txs: skipped (pass -IncludeRawTxs to sync)'" }

  Invoke-LocalSqlFile @"
\echo '-- wallet_dict (id-preserving mirror)'
INSERT INTO wallet_dict (id, address)
OVERRIDING SYSTEM VALUE
SELECT id, address FROM ec2_sync_src.wallet_dict
WHERE id > $walletWm
ON CONFLICT DO NOTHING;

\echo '-- tokens'
INSERT INTO tokens SELECT * FROM ec2_sync_src.tokens
WHERE created_at >= '$tokensWm'::timestamptz
ON CONFLICT (mint_address) DO NOTHING;

\echo '-- tokens_info'
INSERT INTO tokens_info SELECT * FROM ec2_sync_src.tokens_info
WHERE updated_at >= '$tinfoWm'::timestamptz
ON CONFLICT (mint_address) DO UPDATE SET
  current_price = EXCLUDED.current_price, ath_price = EXCLUDED.ath_price,
  ath_timestamp = EXCLUDED.ath_timestamp, volume = EXCLUDED.volume,
  trade_count = EXCLUDED.trade_count, last_trade_at = EXCLUDED.last_trade_at,
  is_dead = EXCLUDED.is_dead, is_migrated = EXCLUDED.is_migrated,
  lifetime_secs = EXCLUDED.lifetime_secs, updated_at = EXCLUDED.updated_at
WHERE EXCLUDED.updated_at >= tokens_info.updated_at;

\echo '-- token_sync_state'
INSERT INTO token_sync_state SELECT * FROM ec2_sync_src.token_sync_state
WHERE last_synced_at >= '$syncWm'::timestamptz
ON CONFLICT (mint_address, venue) DO UPDATE SET
  last_sig = EXCLUDED.last_sig, last_slot = EXCLUDED.last_slot,
  last_synced_at = EXCLUDED.last_synced_at
WHERE EXCLUDED.last_synced_at >= token_sync_state.last_synced_at;

\echo '-- trades (sealed days only)'
INSERT INTO trades SELECT * FROM ec2_sync_src.trades
WHERE block_time >= '$tradesWm'::timestamptz AND block_time < '$sealedCutoff'::timestamptz
ON CONFLICT (block_time, tx_signature, leg_index) DO NOTHING;

$rawTxsSql
"@

  # ---- 8. Sync _sqlx_migrations so local backend doesn't re-apply applied migrations ---
  # The server's checksum records are authoritative (same files, same binary).
  # Without this, _sqlx_migrations is empty locally and sqlx re-runs all migrations
  # on every startup, failing on non-idempotent steps.
  Write-Host "Syncing _sqlx_migrations from server ..."
  $env:PGPASSWORD = $remotePw
  $remoteMigrations = & psql -h 127.0.0.1 -p $TunnelLocalPort -U $remoteUser -d $remoteDb `
    -tAF "`t" -c "SELECT version, description, installed_on, success, checksum, execution_time FROM _sqlx_migrations ORDER BY version"
  Use-LocalPw
  if ($LASTEXITCODE -eq 0 -and $remoteMigrations) {
    $upsertLines = foreach ($row in ($remoteMigrations -split "`n" | Where-Object { $_ -match '\S' })) {
      $cols = $row -split "`t"
      if ($cols.Count -lt 6) { continue }
      $ver   = $cols[0].Trim()
      $desc  = $cols[1].Trim() -replace "'", "''"
      $inst  = $cols[2].Trim()
      $succ  = if ($cols[3].Trim() -eq 't') { 'true' } else { 'false' }
      $cksum = $cols[4].Trim()   # bytea hex from psql
      $exec  = $cols[5].Trim()
      "INSERT INTO _sqlx_migrations (version, description, installed_on, success, checksum, execution_time) VALUES ($ver, '$desc', '$inst', $succ, '$cksum'::bytea, $exec) ON CONFLICT (version) DO NOTHING;"
    }
    if ($upsertLines) {
      Invoke-LocalSqlFile ($upsertLines -join "`n")
      Write-Host "  _sqlx_migrations synced."
    }
  } else {
    Write-Warning "_sqlx_migrations sync skipped (server query failed or empty -- run backend once to apply migrations manually)."
  }

  # ---- 9. Detach (drops the stored server password from the local catalog) ---
  Invoke-LocalSqlFile "DROP SERVER IF EXISTS ec2_sync CASCADE; DROP SCHEMA IF EXISTS ec2_sync_src CASCADE;"
  Write-Host ""
  Write-Host "Incremental sync complete (sealed days through $sealedCutoff UTC; server credentials removed from local catalog)."
}
finally {
  if ($tunnel -and -not $tunnel.HasExited) { Stop-Process -Id $tunnel.Id -Force -ErrorAction SilentlyContinue }
}
