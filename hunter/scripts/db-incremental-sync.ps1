<#
.SYNOPSIS
  Append the EC2 server's newer **sealed daily** data into the local hunter_bot DB --
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
    4. Per table: INSERT INTO <local> (<named cols>) SELECT <named cols> FROM ec2_sync_src.<t>
       WHERE <watermark predicate>  ON CONFLICT ...
       (Never `SELECT *` -- local/server column order can diverge after ALTER TABLE.
       The names are read from the local catalog after the parity check, so a new
       migration column syncs without editing this script.)
       The watermark is a literal, so postgres_fdw PUSHES IT DOWN to the server
       (remote chunk pruning) -- only new rows are fetched.

  **Sealed daily semantics.** High-volume hypertables (trades, raw_txs) are pulled
  only up to the start of *today* (UTC) -- i.e. yesterday-and-older, the days whose
  Timescale chunks are sealed (no longer receiving writes). Today stays open on the
  server and is pulled on the next run once it has sealed. Both bounds are literals,
  so the server prunes to exactly the sealed window. TimescaleDB auto-creates the
  destination chunks on insert, so there is no partition-ensure step anymore.

  **Stability (cold / large windows).** `trades` / `raw_txs` are pulled in fixed-hour
  chunks (default 2h, not one giant INSERT), with a smaller FDW `fetch_size`, SSH
  keepalives, and transient-error retries. A cold local DB (empty trades) clamps
  the watermark to the remote MIN so we never ask the 4GB EC2 box to stream the
  whole rolling window in one cursor. Partial progress is durable: each chunk
  commits independently; re-runs resume from the advanced watermark. Before each
  chunk's real INSERT, a best-effort remote COUNT(*) (its own short statement_timeout,
  never blocks the pull) prints "~N row(s) ... pulling" so a slow chunk reads as
  "working on N rows" instead of silence that looks hung.

  Non-destructive & idempotent throughout: existing local rows are never deleted;
  the dedup PKs + ON CONFLICT make re-running over an overlapping window a no-op.
  Strategy tables upsert server-wins WITHOUT propagating server-side deletes, so the
  lab keeps its accumulated history (rows the server has since deleted/aged out) and
  its lab-authored local-only rows. Conflict policy:
    wallet_dict      PK(id) / UNIQUE(address)                    -> MERGE (server wins by id; local-only old ids kept)
    tokens           PK(mint_address)                            -> DO NOTHING (write-once)
    tokens_info      PK(mint_address)                            -> DO UPDATE (newer updated_at wins)
    trades           PK(block_time, tx_signature, leg_index)     -> DO NOTHING (append-only)
    raw_txs          PK(block_time, tx_signature)                -> DO NOTHING (append-only; opt-in)
    fingerprints         PK(id)                                  -> DO UPDATE, non-destructive (server wins; local rows kept)
                         NOTE: server must have run core 0005-0007 first (wildcard column + the two CHECKs)
    strategy_rules       PK(id)                                  -> DO UPDATE, non-destructive (server wins; local rows kept)
    strategy_runs        PK(id)                                  -> DO UPDATE, non-destructive (server wins; local rows kept)
    strategy_run_metrics PK(run_id)                              -> DO UPDATE, non-destructive (server wins; local rows kept)
    strategy_positions   PK(id)                                  -> DO UPDATE, non-destructive (server wins; local rows kept)

  wallet_dict ids are GENERATED ALWAYS AS IDENTITY. The local DB is a read-mirror of
  the server's market data (lab has no ingest -- no intern() anywhere in the lab bin --
  so it never mints its own ids), so the dictionary must track the server's: trades.
  wallet_id (server id) resolves via `trades JOIN wallet_dict`, and a missing row makes
  those reads render the trade with an `unknown:<id>` wallet (a LEFT join + COALESCE in
  trade_repo.rs) rather than DROP it (there is NO FK on trades.wallet_id, so orphans are
  allowed to exist). We NON-DESTRUCTIVELY MERGE the server dict each run (one txn):
  pull it into a temp table, drop local rows whose address the server reassigned to a
  different id (server wins), then UPSERT every server row by id. Local-only ids the
  server no longer has are PRESERVED, because the lab retains trade history LONGER than
  the server's retention window (trades: 30 days) and those old trades still need them. Two earlier approaches
  were wrong: the original `WHERE id > MAX(local id) ON CONFLICT DO NOTHING` silently
  skipped colliding server rows (after the ~Jul-2026 rebuild re-minted wallet_dict this
  left ~98k ids missing / 58% of trades invisible); and a TRUNCATE+full-replace fixed
  recent days but DISCARDED the accumulated old rows the server had already aged out,
  re-orphaning the oldest retained days on every run. After the merge we setval() the
  IDENTITY sequence past MAX(id). Order: wallet_dict + tokens first, then tokens_info /
  trades. An in-txn completeness guard aborts if any SERVER id is
  missing locally after the merge; a separate post-trades REPORT (step 7a) just logs the
  residual orphans (ids absent from BOTH dicts -- pre-remint history that ages out).

  Strategy tables (fingerprints/rules/runs/run_metrics/positions) ARE synced (server wins)
  so the LIVE box's real+paper positions are viewable on the lab. They are copied
  FULL-TABLE each run (tiny vs trades; no watermark) and upserted with DO UPDATE so a
  server-side status/exit-fill change refreshes the local row. FK-safe order:
  fingerprints -> strategy_rules -> strategy_runs -> strategy_run_metrics ->
  strategy_positions. NOTE:
  server ids overwrite local rows on id-conflict, so a lab-authored rule/run sharing a
  UUID with the server's would be clobbered (UUIDs collide with ~0 probability).

  NON-DESTRUCTIVE (keep old local data): the strategy upsert only ADDS new server rows
  and REFRESHES changed ones -- it NEVER deletes a local row. So the lab retains its
  accumulated history: a run/position the server has since deleted or aged out of its
  rolling window SURVIVES locally, and the `lab` bin's OWN local-only rows (its
  create/update/delete-rule handlers write straight to the local DB -- see
  lab/src/api/handlers/strategies/{swing1,tpsl1,tpsl2}.rs) survive too. This DELIBERATELY
  does NOT propagate server-side deletes: a rule deleted / "clear paper results" / a
  position reaped on the live box lingers on the lab until removed manually -- the
  accepted cost of keeping old local data. (Earlier this script maintained a
  `_ec2_sync_seen_ids(table_name, id)` tombstone table that recorded every server-seen
  id and deleted any local row that had dropped off the server; that machinery was
  removed, and the table is DROPped each run so it doesn't linger on existing local DBs.)
  The one remaining strategy-table DELETE is a constraint-conflict resolver, not a
  tombstone: strategy_runs also has UNIQUE(rule_id, mode, run_seq), and a lab-authored
  run sharing that triple with a server run under a different id would block the insert,
  so such a divergent local row is dropped first (server wins) -- it fires only on a
  genuine secondary-key collision, never on age.

  Requires: ssh + psql on PATH; local Postgres >= 16 with postgres_fdw + TimescaleDB;
  connect as a SUPERUSER local role (CREATE EXTENSION / USER MAPPING). Stop any local
  backend writing to hunter_bot first. Server creds are read from the remote .env
  automatically (DB_PORT/POSTGRES_USER/PASSWORD/DB).

.EXAMPLE
  $env:PGPASSWORD = 'your_LOCAL_pw'
  ./scripts/db-incremental-sync.ps1 -SshTarget ubuntu@1.2.3.4

.EXAMPLE
  # Also pull the short-lived raw_txs feed (BYTEA payloads, large):
  ./scripts/db-incremental-sync.ps1 -SshTarget ubuntu@1.2.3.4 -IncludeRawTxs

.EXAMPLE
  # Pull today's still-open chunk too (partial day; re-run later to backfill the rest).
  # Drops the sealed-day upper bound on trades/raw_txs for this run only.
  ./scripts/db-incremental-sync.ps1 -SshTarget ubuntu@1.2.3.4 -IncludeToday

.EXAMPLE
  # One-shot current-day analysis refresh: DB sync (incl. today) then lake-export
  # --include-today so simulate/sweep see today's trades without a second manual hop.
  ./scripts/db-incremental-sync.ps1 -IncludeToday -ExportLake

.EXAMPLE
  # REPAIR a window the watermark has already passed (server wins on every column).
  # Use it when a day is SHORT locally (an ingest outage, an interrupted run) or when a
  # column reads NULL on the lab that the server has: the normal pull is append-only and
  # can fix neither. Costs the whole window re-transferred, so give it one day at a time
  # and keep it off the server's busy hours. The server keeps `trades` for 30 days, but
  # only the last ~7 are worth repairing this way: past that the local chunk is compressed
  # and a rewrite is refused outright -- use -RepairFillOnly there (next example).
  ./scripts/db-incremental-sync.ps1 -RepairFrom '2026-08-29 00:00:00+00' -RepairTo '2026-08-30 00:00:00+00'

.EXAMPLE
  # Repair a day PAST THE COMPRESSION LINE (7 days). -RepairFillOnly inserts the missing
  # rows and never rewrites an existing one, so it does not decompress: without it the
  # upsert fails outright with "tuple decompression limit exceeded by operation". It fills
  # HOLES but cannot heal a column, which is the right trade on an old chunk.
  ./scripts/db-incremental-sync.ps1 -RepairFrom '2026-08-07 00:00:00+00' -RepairTo '2026-08-08 00:00:00+00' -RepairFillOnly
#>
param(
  [string]$SshTarget       = 'ubuntu@35.158.128.131',                      # user@host of the EC2 box
  [string]$SshKey          = $(foreach ($p in "$PSScriptRoot/../aws-ec2-key.pem", "$HOME/.ssh/aws-ec2-key.pem") { if (Test-Path $p) { $p; break } }),
  [string]$RemoteDir       = '~/trade-launch-bot/hunter',                         # where the server's .env lives
  [string]$Database        = 'hunter_bot',
  [string]$LocalPgHost     = 'localhost',
  [int]   $LocalPgPort     = 5555,                                        # local hunter_bot port (5555 dockerized, 5432 native)
  [string]$LocalPgUser     = 'postgres',
  [int]   $TunnelLocalPort = 5433,                                        # local end of the SSH tunnel (must be free)
  [string]$FdwTunnelHost   = 'host.docker.internal',                      # how the LOCAL postgres reaches the tunnel: 'host.docker.internal' if it runs in Docker, '127.0.0.1' if native
  [int]   $RemotePgPort    = 0,                                           # 0 = auto-detect from remote .env (default 5555)
  [string]$RepairFrom      = '',                                          # repair mode: re-pull trades from this UTC timestamp with server-wins upserts (ignores the watermark)
  [string]$RepairTo        = '',                                          # repair mode upper bound (default: the same cutoff the normal pull uses)
  [switch]$RepairFillOnly,                                                # repair by inserting MISSING rows only, never rewriting an existing one -- required past the 7-day compression line, where a rewrite hits TimescaleDB's decompression limit
  [switch]$IncludeRawTxs,                                                 # also sync raw_txs (BYTEA payloads, large; off by default)
  [switch]$IncludeToday,                                                  # also pull today's still-open chunk (partial day; default = sealed days only)
  [switch]$ExportLake,                                                    # after sync: run `cargo run -p hunter-lab -- lake-export` (passes --include-today when -IncludeToday)
  [int]   $FdwFetchSize    = 10000,                                       # postgres_fdw fetch_size (smaller = gentler on 4GB EC2 RAM; was 50000)
  [int]   $HypertableChunkHours = 2,                                      # trades/raw_txs pull window size (hours); smaller = safer on EC2 RAM + commits/visible progress sooner (was 6)
  [int]   $ChunkRetries    = 4,                                           # retries per chunk on transient FDW/tunnel drops
  [int]   $TunnelReopenMinutes = 15,                                      # how long to keep reopening a dropped tunnel before giving up (the box goes unreachable for minutes at a time)
  [int]   $ChunkPreviewTimeoutMs = 8000,                                  # cap on the cheap remote COUNT(*) printed before each chunk (0 = skip preview)
  [string]$LocalPgPassword = $env:PGPASSWORD
)
# NOTE: 'Continue', not 'Stop'. Under 'Stop', a NATIVE exe (ssh/psql) writing ANY line
# to stderr -- even a benign NOTICE or Windows-OpenSSH's "close - IO is still pending on
# closed socket" teardown warning -- is wrapped as a terminating NativeCommandError in
# Windows PowerShell 5.1, aborting before we can inspect $LASTEXITCODE. Every native call
# below has an EXPLICIT exit-code check + throw, and cmdlet failures are guarded inline,
# so 'Continue' loses no real safety while making the script robust to stderr noise.
$ErrorActionPreference = 'Continue'
if (-not $LocalPgPassword) { throw "Set the LOCAL DB password: `$env:PGPASSWORD='...'  (or -LocalPgPassword)" }
# PowerShell switches are -IncludeToday / -ExportLake / -IncludeRawTxs (not --include-today).
# A leading-dash "host" almost always means a Unix-style flag was bound positionally to $SshTarget.
if ($SshTarget -match '^-') {
  throw "SshTarget looks like a flag ('$SshTarget'). Use PowerShell switches, e.g.: .\db-incremental-sync.ps1 -IncludeToday"
}
# Fail on an unparseable repair bound HERE, not an hour in at the trades step.
foreach ($b in @(@{n='RepairFrom'; v=$RepairFrom}, @{n='RepairTo'; v=$RepairTo})) {
  if ($b.v -and -not [datetimeoffset]::TryParse($b.v, [ref]([datetimeoffset]::MinValue))) {
    throw "-$($b.n) '$($b.v)' is not a timestamp. Use a UTC literal, e.g. -$($b.n) '2026-08-22 00:00:00+00'."
  }
}
if ($RepairTo -and -not $RepairFrom) { throw "-RepairTo needs -RepairFrom (repair mode is the window, not the upper bound alone)." }

# Force UTC so the sealed-day boundary (start-of-today) is computed in UTC
# regardless of the workstation's OS timezone.
$env:PGOPTIONS = '-c timezone=UTC'
$localPg = @('-h', $LocalPgHost, '-p', "$LocalPgPort", '-U', $LocalPgUser, '-d', $Database, '-v', 'ON_ERROR_STOP=1')
$sshOpts = @(
  '-i', $SshKey,
  '-o', 'StrictHostKeyChecking=accept-new',
  '-o', 'ConnectTimeout=10',
  '-o', 'ServerAliveInterval=20',
  '-o', 'ServerAliveCountMax=6',
  '-o', 'TCPKeepAlive=yes'
)

# ---- SSH passphrase handling (Windows-safe, no agent / no admin) -------------
# The script SSHes twice (read .env, then open the backgrounded tunnel). On Windows
# OpenSSH, ControlMaster multiplexing fails ("getsockname failed: Not a socket") and
# the ssh-agent service is often Disabled (enabling needs admin). The backgrounded
# tunnel's passphrase prompt is also invisible -> looks like an idle hang.
#
# Fix: if the key is passphrase-protected, ask ONCE here (masked), then feed every
# ssh call the passphrase via SSH_ASKPASS + SSH_ASKPASS_REQUIRE=force (OpenSSH >=8.4).
# No interactive prompts at all -> nothing to hang on. The passphrase lives only in a
# user-only temp helper that `finally` shreds. If the key has no passphrase, this is
# skipped (ssh just won't call the helper).
$script:askpassFile = $null
function Initialize-SshPassphrase {
  # Does the key need a passphrase? BatchMode=yes forbids prompts: if auth is
  # impossible without one, ssh-keygen -y -P '' fails on the encrypted key.
  $needs = $true
  try { & ssh-keygen -y -P '' -f $SshKey 2>$null | Out-Null; if ($LASTEXITCODE -eq 0) { $needs = $false } } catch {}
  if (-not $needs) { Write-Host "  ssh key has no passphrase (no prompt needed)."; return }

  # Non-interactive override: $env:SSH_KEY_PASSPHRASE lets automation supply it
  # without a prompt. Otherwise ask once, masked.
  if ($env:SSH_KEY_PASSPHRASE) {
    $pp = $env:SSH_KEY_PASSPHRASE
  } else {
    $sec = Read-Host -AsSecureString "Enter SSH key passphrase for $SshKey (entered ONCE; used for all ssh calls)"
    $bstr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($sec)
    try { $pp = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($bstr) }
    finally { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($bstr) }
  }

  # Helper prints ONLY the passphrase. .cmd so Windows ssh can exec it directly.
  $script:askpassFile = Join-Path $env:TEMP ("dbsync-askpass-{0}.cmd" -f $PID)
  # A previous run in THIS PowerShell session (same $PID -> same filename) that was
  # interrupted before `finally` ran leaves the helper locked to (RX) -- no write access,
  # so the Set-Content below would fail with UnauthorizedAccessException. Restore full
  # control and remove any stale copy first so creation is idempotent across re-runs.
  if (Test-Path $script:askpassFile) {
    try { icacls $script:askpassFile /grant:r "$($env:USERNAME):(F)" | Out-Null } catch {}
    Remove-Item $script:askpassFile -Force -ErrorAction SilentlyContinue
  }
  Set-Content -Path $script:askpassFile -Value ("@echo " + $pp) -Encoding ascii
  # Lock to the current user, but grant READ+EXECUTE -- ssh must EXEC this helper,
  # so read-only (RX missing) makes CreateProcess fail with "error:5".
  try { icacls $script:askpassFile /inheritance:r /grant:r "$($env:USERNAME):(RX)" | Out-Null } catch {}
  $env:SSH_ASKPASS         = $script:askpassFile
  $env:SSH_ASKPASS_REQUIRE = 'force'
  if (-not $env:DISPLAY) { $env:DISPLAY = 'localhost:0' }   # older ssh wants DISPLAY set
  Write-Host "  passphrase captured; ssh calls will authenticate non-interactively."
}
Initialize-SshPassphrase

# Tables to mirror, in FK-safe order. raw_txs is appended only with -IncludeRawTxs.
# token_sync_state was dropped server-side (2026-07-07, unused) -- not synced.
$syncTables = @('wallet_dict', 'tokens', 'tokens_info', 'trades')
if ($IncludeRawTxs) { $syncTables += 'raw_txs' }
# Strategy tables (FK chain: fingerprints -> rules -> runs -> run_metrics ->
# positions). Copied full-table each run with DO UPDATE (server wins) so live's
# real+paper positions show on the lab. Appended AFTER market data so the FDW
# import + parity guard cover them; their inserts run in a dedicated FK-ordered
# block (see step 7b). Non-destructive: no local row is deleted (lab-authored
# rows and aged-out server history survive).
$strategyTables = @('fingerprints', 'strategy_rules', 'strategy_runs', 'strategy_run_metrics', 'strategy_positions')
$syncTables += $strategyTables
$importList = ($syncTables -join ', ')

$Utf8NoBom = New-Object System.Text.UTF8Encoding $false   # psql chokes on a UTF-8 BOM
function Use-LocalPw { $env:PGPASSWORD = $LocalPgPassword }
function Invoke-LocalSqlFile([string]$sql) {
  $f = [System.IO.Path]::GetTempFileName() -replace '\.tmp$', '.sql'
  $errf = "$f.err"
  [System.IO.File]::WriteAllText($f, $sql, $Utf8NoBom)
  # Redirect psql's stderr to a FILE (not 2>&1 into the pipeline): in Windows
  # PowerShell 5.1 a native exe writing to stderr under $ErrorActionPreference='Stop'
  # is wrapped as a terminating NativeCommandError -- even for a harmless NOTICE like
  # "extension already exists". File redirection keeps stderr off the PS pipeline, so
  # only a real non-zero exit code aborts. We surface stderr afterward for visibility.
  try {
    & psql @localPg -f $f 2>$errf
    $code = $LASTEXITCODE
    $err = ''
    if (Test-Path $errf) { $err = (Get-Content $errf -Raw); if ($err -and $err.Trim()) { Write-Host $err.TrimEnd() } }
    if ($code -ne 0) {
      $detail = if ($err -and $err.Trim()) { $err.Trim() } else { '(no stderr)' }
      throw "psql failed (exit $code): $detail"
    }
  }
  finally { Remove-Item $f, $errf -ErrorAction SilentlyContinue }
}
function Get-LocalScalar([string]$sql) {
  Use-LocalPw
  $errf = [System.IO.Path]::GetTempFileName()
  try {
    $r = (& psql @localPg -tAc $sql 2>$errf)
    $code = $LASTEXITCODE
    if ($code -ne 0) {
      $err = if (Test-Path $errf) { (Get-Content $errf -Raw) } else { '' }
      throw "psql query failed: $sql`n$err"
    }
    return ("$r").Trim()
  }
  finally { Remove-Item $errf -ErrorAction SilentlyContinue }
}
function Get-LocalRows([string]$sql) {
  Use-LocalPw
  $errf = [System.IO.Path]::GetTempFileName()
  try {
    $r = (& psql @localPg -tAF "`t" -c $sql 2>$errf)
    $code = $LASTEXITCODE
    if ($code -ne 0) {
      $err = if (Test-Path $errf) { (Get-Content $errf -Raw) } else { '' }
      throw "psql query failed: $sql`n$err"
    }
    if ($null -eq $r) { return @() }
    return @($r | ForEach-Object { "$_" } | Where-Object { $_ -match '\S' })
  }
  finally { Remove-Item $errf -ErrorAction SilentlyContinue }
}
# ---- Column lists come from the SCHEMA, never from a list kept in this file ---
# Every INSERT below names its columns: a positional `SELECT *` misaligns the
# moment local and server column ORDER diverge (both sides ALTER TABLE ADD COLUMN
# at their own pace). A hand-written name list fixes that but rots the other way --
# a migration that ADDs a column is then silently not synced (it lands NULL
# locally, no error), and one that DROPs a column fails the run outright. So the
# lists are read out of the LOCAL catalog once, after the parity guard has proven
# local and server carry the same column set for every synced table.
$script:tableCols = @{}
function Initialize-TableColumns {
  foreach ($t in $syncTables) {
    $cols = Get-LocalScalar "SELECT string_agg(column_name, ',' ORDER BY ordinal_position) FROM information_schema.columns WHERE table_schema='public' AND table_name='$t'"
    if (-not $cols) { throw "Local table '$t' has no columns -- is the local DB migrated?" }
    $script:tableCols[$t] = @($cols -split ',')
  }
}
function Get-Cols([string]$t) {
  if (-not $script:tableCols.ContainsKey($t)) { throw "No column list for '$t' (Initialize-TableColumns has not run)" }
  return $script:tableCols[$t]
}
function Get-ColList([string]$t) { (Get-Cols $t) -join ', ' }                                   # INSERT target list
function Get-SelList([string]$t, [string]$alias) { ((Get-Cols $t) | ForEach-Object { "$alias.$_" }) -join ', ' }  # SELECT list
function Get-UpsertSet([string]$t, [string[]]$keys) {                                           # DO UPDATE body, conflict key(s) excluded
  ((Get-Cols $t) | Where-Object { $keys -notcontains $_ } | ForEach-Object { "$_ = EXCLUDED.$_" }) -join ', '
}

# Cheap remote COUNT(*) for the upcoming chunk window, printed BEFORE the real
# INSERT so a slow chunk shows "pulling ~N rows" instead of silence that looks
# hung. Best-effort only: capped by its own statement_timeout (independent of
# the real pull) and returns $null on any failure/timeout -- a stalled preview
# must never block or fail the actual sync, it just prints "(unavailable)".
function Get-RemoteChunkPreviewCount([string]$table, [string]$fromTs, [string]$toTs) {
  if ($ChunkPreviewTimeoutMs -le 0) { return $null }
  Use-LocalPw
  $sql = "SET statement_timeout = $ChunkPreviewTimeoutMs; SELECT count(*) FROM ec2_sync_src.$table WHERE block_time >= '$fromTs'::timestamptz AND block_time < '$toTs'::timestamptz;"
  $errf = [System.IO.Path]::GetTempFileName()
  try {
    $r = (& psql @localPg -tAc $sql 2>$errf)
    if ($LASTEXITCODE -ne 0) { return $null }
    $lines = @($r | ForEach-Object { "$_" } | Where-Object { $_ -match '\S' })
    if ($lines.Count -eq 0) { return $null }
    $n = 0L
    if ([int64]::TryParse($lines[-1].Trim(), [ref]$n)) { return $n }
    return $null
  } catch { return $null }
  finally { Remove-Item $errf -ErrorAction SilentlyContinue }
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
function Test-TransientSyncError([string]$msg) {
  # `relation "ec2_sync_src.*" does not exist` is transient too: the foreign schema is
  # this script's own scratch, so it means the attach was lost (a re-attach that failed
  # halfway, or another run's teardown), never that the SERVER lost a table. The retry's
  # Repair-FdwAttach rebuilds it -- treating it as fatal instead ends the run on
  # something one statement fixes.
  $msg -match '(?i)server closed the connection|no connection to the server|connection reset|could not connect to server|SSL connection has been closed|terminating connection|timeout expired|broken pipe|Connection timed out|server closed the connection unexpectedly|could not receive data from server|relation "ec2_sync_src\.[a-z_]+" does not exist|FATAL:\s+the database system is (starting|recovering|shutting)'
}
function Wait-Port([int]$Port, [string]$label, [int]$Seconds = 60) {
  foreach ($i in 1..$Seconds) {
    if (Test-Port $Port) { return $true }
    Start-Sleep -Seconds 1
  }
  throw "$label port $Port never became reachable (waited ${Seconds}s)"
}
# A long pull outlives its transport: sustained FDW traffic gets the ssh connection
# reset ("client_loop: send disconnect") often enough that treating a dead tunnel as
# fatal throws away hours of committed chunks over a blip that costs seconds to undo.
# So reopen it. Chunks commit one at a time and the retry re-runs only the failed
# one, so a reopened tunnel resumes exactly where the drop happened. `$script:tunnel`
# is rebound to the new process, which the cleanup `finally` then stops.
function Restart-SyncTunnel {
  # What drops the tunnel is usually what makes the FIRST reopen fail: the same outage
  # answers `ssh: connect to host ... port 22: Connection timed out`. Giving up there
  # ends the run on a blip that clears in under a minute, so reopen on a backoff and
  # only fail when the box is durably unreachable.
  # Measured: the box goes unreachable on port 22 for MINUTES at a time (five reopens
  # over ~4 min all answered "Connection timed out", and it was healthy again right
  # after), so the retry budget is minutes, not seconds -- a 3-hour repair must ride out
  # an outage that costs it one chunk, not the whole run.
  $deadline = (Get-Date).AddMinutes($TunnelReopenMinutes)
  $attempt = 0
  # ssh writes its failure to its OWN stderr, which does not reach this script's log
  # (PowerShell stream redirection does not follow a child process). Without capturing
  # it, every failure reads as a bare "did not come back" and says nothing about why.
  $sshErr = Join-Path $env:TEMP "dbsync-tunnel-$PID.err"
  while ((Get-Date) -lt $deadline) {
    $attempt++
    Write-Host ("  reopening the SSH tunnel (attempt {0}, until {1:HH:mm:ss}) ..." -f $attempt, $deadline)
    if ($script:tunnel -and -not $script:tunnel.HasExited) {
      Stop-Process -Id $script:tunnel.Id -Force -ErrorAction SilentlyContinue
    }
    # The forwarded port lingers in TIME_WAIT for a moment after ssh dies; a new tunnel
    # binding 0.0.0.0 fails with "Address already in use" if we race it.
    Start-Sleep -Seconds 2
    Remove-Item $sshErr -Force -ErrorAction SilentlyContinue
    $script:tunnel = Start-Process ssh -ArgumentList $tunnelArgs -PassThru -NoNewWindow -RedirectStandardError $sshErr
    foreach ($i in 1..45) {
      if ($script:tunnel.HasExited) { break }
      if (Test-Port $TunnelLocalPort) { Write-Host "  tunnel back up."; return }
      Start-Sleep -Seconds 1
    }
    $why = if (Test-Path $sshErr) { ((Get-Content $sshErr -Raw) -replace '\s+', ' ').Trim() } else { '' }
    if (-not $why) { $why = 'no ssh output (still connecting?)' }
    Write-Warning ("  tunnel did not come back: {0}" -f $why)
    Start-Sleep -Seconds 30
  }
  throw "SSH tunnel could not be reopened within $TunnelReopenMinutes min ($attempt attempts) -- the server is unreachable, not just busy."
}
function Assert-SyncTransport {
  if (-not (Test-Port $LocalPgPort)) {
    Write-Warning "Local Postgres on :$LocalPgPort is down (OOM restart?). Waiting up to 90s..."
    Wait-Port $LocalPgPort 'Local Postgres' 90 | Out-Null
  }
  if ($script:tunnel.HasExited -or -not (Test-Port $TunnelLocalPort)) { Restart-SyncTunnel }
}
# Recreate the FDW server mapping after a dropped remote connection so the next
# chunk opens a fresh remote session (stale user mappings / cached conns otherwise
# keep failing with "no connection to the server").
function Repair-FdwAttach {
  Write-Host "  re-attaching postgres_fdw after transport error ..."
  Use-LocalPw
  $pwEsc = $remotePw -replace "'", "''"
  Invoke-LocalSqlFile @"
DROP SERVER IF EXISTS ec2_sync CASCADE;
CREATE SERVER ec2_sync FOREIGN DATA WRAPPER postgres_fdw
  OPTIONS (
    host '$FdwTunnelHost',
    port '$TunnelLocalPort',
    dbname '$remoteDb',
    fetch_size '$FdwFetchSize',
    connect_timeout '30',
    keepalives '1',
    keepalives_idle '30',
    keepalives_interval '10',
    keepalives_count '5'
  );
CREATE USER MAPPING FOR CURRENT_USER SERVER ec2_sync
  OPTIONS (user '$remoteUser', password '$pwEsc');
DROP SCHEMA IF EXISTS ec2_sync_src CASCADE;
CREATE SCHEMA ec2_sync_src;
IMPORT FOREIGN SCHEMA public
  LIMIT TO ($importList)
  FROM SERVER ec2_sync INTO ec2_sync_src;
"@
}
function Invoke-LocalSqlFileRetry([string]$sql, [string]$label) {
  $attempt = 0
  while ($true) {
    $attempt++
    try {
      Use-LocalPw
      Invoke-LocalSqlFile $sql
      return
    } catch {
      $msg = "$_"
      if ($attempt -ge $ChunkRetries -or -not (Test-TransientSyncError $msg)) { throw }
      Write-Warning ("{0}: transient failure (attempt {1}/{2}) -- {3}" -f $label, $attempt, $ChunkRetries, $(
        $flat = ("$msg" -replace '\s+', ' ').Trim()
        if ($flat.Length -gt 180) { $flat.Substring(0, 180) + '...' } else { $flat }
      ))
      Assert-SyncTransport
      try { Repair-FdwAttach } catch {
        Write-Warning "  FDW re-attach failed: $_"
      }
      Start-Sleep -Seconds ([Math]::Min(30, 5 * $attempt))
    }
  }
}
# Time windows over [fromTs, toTs) in $HypertableChunkHours steps. First/last
# chunks are clipped to the watermark and cutoff so a mid-window watermark does
# not re-pull earlier hours. Default 2h keeps each FDW cursor small enough for
# the 4GB EC2 box (a full multi-day INSERT was dropping the remote connection).
function Get-HypertableChunks([string]$fromTs, [string]$toTs) {
  if ($HypertableChunkHours -lt 1) { throw "-HypertableChunkHours must be >= 1 (got $HypertableChunkHours)" }
  $rows = Get-LocalRows @"
WITH bounds AS (
  SELECT '$fromTs'::timestamptz AS lo, '$toTs'::timestamptz AS hi
),
grid AS (
  SELECT generate_series(
    date_trunc('hour', lo),
    hi,
    interval '$HypertableChunkHours hours'
  ) AS chunk_start
  FROM bounds
)
SELECT
  GREATEST(chunk_start, (SELECT lo FROM bounds))::text,
  LEAST(chunk_start + interval '$HypertableChunkHours hours', (SELECT hi FROM bounds))::text
FROM grid, bounds
WHERE GREATEST(chunk_start, lo) < LEAST(chunk_start + interval '$HypertableChunkHours hours', hi)
ORDER BY 1;
"@
  $chunks = @()
  foreach ($row in $rows) {
    $parts = $row -split "`t"
    if ($parts.Count -lt 2) { continue }
    $chunks += [pscustomobject]@{ From = $parts[0].Trim(); To = $parts[1].Trim() }
  }
  return $chunks
}
# The normal pull is APPEND-ONLY (`DO NOTHING`): a row already local is never
# touched, which is what keeps a re-run cheap. Repair mode (-RepairFrom) walks a
# window the watermark has already passed and lets the SERVER WIN on every column,
# so it both inserts rows the local mirror missed (an ingest outage, an interrupted
# run) and heals columns that were NULL locally while the server had them. It is
# the only way to fix either: the watermark never looks back, and `DO NOTHING`
# never rewrites. Cost is the whole window re-transferred, so it is opt-in.
function Sync-TradesChunks([string]$fromWm, [string]$toCutoff, [string]$windowLabel, [switch]$Repair, [switch]$FillOnly) {
  $chunks = @(Get-HypertableChunks $fromWm $toCutoff)
  if ($chunks.Count -eq 0) {
    Write-Host "-- trades ($windowLabel): nothing to pull (watermark >= cutoff)"
    return
  }
  # Two repairs, two costs. `DO UPDATE` heals COLUMNS, and to do that it rewrites every
  # matched row -- on a COMPRESSED chunk that means decompressing the chunk, which
  # TimescaleDB refuses outright: "tuple decompression limit exceeded by operation".
  # `DO NOTHING` fills MISSING ROWS only; it never touches a row that is already there,
  # so it neither decompresses nor errors, and it is far cheaper even on an uncompressed
  # chunk. Compression lands at 7 days here, so anything older than that wants -RepairFillOnly.
  $conflict = if ($Repair -and -not $FillOnly) {
    "DO UPDATE SET $(Get-UpsertSet 'trades' @('block_time','tx_signature','leg_index'))"
  } else { 'DO NOTHING' }
  Write-Host ("-- trades ($windowLabel): {0} x {1}h chunk(s) [{2} .. {3})" -f $chunks.Count, $HypertableChunkHours, $chunks[0].From, $toCutoff)
  $i = 0
  foreach ($c in $chunks) {
    $i++
    $label = "trades chunk $i/$($chunks.Count) [$($c.From) .. $($c.To))"
    Write-Host "  $label"
    $preview = Get-RemoteChunkPreviewCount 'trades' $c.From $c.To
    if ($null -ne $preview) { Write-Host "    ~$preview row(s) on server for this window -- pulling ..." }
    else { Write-Host "    (row-count preview unavailable/timed out; pulling anyway -- a large/contended window can take minutes)" }
    $started = Get-Date
    Invoke-LocalSqlFileRetry @"
\echo '-- $label'
INSERT INTO trades ($(Get-ColList 'trades'))
SELECT $(Get-SelList 'trades' 'tr')
FROM ec2_sync_src.trades tr
WHERE block_time >= '$($c.From)'::timestamptz AND block_time < '$($c.To)'::timestamptz
ON CONFLICT (block_time, tx_signature, leg_index) $conflict;
"@ $label
    $secs = [Math]::Round(((Get-Date) - $started).TotalSeconds, 1)
    $rate = if ($preview -and $secs -gt 0) { "  (~{0:N0} rows/s)" -f ([double]$preview / $secs) } else { '' }
    Write-Host ("    chunk done in {0}s{1}" -f $secs, $rate)
  }
}
function Sync-RawTxsChunks([string]$fromWm, [string]$toCutoff, [string]$windowLabel) {
  $chunks = @(Get-HypertableChunks $fromWm $toCutoff)
  if ($chunks.Count -eq 0) {
    Write-Host "-- raw_txs ($windowLabel): nothing to pull (watermark >= cutoff)"
    return
  }
  Write-Host ("-- raw_txs ($windowLabel): {0} x {1}h chunk(s) [{2} .. {3})" -f $chunks.Count, $HypertableChunkHours, $chunks[0].From, $toCutoff)
  $i = 0
  foreach ($c in $chunks) {
    $i++
    $label = "raw_txs chunk $i/$($chunks.Count) [$($c.From) .. $($c.To))"
    Write-Host "  $label"
    $preview = Get-RemoteChunkPreviewCount 'raw_txs' $c.From $c.To
    if ($null -ne $preview) { Write-Host "    ~$preview row(s) on server for this window -- pulling ..." }
    else { Write-Host "    (row-count preview unavailable/timed out; pulling anyway -- a large/contended window can take minutes)" }
    Invoke-LocalSqlFileRetry @"
\echo '-- $label'
INSERT INTO raw_txs ($(Get-ColList 'raw_txs'))
SELECT $(Get-SelList 'raw_txs' 'r')
FROM ec2_sync_src.raw_txs r
WHERE block_time >= '$($c.From)'::timestamptz AND block_time < '$($c.To)'::timestamptz
ON CONFLICT (block_time, tx_signature) DO NOTHING;
"@ $label
  }
}

# ---- 0. Lock down the .pem so Windows OpenSSH accepts it (idempotent) --------
if (Test-Path $SshKey) {
  try {
    icacls $SshKey /reset            | Out-Null
    icacls $SshKey /inheritance:r    | Out-Null
    icacls $SshKey /grant:r "$($env:USERNAME):(R)" | Out-Null
  } catch { Write-Warning "Could not tighten key ACLs ($_). If ssh rejects the key, fix perms manually." }
}

# ---- 0a. One run at a time ----------------------------------------------------
# Two overlapping runs SHARE one name: `ec2_sync_src`. The second attaches its own
# foreign schema over the first's, and whichever finishes first drops it in `finally`
# -- the survivor then dies mid-window on `relation "ec2_sync_src.trades" does not
# exist`, which reads like schema drift and is really a second copy of this script.
# The lock is an EXCLUSIVE FILE HANDLE, not a recorded PID: `$PID` is the *host*
# PowerShell's id, so a run launched from a long-lived shell records a process that
# outlives it and never looks stale -- the next run then refuses forever. A handle is
# owned by the run itself and the OS drops it when the process ends, so there is no
# staleness to reason about.
$script:lockFile = Join-Path $env:TEMP 'db-incremental-sync.lock'
try {
  $script:lockStream = [System.IO.File]::Open($script:lockFile, 'OpenOrCreate', 'ReadWrite', 'None')
} catch {
  throw "Another db-incremental-sync holds $($script:lockFile). Wait for it to finish -- two runs share the ec2_sync_src schema and drop each other's. (If one was killed mid-run from an interactive shell, its handle clears when that shell exits.)"
}

# ---- 0b. Who else is on the local DB ------------------------------------------
# A running `lab` (or `live`) holds pooled connections and keeps writing while this
# script upserts the same strategy tables. Nothing here corrupts under that -- every
# write is a single-statement upsert -- but a long lock wait shows up as an
# unexplained stall, and a rule edited in the lab mid-run is overwritten by the
# server's copy seconds later. So name the other sessions instead of leaving the
# stall unexplained. A warning, never a block: the sync is safe to run alongside.
$otherSessions = Get-LocalRows @"
SELECT app || ' x' || n::text FROM (
  SELECT COALESCE(NULLIF(application_name, ''), 'unnamed') AS app, count(*) AS n
  FROM pg_stat_activity
  WHERE datname = '$Database' AND pid <> pg_backend_pid()
    AND backend_type = 'client backend'          -- not Timescale's own workers
    AND COALESCE(application_name, '') NOT LIKE 'psql%'
  GROUP BY 1
) s ORDER BY app
"@
if ($otherSessions.Count -gt 0) {
  Write-Warning ("Other sessions on {0}: {1}. Stop the local backend for a clean run -- a lab edit made now is overwritten by the server's copy." -f $Database, ($otherSessions -join ', '))
}

# ---- 1. Read server DB creds from its .env ----------------------------------
Write-Host "Reading server DB config from $SshTarget ..."
# Windows OpenSSH can emit a benign "close - IO is still pending on closed socket"
# line to stderr; under $ErrorActionPreference='Stop' that would wrap as a terminating
# NativeCommandError before we can check $LASTEXITCODE. Send ssh stderr to a file so
# only a real non-zero exit aborts.
$sshErr = [System.IO.Path]::GetTempFileName()
$remoteEnv = (ssh @sshOpts $SshTarget "cd $RemoteDir 2>/dev/null && grep -E '^(DB_PORT|POSTGRES_HOST_PORT|POSTGRES_PASSWORD|POSTGRES_USER|POSTGRES_DB)=' .env" 2>$sshErr) -join "`n"
$sshCode = $LASTEXITCODE
Remove-Item $sshErr -ErrorAction SilentlyContinue
if ($sshCode -ne 0 -or -not $remoteEnv) { throw "Could not read $RemoteDir/.env on $SshTarget" }
$renv = @{}
foreach ($line in ($remoteEnv -split "`n")) {
  # Strip unquoted inline comments (`KEY=5555  # postgres`) -- remote .env keeps
  # at-a-glance banners; without this, [int]$renv['DB_PORT'] throws on the comment.
  if ($line -match '^\s*([A-Z_]+)=(.*)$') {
    $val = $Matches[2]
    if ($val -match '^([^#]*?)\s+#') { $val = $Matches[1] }
    $renv[$Matches[1]] = $val.Trim().Trim('"').Trim("'")
  }
}
$remotePw   = $renv['POSTGRES_PASSWORD']; if (-not $remotePw) { throw "POSTGRES_PASSWORD not found in remote .env" }
$remoteUser = if ($renv['POSTGRES_USER']) { $renv['POSTGRES_USER'] } else { 'postgres' }
$remoteDb   = if ($renv['POSTGRES_DB'])   { $renv['POSTGRES_DB'] }   else { 'hunter_bot' }
# DB_PORT is the current name; POSTGRES_HOST_PORT is read as a fallback for a
# server whose .env predates the rename (deploy/*/compose.yml PORTS block).
if ($RemotePgPort -le 0) {
  $portRaw = if ($renv['DB_PORT']) { $renv['DB_PORT'] }
             elseif ($renv['POSTGRES_HOST_PORT']) { $renv['POSTGRES_HOST_PORT'] }
             else { '5555' }
  $parsed = 0
  if (-not [int]::TryParse($portRaw, [ref]$parsed) -or $parsed -le 0) {
    throw "Remote DB port is not an integer ('$portRaw'). Check DB_PORT / POSTGRES_HOST_PORT in $RemoteDir/.env"
  }
  $RemotePgPort = $parsed
}
Write-Host "  server postgres: 127.0.0.1:$RemotePgPort (db=$remoteDb user=$remoteUser)"

# ---- 2. Open the SSH tunnel (background) -------------------------------------
# Bind 0.0.0.0 (not just loopback) so a DOCKERIZED local postgres can reach the
# tunnel via host.docker.internal -- postgres_fdw connects from INSIDE the
# container, where 127.0.0.1 is the container, not this host. Host-side psql
# checks below still reach it via 127.0.0.1 (0.0.0.0 includes loopback).
$fwd = "0.0.0.0:${TunnelLocalPort}:127.0.0.1:${RemotePgPort}"
Write-Host "Opening tunnel  local:$TunnelLocalPort  ->  $SshTarget : $RemotePgPort ..."
Write-Host "  (if a passphrase prompt appears below, enter the key passphrase to open the tunnel)"
$tunnelArgs = $sshOpts + @('-o', 'ExitOnForwardFailure=yes', '-N', '-L', $fwd, $SshTarget)
# -NoNewWindow: share this console so ssh's passphrase prompt is reachable. A hidden
# window would leave the prompt invisible and the tunnel stuck forever.
$script:tunnel = Start-Process ssh -ArgumentList $tunnelArgs -PassThru -NoNewWindow

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
  $verifyErr = [System.IO.Path]::GetTempFileName()
  & psql -h 127.0.0.1 -p $TunnelLocalPort -U $remoteUser -d $remoteDb -v ON_ERROR_STOP=1 -tAc 'SELECT 1' 2>$verifyErr | Out-Null
  $verifyCode = $LASTEXITCODE
  Remove-Item $verifyErr -ErrorAction SilentlyContinue
  if ($verifyCode -ne 0) { throw "Could not reach server Postgres through the tunnel (creds/port?)" }
  Use-LocalPw
  Write-Host "  tunnel up + server Postgres reachable."

  # ---- 4. Attach the server as a foreign server (fresh each run) -------------
  Write-Host "Attaching server via postgres_fdw (tables: $importList) ..."
  $pwEsc = $remotePw -replace "'", "''"
  Invoke-LocalSqlFile @"
CREATE EXTENSION IF NOT EXISTS postgres_fdw;
DROP SERVER IF EXISTS ec2_sync CASCADE;
CREATE SERVER ec2_sync FOREIGN DATA WRAPPER postgres_fdw
  OPTIONS (
    host '$FdwTunnelHost',
    port '$TunnelLocalPort',
    dbname '$remoteDb',
    fetch_size '$FdwFetchSize',
    connect_timeout '30',
    keepalives '1',
    keepalives_idle '30',
    keepalives_interval '10',
    keepalives_count '5'
  );
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
-- Ordered by column_name, NOT ordinal_position: a column added later via ALTER
-- TABLE ADD COLUMN always appends at the end on whichever side received it, so
-- local (recreated fresh from the squashed 0001_init.sql, columns in CREATE-TABLE
-- order) and the server (incrementally altered) can have the SAME columns in a
-- DIFFERENT physical order without being a real drift. All the inserts below now
-- reference columns BY NAME (never `SELECT *`), so physical order no longer
-- matters for correctness -- this guard only needs to catch a genuinely
-- missing/extra column (a real schema drift), not a benign reordering. (Confirmed
-- 2026-07-07: tokens/tokens_info/strategy_positions all had a same-set,
-- different-order false-positive here from exactly this cause.)
DO `$`$
DECLARE t text;
BEGIN
  FOREACH t IN ARRAY $parityArray LOOP
    IF (SELECT string_agg(column_name, ',' ORDER BY column_name)
          FROM information_schema.columns WHERE table_schema='public' AND table_name=t)
       IS DISTINCT FROM
       (SELECT string_agg(column_name, ',' ORDER BY column_name)
          FROM information_schema.columns WHERE table_schema='ec2_sync_src' AND table_name=t)
    THEN RAISE EXCEPTION 'Column mismatch local vs server for table %; aborting (schema drift)', t;
    END IF;
  END LOOP;
END `$`$;
"@

  # Safe to read the column lists now: the guard above has proven local and server
  # hold the same column set for every synced table, so a locally-read list names
  # only columns the foreign table also has.
  Initialize-TableColumns
  Write-Host ("Column lists read from the local catalog: " + (($syncTables | ForEach-Object { "$_ ($((Get-Cols $_).Count))" }) -join ', '))

  # ---- 6. Sealed-day boundary + local watermarks -----------------------------
  # The sealed cutoff: midnight UTC today. Hypertable pulls use [watermark, cutoff)
  # so only fully-sealed days move; today is left open for the next run.
  # Default: exclude today's still-open chunk (start-of-today UTC). With -IncludeToday,
  # push the upper bound to now() so today's partial day is pulled too. Either way the
  # bound is a literal, so postgres_fdw pushes it down for remote chunk pruning.
  if ($IncludeToday) {
    $sealedCutoff = Get-LocalScalar "SELECT (now() AT TIME ZONE 'UTC')::text"
    Write-Host "Cutoff (exclusive upper bound): $sealedCutoff UTC  [-IncludeToday: pulling today's open chunk]"
  } else {
    $sealedCutoff = Get-LocalScalar "SELECT date_trunc('day', now() AT TIME ZONE 'UTC')::text"
    Write-Host "Sealed-day cutoff (exclusive upper bound): $sealedCutoff UTC"
  }

  Write-Host "Local watermarks:"
  $walletWm  = Get-LocalScalar "SELECT COALESCE(MAX(id), 0) FROM wallet_dict"
  $tradesWm  = Get-LocalScalar "SELECT COALESCE(MAX(block_time), '1970-01-01 00:00:00+00')::text FROM trades"
  $tokensWm  = Get-LocalScalar "SELECT COALESCE(MAX(created_at), '1970-01-01 00:00:00+00')::text FROM tokens"
  $tinfoWm   = Get-LocalScalar "SELECT COALESCE(MAX(updated_at), '1970-01-01 00:00:00+00')::text FROM tokens_info"
  Write-Host "  wallet_dict         full mirror (local max id $walletWm; informational only)"
  Write-Host "  trades >=           $tradesWm"
  Write-Host "  tokens >=           $tokensWm"
  Write-Host "  tokens_info >=      $tinfoWm"
  if ($IncludeRawTxs) {
    $rawWm = Get-LocalScalar "SELECT COALESCE(MAX(block_time), '1970-01-01 00:00:00+00')::text FROM raw_txs"
    Write-Host "  raw_txs >=          $rawWm"
  }

  # Cold local DB: epoch watermarks would ask FDW to scan from 1970. Clamp to the
  # remote table's actual MIN so each day-chunk is a real Timescale partition pull
  # (and we don't OOM the 4GB EC2 box on a bogus full-history cursor).
  if ($tradesWm -match '^1970-01-01') {
    Write-Host "  cold local trades -- clamping watermark to remote MIN(block_time)"
    $remoteTradesMin = Get-LocalScalar "SELECT COALESCE(MIN(block_time), '$sealedCutoff'::timestamptz)::text FROM ec2_sync_src.trades"
    $tradesWm = $remoteTradesMin
    Write-Host "  trades >=           $tradesWm  (clamped)"
  }
  if ($IncludeRawTxs -and $rawWm -match '^1970-01-01') {
    Write-Host "  cold local raw_txs -- clamping watermark to remote MIN(block_time)"
    $remoteRawMin = Get-LocalScalar "SELECT COALESCE(MIN(block_time), '$sealedCutoff'::timestamptz)::text FROM ec2_sync_src.raw_txs"
    $rawWm = $remoteRawMin
    Write-Host "  raw_txs >=          $rawWm  (clamped)"
  }

  # ---- 7. Upserts (predicate pushed to server; psql prints each row count) ----
  # Order: wallet_dict + tokens first (referenced), then dependents + the
  # sealed-window hypertable pulls. TimescaleDB routes inserts to chunks (creating
  # them as needed) -- no partition-ensure step.
  Write-Host "Appending new rows ..."

  $windowLabel = if ($IncludeToday) { 'incl. today' } else { 'sealed days only' }

  # wallet_dict as a NON-DESTRUCTIVE FAITHFUL MERGE of the server (self-healing).
  #
  # Was (original): incremental `WHERE id > MAX(local id) ON CONFLICT DO NOTHING`.
  # That could only ADD a contiguous id-suffix and SILENTLY SKIPPED any server
  # (id,address) that collided with a stale local row on PK(id)/UNIQUE(address).
  # After the ~Jul-2026 live-lab schema rebuild re-minted wallet_dict server-side, it
  # left the local mirror missing ~98k scattered server ids -> 58% of synced trades
  # referenced a wallet_id with no wallet_dict row, and every `trades JOIN wallet_dict`
  # read silently DROPPED those trades. (`trades.wallet_id` has NO FK, so orphans are
  # allowed; `lab` never mints its own ids -- no intern() in the lab bin -- so the local
  # dict is meant to track the server's.)
  #
  # A naive TRUNCATE + full-replace is ALSO wrong: the server rolls its own window
  # (trades drop after 30 days) and RE-MINTED/AGED-OUT old wallet ids, but the LAB
  # retains trade history LONGER than that. Replacing wholesale discards the old (id,address) rows the
  # mirror accumulated for those older days -- which the server can no longer supply --
  # re-orphaning historical trades on every run (observed: it fixed Jul+ but re-broke
  # Jun 29-30 to ~90% orphaned).
  #
  # Correct: a non-destructive merge. Pull the server dict into a temp table once, then
  #   1. drop any local row whose ADDRESS the server now maps to a DIFFERENT id (a real
  #      server-side reassignment -- server wins; old trades on the dropped id become
  #      unresolvable, unavoidable for a genuine re-mint), then
  #   2. UPSERT every server row by id (add missing, correct reassigned addresses).
  # Local-only ids the server no longer has are PRESERVED, so historical trades stay
  # resolvable. One server scan/run; all else is local + indexed. Atomic (BEGIN/COMMIT).
  # `$walletWm` above is now only informational.
  Write-Host "Merging wallet_dict (non-destructive faithful mirror; server wins, old rows preserved) ..."
  Invoke-LocalSqlFileRetry @"
\echo '-- wallet_dict (non-destructive faithful merge; self-healing)'
BEGIN;
-- One remote scan into a local temp table so the merge below is all local + indexed.
CREATE TEMP TABLE wd_src ON COMMIT DROP AS SELECT id, address FROM ec2_sync_src.wallet_dict;
CREATE INDEX ON wd_src (id);
CREATE INDEX ON wd_src (address);
ANALYZE wd_src;

-- (1) Server reassigned an address to a new id -> drop the stale local holder so the
--     server row can land without violating UNIQUE(address). (A merely aged-out old
--     wallet has NO server row here, so its local row is left untouched below.)
DELETE FROM wallet_dict l USING wd_src s
  WHERE l.address = s.address AND l.id <> s.id;

-- (2) Upsert the server set by id: add every missing id, correct the address of any id
--     the server maps differently. Local-only ids absent from the server are preserved.
INSERT INTO wallet_dict (id, address)
OVERRIDING SYSTEM VALUE
SELECT id, address FROM wd_src
ON CONFLICT (id) DO UPDATE SET address = EXCLUDED.address;

-- Re-point the IDENTITY sequence past MAX(id) so any future local intern() mints a
-- fresh id instead of colliding on wallet_dict_pkey.
SELECT setval(pg_get_serial_sequence('wallet_dict', 'id'),
              (SELECT GREATEST(MAX(id), 1) FROM wallet_dict));

-- Completeness guard (in-txn): every SERVER id must now be present locally. A non-zero
-- count means the merge dropped/skipped server rows (a real regression) -- abort.
DO `$`$
DECLARE gaps bigint;
BEGIN
  SELECT COUNT(*) INTO gaps FROM wd_src s LEFT JOIN wallet_dict l ON l.id = s.id WHERE l.id IS NULL;
  IF gaps > 0 THEN
    RAISE EXCEPTION 'wallet_dict mirror INCOMPLETE: % server ids missing locally after merge', gaps;
  END IF;
END `$`$;
COMMIT;
"@ 'wallet_dict merge'

  # Repair mode is about `trades`: tokens carry no fee/leg data a repair heals, and
  # `trades` has no FK to them, so re-scanning the token tables buys nothing.
  if ($RepairFrom) { Write-Host "tokens + tokens_info: skipped (repair mode)." } else {
  Invoke-LocalSqlFileRetry @"
\echo '-- tokens'
-- created_at>=watermark is the fast path (FDW pushes it down), but server tokens
-- can arrive with a created_at EARLIER than the local max (out-of-order discovery,
-- backfills). Those would be skipped, yet their tokens_info row may still be pulled
-- below -> FK violation. So also pull any server token whose mint is missing locally.
-- Name-matched column list, read from the catalog (not `SELECT t.*`): local and
-- server can hold the SAME columns in a DIFFERENT physical order (e.g. creation_slot
-- was appended last on the server via ALTER TABLE ADD COLUMN, but sits mid-table
-- locally after the migration squash) -- a positional SELECT * would misalign them.
INSERT INTO tokens ($(Get-ColList 'tokens'))
SELECT $(Get-SelList 'tokens' 't')
FROM ec2_sync_src.tokens t
WHERE t.created_at >= '$tokensWm'::timestamptz
   OR NOT EXISTS (SELECT 1 FROM tokens l WHERE l.mint_address = t.mint_address)
ON CONFLICT (mint_address) DO NOTHING;

\echo '-- tokens_info'
-- Guard the FK: the server can hold a tokens_info row whose parent tokens row is
-- absent on the server too (orphaned info -> the tokens pull above can't backfill a
-- parent that doesn't exist). Only insert info rows whose mint is present in LOCAL
-- tokens (which by now holds everything pullable); skip server-side orphans instead
-- of aborting the whole sync on a single tokens_info_mint_address_fkey violation.
-- Name-matched column list -- same column-order-drift reason as tokens above
-- (first_slot_buy/sell_lamports sit last on the server, mid-table locally).
INSERT INTO tokens_info ($(Get-ColList 'tokens_info'))
SELECT $(Get-SelList 'tokens_info' 'i')
FROM ec2_sync_src.tokens_info i
WHERE i.updated_at >= '$tinfoWm'::timestamptz
  AND EXISTS (SELECT 1 FROM tokens l WHERE l.mint_address = i.mint_address)
ON CONFLICT (mint_address) DO UPDATE SET
  $(Get-UpsertSet 'tokens_info' @('mint_address'))
WHERE EXCLUDED.updated_at >= tokens_info.updated_at;
"@ 'tokens + tokens_info'
  }

  # Hypertables: fixed-hour chunks + retries. Never one giant INSERT over the full
  # [watermark, cutoff) window -- that OOMs / drops the FDW connection on EC2.
  if ($RepairFrom) {
    # Repair replaces the watermark pull for trades: the window is given, not derived,
    # and it is walked with server-wins upserts. -RepairTo defaults to the same cutoff
    # the normal pull uses, so a repair also brings the tail current.
    $repairTo = if ($RepairTo) { $RepairTo } else { $sealedCutoff }
    $how = if ($RepairFillOnly) { 'filling MISSING ROWS only (existing rows untouched)' } else { 'server-wins upserts (heals columns; rewrites every row)' }
    Write-Host "REPAIR: re-pulling trades [$RepairFrom .. $repairTo) -- $how, ignoring the watermark ..."
    Sync-TradesChunks $RepairFrom $repairTo "repair" -Repair -FillOnly:$RepairFillOnly
  } else {
    Sync-TradesChunks $tradesWm $sealedCutoff $windowLabel
  }
  if ($IncludeRawTxs) {
    Sync-RawTxsChunks $rawWm $sealedCutoff $windowLabel
  } else {
    Write-Host "raw_txs: skipped (pass -IncludeRawTxs to sync)"
  }

  # ---- 7a. Integrity report: residual orphaned trades ------------------------
  # The HARD completeness guard (every SERVER id present locally) runs INSIDE the
  # wallet_dict merge transaction above and aborts on a real mirror regression. Here
  # we only REPORT the residual: trades whose wallet_id is absent from BOTH dicts --
  # wallets the server re-minted/aged out of its retention window whose historical trades
  # the lab still retains. These render as `unknown:<id>` in the UI (the LEFT-join
  # fallback in trade_repo.rs), NOT as dropped rows, and they age out of the lab's own
  # retention over time. Informational only -- a rolling window of old orphans is
  # EXPECTED (server retention < lab retention) and must not fail the sync.
  # A full-table LEFT JOIN over every retained trade -- minutes on the lab's history,
  # and it says nothing about the window a repair just rewrote. Skipped in repair mode.
  if ($RepairFrom) { Write-Host "Orphan report: skipped (repair mode)." } else {
  Write-Host "Reporting residual trade->wallet_dict orphans (informational) ..."
  Invoke-LocalSqlFile @"
DO `$`$
DECLARE orphans bigint; oldest timestamptz; newest timestamptz;
BEGIN
  SELECT COUNT(*), MIN(t.block_time), MAX(t.block_time) INTO orphans, oldest, newest
    FROM trades t
    LEFT JOIN wallet_dict wd ON wd.id = t.wallet_id
    WHERE wd.id IS NULL;
  IF orphans > 0 THEN
    RAISE NOTICE 'Residual orphans: % trades reference wallet ids absent from both dicts (block_time % .. %). Shown as unknown:<id> in the UI; server re-minted/aged out those ids; ages out of lab retention.', orphans, oldest, newest;
  ELSE
    RAISE NOTICE 'Integrity OK: every trade resolves to a wallet_dict row.';
  END IF;
END `$`$;
"@
  }

  # ---- 7b. Strategy tables (view LIVE positions on the lab) ------------------
  # Full-table copy each run, server wins, NON-DESTRUCTIVE. FK-safe order:
  # fingerprints -> rules -> runs -> run_metrics -> positions. These tables are
  # tiny vs trades, so no watermark: we pull the whole remote table and upsert,
  # with a name-matched column list on both the INSERT and the DO UPDATE SET (see
  # the tokens/trades comments above -- a positional SELECT * would silently
  # misalign columns the moment local/server column order diverges). Both real +
  # paper rows are pulled.
  #
  # The lists come from the local catalog, so a core migration that adds a column
  # to any of these tables syncs it without an edit here: this script tracks the
  # SCHEMA, not the repos' *_COLS constants.
  #
  # NON-DESTRUCTIVE by design: the upsert ADDS new server rows and REFRESHES
  # changed ones (server wins), but NEVER deletes a local row. So the lab KEEPS
  # its accumulated history -- a run/position the server has since deleted or aged
  # out of its rolling window SURVIVES locally, and lab-authored local-only rows
  # (the lab's create/update/delete-rule handlers write straight to the local DB)
  # survive too. This deliberately does NOT propagate server-side deletes: a rule
  # deleted / paper results cleared / a position reaped on the live box lingers on
  # the lab until removed manually -- the accepted cost of retaining old local data.
  # (A former `_ec2_sync_seen_ids` tombstone table propagated those deletes; it was
  # removed, and dropped below so it doesn't linger on existing local DBs.)
  # Skipped in repair mode: these are a full-table mirror the next normal run redoes,
  # and a repair is usually long -- no reason to hold the strategy tables' locks at the
  # end of it. Run the script without -RepairFrom to refresh them.
  if ($RepairFrom) { Write-Host "Strategy tables: skipped (repair mode)." } else {
  Write-Host "Mirroring strategy tables (fingerprints/rules/runs/run_metrics/positions; server wins, non-destructive upsert) ..."
  # Explicit, plain INSERT..SELECT..ON CONFLICT DO UPDATE per table, in FK-safe
  # order. Server wins. DO UPDATE excludes the PK; EXCLUDED refreshes every other
  # column so a server-side status/exit-fill change updates the local row. NOTE:
  # strategy_runs ALSO has UNIQUE(rule_id,mode,run_seq). ON CONFLICT (id) only
  # resolves the PK, so a lab-authored run that shares that triple with a server
  # run under a different id WOULD collide on the secondary key -- the
  # strategy_runs block below deletes such divergent local rows first (server
  # wins). That is the ONLY delete here (a constraint-conflict resolver, not a
  # tombstone).
  Invoke-LocalSqlFileRetry @"
-- Retire the old tombstone bookkeeping table (deletes are no longer propagated).
DROP TABLE IF EXISTS _ec2_sync_seen_ids;

\echo '-- fingerprints'
-- Must land before strategy_rules (FK fingerprint_id). Every column moves, incl.
-- `criteria` (the axis registry) and `wildcard` -- both are MATCH IDENTITY, not
-- decoration: a server wildcard row landing locally with `wildcard` FALSE and an
-- empty `criteria` reads to the matcher as *matches nothing*, the exact opposite
-- of the row it copied. The catalog-read list carries them without being told.
--
-- ORDERING: the server must be redeployed onto the same core-migration state as
-- the lab before this sync runs -- otherwise the two shapes differ and the parity
-- guard above aborts the run by name. A pre-0009 server also carries per-axis
-- columns whose values the local `fingerprints_has_a_criterion` /
-- `fingerprints_wildcard_excludes_axes` CHECKs would reject anyway.
--
-- SECOND unique key: `fingerprints_identity_uniq` (criteria, wildcard, metric_config).
-- ON CONFLICT (id) resolves only the PK, so a lab-authored fingerprint matching a
-- server one under a different id would abort the whole insert on the secondary key.
-- Both sides run find_or_create against the same identity, so this is reachable, not
-- theoretical. Resolve it first, server wins: re-point every local rule at the SERVER
-- id, then drop the now-unreferenced local duplicate. Re-pointing is what makes the
-- delete legal -- the FK is NO ACTION, so a still-referenced row cannot be dropped and
-- the run would fail loudly rather than take a lab rule's fingerprint with it. The
-- match uses the SAME md5(jsonb::text) expressions as the index, so a row that would
-- collide is exactly a row this finds.
BEGIN;
CREATE TEMP TABLE _fp_identity_dupes ON COMMIT DROP AS
SELECT l.id AS local_id, r.id AS server_id
FROM fingerprints l
JOIN ec2_sync_src.fingerprints r
  ON md5(l.criteria::text) = md5(r.criteria::text)
 AND l.wildcard = r.wildcard
 AND md5(l.metric_config::text) = md5(r.metric_config::text)
WHERE l.id <> r.id;
UPDATE strategy_rules sr SET fingerprint_id = d.server_id, updated_at = now()
FROM _fp_identity_dupes d WHERE sr.fingerprint_id = d.local_id;
DELETE FROM fingerprints l USING _fp_identity_dupes d WHERE l.id = d.local_id;
COMMIT;

INSERT INTO fingerprints ($(Get-ColList 'fingerprints'))
SELECT $(Get-SelList 'fingerprints' 'f')
FROM ec2_sync_src.fingerprints f
ON CONFLICT (id) DO UPDATE SET
  $(Get-UpsertSet 'fingerprints' @('id'));

\echo '-- strategy_rules'
-- Post-0004 redesign columns + 0002 tags (rule_repo::RULE_COLS) -- NOT the
-- legacy strategy_id / buy_amount_sol shape.
--
-- ORDERING: `tags` is read off the FOREIGN table, so the SERVER must have run
-- core migration 0002 (i.e. the live bin must have been redeployed) before this
-- sync runs -- otherwise the SELECT fails on an unknown column. Server wins on
-- every column here, `tags` included: a tag edited LOCALLY on a rule the server
-- also owns is overwritten. Tag server-owned rules in the live app.
INSERT INTO strategy_rules ($(Get-ColList 'strategy_rules'))
SELECT $(Get-SelList 'strategy_rules' 'r')
FROM ec2_sync_src.strategy_rules r
ON CONFLICT (id) DO UPDATE SET
  $(Get-UpsertSet 'strategy_rules' @('id'));

\echo '-- strategy_runs'
-- strategy_runs has TWO unique constraints: PK(id) and UNIQUE(rule_id, mode, run_seq).
-- ON CONFLICT (id) only resolves the PK. If the lab authored its own run for a rule the
-- server also ran, the local row carries a DIFFERENT id but the SAME (rule_id, mode,
-- run_seq) -- the id-upsert misses and the secondary key blocks the insert. Server wins,
-- so drop any local run that collides on the secondary key under a different id first;
-- its metrics + positions cascade (ON DELETE CASCADE) and are re-inserted from the server.
-- This is a constraint-conflict resolver (needed so the INSERT can't abort), NOT a
-- tombstone -- it only fires on a genuine secondary-key collision, never on age.
DELETE FROM strategy_runs l
USING ec2_sync_src.strategy_runs r
WHERE l.rule_id IS NOT DISTINCT FROM r.rule_id
  AND l.mode = r.mode AND l.run_seq = r.run_seq
  AND l.id <> r.id;
-- Name-matched column list -- see the tokens/trades comments above for why a
-- positional SELECT * is unsafe once local/server column order can diverge.
INSERT INTO strategy_runs ($(Get-ColList 'strategy_runs'))
SELECT $(Get-SelList 'strategy_runs' 'u')
FROM ec2_sync_src.strategy_runs u
ON CONFLICT (id) DO UPDATE SET
  $(Get-UpsertSet 'strategy_runs' @('id'));

\echo '-- strategy_run_metrics'
-- Name-matched column list -- see the tokens/trades comments above for why a
-- positional SELECT * is unsafe once local/server column order can diverge. The
-- per-exit-reason counters are a moving set (one column per reason), so reading
-- them off the catalog is also what keeps a new reason from landing NULL here.
INSERT INTO strategy_run_metrics ($(Get-ColList 'strategy_run_metrics'))
SELECT $(Get-SelList 'strategy_run_metrics' 'm')
FROM ec2_sync_src.strategy_run_metrics m
ON CONFLICT (run_id) DO UPDATE SET
  $(Get-UpsertSet 'strategy_run_metrics' @('run_id'));

\echo '-- strategy_positions'
-- Name-matched column list -- same column-order-drift reason as tokens above
-- (token_account sits last on the server, mid-table locally). Slot accounting
-- (target/entry/exit_slot), park state and the scale-out stage ride along by
-- being in the catalog: a hand-kept list left them NULL on the lab.
INSERT INTO strategy_positions ($(Get-ColList 'strategy_positions'))
SELECT $(Get-SelList 'strategy_positions' 'p')
FROM ec2_sync_src.strategy_positions p
ON CONFLICT (id) DO UPDATE SET
  $(Get-UpsertSet 'strategy_positions' @('id'));
"@ 'strategy tables'
  }

  # ---- 8. Sync _sqlx_migrations so local backend doesn't re-apply applied migrations ---
  # The server's checksum records are authoritative (same files, same binary --
  # .gitattributes pins **/migrations/*.sql to eol=lf so the SHA-384 is identical on
  # Windows and in the Linux build container). Without this, _sqlx_migrations is
  # empty locally and sqlx re-runs all migrations on every startup, failing on
  # non-idempotent steps.
  #
  # CAUTION after a migration squash: this copies the SERVER's rows in, so if the
  # server ledger still lists versions 2..N and the local one was already collapsed
  # to a single version 1, this re-pollutes local and the next local boot aborts
  # ("previously applied but is missing"). Run scripts/consolidate-migration-ledgers.ps1
  # against the SERVER database first, then sync.
  Write-Host "Syncing _sqlx_migrations from server ..."
  $env:PGPASSWORD = $remotePw
  $migErr = [System.IO.Path]::GetTempFileName()
  $remoteMigrations = & psql -h 127.0.0.1 -p $TunnelLocalPort -U $remoteUser -d $remoteDb `
    -tAF "`t" -c "SELECT version, description, installed_on, success, checksum, execution_time FROM _sqlx_migrations ORDER BY version" 2>$migErr
  $migCode = $LASTEXITCODE
  Remove-Item $migErr -ErrorAction SilentlyContinue
  Use-LocalPw
  if ($migCode -eq 0 -and $remoteMigrations) {
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
  $doneWindow = if ($IncludeToday) { "through $sealedCutoff UTC, incl. today's partial chunk" } else { "sealed days through $sealedCutoff UTC" }
  Write-Host "Incremental sync complete ($doneWindow; server credentials removed from local catalog)."

  # ---- 10. Optional hop-2: PG -> Parquet lake (couples current-day analysis) ---
  # Keeps simulate/sweep on one command instead of a separate `lake-export` hop.
  # Runs AFTER detach so a long DuckDB export never holds the FDW server mapping.
  if ($ExportLake) {
    Write-Host ""
    Write-Host "Exporting Parquet lake (hop 2)..."
    $repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
    $exportArgs = @('run', '-p', 'hunter-lab', '--', 'lake-export')
    if ($IncludeToday) { $exportArgs += '--include-today' }
    Push-Location $repoRoot
    try {
      & cargo @exportArgs
      if ($LASTEXITCODE -ne 0) {
        throw "lake-export failed (exit $LASTEXITCODE). Local DB sync already completed -- re-run with -ExportLake only after fixing the lab build, or run: cargo run -p hunter-lab -- lake-export$(if ($IncludeToday) { ' --include-today' })"
      }
      Write-Host "Lake export complete."
    } finally {
      Pop-Location
    }
  }
}
finally {
  if ($tunnel -and -not $tunnel.HasExited) { Stop-Process -Id $tunnel.Id -Force -ErrorAction SilentlyContinue }
  if ($script:lockStream) { $script:lockStream.Dispose(); $script:lockStream = $null }
  if ($script:lockFile -and (Test-Path $script:lockFile)) { Remove-Item $script:lockFile -Force -ErrorAction SilentlyContinue }
  Remove-Item (Join-Path $env:TEMP "dbsync-tunnel-$PID.err") -Force -ErrorAction SilentlyContinue
  # Shred the passphrase helper and clear the env so the secret doesn't linger.
  # The file was locked to (RX) for ssh to exec it; restore (F) first or the
  # overwrite/delete is denied (UnauthorizedAccessException).
  if ($script:askpassFile -and (Test-Path $script:askpassFile)) {
    try { icacls $script:askpassFile /grant:r "$($env:USERNAME):(F)" | Out-Null } catch {}
    Set-Content -Path $script:askpassFile -Value '@echo.' -Encoding ascii -ErrorAction SilentlyContinue
    Remove-Item $script:askpassFile -Force -ErrorAction SilentlyContinue
  }
  Remove-Item Env:SSH_ASKPASS, Env:SSH_ASKPASS_REQUIRE -ErrorAction SilentlyContinue
}
