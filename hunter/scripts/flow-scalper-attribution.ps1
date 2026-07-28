# Is a rule's residual loss SELECTION or MECHANICS?
#
# Pulls one simulate run's per-episode rows from the lab bin and splits its PnL by
# whether a reference wallet ever traded that mint. If the loss concentrates on mints
# it never touched, selection is binding and a sharper entry gate earns its place; if
# it is spread evenly, selection is not what is broken.
#
# CAUTION, learned the hard way (docs/plans/strategies/flow-scalper-findings.md): the
# first run of this split looked decisive and was an ARTEFACT. The rule under test had
# `m_price_lifetime.stall` in its exit, which silently capped every hold at ~15s AND
# filtered entries; once that was removed the cohort ordering INVERTED. Only trust
# this split on a rule whose exits you have already verified are not truncating.
#
# Note: simulate emits one row PER EPISODE (`outcome_to_row` maps each
# `PositionOutcome`), so a re-entry run yields several rows per mint. Rows with
# fired=false are the padded NoEntry rows and are excluded.
#
# Usage:
#   ./hunter/scripts/flow-scalper-attribution.ps1 -RunId <uuid>

[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)][string]$RunId,
  [string]$LabUrl = 'http://127.0.0.1:8140',
  [string]$Wallet = 'omegoMAe1AMY5MFKQQr3JwXVy8F4eCvmBAfcpo8XAfq'
)

$ErrorActionPreference = 'Stop'

$envFile = Join-Path $PSScriptRoot '..\.env'
$token = (Get-Content $envFile | Where-Object { $_ -match '^API_AUTH_TOKEN=' } |
          Select-Object -First 1) -replace '^API_AUTH_TOKEN=', ''
$dbUrl = (Get-Content $envFile | Where-Object { $_ -match '^DATABASE_URL=' } |
          Select-Object -First 1) -replace '^DATABASE_URL=', ''
if (-not $token) { throw "API_AUTH_TOKEN not found in $envFile" }
if (-not $dbUrl) { throw "DATABASE_URL not found in $envFile" }

# The result endpoint is the shared server-side pager (`TableRequest`), NOT a
# `limit` field: an unrecognised body silently yields the default 200-row page, which
# for a run whose matched set is ~30k mostly-NoEntry rows returns almost no episodes.
# Filter to fired rows server-side and page — `Page::bounds` clamps pageSize to 1000.
$fired = @()
$page = 1
while ($true) {
  $body = @{
    pagination = @{ page = $page; pageSize = 1000 }
    filters    = @{ exit_reason = @{ op = 'neq'; val = 'NoEntry' } }
  } | ConvertTo-Json -Depth 6
  $resp = Invoke-RestMethod -Method Post -Uri "$LabUrl/api/strategies/simulate/$RunId/result" `
    -Headers @{ Authorization = "Bearer $token" } -ContentType 'application/json' -Body $body
  $batch = @($resp.tokens)
  if ($batch.Count -eq 0) { break }
  $fired += $batch | Where-Object { $_.fired -and $null -ne $_.pnl_sol }
  if ($batch.Count -lt 1000) { break }
  $page++
}
if ($fired.Count -eq 0) { throw "run $RunId has no fired episodes" }
Write-Host ("run {0}: {1} fired episodes over {2} distinct mints" -f `
              $RunId, $fired.Count, ($fired.mint_address | Sort-Object -Unique).Count)

# His mints in the same window. LEFT-JOIN convention does not apply here -- we WANT
# only the wallet's own rows -- but the address lives solely in wallet_dict.
$hisMints = & psql $dbUrl -tA -c @"
SELECT DISTINCT t.mint_address
FROM trades t
JOIN wallet_dict w ON t.wallet_id = w.id
WHERE w.address = '$Wallet'
"@
$his = [System.Collections.Generic.HashSet[string]]::new()
foreach ($m in $hisMints) { if ($m.Trim()) { [void]$his.Add($m.Trim()) } }
Write-Host ("omego touched {0} mints in the local window" -f $his.Count)

$split = $fired | ForEach-Object {
  [pscustomobject]@{
    cohort  = if ($his.Contains($_.mint_address)) { 'his mints' } else { 'ours only' }
    pnl_sol = [double]$_.pnl_sol
    pnl_pct = [double]$_.pnl_percent
    win     = ([double]$_.pnl_sol -gt 0)
  }
}

$split | Group-Object cohort | ForEach-Object {
  $g = $_.Group
  [pscustomobject]@{
    cohort         = $_.Name
    episodes       = $g.Count
    share_of_eps   = '{0:P1}' -f ($g.Count / $split.Count)
    total_pnl_sol  = [math]::Round(($g | Measure-Object pnl_sol -Sum).Sum, 3)
    mean_pnl_pct   = [math]::Round(($g | Measure-Object pnl_pct -Average).Average, 3)
    win_rate       = '{0:P1}' -f (($g | Where-Object win).Count / $g.Count)
  }
} | Sort-Object cohort | Format-Table -AutoSize

Write-Host @"

Reading it: if 'ours only' carries a large majority of the LOSS while 'his mints'
is near break-even or better, selection is binding -> build the crowd gate. If both
cohorts lose similarly, the entry gate is not what is broken and the next lever is
mechanics or the fill, not a new metric.
"@
