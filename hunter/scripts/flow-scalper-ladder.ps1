# Flow-scalper backtest ladder driver.
#
# Posts engine-simulate DRAFTS (no DB rule rows) to the running lab bin, polls each
# run, and appends one CSV row per run. This is the measurement harness for
# hunter/docs/roadmap/flow-scalper-build-plan.md phases A and B.
#
# Why a script and not the UI: the grouped sweep cannot score re-entry rules (see
# docs/roadmap/grouped-sweep-reentry.md item C), so every re-entry knob has to be
# calibrated one simulate at a time. That is a ladder, not a grid.
#
# Usage:
#   ./hunter/scripts/flow-scalper-ladder.ps1 -Plan phaseA
#   ./hunter/scripts/flow-scalper-ladder.ps1 -Plan retrace -Since 2026-07-22 -Until 2026-07-28
#
# Output: hunter/docs/roadmap/data/flow-scalper-ladder.csv (appended, one row per run).

[CmdletBinding()]
param(
  [string]$Plan       = 'phaseA',
  [string]$Since      = '2026-07-22T00:00:00Z',
  [string]$Until      = '2026-07-28T00:00:00Z',
  [string]$LabUrl     = 'http://127.0.0.1:8140',
  # Broad control fingerprint: init_buy bucket width 1000 SOL => matches every token
  # with any dev buy. A fingerprint with zero configured axes matches NOTHING, which
  # is why "broad" is spelled as one very wide bucket.
  [string]$FingerprintId = 'a23cf424-8d4c-45ae-aa51-37ba4f2fc6ca',
  [string]$OutCsv     = "$PSScriptRoot/../docs/roadmap/data/flow-scalper-ladder.csv",
  [int]$TimeoutSec    = 3600
)

$ErrorActionPreference = 'Stop'

# ---------------------------------------------------------------------------
# Auth: the lab bin's write endpoints are behind the shared fail-closed bearer
# gate (shared/http-auth). Read the token from hunter/.env, never hardcode it.
# ---------------------------------------------------------------------------
$envFile = Join-Path $PSScriptRoot '..\.env'
$token = (Get-Content $envFile | Where-Object { $_ -match '^API_AUTH_TOKEN=' } |
          Select-Object -First 1) -replace '^API_AUTH_TOKEN=', ''
if (-not $token) { throw "API_AUTH_TOKEN not found in $envFile" }
$headers = @{ Authorization = "Bearer $token" }

# ---------------------------------------------------------------------------
# The anchor rule: omego's mechanics with the knobs recalibrated on the fresh
# 5-day window (docs/roadmap/flow-reversion-scalper.md, 2026-07-27 section).
# Each ladder entry is a scriptblock that mutates a clone of this.
# ---------------------------------------------------------------------------
function New-AnchorParams {
  # Rebuilt per call so a ladder entry can never mutate a shared object.
  return @{
    stop_loss = 25
    entry = @{
      m_snapshot = @{
        time      = @(@{ operator = '>='; value = 150 })
        liquidity = @(@{ operator = '>='; value = 55 }, @{ operator = '<='; value = 100 })
      }
      m_price_window = @{
        window_size_sec = 30
        trail           = @(@{ operator = '>='; value = 12 })
      }
      # Multi-window: 60 s hot gate AND a 2 s sell-exhaustion floor.
      m_flow_window = @(
        @{ window_size_sec = 60; gross_flow = @(@{ operator = '>='; value = 11 }) },
        @{ window_size_sec = 2;  net_flow   = @(@{ operator = '>='; value = 0 })  }
      )
    }
    exit = @{
      m_position       = @{ retrace = @(@{ operator = '>='; value = 3 }) }
      m_price_lifetime = @{ stall   = @(@{ operator = '>='; value = 15 }) }
    }
    reentry = @{ cooldown_sec = 5; max_episodes_per_token = 40 }
  }
}

