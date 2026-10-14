<#
.SYNOPSIS
  Copy Hunter wallet files and wallet rows between this workstation and the server.

.DESCRIPTION
  Run this on the workstation. -SshTarget is the server. It does not hook into
  db-incremental-sync.ps1, and it does not move SOL.

  Files: every name.enc under WALLET_KEYSTORE, both directions, add-only.
  The same filename with different bytes stops the run. Nothing is deleted.

  Rows: trader_wallets, trader_nonces, wallet_moves, rule_wallets.
  A new pubkey is inserted on the other side only after its file is there.
  When both sides already have a pubkey, the server's status, primary flag,
  and nonce authority win, and server moves replace the local move rows.
  A rule assignment the server already has wins. A local-only assignment is
  pushed when the server has that rule and that wallet.
  Trades, positions, rules, fingerprints, and .env are not touched.

.EXAMPLE
  ./scripts/wallet-sync.ps1 -SshTarget ubuntu@1.2.3.4
#>
param(
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
if (-not (Test-Path $localStore)) { throw "Local keystore directory does not exist: $localStore" }

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
$remoteUser = if ($remoteEnv['POSTGRES_USER']) { $remoteEnv['POSTGRES_USER'] } else { 'postgres' }
$remoteDb = if ($remoteEnv['POSTGRES_DB']) { $remoteEnv['POSTGRES_DB'] } else { $Database }
if ($RemotePgPort -le 0) {
  $portRaw = if ($remoteEnv['DB_PORT']) { $remoteEnv['DB_PORT'] } elseif ($remoteEnv['POSTGRES_HOST_PORT']) { $remoteEnv['POSTGRES_HOST_PORT'] } else { '5555' }
  $RemotePgPort = [int]$portRaw
}

Write-Host "Comparing envelope files ..."
$localNames = @(Get-ChildItem -Path $localStore -Filter '*.enc' -File | ForEach-Object { $_.Name })
$remoteListing = Invoke-Ssh "find '$remoteStore' -maxdepth 1 -type f -name '*.enc' -printf '%f\n'"
$remoteNames = @($remoteListing | Where-Object { $_ -and $_.Trim() } | ForEach-Object { $_.Trim() })
$allNames = @($localNames + $remoteNames | Select-Object -Unique)
foreach ($name in $allNames) {
  if ($name -match '[\\/]' -or $name -eq '.' -or $name -eq '..') { throw "Refusing keystore name '$name'" }
  $onLocal = $localNames -contains $name
  $onRemote = $remoteNames -contains $name
  if ($onLocal -and $onRemote) {
    $localHash = (Get-FileHash -Algorithm SHA256 (Join-Path $localStore $name)).Hash.ToLower()
    $remoteHashLine = Invoke-Ssh "sha256sum '$remoteStore/$name'"
    $remoteHash = (($remoteHashLine | Select-Object -First 1) -split '\s+')[0].ToLower()
    if ($localHash -ne $remoteHash) {
      throw "Envelope '$name' differs between this machine and the server. Nothing was copied."
    }
  }
}
foreach ($name in $allNames) {
  $onLocal = $localNames -contains $name
  $onRemote = $remoteNames -contains $name
  if ($onLocal -and -not $onRemote) {
    Write-Host "  push $name"
    & scp @sshOpts (Join-Path $localStore $name) "${SshTarget}:${remoteStore}/"
    if ($LASTEXITCODE -ne 0) { throw "scp push failed for $name" }
  } elseif ($onRemote -and -not $onLocal) {
    Write-Host "  pull $name"
    & scp @sshOpts "${SshTarget}:${remoteStore}/$name" (Join-Path $localStore $name)
    if ($LASTEXITCODE -ne 0) { throw "scp pull failed for $name" }
  }
}
$landed = @(Get-ChildItem -Path $localStore -Filter '*.enc' -File | ForEach-Object { $_.Name })

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

  function Invoke-Psql([string]$password, [int]$port, [string]$user, [string]$db, [string]$sql) {
    $prev = $env:PGPASSWORD
    $env:PGPASSWORD = $password
    $f = [System.IO.Path]::GetTempFileName() -replace '\.tmp$', '.sql'
    $errf = "$f.err"
    [System.IO.File]::WriteAllText($f, $sql, $Utf8NoBom)
    try {
      $out = & psql -h 127.0.0.1 -p $port -U $user -d $db -v ON_ERROR_STOP=1 -f $f 2>$errf
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

  function Get-CsvRows([string]$password, [int]$port, [string]$user, [string]$db, [string]$sql) {
    $prev = $env:PGPASSWORD
    $env:PGPASSWORD = $password
    $errf = [System.IO.Path]::GetTempFileName()
    try {
      $raw = & psql -h 127.0.0.1 -p $port -U $user -d $db -v ON_ERROR_STOP=1 --csv -c $sql 2>$errf
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

  $walletSql = "SELECT pubkey, name, status, key_ref, (is_primary::int)::text AS is_primary, created_at::text AS created_at, COALESCE(archived_at::text, '') AS archived_at FROM trader_wallets ORDER BY pubkey"
  $nonceSql = "SELECT pubkey, authority, status FROM trader_nonces ORDER BY pubkey"
  $moveSql = "SELECT id::text AS id, from_wallet, to_wallet, step, COALESCE(error, '') AS error, started_at::text AS started_at, COALESCE(finished_at::text, '') AS finished_at FROM wallet_moves ORDER BY started_at"
  $ruleSql = "SELECT rule_id::text AS rule_id, wallet FROM rule_wallets ORDER BY rule_id"

  Write-Host "Reading wallet rows ..."
  $localWallets = Get-CsvRows $LocalPgPassword $LocalPgPort $LocalPgUser $Database $walletSql
  $serverWallets = Get-CsvRows $remoteEnv['POSTGRES_PASSWORD'] $TunnelLocalPort $remoteUser $remoteDb $walletSql
  $localNonces = Get-CsvRows $LocalPgPassword $LocalPgPort $LocalPgUser $Database $nonceSql
  $serverNonces = Get-CsvRows $remoteEnv['POSTGRES_PASSWORD'] $TunnelLocalPort $remoteUser $remoteDb $nonceSql
  $serverMoves = Get-CsvRows $remoteEnv['POSTGRES_PASSWORD'] $TunnelLocalPort $remoteUser $remoteDb $moveSql
  $localAssign = Get-CsvRows $LocalPgPassword $LocalPgPort $LocalPgUser $Database $ruleSql
  $serverAssign = Get-CsvRows $remoteEnv['POSTGRES_PASSWORD'] $TunnelLocalPort $remoteUser $remoteDb $ruleSql
  $serverRuleIds = @(Get-CsvRows $remoteEnv['POSTGRES_PASSWORD'] $TunnelLocalPort $remoteUser $remoteDb "SELECT id::text AS id FROM strategy_rules" | ForEach-Object { $_.id })

  $serverByKey = @{}; foreach ($r in $serverWallets) { $serverByKey[$r.pubkey] = $r }
  $localByKey = @{}; foreach ($r in $localWallets) { $localByKey[$r.pubkey] = $r }
  $serverNonceKeys = @{}; foreach ($r in $serverNonces) { $serverNonceKeys[$r.pubkey] = $r }
  $localNonceKeys = @{}; foreach ($r in $localNonces) { $localNonceKeys[$r.pubkey] = $r }
  $serverAssignByRule = @{}; foreach ($r in $serverAssign) { $serverAssignByRule[$r.rule_id] = $r }

  $pushWallet = @()
  $pullWallet = @()
  foreach ($r in $localWallets) {
    if ($serverByKey.ContainsKey($r.pubkey)) {
      if ($serverByKey[$r.pubkey].key_ref -ne $r.key_ref) {
        throw "Wallet $($r.pubkey) has key_ref '$($r.key_ref)' here and '$($serverByKey[$r.pubkey].key_ref)' on the server."
      }
      continue
    }
    if ($landed -notcontains $r.key_ref) { Write-Warning "Skip push of $($r.name): $($r.key_ref) is not on this machine."; continue }
    $pushWallet += $r
  }
  foreach ($r in $serverWallets) {
    if ($localByKey.ContainsKey($r.pubkey)) { continue }
    if ($landed -notcontains $r.key_ref) { Write-Warning "Skip pull of $($r.name): $($r.key_ref) is not on this machine."; continue }
    $pullWallet += $r
  }

  $serverSql = New-Object System.Text.StringBuilder
  [void]$serverSql.AppendLine('BEGIN;')
  foreach ($r in $pushWallet) {
    $status = if ($r.status -eq 'archived') { 'archived' } else { 'active' }
    $archived = if ($status -eq 'archived' -and $r.archived_at) { (Sql-Quote $r.archived_at) + '::timestamptz' } else { 'NULL' }
    [void]$serverSql.AppendLine(@"
INSERT INTO trader_wallets (pubkey, name, status, key_ref, is_primary, created_at, archived_at)
VALUES ($(Sql-Quote $r.pubkey), $(Sql-Quote $r.name), $(Sql-Quote $status), $(Sql-Quote $r.key_ref), false, $(Sql-Quote $r.created_at)::timestamptz, $archived)
ON CONFLICT (pubkey) DO NOTHING;
"@)
  }
  $serverPubkeys = @($serverWallets | ForEach-Object { $_.pubkey }) + @($pushWallet | ForEach-Object { $_.pubkey })
  foreach ($n in $localNonces) {
    if ($serverNonceKeys.ContainsKey($n.pubkey)) { continue }
    if ($serverPubkeys -notcontains $n.authority) { continue }
    [void]$serverSql.AppendLine("INSERT INTO trader_nonces (pubkey, authority, status) VALUES ($(Sql-Quote $n.pubkey), $(Sql-Quote $n.authority), $(Sql-Quote $n.status)) ON CONFLICT (pubkey) DO NOTHING;")
  }
  foreach ($a in $localAssign) {
    if ($serverAssignByRule.ContainsKey($a.rule_id)) { continue }
    if ($serverRuleIds -notcontains $a.rule_id) { Write-Warning "Skip assignment $($a.rule_id): the server has no such rule."; continue }
    if ($serverPubkeys -notcontains $a.wallet) { continue }
    [void]$serverSql.AppendLine("INSERT INTO rule_wallets (rule_id, wallet) VALUES ($(Sql-Quote $a.rule_id)::uuid, $(Sql-Quote $a.wallet)) ON CONFLICT (rule_id) DO NOTHING;")
  }
  [void]$serverSql.AppendLine('COMMIT;')

  $localSql = New-Object System.Text.StringBuilder
  [void]$localSql.AppendLine('BEGIN;')
  [void]$localSql.AppendLine('UPDATE trader_wallets SET is_primary = false WHERE is_primary;')
  $localTargets = @{}
  foreach ($r in $serverWallets) { if ($landed -contains $r.key_ref -or $localByKey.ContainsKey($r.pubkey)) { $localTargets[$r.pubkey] = $r } }
  foreach ($r in $pullWallet) { $localTargets[$r.pubkey] = $r }
  foreach ($r in $localTargets.Values) {
    $primary = if ($r.is_primary -eq '1' -or $r.is_primary -eq 't' -or $r.is_primary -eq 'true') { 'true' } else { 'false' }
    $archived = if ($r.archived_at) { (Sql-Quote $r.archived_at) + '::timestamptz' } else { 'NULL' }
    [void]$localSql.AppendLine(@"
INSERT INTO trader_wallets (pubkey, name, status, key_ref, is_primary, created_at, archived_at)
VALUES ($(Sql-Quote $r.pubkey), $(Sql-Quote $r.name), $(Sql-Quote $r.status), $(Sql-Quote $r.key_ref), $primary, $(Sql-Quote $r.created_at)::timestamptz, $archived)
ON CONFLICT (pubkey) DO UPDATE SET
  name = EXCLUDED.name,
  status = EXCLUDED.status,
  key_ref = EXCLUDED.key_ref,
  is_primary = EXCLUDED.is_primary,
  archived_at = EXCLUDED.archived_at;
"@)
  }
  foreach ($n in $serverNonces) {
    if (-not $localTargets.ContainsKey($n.authority) -and -not $localByKey.ContainsKey($n.authority)) { continue }
    [void]$localSql.AppendLine(@"
INSERT INTO trader_nonces (pubkey, authority, status)
VALUES ($(Sql-Quote $n.pubkey), $(Sql-Quote $n.authority), $(Sql-Quote $n.status))
ON CONFLICT (pubkey) DO UPDATE SET authority = EXCLUDED.authority, status = EXCLUDED.status;
"@)
  }
  [void]$localSql.AppendLine('DELETE FROM wallet_moves;')
  foreach ($m in $serverMoves) {
    $known = { param($pk) $localTargets.ContainsKey($pk) -or $localByKey.ContainsKey($pk) }
    if (-not (& $known $m.from_wallet) -or -not (& $known $m.to_wallet)) { continue }
    $err = if ($m.error) { Sql-Quote $m.error } else { 'NULL' }
    $fin = if ($m.finished_at) { (Sql-Quote $m.finished_at) + '::timestamptz' } else { 'NULL' }
    [void]$localSql.AppendLine(@"
INSERT INTO wallet_moves (id, from_wallet, to_wallet, step, error, started_at, finished_at)
VALUES ($(Sql-Quote $m.id)::uuid, $(Sql-Quote $m.from_wallet), $(Sql-Quote $m.to_wallet), $(Sql-Quote $m.step), $err, $(Sql-Quote $m.started_at)::timestamptz, $fin);
"@)
  }
  foreach ($a in $serverAssign) {
    if (-not $localTargets.ContainsKey($a.wallet) -and -not $localByKey.ContainsKey($a.wallet)) { continue }
    [void]$localSql.AppendLine(@"
INSERT INTO rule_wallets (rule_id, wallet)
VALUES ($(Sql-Quote $a.rule_id)::uuid, $(Sql-Quote $a.wallet))
ON CONFLICT (rule_id) DO UPDATE SET wallet = EXCLUDED.wallet;
"@)
  }
  [void]$localSql.AppendLine('COMMIT;')

  Write-Host "Writing server rows (new wallets only; existing status stays) ..."
  Invoke-Psql $remoteEnv['POSTGRES_PASSWORD'] $TunnelLocalPort $remoteUser $remoteDb $serverSql.ToString() | Out-Null
  Write-Host "Writing local rows (server status, primary, nonces, and moves win) ..."
  Invoke-Psql $LocalPgPassword $LocalPgPort $LocalPgUser $Database $localSql.ToString() | Out-Null
  Write-Host "Wallet sync finished."
} finally {
  if ($script:tunnel -and -not $script:tunnel.HasExited) { Stop-Process -Id $script:tunnel.Id -Force -ErrorAction SilentlyContinue }
  if ($script:askpassFile -and (Test-Path $script:askpassFile)) {
    try { icacls $script:askpassFile /grant:r "$($env:USERNAME):(F)" | Out-Null } catch {}
    Remove-Item $script:askpassFile -Force -ErrorAction SilentlyContinue
  }
}
