<#
.SYNOPSIS
  Pull the latest server data snapshot and REFRESH it into the local meme_bot DB
  WITHOUT dropping anything local. Run on your local Windows machine.

.DESCRIPTION
  Data-only refresh — NOT a drop & recreate. The local DB keeps its sweep results,
  settings, and raw_transactions; only the dumped tables (tokens, trades, rules,
  positions, wallets) are truncated and reloaded.

  Steps:
    1. download the newest dump from the box,
    2. ensure local trades day-partitions exist for the server's window
       (idempotent ensure_trades_partition — load-via-partition-root needs them),
    3. TRUNCATE only the refresh set (everything except raw / *grouped_sweep* /
       app_settings / _sqlx_migrations and the trades leaf partitions). Verified
       against the schema: nothing outside that set has an FK INTO it, so CASCADE
       cannot reach the sweep-result tables.
    4. pg_restore --data-only --disable-triggers (parallel; local has spare cores).

  Requires on PATH: ssh + scp (Windows OpenSSH), psql + pg_restore. Your LOCAL
  Postgres must be >= 16 (zstd dumps) and you must connect as a SUPERUSER role
  (--disable-triggers). Stop any local backend writing to meme_bot before running.

.EXAMPLE
  $env:PGPASSWORD = 'your_local_pw'
  ./scripts/db-snapshot-restore.ps1 -SshTarget ubuntu@1.2.3.4 -RemoteDir '~/meme-trading/snapshots'
#>
param(
  [Parameter(Mandatory = $true)][string]$SshTarget,        # user@host of the EC2 box
  [string]$RemoteDir   = '~/meme-trading/snapshots',       # where dumps live on the box
  [string]$RemoteFile  = '',                               # specific dump; default = newest
  [string]$LocalDir    = "$PSScriptRoot/../snapshots",     # local download dir
  [string]$Db          = 'meme_bot',                       # same name as on the server
  [string]$PgUser      = 'postgres',
  [string]$PgHost      = 'localhost',
  [int]   $PgPort      = 5432,
  [int]   $Jobs        = 4,
  [int]   $PartitionBackfillDays = 40,                     # >= server KEEP_DAYS + margin
  [string]$PgPassword  = $env:PGPASSWORD
)
$ErrorActionPreference = 'Stop'
if ($PgPassword) { $env:PGPASSWORD = $PgPassword }
$psql = @('-U', $PgUser, '-h', $PgHost, '-p', "$PgPort", '-v', 'ON_ERROR_STOP=1')

# 1. Resolve the newest remote dump if a specific one wasn't given.
if (-not $RemoteFile) {
  $RemoteFile = (ssh $SshTarget "ls -t $RemoteDir/*.dump 2>/dev/null | head -1").Trim()
  if (-not $RemoteFile) { throw "No .dump found in ${SshTarget}:$RemoteDir" }
}
Write-Host "Remote dump: $RemoteFile"

# 2. Download it.
New-Item -ItemType Directory -Force -Path $LocalDir | Out-Null
$Local = Join-Path $LocalDir (Split-Path $RemoteFile -Leaf)
scp "${SshTarget}:$RemoteFile" $Local
if ($LASTEXITCODE -ne 0) { throw "scp failed ($LASTEXITCODE)" }
Write-Host "Downloaded -> $Local"

# 3. Ensure local trades partitions cover the server's date window (idempotent).
$today = (Get-Date).ToUniversalTime().Date
$lines = for ($i = -$PartitionBackfillDays; $i -le 2; $i++) {
  "SELECT ensure_trades_partition('{0}');" -f $today.AddDays($i).ToString('yyyy-MM-dd')
}
($lines -join "`n") | & psql @psql -d $Db -f - | Out-Null
if ($LASTEXITCODE -ne 0) { throw "partition ensure failed" }

# 4. Truncate ONLY the refresh set. The pg_tables filter mirrors the dump's
#    exclusions; trades\_% drops the leaf partitions (the root 'trades' truncates
#    them via the parent). CASCADE is safe — verified no external FK reaches in.
$truncate = @'
DO $$
DECLARE tbls text;
BEGIN
  SELECT string_agg(format('%I.%I', schemaname, tablename), ', ')
    INTO tbls
  FROM pg_tables
  WHERE schemaname = 'public'
    AND tablename NOT LIKE 'raw\_transactions%'
    AND tablename NOT LIKE '%grouped\_sweep%'
    AND tablename NOT LIKE 'trades\_%'
    AND tablename NOT IN ('app_settings', '_sqlx_migrations');
  IF tbls IS NULL THEN RAISE EXCEPTION 'no tables matched for truncation'; END IF;
  RAISE NOTICE 'truncating refresh set: %', tbls;
  EXECUTE 'TRUNCATE ' || tbls || ' CASCADE';
END $$;
'@
$truncate | & psql @psql -d $Db -f -
if ($LASTEXITCODE -ne 0) { throw "truncate failed" }

# 5. Data-only restore. --disable-triggers defers FK checks during the parallel
#    load (needs a superuser local role). Sweep/settings/raw aren't in the dump,
#    so they are never written.
& pg_restore -U $PgUser -h $PgHost -p $PgPort -d $Db -j $Jobs `
    --data-only --disable-triggers $Local
if ($LASTEXITCODE -ne 0) { throw "pg_restore failed ($LASTEXITCODE)" }
Write-Host "Refreshed '$Db' (sweep results / settings / raw_transactions preserved)."