# Convenience mutators (kept tiny so a ladder row reads as the one thing it changes).
function Set-Retrace  ($p, $v) { $p.exit.m_position.retrace = @(@{ operator = '>='; value = $v }) }
function Set-Dip      ($p, $v) { $p.entry.m_price_window.trail = @(@{ operator = '>='; value = $v }) }
function Set-Stall    ($p, $v) {
  if ($null -eq $v) { $p.exit.Remove('m_price_lifetime') }
  else { $p.exit.m_price_lifetime.stall = @(@{ operator = '>='; value = $v }) }
}
function Set-StopLoss ($p, $v) {
  if ($null -eq $v) { $p.Remove('stop_loss') } else { $p.stop_loss = $v }
}
function Set-Episodes ($p, $v) {
  if ($null -eq $v) { $p.Remove('reentry') } else { $p.reentry.max_episodes_per_token = $v }
}
function Set-Cooldown ($p, $v) { $p.reentry.cooldown_sec = $v }
function Set-ArmAbove ($p, $v) {
  # `m_position.arm_above_pct` - hold the trailing stop off until the position is
  # this far in profit. Absent = today's behaviour, where the since-entry peak seeds
  # at the entry fill so `retrace` doubles as a hard stop from entry. Measured on
  # omego's own 2,974 episodes, that unarmed trail turns 21% of his winners into
  # losers (docs/roadmap/flow-scalper-build-plan.md phase C1).
  if ($null -eq $v) { $p.exit.m_position.Remove('arm_above_pct') }
  else { $p.exit.m_position.arm_above_pct = $v }
}
function Set-Held ($p, $v) {
  # `m_position.held >= N` - an HONEST time stop (seconds since the entry fill).
  # This is what people think `m_price_lifetime.stall` does; stall is really
  # "seconds since the last all-time HIGH", which on a dip-entry rule silently caps
  # every position at ~15s. See the build plan's stall finding.
  if ($null -eq $v) { if ($p.exit.ContainsKey('m_position')) { $p.exit.m_position.Remove('held') } }
  else { $p.exit.m_position.held = @(@{ operator = '>='; value = $v }) }
}
function Set-Rise ($p, $v) {
  # `m_price_window(30).rise <= N` - percent ABOVE the rolling 30s low, so a small
  # upper bound means "buy at the bottom of the dip, not anywhere inside it".
  # Measured on L3's 1,661 episodes: rise30 <= 1 is +1.28%/ep vs -0.73% overall.
  # NOTE this is NOT copying omego - his own rise30 is HIGHER than ours (median 9.6
  # vs 5.6). It is an edge in our own fired set, found by supervised bucketing, and
  # therefore at risk of being in-sample noise: validate out of window before use.
  if ($null -eq $v) { $p.entry.m_price_window.Remove('rise') }
  else { $p.entry.m_price_window.rise = @(@{ operator = '<='; value = $v }) }
}
function Remove-NetFlowGate ($p) {
  # Drop the 2 s net_flow clause, keeping the 60 s hot gate. He enters AT the window
  # low (rise60 p25 = 1.2), so requiring a started reversal may be wrong.
  $p.entry.m_flow_window = @(
    @{ window_size_sec = 60; gross_flow = @(@{ operator = '>='; value = 11 }) }
  )
}

