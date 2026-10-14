<#
.SYNOPSIS
  Copy Hunter wallet files and wallet rows from one box to the other, in the
  direction you name.

.DESCRIPTION
  Run this on the workstation. -SshTarget is the server. -Direction picks the
  source; the source wins. It does not move SOL and does not hook into
  db-incremental-sync.ps1.

  Stops before writing anything when either side has a wallet move in
  progress, when the same key filename holds different bytes on each side,
  when a wallet has a different key_ref on each side, or when a source name
  is taken by another wallet on the target.

  Files: every *.enc under WALLET_KEYSTORE, source to target, add-only.
  Nothing is deleted or replaced.

  Rows, source to target, in one transaction:
    trader_wallets  every source wallet whose key file is on the target is
                    inserted or overwritten (name, status, primary,
                    archived_at). The source primary becomes the only target
                    primary. A wallet only the target has stays as it is.
    trader_nonces   every source nonce whose authority is on the target.
    wallet_moves    finished source moves are copied by id.
    rule_wallets    for every rule both sides have, the target assignment
                    becomes the source assignment (set or cleared).
  Trades, positions, rules, fingerprints, .env, and WALLET_KEK are never
  touched or copied.

  After writing it checks the target: exactly one active primary, and every
  active wallet's key file present. Press Refresh on the target's Wallets
  page: it re-reads the registry, decrypts new key files, and reads balances
  and nonces from the chain.

.EXAMPLE
  ./scripts/wallet-sync.ps1 -Direction ServerToLocal -DryRun
  ./scripts/wallet-sync.ps1 -Direction LocalToServer
#>
param(
  [Parameter(Mandatory = $true)][ValidateSet('ServerToLocal', 'LocalToServer')][string]$Direction,
  [switch]$DryRun,
  [string]$SshTarget = 'ubuntu@35.158.128.131',
  [string]$SshKey = $(foreach ($p in "$PSScriptRoot/../aws-ec2-key.pem", "$HOME/.ssh/aws-ec2-key.pem") { if (Test-Path $p) { $p; break } }),
  [string]$RemoteDir = '~/trade-launch-bot/hunter',
  [string]$Database = 'hunter_bot',
  [string]$LocalPgHost = 'localhost',
  [int]$LocalPgPort = 5555,
  [string]$LocalPgUser = 'postgres',
  [int]$TunnelLocalPort = 5433,
  [int]$RemotePgPort = 0,
  [string]$LocalPgPassword = $env:PGPASSWORD
)

$ErrorActionPreference = 'Continue'
if ($SshTarget -match '^-') { throw "SshTarget looks like a flag ('$SshTarget')." }
if (-not (Test-Path $SshKey)) { throw "SSH key not found: $SshKey" }

$HunterDir = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$Utf8NoBom = New-Object System.Text.UTF8Encoding $false

function Read-EnvFile([string]$path) {
  $map = @{}
  if (-not (Test-Path $path)) { return $map }
  foreach ($line in (Get-Content -Path $path)) {
    if ($line -match '^\s*([A-Z0-9_]+)=(.*)$') {
      $val = $Matches[2]
      if ($val -match '^([^#]*?)\s+#') { $val = $Matches[1] }
      $map[$Matches[1]] = $val.Trim().Trim('"').Trim("'")
    }
  }
  $map
}

function Sql-Quote([string]$value) {
  if ($null -eq $value) { return 'NULL' }
  "'" + ($value -replace "'", "''") + "'"
}

function Read-EnvFileFromText([string]$text) {
  $map = @{}
  foreach ($line in ($text -split "`n")) {
    if ($line -match '^\s*([A-Z0-9_]+)=(.*)$') {
      $val = $Matches[2]
      if ($val -match '^([^#]*?)\s+#') { $val = $Matches[1] }
      $map[$Matches[1]] = $val.Trim().Trim('"').Trim("'")
    }
  }
  $map
}

