# Sequential daily simulate for hvt-a / hvt-b through hunter-engine::reduce.
# curve_only, lag_115, pumpfun_impact. Writes each day's summary before the next
# run overwrites it.

$ErrorActionPreference = "Stop"
$lab = "http://127.0.0.1:8140"
$out = Join-Path $PSScriptRoot "..\lake-data\harvest-chunk-sims-v2.jsonl"

Get-Content (Join-Path $PSScriptRoot "..\.env") | ForEach-Object {
    if ($_ -match '^API_AUTH_TOKEN=(.*)$') {
        $env:API_AUTH_TOKEN = $matches[1].Trim('"').Trim("'")
    }
}
if (-not $env:API_AUTH_TOKEN) { throw "API_AUTH_TOKEN missing" }
$headers = @{
    Authorization = "Bearer $env:API_AUTH_TOKEN"
    "Content-Type" = "application/json"
}

function Wait-Sim([string]$ruleId) {
    $deadline = (Get-Date).AddMinutes(45)
    while ((Get-Date) -lt $deadline) {
        Start-Sleep -Seconds 8
        try {
            $sum = Invoke-RestMethod -Uri "$lab/api/strategies/simulate/$ruleId/result/summary" -Method POST -Headers $headers -Body "{}"
        } catch {
            continue
        }
        if ($sum.error) { return $sum }
        if ($null -ne $sum.realized) { return $sum }
    }
    throw "timeout waiting for $ruleId"
}

function Invoke-Day([string]$name, [string]$ruleId, [string]$since, [string]$until) {
    Write-Host "$(Get-Date -Format HH:mm:ss) START $name $since"
    $body = @{
        rule_id = $ruleId
        since = $since
        until = $until
        fill_model = "lag_115"
        cost_model = "pumpfun_impact"
        skip_duplicate_identity = $false
        curve_only = $true
    } | ConvertTo-Json
    $start = Invoke-RestMethod -Uri "$lab/api/strategies/simulate" -Method POST -Headers $headers -Body $body
    if (-not $start.started) { throw "simulate did not start for $name" }
    $sum = Wait-Sim $ruleId
    if ($sum.error) { throw "sim error $name : $($sum.error)" }
    $r = $sum.realized
    $row = [ordered]@{
        rule = $name
        since = $since
        until = $until
        n_fired = $r.n_fired
        n_tokens_entered = $sum.n_tokens_entered
        n_matched = $sum.n_matched
        mean_pnl_pct = $r.mean_pnl_pct
        median_pnl_pct = $r.median_pnl_pct
        win_rate = $r.win_rate
        total_pnl_sol = $r.total_pnl_sol
        median_holding_secs = $r.median_holding_secs
        n_exit_metrics = $r.n_exit_metrics
        n_exit_dead = $r.n_exit_dead
        fill_model = "lag_115"
        cost_model = "pumpfun_impact"
        curve_only = $true
    }
    ($row | ConvertTo-Json -Compress) | Add-Content -Path $out
    Write-Host ("  n={0} tok={1} mean={2:N2}% SOL={3:N2}" -f $r.n_fired, $sum.n_tokens_entered, $r.mean_pnl_pct, $r.total_pnl_sol)
}

$ruleA = "93197f7d-2253-4fa0-8da2-51ce6a0714cc"
$ruleB = "49bc1c33-6970-49dc-9ba0-7d82bca561c4"
Write-Host "A=$ruleA  B=$ruleB"

# Python island window: 2026-08-11 .. 2026-08-23 exclusive.
$days = 11..22
foreach ($d in $days) {
    $since = "2026-08-{0:D2}T00:00:00Z" -f $d
    $until = "2026-08-{0:D2}T00:00:00Z" -f ($d + 1)
    if ($d -eq 22) { $until = "2026-08-23T00:00:00Z" }
    Invoke-Day "hvt-a-same-template" $ruleA $since $until
    Invoke-Day "hvt-b-mixed" $ruleB $since $until
}

Write-Host "done -> $out"