# ---------------------------------------------------------------------------
# Run one simulate to completion and return its realized summary.
# ---------------------------------------------------------------------------
function Invoke-SimRun {
  param($Label, $Params, $Fill, $Cost, $Concurrency, $BuySol)

  $body = @{
    draft = @{
      fingerprint_id        = $FingerprintId
      params                = $Params
      buy_amount_sol        = $BuySol
      max_concurrent_tokens = $Concurrency
      max_total_tokens      = 0
      trade_mode            = 'paper'
    }
    since      = $Since
    until      = $Until
    fill_model = $Fill
    cost_model = $Cost
  } | ConvertTo-Json -Depth 20

  $start = Get-Date
  $post = Invoke-RestMethod -Method Post -Uri "$LabUrl/api/strategies/simulate" `
            -Headers $headers -ContentType 'application/json' -Body $body
  $runId = $post.run_id
  Write-Host ("[{0}] run {1} ..." -f $Label, $runId) -NoNewline

  $summary = $null
  while (((Get-Date) - $start).TotalSeconds -lt $TimeoutSec) {
    Start-Sleep -Seconds 10
    try {
      $summary = Invoke-RestMethod -Method Post `
        -Uri "$LabUrl/api/strategies/simulate/$runId/result/summary" `
        -Headers $headers -ContentType 'application/json' -Body '{}'
      break
    } catch {
      # 404 while the job is still folding - the one expected transient.
      Write-Host '.' -NoNewline
    }
  }
  if ($null -eq $summary) { throw "run $runId timed out after $TimeoutSec s" }

  $elapsed = [math]::Round(((Get-Date) - $start).TotalSeconds, 1)
  $r = $summary.realized
  Write-Host (" done in {0}s: n={1} win={2:P1} pnl={3:N2} SOL" -f `
                $elapsed, $r.n_closed, $r.win_rate, $r.total_pnl_sol)

  return [pscustomobject]@{
    label            = $Label
    run_id           = $runId
    fill             = $Fill
    cost             = $Cost
    concurrency      = $Concurrency
    buy_sol          = $BuySol
    n_fired          = $r.n_fired
    n_closed         = $r.n_closed
    n_open           = $r.n_open
    win_rate         = [math]::Round($r.win_rate, 4)
    total_pnl_sol    = [math]::Round($r.total_pnl_sol, 4)
    expectancy_sol   = [math]::Round($r.expectancy_sol, 5)
    median_pnl_pct   = [math]::Round($r.median_pnl_pct, 3)
    mean_pnl_pct     = [math]::Round($r.mean_pnl_pct, 3)
    p90_pnl_pct      = [math]::Round($r.p90_pnl_pct, 3)
    worst_pnl_pct    = [math]::Round($r.worst_pnl_pct, 3)
    profit_factor    = [math]::Round($r.profit_factor, 4)
    median_hold_s    = $r.median_holding_secs
    avg_hold_s       = [math]::Round($r.avg_holding_secs, 2)
    n_exit_metrics   = $r.n_exit_metrics
    n_exit_stop_loss = $r.n_exit_stop_loss
    n_exit_dead      = $r.n_exit_dead
    elapsed_s        = $elapsed
    since            = $Since
    until            = $Until
  }
}