$localEnv = Read-EnvFile (Join-Path $HunterDir '.env')
if (-not $LocalPgPassword) { $LocalPgPassword = $localEnv['POSTGRES_PASSWORD'] }
if (-not $LocalPgPassword) { throw "Set `$env:PGPASSWORD or POSTGRES_PASSWORD in hunter/.env" }
if ($localEnv['DB_PORT']) { $LocalPgPort = [int]$localEnv['DB_PORT'] }
$localStore = $localEnv['WALLET_KEYSTORE']
if (-not $localStore) { throw "WALLET_KEYSTORE is unset in hunter/.env" }
if (-not [System.IO.Path]::IsPathRooted($localStore)) {
  $localStore = Join-Path $HunterDir $localStore
}
New-Item -ItemType Directory -Force -Path $localStore | Out-Null

if (Test-Path $SshKey) {
  icacls $SshKey /reset | Out-Null
  icacls $SshKey /inheritance:r | Out-Null
  icacls $SshKey /grant:r "$($env:USERNAME):(R)" | Out-Null
}

$sshOpts = @(
  '-i', $SshKey,
  '-o', 'StrictHostKeyChecking=accept-new',
  '-o', 'ConnectTimeout=10',
  '-o', 'ServerAliveInterval=20',
  '-o', 'ServerAliveCountMax=6',
  '-o', 'TCPKeepAlive=yes'
)
$script:askpassFile = $null
function Initialize-SshPassphrase {
  $needs = $true
  try { & ssh-keygen -y -P '' -f $SshKey 2>$null | Out-Null; if ($LASTEXITCODE -eq 0) { $needs = $false } } catch {}
  if (-not $needs) { return }
  if ($env:SSH_KEY_PASSPHRASE) { $pp = $env:SSH_KEY_PASSPHRASE }
  else {
    $sec = Read-Host -AsSecureString "Enter SSH key passphrase for $SshKey (entered ONCE)"
    $bstr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($sec)
    try { $pp = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($bstr) }
    finally { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($bstr) }
  }
  $script:askpassFile = Join-Path $env:TEMP ("walletsync-askpass-{0}.cmd" -f $PID)
  if (Test-Path $script:askpassFile) {
    try { icacls $script:askpassFile /grant:r "$($env:USERNAME):(F)" | Out-Null } catch {}
    Remove-Item $script:askpassFile -Force -ErrorAction SilentlyContinue
  }
  Set-Content -Path $script:askpassFile -Value ("@echo " + $pp) -Encoding ascii
  try { icacls $script:askpassFile /inheritance:r /grant:r "$($env:USERNAME):(RX)" | Out-Null } catch {}
  $env:SSH_ASKPASS = $script:askpassFile
  $env:SSH_ASKPASS_REQUIRE = 'force'
  if (-not $env:DISPLAY) { $env:DISPLAY = 'localhost:0' }
}
Initialize-SshPassphrase

function Invoke-Ssh([string]$remoteCommand) {
  $errf = [System.IO.Path]::GetTempFileName()
  try {
    $out = & ssh @sshOpts $SshTarget $remoteCommand 2>$errf
    if ($LASTEXITCODE -ne 0) {
      $err = if (Test-Path $errf) { Get-Content $errf -Raw } else { '' }
      throw "ssh failed (exit $LASTEXITCODE): $err"
    }
    return $out
  } finally { Remove-Item $errf -ErrorAction SilentlyContinue }
}

Write-Host "Reading server wallet config from $SshTarget ..."
$remoteText = (Invoke-Ssh "cd $RemoteDir 2>/dev/null && grep -E '^(DB_PORT|POSTGRES_HOST_PORT|POSTGRES_PASSWORD|POSTGRES_USER|POSTGRES_DB|WALLET_KEYSTORE)=' .env") -join "`n"
$remoteEnv = Read-EnvFileFromText $remoteText
if (-not $remoteEnv['WALLET_KEYSTORE']) { throw "WALLET_KEYSTORE is unset in the server .env" }
if (-not $remoteEnv['POSTGRES_PASSWORD']) { throw "POSTGRES_PASSWORD is missing from the server .env" }
$remoteStore = $remoteEnv['WALLET_KEYSTORE']
if (-not $remoteStore.StartsWith('/')) {
  $remoteHunter = (Invoke-Ssh "cd $RemoteDir && pwd" | Select-Object -First 1).Trim()
  $remoteStore = "$remoteHunter/" + ($remoteStore -replace '^\./', '')
}
Invoke-Ssh "mkdir -p '$remoteStore'" | Out-Null
$remoteUser = if ($remoteEnv['POSTGRES_USER']) { $remoteEnv['POSTGRES_USER'] } else { 'postgres' }
$remoteDb = if ($remoteEnv['POSTGRES_DB']) { $remoteEnv['POSTGRES_DB'] } else { $Database }
if ($RemotePgPort -le 0) {
  $portRaw = if ($remoteEnv['DB_PORT']) { $remoteEnv['DB_PORT'] } elseif ($remoteEnv['POSTGRES_HOST_PORT']) { $remoteEnv['POSTGRES_HOST_PORT'] } else { '5555' }
  $RemotePgPort = [int]$portRaw
}

