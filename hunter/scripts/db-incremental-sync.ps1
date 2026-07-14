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
  the server's 7-day window and those old trades still need them. Two earlier approaches
  were wrong: the original `WHERE id > MAX(local id) ON CONFLICT DO NOTHING` silently
  skipped colliding server rows (after the ~Jul-2026 rebuild re-minted wallet_dict this
  left ~98k ids missing / 58% of trades invisible); and a TRUNCATE+full-replace fixed
  recent days but DISCARDED the accumulated old rows the server had already aged out,
  re-orphaning the oldest retained days on every run. After the merge we setval() the
  IDENTITY sequence past MAX(id). Order: wallet_dict + tokens first, then tokens_info /
  trades. An in-txn completeness guard aborts if any SERVER id is
  missing locally after the merge; a separate post-trades REPORT (step 7a) just logs the
  residual orphans (ids absent from BOTH dicts -- pre-remint history that ages out).

  Strategy tables (strategy_rules/runs/run_metrics/positions) ARE synced (server wins)
  so the LIVE box's real+paper positions are viewable on the lab. They are copied
  FULL-TABLE each run (tiny vs trades; no watermark) and upserted with DO UPDATE so a
  server-side status/exit-fill change refreshes the local row. FK-safe order:
  strategy_rules -> strategy_runs -> strategy_run_metrics -> strategy_positions. NOTE:
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
#>
param(
  [string]$SshTarget       = 'ubuntu@54.93.174.192',                      # user@host of the EC2 box
  [string]$SshKey          = $(foreach ($p in "$PSScriptRoot/../aws-ec2-key.pem", "$HOME/.ssh/aws-ec2-key.pem") { if (Test-Path $p) { $p; break } }),
  [string]$RemoteDir       = '~/projects/hunter',                         # where the server's .env lives
  [string]$Database        = 'hunter_bot',
  [string]$LocalPgHost     = 'localhost',
  [int]   $LocalPgPort     = 5555,                                        # local hunter_bot port (5555 dockerized, 5432 native)
  [string]$LocalPgUser     = 'postgres',
  [int]   $TunnelLocalPort = 5433,                                        # local end of the SSH tunnel (must be free)
  [string]$FdwTunnelHost   = 'host.docker.internal',                      # how the LOCAL postgres reaches the tunnel: 'host.docker.internal' if it runs in Docker, '127.0.0.1' if native
  [int]   $RemotePgPort    = 0,                                           # 0 = auto-detect from remote .env (default 5555)
  [switch]$IncludeRawTxs,                                                 # also sync raw_txs (BYTEA payloads, large; off by default)
  [switch]$IncludeToday,                                                  # also pull today's still-open chunk (partial day; default = sealed days only)
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

# Force UTC so the sealed-day boundary (start-of-today) is computed in UTC
# regardless of the workstation's OS timezone.
$env:PGOPTIONS = '-c timezone=UTC'
$localPg = @('-h', $LocalPgHost, '-p', "$LocalPgPort", '-U', $LocalPgUser, '-d', $Database, '-v', 'ON_ERROR_STOP=1')
$sshOpts = @('-i', $SshKey, '-o', 'StrictHostKeyChecking=accept-new', '-o', 'ConnectTimeout=10')

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
# Strategy tables (FK chain: rules -> runs -> run_metrics -> positions). Copied
# full-table each run with DO UPDATE (server wins) so live's real+paper positions
# show on the lab. Appended AFTER market data so the FDW import + parity guard cover
# them; their inserts + tombstone-delete propagation run in a dedicated FK-ordered
# block (see step 7b) -- deletes are scoped to ids previously seen from the server
# (see `_ec2_sync_seen_ids` in step 7b), so a lab-authored rule/run/position is never
# a delete candidate.
$strategyTables = @('strategy_rules', 'strategy_runs', 'strategy_run_metrics', 'strategy_positions')
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
    if (Test-Path $errf) { $err = (Get-Content $errf -Raw); if ($err -and $err.Trim()) { Write-Host $err.TrimEnd() } }
    if ($code -ne 0) { throw "psql failed (exit $code; see above)" }
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
  if ($line -match '^\s*([A-Z_]+)=(.*)$') { $renv[$Matches[1]] = $Matches[2].Trim().Trim('"').Trim("'") }
}
$remotePw   = $renv['POSTGRES_PASSWORD']; if (-not $remotePw) { throw "POSTGRES_PASSWORD not found in remote .env" }
$remoteUser = if ($renv['POSTGRES_USER']) { $renv['POSTGRES_USER'] } else { 'postgres' }
$remoteDb   = if ($renv['POSTGRES_DB'])   { $renv['POSTGRES_DB'] }   else { 'hunter_bot' }
# DB_PORT is the current name; POSTGRES_HOST_PORT is read as a fallback for a
# server whose .env predates the rename (deploy/*/compose.yml PORTS block).
if ($RemotePgPort -le 0) {
  $RemotePgPort = if ($renv['DB_PORT']) { [int]$renv['DB_PORT'] }
                  elseif ($renv['POSTGRES_HOST_PORT']) { [int]$renv['POSTGRES_HOST_PORT'] }
                  else { 5555 }
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
  OPTIONS (host '$FdwTunnelHost', port '$TunnelLocalPort', dbname '$remoteDb', fetch_size '50000');
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

  # ---- 7. Upserts (predicate pushed to server; psql prints each row count) ----
  # Order: wallet_dict + tokens first (referenced), then dependents + the
  # sealed-window hypertable pulls. TimescaleDB routes inserts to chunks (creating
  # them as needed) -- no partition-ensure step.
  Write-Host "Appending new rows ..."

  $windowLabel = if ($IncludeToday) { 'incl. today' } else { 'sealed days only' }

  $rawTxsSql = if ($IncludeRawTxs) { @"
\echo '-- raw_txs ($windowLabel)'
-- Explicit, name-matched column list -- see the tokens/trades comments above for
-- why positional SELECT * is unsafe once local/server column order can diverge.
INSERT INTO raw_txs (tx_signature, slot, block_time, tx_index, payload, source)
SELECT r.tx_signature, r.slot, r.block_time, r.tx_index, r.payload, r.source
FROM ec2_sync_src.raw_txs r
WHERE block_time >= '$rawWm'::timestamptz AND block_time < '$sealedCutoff'::timestamptz
ON CONFLICT (block_time, tx_signature) DO NOTHING;
"@ } else { "\echo 'raw_txs: skipped (pass -IncludeRawTxs to sync)'" }

  # wallet_dict as a NON-DESTRUCTIVE FAITHFUL MERGE of the server (self-healing).
  #
  # Was (original): incremental `WHERE id > MAX(local id) ON CONFLICT DO NOTHING`.
  # That could only ADD a contiguous id-suffix and SILENTLY SKIPPED any server
  # (id,address) that collided with a stale local row on PK(id)/UNIQUE(address).
  # After the ~Jul-2026 live-lab schema rebuild re-minted wallet_dict server-side, it
  # left the local mirror missing ~98k scattered server ids -> 58% of synced trades
  # referenced a wallet_id with no wallet_dict row, and every `trades JOIN wallet_dict`
  # read silently DROPPED those trades. (`trades.wallet_id` has NO FK, so orphans are
  # allowed; `lab` never mints its own ids — no intern() in the lab bin — so the local
  # dict is meant to track the server's.)
  #
  # A naive TRUNCATE + full-replace is ALSO wrong: the server has a 7-day rolling
  # window and RE-MINTED/AGED-OUT old wallet ids, but the LAB retains trade history
  # LONGER than 7 days. Replacing wholesale discards the old (id,address) rows the
  # mirror accumulated for those older days — which the server can no longer supply —
  # re-orphaning historical trades on every run (observed: it fixed Jul+ but re-broke
  # Jun 29-30 to ~90% orphaned).
  #
  # Correct: a non-destructive merge. Pull the server dict into a temp table once, then
  #   1. drop any local row whose ADDRESS the server now maps to a DIFFERENT id (a real
  #      server-side reassignment — server wins; old trades on the dropped id become
  #      unresolvable, unavoidable for a genuine re-mint), then
  #   2. UPSERT every server row by id (add missing, correct reassigned addresses).
  # Local-only ids the server no longer has are PRESERVED, so historical trades stay
  # resolvable. One server scan/run; all else is local + indexed. Atomic (BEGIN/COMMIT).
  # `$walletWm` above is now only informational.
  Write-Host "Merging wallet_dict (non-destructive faithful mirror; server wins, old rows preserved) ..."
  Invoke-LocalSqlFile @"
\echo '-- wallet_dict (non-destructive faithful merge; self-healing)'
BEGIN;
-- One remote scan into a local temp table so the merge below is all local + indexed.
CREATE TEMP TABLE wd_src ON COMMIT DROP AS SELECT id, address FROM ec2_sync_src.wallet_dict;
CREATE INDEX ON wd_src (id);
CREATE INDEX ON wd_src (address);
ANALYZE wd_src;

-- (1) Server reassigned an address to a new id → drop the stale local holder so the
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
-- count means the merge dropped/skipped server rows (a real regression) — abort.
DO `$`$
DECLARE gaps bigint;
BEGIN
  SELECT COUNT(*) INTO gaps FROM wd_src s LEFT JOIN wallet_dict l ON l.id = s.id WHERE l.id IS NULL;
  IF gaps > 0 THEN
    RAISE EXCEPTION 'wallet_dict mirror INCOMPLETE: % server ids missing locally after merge', gaps;
  END IF;
END `$`$;
COMMIT;
"@

  Invoke-LocalSqlFile @"
\echo '-- tokens'
-- created_at>=watermark is the fast path (FDW pushes it down), but server tokens
-- can arrive with a created_at EARLIER than the local max (out-of-order discovery,
-- backfills). Those would be skipped, yet their tokens_info row may still be pulled
-- below -> FK violation. So also pull any server token whose mint is missing locally.
-- Explicit, name-matched column list (not `SELECT t.*`): local and server can hold
-- the SAME columns in a DIFFERENT physical order (e.g. creation_slot was appended
-- last on the server via ALTER TABLE ADD COLUMN, but sits mid-table locally after
-- the migration squash) -- a positional SELECT * would silently misalign columns.
INSERT INTO tokens (
  mint_address, creator_wallet, name, symbol, bonding_curve_address, token_program_id,
  initial_supply_token, total_supply_token, initial_buy_lamports, cu_limit, cu_price, is_mayhem_mode,
  is_cashback_enabled, creation_slot, creation_tx_signature, ix_labels,
  initial_buy_instruction, meta, created_at
)
SELECT
  t.mint_address, t.creator_wallet, t.name, t.symbol, t.bonding_curve_address, t.token_program_id,
  t.initial_supply_token, t.total_supply_token, t.initial_buy_lamports, t.cu_limit, t.cu_price, t.is_mayhem_mode,
  t.is_cashback_enabled, t.creation_slot, t.creation_tx_signature, t.ix_labels,
  t.initial_buy_instruction, t.meta, t.created_at
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
-- Explicit, name-matched column list -- same column-order-drift reason as tokens
-- above (first_slot_buy/sell_lamports sit last on the server, mid-table locally).
INSERT INTO tokens_info (
  mint_address, current_price, ath_price, ath_timestamp, volume_sol, trade_count,
  last_trade_at, first_slot_buy_lamports, first_slot_sell_lamports, is_dead,
  is_migrated, lifetime_secs, updated_at
)
SELECT
  i.mint_address, i.current_price, i.ath_price, i.ath_timestamp, i.volume_sol, i.trade_count,
  i.last_trade_at, i.first_slot_buy_lamports, i.first_slot_sell_lamports, i.is_dead,
  i.is_migrated, i.lifetime_secs, i.updated_at
FROM ec2_sync_src.tokens_info i
WHERE i.updated_at >= '$tinfoWm'::timestamptz
  AND EXISTS (SELECT 1 FROM tokens l WHERE l.mint_address = i.mint_address)
ON CONFLICT (mint_address) DO UPDATE SET
  current_price = EXCLUDED.current_price, ath_price = EXCLUDED.ath_price,
  ath_timestamp = EXCLUDED.ath_timestamp, volume_sol = EXCLUDED.volume_sol,
  trade_count = EXCLUDED.trade_count, last_trade_at = EXCLUDED.last_trade_at,
  is_dead = EXCLUDED.is_dead, is_migrated = EXCLUDED.is_migrated,
  lifetime_secs = EXCLUDED.lifetime_secs, updated_at = EXCLUDED.updated_at,
  first_slot_buy_lamports = EXCLUDED.first_slot_buy_lamports,
  first_slot_sell_lamports = EXCLUDED.first_slot_sell_lamports
WHERE EXCLUDED.updated_at >= tokens_info.updated_at;

\echo '-- trades ($windowLabel)'
-- Explicit, name-matched column list -- trades is the highest-stakes table here
-- (financial data); a positional SELECT * would silently miswire columns on any
-- future one-sided ALTER TABLE, same failure mode as tokens/tokens_info above.
INSERT INTO trades (
  mint_address, wallet_id, trade_type, venue, amount_lamports, token_amount,
  reserve_lamports, reserve_token, slot, tx_index, leg_index, block_time, tx_signature
)
SELECT
  tr.mint_address, tr.wallet_id, tr.trade_type, tr.venue, tr.amount_lamports, tr.token_amount,
  tr.reserve_lamports, tr.reserve_token, tr.slot, tr.tx_index, tr.leg_index, tr.block_time, tr.tx_signature
FROM ec2_sync_src.trades tr
WHERE block_time >= '$tradesWm'::timestamptz AND block_time < '$sealedCutoff'::timestamptz
ON CONFLICT (block_time, tx_signature, leg_index) DO NOTHING;

$rawTxsSql
"@

  # ---- 7a. Integrity report: residual orphaned trades ------------------------
  # The HARD completeness guard (every SERVER id present locally) runs INSIDE the
  # wallet_dict merge transaction above and aborts on a real mirror regression. Here
  # we only REPORT the residual: trades whose wallet_id is absent from BOTH dicts —
  # wallets the server re-minted/aged out of its 7-day window whose historical trades
  # the lab still retains. These render as `unknown:<id>` in the UI (the LEFT-join
  # fallback in trade_repo.rs), NOT as dropped rows, and they age out of the lab's own
  # retention over time. Informational only — a rolling window of old orphans is
  # EXPECTED (server retention < lab retention) and must not fail the sync.
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

  # ---- 7b. Strategy tables (view LIVE positions on the lab) ------------------
  # Full-table copy each run, server wins, NON-DESTRUCTIVE. FK-safe order: rules ->
  # runs -> run_metrics -> positions. These tables are tiny vs trades, so no
  # watermark: we pull the whole remote table and upsert, with an explicit
  # name-matched column list on both the INSERT and the DO UPDATE SET (see the
  # tokens/trades comments above -- a positional SELECT * would silently misalign
  # columns the moment local/server column order diverges, e.g. a future
  # migration's ALTER TABLE ADD COLUMN). Both real + paper rows are pulled.
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
  Write-Host "Mirroring strategy tables (rules/runs/run_metrics/positions; server wins, non-destructive upsert) ..."
  # Explicit, plain INSERT..SELECT..ON CONFLICT DO UPDATE per table, in FK-safe order
  # (rules -> runs -> run_metrics -> positions). Server wins. DO UPDATE excludes the
  # PK; EXCLUDED refreshes every other column so a server-side status/exit-fill change
  # updates the local row. NOTE: strategy_runs ALSO has UNIQUE(rule_id,mode,run_seq).
  # ON CONFLICT (id) only resolves the PK, so a lab-authored run that shares that triple
  # with a server run under a different id WOULD collide on the secondary key -- the
  # strategy_runs block below deletes such divergent local rows first (server wins).
  # That is the ONLY delete here (a constraint-conflict resolver, not a tombstone).
  Invoke-LocalSqlFile @"
-- Retire the old tombstone bookkeeping table (deletes are no longer propagated).
DROP TABLE IF EXISTS _ec2_sync_seen_ids;

\echo '-- strategy_rules'
-- Explicit, name-matched column list -- see the tokens/trades comments above for
-- why positional SELECT * is unsafe once local/server column order can diverge.
INSERT INTO strategy_rules (
  id, strategy_id, rule_name, buy_amount_sol, trade_mode, is_active,
  max_concurrent_tokens, max_total_tokens, params, created_at, updated_at
)
SELECT
  r.id, r.strategy_id, r.rule_name, r.buy_amount_sol, r.trade_mode, r.is_active,
  r.max_concurrent_tokens, r.max_total_tokens, r.params, r.created_at, r.updated_at
FROM ec2_sync_src.strategy_rules r
ON CONFLICT (id) DO UPDATE SET
  strategy_id = EXCLUDED.strategy_id, rule_name = EXCLUDED.rule_name,
  buy_amount_sol = EXCLUDED.buy_amount_sol, trade_mode = EXCLUDED.trade_mode,
  is_active = EXCLUDED.is_active, max_concurrent_tokens = EXCLUDED.max_concurrent_tokens,
  max_total_tokens = EXCLUDED.max_total_tokens, params = EXCLUDED.params,
  created_at = EXCLUDED.created_at, updated_at = EXCLUDED.updated_at;

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
-- Explicit, name-matched column list -- see the tokens/trades comments above for
-- why positional SELECT * is unsafe once local/server column order can diverge.
INSERT INTO strategy_runs (
  id, strategy_id, rule_id, mode, run_seq, status, params_snapshot,
  max_total_tokens, started_at, finished_at
)
SELECT
  u.id, u.strategy_id, u.rule_id, u.mode, u.run_seq, u.status, u.params_snapshot,
  u.max_total_tokens, u.started_at, u.finished_at
FROM ec2_sync_src.strategy_runs u
ON CONFLICT (id) DO UPDATE SET
  strategy_id = EXCLUDED.strategy_id, rule_id = EXCLUDED.rule_id,
  mode = EXCLUDED.mode, run_seq = EXCLUDED.run_seq, status = EXCLUDED.status,
  params_snapshot = EXCLUDED.params_snapshot, max_total_tokens = EXCLUDED.max_total_tokens,
  started_at = EXCLUDED.started_at, finished_at = EXCLUDED.finished_at;

\echo '-- strategy_run_metrics'
-- Explicit, name-matched column list -- see the tokens/trades comments above for
-- why positional SELECT * is unsafe once local/server column order can diverge.
INSERT INTO strategy_run_metrics (
  run_id, rolled_up_at, n_fired, n_open, n_closed, win_rate, total_pnl_sol,
  expectancy_sol, mean_pnl_pct, median_pnl_pct, p90_pnl_pct, best_pnl_pct,
  worst_pnl_pct, std_pnl_pct, profit_factor, avg_holding_secs, median_holding_secs,
  n_exit_take_profit, n_exit_stop_loss, n_exit_trailing, n_exit_stall, n_exit_time,
  n_exit_liquidity, n_exit_open
)
SELECT
  m.run_id, m.rolled_up_at, m.n_fired, m.n_open, m.n_closed, m.win_rate, m.total_pnl_sol,
  m.expectancy_sol, m.mean_pnl_pct, m.median_pnl_pct, m.p90_pnl_pct, m.best_pnl_pct,
  m.worst_pnl_pct, m.std_pnl_pct, m.profit_factor, m.avg_holding_secs, m.median_holding_secs,
  m.n_exit_take_profit, m.n_exit_stop_loss, m.n_exit_trailing, m.n_exit_stall, m.n_exit_time,
  m.n_exit_liquidity, m.n_exit_open
FROM ec2_sync_src.strategy_run_metrics m
ON CONFLICT (run_id) DO UPDATE SET
  rolled_up_at = EXCLUDED.rolled_up_at, n_fired = EXCLUDED.n_fired,
  n_open = EXCLUDED.n_open, n_closed = EXCLUDED.n_closed, win_rate = EXCLUDED.win_rate,
  total_pnl_sol = EXCLUDED.total_pnl_sol, expectancy_sol = EXCLUDED.expectancy_sol,
  mean_pnl_pct = EXCLUDED.mean_pnl_pct, median_pnl_pct = EXCLUDED.median_pnl_pct,
  p90_pnl_pct = EXCLUDED.p90_pnl_pct, best_pnl_pct = EXCLUDED.best_pnl_pct,
  worst_pnl_pct = EXCLUDED.worst_pnl_pct, std_pnl_pct = EXCLUDED.std_pnl_pct,
  profit_factor = EXCLUDED.profit_factor, avg_holding_secs = EXCLUDED.avg_holding_secs,
  median_holding_secs = EXCLUDED.median_holding_secs,
  n_exit_take_profit = EXCLUDED.n_exit_take_profit, n_exit_stop_loss = EXCLUDED.n_exit_stop_loss,
  n_exit_trailing = EXCLUDED.n_exit_trailing, n_exit_stall = EXCLUDED.n_exit_stall,
  n_exit_time = EXCLUDED.n_exit_time, n_exit_liquidity = EXCLUDED.n_exit_liquidity,
  n_exit_open = EXCLUDED.n_exit_open;

\echo '-- strategy_positions'
-- Explicit, name-matched column list -- same column-order-drift reason as tokens
-- above (token_account sits last on the server, mid-table locally).
INSERT INTO strategy_positions (
  id, run_id, strategy_id, rule_id, mode, mint_address, wallet, token_program_id,
  token_account, target_price, target_token_amount, target_time, target_tx,
  entry_price, entry_token_amount, entry_lamports, entry_time, entry_tx_signatures,
  exit_price, exit_token_amount, exit_lamports, exit_time, exit_tx_signatures,
  submitted_buy_signatures, status, exit_reason, extra, created_at, updated_at
)
SELECT
  p.id, p.run_id, p.strategy_id, p.rule_id, p.mode, p.mint_address, p.wallet, p.token_program_id,
  p.token_account, p.target_price, p.target_token_amount, p.target_time, p.target_tx,
  p.entry_price, p.entry_token_amount, p.entry_lamports, p.entry_time, p.entry_tx_signatures,
  p.exit_price, p.exit_token_amount, p.exit_lamports, p.exit_time, p.exit_tx_signatures,
  p.submitted_buy_signatures, p.status, p.exit_reason, p.extra, p.created_at, p.updated_at
FROM ec2_sync_src.strategy_positions p
ON CONFLICT (id) DO UPDATE SET
  run_id = EXCLUDED.run_id, strategy_id = EXCLUDED.strategy_id, rule_id = EXCLUDED.rule_id,
  mode = EXCLUDED.mode, mint_address = EXCLUDED.mint_address, wallet = EXCLUDED.wallet,
  token_program_id = EXCLUDED.token_program_id, target_price = EXCLUDED.target_price,
  target_token_amount = EXCLUDED.target_token_amount, target_time = EXCLUDED.target_time,
  target_tx = EXCLUDED.target_tx, entry_price = EXCLUDED.entry_price,
  entry_token_amount = EXCLUDED.entry_token_amount, entry_lamports = EXCLUDED.entry_lamports,
  entry_time = EXCLUDED.entry_time, entry_tx_signatures = EXCLUDED.entry_tx_signatures,
  exit_price = EXCLUDED.exit_price, exit_token_amount = EXCLUDED.exit_token_amount,
  exit_lamports = EXCLUDED.exit_lamports, exit_time = EXCLUDED.exit_time,
  exit_tx_signatures = EXCLUDED.exit_tx_signatures,
  submitted_buy_signatures = EXCLUDED.submitted_buy_signatures, status = EXCLUDED.status,
  exit_reason = EXCLUDED.exit_reason, extra = EXCLUDED.extra,
  created_at = EXCLUDED.created_at, updated_at = EXCLUDED.updated_at,
  token_account = EXCLUDED.token_account;
"@

  # ---- 8. Sync _sqlx_migrations so local backend doesn't re-apply applied migrations ---
  # The server's checksum records are authoritative (same files, same binary).
  # Without this, _sqlx_migrations is empty locally and sqlx re-runs all migrations
  # on every startup, failing on non-idempotent steps.
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
}
finally {
  if ($tunnel -and -not $tunnel.HasExited) { Stop-Process -Id $tunnel.Id -Force -ErrorAction SilentlyContinue }
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