# ---------------------------------------------------------------------------
# Ladders. Each is a list of @{ name; apply; fill; cost; conc; buy }.
# ---------------------------------------------------------------------------
$ladders = @{
  # Phase A - does it work at all, does re-entry matter, does it survive the fill.
  phaseA = @(
    @{ name = 'A1 anchor signal';   apply = {}; fill = 'signal' }
    @{ name = 'A2 anchor first';    apply = {}; fill = 'first'  }
    @{ name = 'A3 anchor worst';    apply = {}; fill = 'worst'  }
    @{ name = 'A4 one-shot';        apply = { param($p) Set-Episodes $p $null }; fill = 'first' }
    @{ name = 'A5 no 2s netflow';   apply = { param($p) Remove-NetFlowGate $p }; fill = 'first' }
  )
  # The decisive comparison: today's authorable unarmed trail vs the armed trail
  # `arm_above_pct` buys. Every row shares one entry gate so ONLY the exit differs.
  armed = @(
    @{ name = 'X0 unarmed r3 sl25 (today)'
       apply = { param($p) Set-Retrace $p 3; Set-StopLoss $p 25 } }
    @{ name = 'X1 unarmed r5 sl12'
       apply = { param($p) Set-Retrace $p 5; Set-StopLoss $p 12 } }
    @{ name = 'X2 armed g0 r3 sl12'
       apply = { param($p) Set-Retrace $p 3; Set-StopLoss $p 12; Set-ArmAbove $p 0 } }
    @{ name = 'X3 armed g2 r4 sl12'
       apply = { param($p) Set-Retrace $p 4; Set-StopLoss $p 12; Set-ArmAbove $p 2 } }
    @{ name = 'X4 armed g5 r5 sl12'
       apply = { param($p) Set-Retrace $p 5; Set-StopLoss $p 12; Set-ArmAbove $p 5 } }
    # The re-entry delta, measured on the armed exit rather than the broken one.
    @{ name = 'X5 armed g2 r4 one-shot'
       apply = { param($p) Set-Retrace $p 4; Set-StopLoss $p 12; Set-ArmAbove $p 2
                 Set-Episodes $p $null } }
    # Does it survive the adversarial in-slot fill live paper books?
    @{ name = 'X6 armed g2 r4 WORST fill'
       apply = { param($p) Set-Retrace $p 4; Set-StopLoss $p 12; Set-ArmAbove $p 2 }
       fill = 'worst' }
    # He enters AT the window low, so requiring a started reversal may be wrong.
    @{ name = 'X7 armed g2 r4 no 2s gate'
       apply = { param($p) Set-Retrace $p 4; Set-StopLoss $p 12; Set-ArmAbove $p 2
                 Remove-NetFlowGate $p } }
  )
  # THE STALL LADDER. Measured 2026-07-28: across 445 episodes NO position ever held
  # longer than 16s, under every exit reason. `m_price_lifetime.stall` counts seconds
  # since the last ALL-TIME HIGH, and it only resets on a new high - but this rule
  # enters on a >=12% dip, so a new high during the hold is rare. Entry needs
  # stall < 15 (can_enter refuses while an exit metric holds) and the median entry is
  # already 8.3s past the ATH, so ~7s of headroom force-closes every trade. The armed
  # trail returns +5.64% in the <=5s it is allowed to work; this ladder gives it room.
  stallfix = @(
    @{ name = 'R1 armed g2 r4 stall OFF'
       apply = { param($p) Set-Retrace $p 4; Set-StopLoss $p 12; Set-ArmAbove $p 2
                 Set-Stall $p $null } }
    @{ name = 'R2 armed g2 r4 stall 60'
       apply = { param($p) Set-Retrace $p 4; Set-StopLoss $p 12; Set-ArmAbove $p 2
                 Set-Stall $p 60 } }
    @{ name = 'R3 armed g2 r4 stall 120'
       apply = { param($p) Set-Retrace $p 4; Set-StopLoss $p 12; Set-ArmAbove $p 2
                 Set-Stall $p 120 } }
    # An explicit time stop instead of the accidental one - bounds the hold without
    # tying it to the ATH clock.
    @{ name = 'R4 armed g2 r4 held<=120'
       apply = { param($p) Set-Retrace $p 4; Set-StopLoss $p 12; Set-ArmAbove $p 2
                 Set-Stall $p $null; Set-Held $p 120 } }
    @{ name = 'R5 armed g0 r3 stall OFF'
       apply = { param($p) Set-Retrace $p 3; Set-StopLoss $p 12; Set-ArmAbove $p 0
                 Set-Stall $p $null } }
    # Re-entry re-tested against an exit that can actually hold.
    @{ name = 'R6 armed g2 r4 stall OFF one-shot'
       apply = { param($p) Set-Retrace $p 4; Set-StopLoss $p 12; Set-ArmAbove $p 2
                 Set-Stall $p $null; Set-Episodes $p $null } }
    # And the unarmed control, so the two fixes are separable.
    @{ name = 'R7 UNARMED r4 stall OFF'
       apply = { param($p) Set-Retrace $p 4; Set-StopLoss $p 12; Set-Stall $p $null } }
    @{ name = 'R8 armed g2 r4 stall OFF WORST fill'
       apply = { param($p) Set-Retrace $p 4; Set-StopLoss $p 12; Set-ArmAbove $p 2
                 Set-Stall $p $null }
       fill = 'worst' }
  )
  # PROFIT-LOCK GEOMETRY. Base = R4: no `stall` (it is a hidden entry filter AND a
  # hidden ~15s hold cap), explicit `m_position.held >= 120` time stop instead.
  #
  # The flaw this ladder fixes: `arm_above_pct` must exceed `retrace`, or the trail
  # gives back more than it locked in. Arm at +2 and trail 4 off the peak: if the peak
  # IS +2, the exit lands at 1.02*0.96 = -2.1%. Every armed run so far violated that,
  # which is why they show a healthy 55% win rate but small winners against losers
  # that run to the stop. `arm > retrace` guarantees a green exit once armed:
  # worst armed exit ~= (1 + arm/100) * (1 - retrace/100).
  lock = @(
    @{ name = 'L1 arm5 r3 sl8'; apply = { param($p) Set-Stall $p $null; Set-Held $p 120
                 Set-ArmAbove $p 5; Set-Retrace $p 3; Set-StopLoss $p 8 } }
    @{ name = 'L2 arm8 r4 sl8'; apply = { param($p) Set-Stall $p $null; Set-Held $p 120
                 Set-ArmAbove $p 8; Set-Retrace $p 4; Set-StopLoss $p 8 } }
    @{ name = 'L3 arm6 r2 sl8'; apply = { param($p) Set-Stall $p $null; Set-Held $p 120
                 Set-ArmAbove $p 6; Set-Retrace $p 2; Set-StopLoss $p 8 } }
    @{ name = 'L4 arm5 r3 sl5'; apply = { param($p) Set-Stall $p $null; Set-Held $p 120
                 Set-ArmAbove $p 5; Set-Retrace $p 3; Set-StopLoss $p 5 } }
    @{ name = 'L5 arm5 r3 sl12'; apply = { param($p) Set-Stall $p $null; Set-Held $p 120
                 Set-ArmAbove $p 5; Set-Retrace $p 3; Set-StopLoss $p 12 } }
    @{ name = 'L6 arm10 r4 sl8'; apply = { param($p) Set-Stall $p $null; Set-Held $p 120
                 Set-ArmAbove $p 10; Set-Retrace $p 4; Set-StopLoss $p 8 } }
    @{ name = 'L7 arm3 r1.5 sl6'; apply = { param($p) Set-Stall $p $null; Set-Held $p 120
                 Set-ArmAbove $p 3; Set-Retrace $p 1.5; Set-StopLoss $p 6 } }
    # Longer leash on the winners - his money is in the >1min holds.
    @{ name = 'L8 arm5 r3 sl8 held300'; apply = { param($p) Set-Stall $p $null; Set-Held $p 300
                 Set-ArmAbove $p 5; Set-Retrace $p 3; Set-StopLoss $p 8 } }
    # Concurrency is a first-class knob now that holds are no longer capped at ~5s:
    # with a 4-slot cap and long holds, re-entry CANNIBALISES fresh opportunities
    # (measured R6 one-shot -1.19%/ep over 184 eps vs R1 re-entry -1.90% over 163).
    @{ name = 'L9 arm5 r3 sl8 conc12'; conc = 12
       apply = { param($p) Set-Stall $p $null; Set-Held $p 120
                 Set-ArmAbove $p 5; Set-Retrace $p 3; Set-StopLoss $p 8 } }
    @{ name = 'L10 arm5 r3 sl8 conc12 one-shot'; conc = 12
       apply = { param($p) Set-Stall $p $null; Set-Held $p 120
                 Set-ArmAbove $p 5; Set-Retrace $p 3; Set-StopLoss $p 8
                 Set-Episodes $p $null } }
  )
  # THE `rise` GATE, on the L10 base (arm5/r2/sl8, held 120, no stall, conc 12,
  # ONE-SHOT). One-shot matters: L10 vs L9 differ only by re-entry and score
  # -0.084 vs -0.859 %/ep, so leaving re-entry on would bury a ~2pp entry signal
  # under ~1.15pp of re-entry noise. Re-entry amplifies selection quality - omego's
  # re-entries improve with index because HIS picks are good; ours compound a bad
  # pick. Revisit re-entry only after an entry edge exists.
  #
  # The gate: `m_price_window(30).rise <= N` = buy near the bottom of the dip.
  # Supervised bucketing of L3's 1,661 episodes put rise30 <= 1 at +1.28%/ep vs
  # -0.73% overall. That is a POST-HOC pick of the best of five buckets, so the
  # `risevalidate` plan re-runs the winner on the untouched 07-22..07-25 window.
  rise = @(
    @{ name = 'V0 base no rise gate'; conc = 12
       apply = { param($p) Set-Stall $p $null; Set-Held $p 120; Set-Episodes $p $null
                 Set-ArmAbove $p 5; Set-Retrace $p 2; Set-StopLoss $p 8 } }
    @{ name = 'V1 rise<=1'; conc = 12
       apply = { param($p) Set-Stall $p $null; Set-Held $p 120; Set-Episodes $p $null
                 Set-ArmAbove $p 5; Set-Retrace $p 2; Set-StopLoss $p 8; Set-Rise $p 1 } }
    @{ name = 'V2 rise<=2'; conc = 12
       apply = { param($p) Set-Stall $p $null; Set-Held $p 120; Set-Episodes $p $null
                 Set-ArmAbove $p 5; Set-Retrace $p 2; Set-StopLoss $p 8; Set-Rise $p 2 } }
    @{ name = 'V3 rise<=3'; conc = 12
       apply = { param($p) Set-Stall $p $null; Set-Held $p 120; Set-Episodes $p $null
                 Set-ArmAbove $p 5; Set-Retrace $p 2; Set-StopLoss $p 8; Set-Rise $p 3 } }
    # L7 said tighter trail is monotonically better; check it still holds with the gate.
    @{ name = 'V4 rise<=1 trail1.5'; conc = 12
       apply = { param($p) Set-Stall $p $null; Set-Held $p 120; Set-Episodes $p $null
                 Set-ArmAbove $p 3; Set-Retrace $p 1.5; Set-StopLoss $p 6; Set-Rise $p 1 } }
    # L8 said a longer leash helps at fixed geometry.
    @{ name = 'V5 rise<=1 held300'; conc = 12
       apply = { param($p) Set-Stall $p $null; Set-Held $p 300; Set-Episodes $p $null
                 Set-ArmAbove $p 5; Set-Retrace $p 2; Set-StopLoss $p 8; Set-Rise $p 1 } }
    # The bar that actually matters for real money (live paper books `worst`).
    @{ name = 'V6 rise<=1 WORST fill'; conc = 12; fill = 'worst'
       apply = { param($p) Set-Stall $p $null; Set-Held $p 120; Set-Episodes $p $null
                 Set-ArmAbove $p 5; Set-Retrace $p 2; Set-StopLoss $p 8; Set-Rise $p 1 } }
  )
  # OUT-OF-SAMPLE validation. Run with the EARLIER, untouched window:
  #   -Since 2026-07-22T00:00:00Z -Until 2026-07-25T00:00:00Z
  # Everything above was tuned on 07-25..07-28; if the gate is real it survives here.
  risevalidate = @(
    @{ name = 'W0 OOS base no rise'; conc = 12
       apply = { param($p) Set-Stall $p $null; Set-Held $p 120; Set-Episodes $p $null
                 Set-ArmAbove $p 5; Set-Retrace $p 2; Set-StopLoss $p 8 } }
    @{ name = 'W1 OOS rise<=1'; conc = 12
       apply = { param($p) Set-Stall $p $null; Set-Held $p 120; Set-Episodes $p $null
                 Set-ArmAbove $p 5; Set-Retrace $p 2; Set-StopLoss $p 8; Set-Rise $p 1 } }
    @{ name = 'W2 OOS rise<=1 WORST fill'; conc = 12; fill = 'worst'
       apply = { param($p) Set-Stall $p $null; Set-Held $p 120; Set-Episodes $p $null
                 Set-ArmAbove $p 5; Set-Retrace $p 2; Set-StopLoss $p 8; Set-Rise $p 1 } }
    # Does the one-shot > re-entry result also hold out of sample?
    @{ name = 'W3 OOS rise<=1 REENTRY on'; conc = 12
       apply = { param($p) Set-Stall $p $null; Set-Held $p 120
                 Set-ArmAbove $p 5; Set-Retrace $p 2; Set-StopLoss $p 8; Set-Rise $p 1 } }
  )
  # V0 VARIANTS - the first net-positive config, without the refuted `rise` gate.
  # V0 = arm5 / trail2 / sl8, held 120, no stall, ONE-SHOT, conc 12  => +0.133 %/ep
  # (+0.60 SOL / 451 eps, 53.9% win) on 07-25..07-28, `first` fill, fee-only.
  # `rise <= 1` was measured at -0.757 %/ep => REFUTED, do not re-add it.
  v0var = @(
    @{ name = 'P0 V0 control'; conc = 12
       apply = { param($p) Set-Stall $p $null; Set-Held $p 120; Set-Episodes $p $null
                 Set-ArmAbove $p 5; Set-Retrace $p 2; Set-StopLoss $p 8 } }
    # Trail was monotonically better as it tightened (4 -> 1.5); find where it turns.
    @{ name = 'P1 trail 1.5'; conc = 12
       apply = { param($p) Set-Stall $p $null; Set-Held $p 120; Set-Episodes $p $null
                 Set-ArmAbove $p 5; Set-Retrace $p 1.5; Set-StopLoss $p 8 } }
    @{ name = 'P2 trail 1.0'; conc = 12
       apply = { param($p) Set-Stall $p $null; Set-Held $p 120; Set-Episodes $p $null
                 Set-ArmAbove $p 5; Set-Retrace $p 1; Set-StopLoss $p 8 } }
    # arm must stay > trail or the trail gives back more than it locks in.
    @{ name = 'P3 arm 4 trail 2'; conc = 12
       apply = { param($p) Set-Stall $p $null; Set-Held $p 120; Set-Episodes $p $null
                 Set-ArmAbove $p 4; Set-Retrace $p 2; Set-StopLoss $p 8 } }
    @{ name = 'P4 arm 6 trail 2'; conc = 12
       apply = { param($p) Set-Stall $p $null; Set-Held $p 120; Set-Episodes $p $null
                 Set-ArmAbove $p 6; Set-Retrace $p 2; Set-StopLoss $p 8 } }
    @{ name = 'P5 held 300'; conc = 12
       apply = { param($p) Set-Stall $p $null; Set-Held $p 300; Set-Episodes $p $null
                 Set-ArmAbove $p 5; Set-Retrace $p 2; Set-StopLoss $p 8 } }
    @{ name = 'P6 stop 12'; conc = 12
       apply = { param($p) Set-Stall $p $null; Set-Held $p 120; Set-Episodes $p $null
                 Set-ArmAbove $p 5; Set-Retrace $p 2; Set-StopLoss $p 12 } }
    # THE bar for real money: live paper books `worst`.
    @{ name = 'P7 V0 WORST fill'; conc = 12; fill = 'worst'
       apply = { param($p) Set-Stall $p $null; Set-Held $p 120; Set-Episodes $p $null
                 Set-ArmAbove $p 5; Set-Retrace $p 2; Set-StopLoss $p 8 } }
  )
  # OUT-OF-SAMPLE. Everything above was tuned on 07-25..07-28. Run with:
  #   -Since 2026-07-22T00:00:00Z -Until 2026-07-25T00:00:00Z
  v0oos = @(
    @{ name = 'O0 OOS V0'; conc = 12
       apply = { param($p) Set-Stall $p $null; Set-Held $p 120; Set-Episodes $p $null
                 Set-ArmAbove $p 5; Set-Retrace $p 2; Set-StopLoss $p 8 } }
    @{ name = 'O1 OOS V0 WORST fill'; conc = 12; fill = 'worst'
       apply = { param($p) Set-Stall $p $null; Set-Held $p 120; Set-Episodes $p $null
                 Set-ArmAbove $p 5; Set-Retrace $p 2; Set-StopLoss $p 8 } }
    # Does one-shot > re-entry hold out of sample too?
    @{ name = 'O2 OOS V0 REENTRY on'; conc = 12
       apply = { param($p) Set-Stall $p $null; Set-Held $p 120
                 Set-ArmAbove $p 5; Set-Retrace $p 2; Set-StopLoss $p 8 } }
    # And the old shipped exit, as the do-nothing baseline on the same window.
    @{ name = 'O3 OOS today-rule (unarmed r3 sl25 stall15)'; conc = 4
       apply = { param($p) Set-Retrace $p 3; Set-StopLoss $p 25 } }
  )
  # Phase B - the mechanics ladder, one knob at a time.
  retrace = @(
    @{ name = 'B retrace 2';  apply = { param($p) Set-Retrace $p 2  } }
    @{ name = 'B retrace 3';  apply = { param($p) Set-Retrace $p 3  } }
    @{ name = 'B retrace 5';  apply = { param($p) Set-Retrace $p 5  } }
    @{ name = 'B retrace 8';  apply = { param($p) Set-Retrace $p 8  } }
    @{ name = 'B retrace 12'; apply = { param($p) Set-Retrace $p 12 } }
    @{ name = 'B retrace 20'; apply = { param($p) Set-Retrace $p 20 } }
  )
  episodes = @(
    @{ name = 'B episodes 1';  apply = { param($p) Set-Episodes $p $null } }
    @{ name = 'B episodes 3';  apply = { param($p) Set-Episodes $p 3  } }
    @{ name = 'B episodes 8';  apply = { param($p) Set-Episodes $p 8  } }
    @{ name = 'B episodes 20'; apply = { param($p) Set-Episodes $p 20 } }
    @{ name = 'B episodes 40'; apply = { param($p) Set-Episodes $p 40 } }
  )
  cooldown = @(
    @{ name = 'B cooldown 0';  apply = { param($p) Set-Cooldown $p 0  } }
    @{ name = 'B cooldown 5';  apply = { param($p) Set-Cooldown $p 5  } }
    @{ name = 'B cooldown 15'; apply = { param($p) Set-Cooldown $p 15 } }
    @{ name = 'B cooldown 35'; apply = { param($p) Set-Cooldown $p 35 } }
  )
  dip = @(
    @{ name = 'B dip 8';  apply = { param($p) Set-Dip $p 8  } }
    @{ name = 'B dip 12'; apply = { param($p) Set-Dip $p 12 } }
    @{ name = 'B dip 16'; apply = { param($p) Set-Dip $p 16 } }
    @{ name = 'B dip 20'; apply = { param($p) Set-Dip $p 20 } }
  )
  stall = @(
    @{ name = 'B stall 10';  apply = { param($p) Set-Stall $p 10 } }
    @{ name = 'B stall 15';  apply = { param($p) Set-Stall $p 15 } }
    @{ name = 'B stall 30';  apply = { param($p) Set-Stall $p 30 } }
    @{ name = 'B stall off'; apply = { param($p) Set-Stall $p $null } }
  )
  stoploss = @(
    @{ name = 'B sl 10';  apply = { param($p) Set-StopLoss $p 10 } }
    @{ name = 'B sl 15';  apply = { param($p) Set-StopLoss $p 15 } }
    @{ name = 'B sl 25';  apply = { param($p) Set-StopLoss $p 25 } }
    @{ name = 'B sl off'; apply = { param($p) Set-StopLoss $p $null } }
  )
  concurrency = @(
    @{ name = 'B conc 1'; apply = {}; conc = 1 }
    @{ name = 'B conc 3'; apply = {}; conc = 3 }
    @{ name = 'B conc 4'; apply = {}; conc = 4 }
    @{ name = 'B conc 8'; apply = {}; conc = 8 }
  )
}