$toServer = $Direction -eq 'LocalToServer'
$srcLabel = if ($toServer) { 'local' } else { 'server' }
$dstLabel = if ($toServer) { 'server' } else { 'local' }
Write-Host "Direction: $srcLabel -> $dstLabel$(if ($DryRun) { ' (dry run, nothing is written)' })"

Write-Host "Comparing envelope files ..."
$localNames = @(Get-ChildItem -Path $localStore -Filter '*.enc' -File | ForEach-Object { $_.Name })
$remoteListing = Invoke-Ssh "find '$remoteStore' -maxdepth 1 -type f -name '*.enc' -printf '%f\n'"
$remoteNames = @($remoteListing | Where-Object { $_ -and $_.Trim() } | ForEach-Object { $_.Trim() })
foreach ($name in @($localNames + $remoteNames | Select-Object -Unique)) {
  if ($name -match '[\\/]' -or $name -eq '.' -or $name -eq '..') { throw "Refusing keystore name '$name'" }
}
foreach ($name in $localNames) {
  if ($remoteNames -notcontains $name) { continue }
  $localHash = (Get-FileHash -Algorithm SHA256 (Join-Path $localStore $name)).Hash.ToLower()
  $remoteHashLine = Invoke-Ssh "sha256sum '$remoteStore/$name'"
  $remoteHash = (($remoteHashLine | Select-Object -First 1) -split '\s+')[0].ToLower()
  if ($localHash -ne $remoteHash) {
    throw "Envelope '$name' differs between this machine and the server. Nothing was copied."
  }
}
$srcFiles = if ($toServer) { $localNames } else { $remoteNames }
$dstFiles = if ($toServer) { $remoteNames } else { $localNames }
$copyFiles = @($srcFiles | Where-Object { $dstFiles -notcontains $_ })
$dstFilesAfter = @($dstFiles + $copyFiles | Select-Object -Unique)