if (-not $ladders.ContainsKey($Plan)) {
  throw "unknown -Plan '$Plan'. Known: $($ladders.Keys -join ', ')"
}

$outDir = Split-Path -Parent $OutCsv
if (-not (Test-Path $outDir)) { New-Item -ItemType Directory -Force -Path $outDir | Out-Null }

$rows = @()
foreach ($step in $ladders[$Plan]) {
  $p = New-AnchorParams
  if ($step.apply) { & $step.apply $p }
  $fill = if ($step.fill) { $step.fill } else { 'first' }
  $conc = if ($step.conc) { $step.conc } else { 4 }
  $buy  = if ($step.buy)  { $step.buy  } else { 1.0 }
  # fee_only is the ONLY correct pairing with an explicit fill model - pumpfun_default
  # charges slippage_bps on top of a fill that already priced it (double count).
  $rows += Invoke-SimRun -Label $step.name -Params $p -Fill $fill `
             -Cost 'pumpfun_fee_only' -Concurrency $conc -BuySol $buy
}

$rows | Format-Table label, n_closed, win_rate, total_pnl_sol, median_pnl_pct, median_hold_s -AutoSize

if (Test-Path $OutCsv) { $rows | Export-Csv -Path $OutCsv -Append -NoTypeInformation -Encoding utf8 }
else                   { $rows | Export-Csv -Path $OutCsv          -NoTypeInformation -Encoding utf8 }
Write-Host "appended $($rows.Count) rows -> $OutCsv"