Write-Host "Opening tunnel local:${TunnelLocalPort} -> ${SshTarget}:${RemotePgPort} ..."
$fwd = "127.0.0.1:${TunnelLocalPort}:127.0.0.1:${RemotePgPort}"
$tunnelArgs = $sshOpts + @('-o', 'ExitOnForwardFailure=yes', '-N', '-L', $fwd, $SshTarget)
$script:tunnel = Start-Process ssh -ArgumentList $tunnelArgs -PassThru -NoNewWindow
try {
  $ok = $false
  foreach ($i in 1..60) {
    if ($tunnel.HasExited) { throw "SSH tunnel exited" }
    $client = New-Object System.Net.Sockets.TcpClient
    try { $client.Connect('127.0.0.1', $TunnelLocalPort); $ok = $true; break } catch {} finally { $client.Dispose() }
    Start-Sleep -Milliseconds 500
  }
  if (-not $ok) { throw "Tunnel port $TunnelLocalPort never opened" }

  $localDb = @{ Password = $LocalPgPassword; Port = $LocalPgPort; User = $LocalPgUser; Db = $Database }
  $serverDb = @{ Password = $remoteEnv['POSTGRES_PASSWORD']; Port = $TunnelLocalPort; User = $remoteUser; Db = $remoteDb }
  $src = if ($toServer) { $localDb } else { $serverDb }
  $dst = if ($toServer) { $serverDb } else { $localDb }

  function Invoke-Psql($conn, [string]$sql) {
    $prev = $env:PGPASSWORD
    $env:PGPASSWORD = $conn.Password
    $f = [System.IO.Path]::GetTempFileName() -replace '\.tmp$', '.sql'
    $errf = "$f.err"
    [System.IO.File]::WriteAllText($f, $sql, $Utf8NoBom)
    try {
      $out = & psql -h 127.0.0.1 -p $conn.Port -U $conn.User -d $conn.Db -v ON_ERROR_STOP=1 -f $f 2>$errf
      if ($LASTEXITCODE -ne 0) {
        $err = if (Test-Path $errf) { Get-Content $errf -Raw } else { '' }
        throw "psql failed: $err"
      }
      return $out
    } finally {
      $env:PGPASSWORD = $prev
      Remove-Item $f, $errf -ErrorAction SilentlyContinue
    }
  }

  function Get-CsvRows($conn, [string]$sql) {
    $prev = $env:PGPASSWORD
    $env:PGPASSWORD = $conn.Password
    $errf = [System.IO.Path]::GetTempFileName()
    try {
      $raw = & psql -h 127.0.0.1 -p $conn.Port -U $conn.User -d $conn.Db -v ON_ERROR_STOP=1 --csv -c $sql 2>$errf
      if ($LASTEXITCODE -ne 0) {
        $err = if (Test-Path $errf) { Get-Content $errf -Raw } else { '' }
        throw "psql failed: $err"
      }
      $text = ($raw | Out-String).Trim()
      if (-not $text) { return @() }
      return @($text | ConvertFrom-Csv)
    } finally {
      $env:PGPASSWORD = $prev
      Remove-Item $errf -ErrorAction SilentlyContinue
    }
  }

  function Test-True([string]$v) { $v -eq '1' -or $v -eq 't' -or $v -eq 'true' }

  $walletSql = "SELECT pubkey, name, status, key_ref, (is_primary::int)::text AS is_primary, created_at::text AS created_at, COALESCE(archived_at::text, '') AS archived_at FROM trader_wallets ORDER BY pubkey"
  $nonceSql = "SELECT pubkey, authority, status FROM trader_nonces ORDER BY pubkey"
  $moveSql = "SELECT id::text AS id, from_wallet, to_wallet, step, COALESCE(error, '') AS error, started_at::text AS started_at, COALESCE(finished_at::text, '') AS finished_at FROM wallet_moves ORDER BY started_at"
  $ruleSql = "SELECT rule_id::text AS rule_id, wallet FROM rule_wallets ORDER BY rule_id"
  $ruleIdSql = "SELECT id::text AS id FROM strategy_rules"

  Write-Host "Reading wallet rows ..."
  $srcWallets = @(Get-CsvRows $src $walletSql)
  $dstWallets = @(Get-CsvRows $dst $walletSql)
  $srcNonces = @(Get-CsvRows $src $nonceSql)
  $srcMoves = @(Get-CsvRows $src $moveSql)
  $dstMoves = @(Get-CsvRows $dst $moveSql)
  $srcAssign = @(Get-CsvRows $src $ruleSql)
  $dstAssign = @(Get-CsvRows $dst $ruleSql)
  $srcRuleIds = @(Get-CsvRows $src $ruleIdSql | ForEach-Object { $_.id })
  $dstRuleIds = @(Get-CsvRows $dst $ruleIdSql | ForEach-Object { $_.id })

  # A move changes wallets, nonces, and rules step by step; a copy taken
  # halfway would land a state neither side ever had.
  foreach ($m in @($srcMoves + $dstMoves)) {
    if (-not $m.finished_at) { throw "A wallet move ($($m.id)) is in progress. Let it finish, then sync." }
  }

  $dstByKey = @{}; foreach ($r in $dstWallets) { $dstByKey[$r.pubkey] = $r }
  $srcKeys = @($srcWallets | ForEach-Object { $_.pubkey })
  $syncWallets = @()
  foreach ($r in $srcWallets) {
    if ($dstByKey.ContainsKey($r.pubkey) -and $dstByKey[$r.pubkey].key_ref -ne $r.key_ref) {
      throw "Wallet $($r.pubkey) has key_ref '$($r.key_ref)' on $srcLabel and '$($dstByKey[$r.pubkey].key_ref)' on $dstLabel."
    }
    if ($dstFilesAfter -notcontains $r.key_ref) { Write-Warning "Skip $($r.name): $($r.key_ref) is not on $srcLabel either."; continue }
    $clash = $dstWallets | Where-Object { $_.pubkey -ne $r.pubkey -and $_.name.ToLower() -eq $r.name.ToLower() } | Select-Object -First 1
    if ($clash -and $srcKeys -notcontains $clash.pubkey) {
      throw "The name '$($r.name)' belongs to another wallet ($($clash.pubkey)) on $dstLabel. Rename one of them first."
    }
    $syncWallets += $r
  }
  $targetKeys = @($dstWallets | ForEach-Object { $_.pubkey }) + @($syncWallets | ForEach-Object { $_.pubkey })
  $srcPrimary = $syncWallets | Where-Object { (Test-True $_.is_primary) -and $_.status -ne 'archived' } | Select-Object -First 1

  $sql = New-Object System.Text.StringBuilder
  [void]$sql.AppendLine('BEGIN;')
  if ($srcPrimary) { [void]$sql.AppendLine('UPDATE trader_wallets SET is_primary = false WHERE is_primary;') }
  # Names move in two passes so two wallets swapping names never collide midway.
  foreach ($r in $syncWallets) {
    if ($dstByKey.ContainsKey($r.pubkey)) {
      [void]$sql.AppendLine("UPDATE trader_wallets SET name = $(Sql-Quote ('~sync~' + $r.pubkey)) WHERE pubkey = $(Sql-Quote $r.pubkey);")
    }
  }
  foreach ($r in $syncWallets) {
    $primary = if ($srcPrimary -and $r.pubkey -eq $srcPrimary.pubkey) { 'true' } else { 'false' }
    $archived = if ($r.archived_at) { (Sql-Quote $r.archived_at) + '::timestamptz' } else { 'NULL' }
    [void]$sql.AppendLine(@"
INSERT INTO trader_wallets (pubkey, name, status, key_ref, is_primary, created_at, archived_at)
VALUES ($(Sql-Quote $r.pubkey), $(Sql-Quote $r.name), $(Sql-Quote $r.status), $(Sql-Quote $r.key_ref), $primary, $(Sql-Quote $r.created_at)::timestamptz, $archived)
ON CONFLICT (pubkey) DO UPDATE SET
  name = EXCLUDED.name,
  status = EXCLUDED.status,
  is_primary = EXCLUDED.is_primary,
  archived_at = EXCLUDED.archived_at;
"@)
  }
  foreach ($n in $srcNonces) {
    if ($targetKeys -notcontains $n.authority) { continue }
    [void]$sql.AppendLine(@"
INSERT INTO trader_nonces (pubkey, authority, status)
VALUES ($(Sql-Quote $n.pubkey), $(Sql-Quote $n.authority), $(Sql-Quote $n.status))
ON CONFLICT (pubkey) DO UPDATE SET authority = EXCLUDED.authority, status = EXCLUDED.status;
"@)
  }
  foreach ($m in $srcMoves) {
    if ($targetKeys -notcontains $m.from_wallet -or $targetKeys -notcontains $m.to_wallet) { continue }
    $err = if ($m.error) { Sql-Quote $m.error } else { 'NULL' }
    [void]$sql.AppendLine(@"
INSERT INTO wallet_moves (id, from_wallet, to_wallet, step, error, started_at, finished_at)
VALUES ($(Sql-Quote $m.id)::uuid, $(Sql-Quote $m.from_wallet), $(Sql-Quote $m.to_wallet), $(Sql-Quote $m.step), $err, $(Sql-Quote $m.started_at)::timestamptz, $(Sql-Quote $m.finished_at)::timestamptz)
ON CONFLICT (id) DO NOTHING;
"@)
  }
  $srcAssignByRule = @{}; foreach ($a in $srcAssign) { $srcAssignByRule[$a.rule_id] = $a.wallet }
  $dstAssignByRule = @{}; foreach ($a in $dstAssign) { $dstAssignByRule[$a.rule_id] = $a.wallet }
  $assignChanges = 0
  foreach ($rule in $srcRuleIds) {
    if ($dstRuleIds -notcontains $rule) { continue }
    $want = $srcAssignByRule[$rule]
    $have = $dstAssignByRule[$rule]
    if ($want -eq $have) { continue }
    if ($want) {
      if ($targetKeys -notcontains $want) { Write-Warning "Skip rule $rule assignment: wallet $want is not on $dstLabel."; continue }
      [void]$sql.AppendLine("INSERT INTO rule_wallets (rule_id, wallet) VALUES ($(Sql-Quote $rule)::uuid, $(Sql-Quote $want)) ON CONFLICT (rule_id) DO UPDATE SET wallet = EXCLUDED.wallet;")
    } else {
      [void]$sql.AppendLine("DELETE FROM rule_wallets WHERE rule_id = $(Sql-Quote $rule)::uuid;")
    }
    $assignChanges++
  }
  [void]$sql.AppendLine('COMMIT;')

  Write-Host ""
  Write-Host "Plan ($srcLabel -> $dstLabel):"
  Write-Host "  key files to copy: $($copyFiles.Count) $($copyFiles -join ', ')"
  foreach ($r in $syncWallets) {
    $verb = if ($dstByKey.ContainsKey($r.pubkey)) { 'update' } else { 'add' }
    $tag = if ($srcPrimary -and $r.pubkey -eq $srcPrimary.pubkey) { ' primary' } else { '' }
    Write-Host "  wallet ${verb}: $($r.name) ($($r.pubkey)) $($r.status)$tag"
  }
  if (-not $srcPrimary) { Write-Warning "$srcLabel has no active primary; the $dstLabel primary is left as it is." }
  Write-Host "  rule assignments changed: $assignChanges"
  if ($DryRun) { Write-Host "Dry run: nothing was written."; return }

  foreach ($name in $copyFiles) {
    if ($toServer) {
      Write-Host "  push $name"
      & scp @sshOpts (Join-Path $localStore $name) "${SshTarget}:${remoteStore}/"
    } else {
      Write-Host "  pull $name"
      & scp @sshOpts "${SshTarget}:${remoteStore}/$name" (Join-Path $localStore $name)
    }
    if ($LASTEXITCODE -ne 0) { throw "scp failed for $name; no rows were written" }
  }

  Write-Host "Writing $dstLabel rows ..."
  Invoke-Psql $dst $sql.ToString() | Out-Null

  Write-Host "Checking $dstLabel ..."
  $after = @(Get-CsvRows $dst $walletSql)
  $primaries = @($after | Where-Object { (Test-True $_.is_primary) -and $_.status -ne 'archived' })
  if ($primaries.Count -ne 1) { Write-Warning "$dstLabel has $($primaries.Count) active primary wallets; set one on its Wallets page." }
  $filesNow = if ($toServer) {
    @(Invoke-Ssh "find '$remoteStore' -maxdepth 1 -type f -name '*.enc' -printf '%f\n'" | Where-Object { $_ -and $_.Trim() } | ForEach-Object { $_.Trim() })
  } else {
    @(Get-ChildItem -Path $localStore -Filter '*.enc' -File | ForEach-Object { $_.Name })
  }
  foreach ($r in $after) {
    if ($r.status -ne 'archived' -and $filesNow -notcontains $r.key_ref) {
      Write-Warning "$($r.name) ($($r.pubkey)) is active on $dstLabel but $($r.key_ref) is not there; it cannot sign."
    }
  }
  Write-Host "Wallet sync finished. Press Refresh on the $dstLabel Wallets page to load the changes."
} finally {
  if ($script:tunnel -and -not $script:tunnel.HasExited) { Stop-Process -Id $script:tunnel.Id -Force -ErrorAction SilentlyContinue }
  if ($script:askpassFile -and (Test-Path $script:askpassFile)) {
    try { icacls $script:askpassFile /grant:r "$($env:USERNAME):(F)" | Out-Null } catch {}
    Remove-Item $script:askpassFile -Force -ErrorAction SilentlyContinue
  }
}
