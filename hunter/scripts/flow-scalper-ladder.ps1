# Flow-scalper backtest ladder driver.
#
# Posts engine-simulate DRAFTS (no DB rule rows) to the running lab bin, polls each
# run, and appends one CSV row per run. Conclusions this harness produced live in
# hunter/docs/plans/strategies/flow-scalper-findings.md; the raw rows are the CSV
# below. READ THE FINDINGS FIRST - in particular the noise floor (per-episode sd is
# 9-15%, i.e. a bigger standard error than most differences a ladder ranks) and the
# fact that every row logged before 2026-07-28 was priced without our own market
# impact and at a 100bps fee, so it understates cost by ~3pp/round-trip.
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
  # NOT `$PSScriptRoot` alone: it is empty in a param default under some invocations
  # (observed via `Start-Process powershell -File …`), and the path then degrades to
  # `/../docs/…`, which resolves against the DRIVE ROOT — the ladder runs, reports
  # success, and appends every row to `C:\docs\roadmap\data\` where nobody looks.
  # `$PSCommandPath` is populated in the same places and survives that.
  [string]$OutCsv     = (Join-Path (Split-Path -Parent $PSCommandPath) '../docs/roadmap/data/flow-scalper-ladder.csv'),
  # Per-run, not per-ladder. Fold cost varies by more than an order of magnitude with
  # the GEOMETRY, not just the window: the omego anchor folds a 5-day window in ~8 min
  # because `stall >= 15` caps every hold at ~15 s, while the 64hP geometry
  # (`time >= 45`, `held >= 90`, re-entry to 40 episodes) keeps ~6x as many positions
  # alive and runs well past 25 min. Do not tune this down off one geometry's timing -
  # a too-short value silently kills a HEALTHY run (observed 2026-07-28).
  [int]$TimeoutSec    = 5400
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
# 5-day window (docs/plans/strategies/wallet-analysis.md, 2026-07-27 section).
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
  # losers (docs/plans/strategies/armed-trailing-stop.md).
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
function Set-Liquidity ($p, $lo, $hi) {
  $p.entry.m_snapshot.liquidity = @(
    @{ operator = '>='; value = $lo }, @{ operator = '<='; value = $hi }
  )
}
function Set-Gross ($p, $v) {
  # The 60 s hot gate. Kept as element 0 of the multi-window array; element 1 is the
  # 2 s net_flow floor (see `Remove-NetFlowGate`).
  $p.entry.m_flow_window[0].gross_flow = @(@{ operator = '>='; value = $v })
}
function Set-Age ($p, $v) { $p.entry.m_snapshot.time = @(@{ operator = '>='; value = $v }) }

# `scale_out` - the ordered partial-exit ladder (docs/plans/strategies/partial-exits.md).
# Wire shape per stage is { sell_bps?, take_profit?, conditions? }: `sell_bps` omitted
# means the REMAINDER stage (sells all that is left, must be last), and `conditions` is
# a nested SideConditions object, NOT flattened into the stage.
#
# The one semantic that decides how a row must be read: the authored global `exit` side
# still closes 100% at any stage. So a ladder does not replace the trail, it races it -
# whichever fires first wins, and after a partial banks, the stub is still governed by
# that same global exit. A stage therefore measures "bank a tranche BEFORE the trail
# gets there", which is exactly the give-back this ladder is built to probe.
function Set-ScaleOut ($p, $stages) {
  if ($null -eq $stages) { $p.Remove('scale_out') } else { $p.scale_out = $stages }
}
function New-TpStage   ($SellBps, $Tp) { @{ sell_bps = $SellBps; take_profit = $Tp } }
function New-HeldStage ($Secs) {
  # Remainder stage: no `sell_bps` key at all, so the stub closes on a pure time stop.
  @{ conditions = @{ m_position = @{ held = @(@{ operator = '>='; value = $Secs }) } } }
}

function Set-UniqueWallets ($p, $v) {
  # `m_flow_window(60).unique_wallets >= N` - distinct buyers in the window, the crowd
  # gate. Element 0 is the 60 s instance the gross gate already lives on, so this rides
  # the SAME window rather than registering another one.
  if ($null -eq $v) { $p.entry.m_flow_window[0].Remove('unique_wallets') }
  else { $p.entry.m_flow_window[0].unique_wallets = @(@{ operator = '>='; value = $v }) }
}
function Remove-GrossGate ($p) {
  # Drop `gross_flow` from the 60 s instance, keeping the instance itself (its window
  # is what `unique_wallets` reads). For the REPLACE arm of the crowd-vs-volume test.
  $p.entry.m_flow_window[0].Remove('gross_flow')
}

function Use-64hpGeometry ($p) {
  # `64hP97Bwr5...` - the SECOND scalper wallet, whose closed-episode economics clear
  # the pump.fun fee by ~2.5pp where omego's are flat. Knobs from the 2026-07-28
  # section of docs/plans/strategies/wallet-analysis.md (rule-ladder spec: the
  # "fs2-* ladder" section of that file). Every one differs from the omego
  # calibration this script was originally built around.
  Set-Age       $p 45      # his med token age at first buy is 0.8 min, not 5.3
  Set-Liquidity $p 36 70   # his first-buy vsol p25-p75; 40-55 is his sweet spot
  Set-Dip       $p 18      # med entry dip -22.7%; his >-8% bucket is his ONLY loser
  Set-Gross     $p 45      # his p25 gross60 is 45.6 SOL, med 85 - far hotter than omego
  Set-Retrace   $p 7       # his measured trail is -6.8% off the since-entry peak
  Set-ArmAbove  $p $null   # he trails UNARMED - the width is what makes it survivable
  Set-StopLoss  $p $null   # stop overlays at 15..50 all make his book WORSE
  Set-Stall     $p $null   # replaced by an honest `held` bound, below
  Set-Held      $p 90      # his >90s cohort is net negative (-0.28%/ep)
  Set-Episodes  $p 40      # ep9+ is his BEST cohort (65% win) - never cap this low
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

  # The run registry is IN MEMORY. If the lab bin restarts mid-run - which happens
  # when another session rebuilds it, and this box hosts exactly one on :8140 - the
  # run id is gone for good and the summary 404s forever. Treating every failure as
  # "still folding" turns that into a silent full-timeout stall (observed 2026-07-28:
  # 40 wasted minutes polling a bin that no longer knew the run). So separate the
  # three cases instead of swallowing them all.
  $summary = $null
  while (((Get-Date) - $start).TotalSeconds -lt $TimeoutSec) {
    Start-Sleep -Seconds 10
    try {
      $summary = Invoke-RestMethod -Method Post `
        -Uri "$LabUrl/api/strategies/simulate/$runId/result/summary" `
        -Headers $headers -ContentType 'application/json' -Body '{}'
      break
    } catch {
      $code = $null
      if ($_.Exception.Response) { $code = $_.Exception.Response.StatusCode.value__ }
      if ($null -eq $code) {
        # No HTTP response at all => the bin is down. Nothing to wait for.
        throw "lab at $LabUrl stopped responding during run $runId ($($_.Exception.Message)). It was probably restarted; re-run this ladder."
      }
      if ($code -eq 401 -or $code -eq 403) { throw "auth rejected polling run $runId (HTTP $code)" }
      if ($code -ge 500) {
        # The run FAILED server-side and the id will 500 forever. This is the state
        # that used to masquerade as "still folding" for the whole timeout. The body
        # carries the real reason - on this box it is usually
        #   "lake trade fetch failed: Out of Memory Error: Allocation failure"
        # i.e. DuckDB could not allocate while reading the lake, which happens when
        # something else (a second lab instance, a sweep) is holding RAM on the 16 GB
        # workstation. Surface it immediately instead of polling a corpse.
        $detail = $null
        if ($_.ErrorDetails) { $detail = $_.ErrorDetails.Message }
        elseif ($_.Exception.Response) {
          try {
            $sr = New-Object System.IO.StreamReader($_.Exception.Response.GetResponseStream())
            $detail = $sr.ReadToEnd()
          } catch { }
        }
        throw "run $runId FAILED server-side (HTTP $code): $detail"
      }
      # 404 while the job is still folding is the one expected transient.
      Write-Host '.' -NoNewline
    }
  }
  if ($null -eq $summary) {
    # State the ambiguity rather than guessing a cause. A 404 means only "this bin has
    # no summary for that id", which is equally consistent with (a) still folding and
    # (b) the bin having restarted and lost the run. Distinguishing them from inside
    # the script is not possible - there is no run-status endpoint - so hand the caller
    # the two checks that DO separate them.
    throw ("run $runId produced no summary within $TimeoutSec s, and the lab is still " +
           "reachable. That is 404-ambiguous: the run may still be folding, or the bin " +
           "may have restarted and dropped it. To tell them apart: (1) is hunter-lab's " +
           "PID the same as when the run started? (2) is it burning CPU - a fold pegs " +
           "one core, an idle bin sits at 0%. If it is still folding, the summary will " +
           "appear on its own; poll " +
           "$LabUrl/api/strategies/simulate/$runId/result/summary rather than re-running.")
  }

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
  # DEV-BUY FINGERPRINT A/B (2026-07-28). The one creation axis that predicts 64hP's
  # per-episode OUTCOME rather than his selection: on his 6,515 closed episodes a dev
  # buy >= 12.8 SOL (net; = ~13 SOL gross) scores 59.2% win / +7.11%/ep against 49.1%
  # / +2.44% below it, and the lift holds on an untouched 07-26..27 holdout. Mint-level
  # block permutation p=0.006 on win rate. It is NOT a liquidity proxy - the lift
  # survives conditioning on the entry-vsol band. See
  # docs/plans/strategies/wallet-analysis.md ("dev-buy size").
  #
  # Run this plan TWICE, once per fingerprint, and compare like for like. The two are
  # exact complements at bucket width 12.8 SOL, so the ONLY difference is dev-buy size:
  #   -FingerprintId 49471cf5-41d5-4929-973a-4bc0afc63754   # fs3-dev big   [12.8,25.6)
  #   -FingerprintId c9a76f18-9a14-4a5a-83fe-99bd97dea05e   # fs3-dev small [0,12.8)
  # Buy size is 0.30 SOL, not the ladder's historical 1.0: execution-costs.md puts the
  # impact-aware optimum at sqrt(fixed_per_leg * vsol) ~ 0.27 on this vsol band.
  fp13 = @(
    # Base = the 64hP geometry with the two gates the dev-buy cohort measured best at
    # (vsol 40-75 rather than the seed's 36-70; dip >= 18).
    @{ name = 'N0 dev13 base'; buy = 0.30
       apply = { param($p) Use-64hpGeometry $p; Set-Liquidity $p 40 75
                 Set-ArmAbove $p 2 } }
    # His single best dip cohort (-25..-40); on the big-dev-buy subset it took win
    # 67.3% -> 71.8% in-sample and 75.9% -> 78.6% on the holdout.
    @{ name = 'N1 dev13 dip 25'; buy = 0.30
       apply = { param($p) Use-64hpGeometry $p; Set-Liquidity $p 40 75; Set-Dip $p 25
                 Set-ArmAbove $p 2 } }
    # The `lock` ladder's invariant: arm MUST exceed retrace or the trail gives back
    # more than it locked in. fs2-00 (arm 2 / retrace 7) violates it; this is the fix,
    # and it is the knob that should move WIN RATE most.
    @{ name = 'N2 dev13 arm8 r4 sl10'; buy = 0.30
       apply = { param($p) Use-64hpGeometry $p; Set-Liquidity $p 40 75
                 Set-ArmAbove $p 8; Set-Retrace $p 4; Set-StopLoss $p 10 } }
    @{ name = 'N3 dev13 dip25 arm8 r4 sl10'; buy = 0.30
       apply = { param($p) Use-64hpGeometry $p; Set-Liquidity $p 40 75; Set-Dip $p 25
                 Set-ArmAbove $p 8; Set-Retrace $p 4; Set-StopLoss $p 10 } }
    # Re-entry was PnL-negative on the broad universe because it compounds bad picks.
    # On a fingerprint that selects well it should behave like 64hP's (ep9+ = his best
    # cohort), so the sign of this row is the test of whether selection improved.
    @{ name = 'N4 dev13 dip25 arm8 one-shot'; buy = 0.30
       apply = { param($p) Use-64hpGeometry $p; Set-Liquidity $p 40 75; Set-Dip $p 25
                 Set-ArmAbove $p 8; Set-Retrace $p 4; Set-StopLoss $p 10
                 Set-Episodes $p $null } }
    # The bar that decides real money: live paper books `worst`.
    @{ name = 'N5 dev13 dip25 arm8 WORST fill'; buy = 0.30; fill = 'worst'
       apply = { param($p) Use-64hpGeometry $p; Set-Liquidity $p 40 75; Set-Dip $p 25
                 Set-ArmAbove $p 8; Set-Retrace $p 4; Set-StopLoss $p 10 } }
  )
  # Round 2, on the fp13 winner. N1 (dip 25 / arm 2 / retrace 7) took 59.5% win and
  # +5.42 %/ep; the arm8+retrace4 rows (N2-N5) all FAILED at a median -14.6 %, because
  # arming at +8 disables the trail below +8 and leaves `stop_loss` as the only exit -
  # so every position runs to the stop. 64hP's own wide, barely-armed trail is right.
  # Open questions this round settles: (a) does N1 survive `worst` fill, (b) is the dip
  # gate monotone, (c) does more concurrency just add worse episodes.
  fp13b = @(
    # THE bar for real money: live paper books `worst`.
    @{ name = 'M1 N1 WORST fill'; buy = 0.30; fill = 'worst'
       apply = { param($p) Use-64hpGeometry $p; Set-Liquidity $p 40 75; Set-Dip $p 25
                 Set-ArmAbove $p 2 } }
    # C6 measured 64hP's real fills at +1.18% vs signal, i.e. much nearer `signal`
    # than `worst` - the realistic bound for an operator with comparable latency.
    @{ name = 'M2 N1 SIGNAL fill'; buy = 0.30; fill = 'signal'
       apply = { param($p) Use-64hpGeometry $p; Set-Liquidity $p 40 75; Set-Dip $p 25
                 Set-ArmAbove $p 2 } }
    @{ name = 'M3 N1 dip 20'; buy = 0.30
       apply = { param($p) Use-64hpGeometry $p; Set-Liquidity $p 40 75; Set-Dip $p 20
                 Set-ArmAbove $p 2 } }
    @{ name = 'M4 N1 dip 30'; buy = 0.30
       apply = { param($p) Use-64hpGeometry $p; Set-Liquidity $p 40 75; Set-Dip $p 30
                 Set-ArmAbove $p 2 } }
    @{ name = 'M5 N1 one-shot'; buy = 0.30
       apply = { param($p) Use-64hpGeometry $p; Set-Liquidity $p 40 75; Set-Dip $p 25
                 Set-ArmAbove $p 2; Set-Episodes $p $null } }
    @{ name = 'M6 N1 conc 12'; buy = 0.30; conc = 12
       apply = { param($p) Use-64hpGeometry $p; Set-Liquidity $p 40 75; Set-Dip $p 25
                 Set-ArmAbove $p 2 } }
    @{ name = 'M7 N1 unarmed (64hP literal)'; buy = 0.30
       apply = { param($p) Use-64hpGeometry $p; Set-Liquidity $p 40 75; Set-Dip $p 25 } }
  )
  # The CONTROL arm of the A/B: the fp13 winner's geometry, UNCHANGED, run against a
  # lower dev-buy band. Anything it fails to reproduce is attributable to dev-buy size
  # rather than to the dip-25 / liq-40-75 gate changes that came with it.
  #
  # Do NOT use the exact complement `fs3-dev small [0-12.8)` for this: at ~85k tokens
  # the 6-day fold dies with `lake trade fetch failed: Out of Memory Error` (measured
  # 2026-07-28, 16 GB box). The size-matched bands below fold in minutes AND hold token
  # count roughly constant, which is the better-controlled comparison anyway. Run one
  # per invocation:
  #   -FingerprintId 7b2e4c10-5d3a-4f81-9c6e-2a1b0d4e8f37  # adj [9.6,12.8)  1,269 tok
  #   -FingerprintId 3f9a1d62-8c47-4e05-b1d2-6e7c93a4b508  # mid [6.4,9.6)   1,020 tok
  # Predictions from his own episodes: adj = 48.7% win / +0.89 %/ep (baseline),
  # mid = 55.6% / +3.57, vs big = 59.2% / +7.11.
  fp13ctl = @(
    @{ name = 'C1 control-band N1 geometry'; buy = 0.30
       apply = { param($p) Use-64hpGeometry $p; Set-Liquidity $p 40 75; Set-Dip $p 25
                 Set-ArmAbove $p 2 } }
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
  # The first HONEST baseline: 64hP's geometry priced under `pumpfun_impact` (which
  # charges buy_amount/reserve_sol per leg) and the corrected 125 bps/leg fee. Every
  # earlier ladder in this file ran zero-impact at 1.0 SOL under a 100 bps fee, so it
  # understated cost by ~3pp/round-trip. Nothing here is comparable to those rows.
  #
  # Size: cost is U-shaped - the Jito tip is fixed SOL/leg so it dominates small
  # orders, impact grows with big ones. Optimum is sqrt(fixed_per_leg * vsol);
  # on his 36-70 vsol band (med ~45) that is ~0.21 SOL, NOT the 1.0 we tested at.
  v64 = @(
    @{ name = 'V64-0 base 0.2 impact'
       apply = { param($p) Use-64hpGeometry $p }; buy = 0.2 }
    # The size curve, same rule - this is the knob the sim could not see until today.
    @{ name = 'V64-1 buy 0.1'; apply = { param($p) Use-64hpGeometry $p }; buy = 0.1 }
    @{ name = 'V64-2 buy 0.4'; apply = { param($p) Use-64hpGeometry $p }; buy = 0.4 }
    @{ name = 'V64-3 buy 1.0'; apply = { param($p) Use-64hpGeometry $p }; buy = 1.0 }
    # Same run, old cost model: the gap IS the impact the ladder never charged.
    @{ name = 'V64-4 base 0.2 FEE-ONLY (old model)'
       apply = { param($p) Use-64hpGeometry $p }; buy = 0.2; cost = 'pumpfun_fee_only' }
    # Does arming still pay once the trail is WIDE (7) instead of tight (3)?
    @{ name = 'V64-5 armed g3'
       apply = { param($p) Use-64hpGeometry $p; Set-ArmAbove $p 3 }; buy = 0.2 }
    # Isolate the dip change: his -22.7% median vs the omego-calibrated 12.
    @{ name = 'V64-6 dip 12 (omego depth)'
       apply = { param($p) Use-64hpGeometry $p; Set-Dip $p 12 }; buy = 0.2 }
    # The adversarial in-slot fill live paper books.
    @{ name = 'V64-7 WORST fill'
       apply = { param($p) Use-64hpGeometry $p }; buy = 0.2; fill = 'worst' }
  )
  # Follow-up to V64-0 (-3.419%/ep, 2,215 eps, win 28.6%, ALL exits via metrics,
  # **median hold 6.0 s**). A 6 s median under `retrace >= 7` + `held >= 90` means the
  # trail is firing almost immediately and `held` never binds - i.e. the unarmed-trail
  # trap again (peak seeds at the entry fill, so `retrace >= 7` is a hard -7% stop from
  # entry). 64hP survives that because he buys a -22.7% dip; we evidently do not enter
  # where he does. Ordered by what would change a decision, since each run is ~32 min.
  v64b = @(
    # THE hypothesis: does arming rescue the 6 s hold?
    @{ name = 'V64b-1 armed g3'
       apply = { param($p) Use-64hpGeometry $p; Set-ArmAbove $p 3 }; buy = 0.2 }
    @{ name = 'V64b-2 armed g6 trail 7'
       apply = { param($p) Use-64hpGeometry $p; Set-ArmAbove $p 6 }; buy = 0.2 }
    # What the new cost model actually changed on THIS geometry (same run, old pricing).
    @{ name = 'V64b-3 base FEE-ONLY (old model)'
       apply = { param($p) Use-64hpGeometry $p }; buy = 0.2; cost = 'pumpfun_fee_only' }
    # Size curve extreme - 5x the notional, so ~5x the impact term.
    @{ name = 'V64b-4 buy 1.0'
       apply = { param($p) Use-64hpGeometry $p }; buy = 1.0 }
  )
  # THE COUPLING FIX. V64-0 (unarmed, no stop) and V64b-1 (arm 3, no stop) both lose,
  # for OPPOSITE mechanical reasons, and the second one is my configuration error:
  #
  #   `arm_above_pct` and "no `stop_loss`" cannot both be copied from a wallet that
  #   trails UNARMED. With the peak seeded at the entry fill, an unarmed trail IS the
  #   stop-loss - that is why 64hP needs no separate one. Arming it removes that
  #   function, so anything that never reaches the arm threshold has no exit at all
  #   until `held >= 90`. Hence V64b-1's win rate 28.6% -> 56.1% WITH PnL falling
  #   -3.42% -> -5.74%/ep: more winners held, but the losers ran unbounded.
  #
  # So arming requires re-adding an explicit stop. This is the only configuration of
  # this geometry that has not had a fair test; without it we would be dismissing 64hP
  # on my error rather than on his economics.
  v64c = @(
    @{ name = 'V64c-1 armed g3 + stop 10'
       apply = { param($p) Use-64hpGeometry $p; Set-ArmAbove $p 3; Set-StopLoss $p 10 }; buy = 0.2 }
    @{ name = 'V64c-2 armed g3 + stop 15'
       apply = { param($p) Use-64hpGeometry $p; Set-ArmAbove $p 3; Set-StopLoss $p 15 }; buy = 0.2 }
  )
  # THE CROWD GATE, OUT OF SAMPLE. `m_flow_window.unique_wallets` = distinct wallets in
  # the window, against `gross_flow`'s SOL. The evidence for it was contested: a
  # gate-payoff SQL sweep called it "worth building" (monotone precision lift, strictly
  # dominating gross at matched recall), while flow-scalper-findings.md lists it among
  # four post-hoc bucket picks on this dataset that did NOT generalise. Both were
  # in-sample, so this ladder is the tiebreak the methodology rule demands.
  #
  # Window: 07-29..08-09, untouched by every fs3 calibration (fp13/fp13b tuned on
  # 07-25..07-28). Run it and nothing else on that window, or it stops being OOS.
  #
  #   ./hunter/scripts/flow-scalper-ladder.ps1 -Plan uw13 `
  #       -Since 2026-07-29T00:00:00Z -Until 2026-08-09T00:00:00Z `
  #       -FingerprintId 49471cf5-41d5-4929-973a-4bc0afc63754
  #
  # Two arms, because the SQL made two different claims: ADD (U1/U2 — crowd on top of
  # volume, which the SQL said is worse than crowd alone) and REPLACE (U3/U4 — crowd
  # instead of volume, the claim that it dominates). U0 is the shared control.
  uw13 = @(
    @{ name = 'U0 control (gross60 >= 45)'; buy = 0.30
       apply = { param($p) Use-64hpGeometry $p; Set-Liquidity $p 40 75; Set-Dip $p 25
                 Set-ArmAbove $p 2 } }
    @{ name = 'U1 add uw60 >= 20'; buy = 0.30
       apply = { param($p) Use-64hpGeometry $p; Set-Liquidity $p 40 75; Set-Dip $p 25
                 Set-ArmAbove $p 2; Set-UniqueWallets $p 20 } }
    @{ name = 'U2 add uw60 >= 30'; buy = 0.30
       apply = { param($p) Use-64hpGeometry $p; Set-Liquidity $p 40 75; Set-Dip $p 25
                 Set-ArmAbove $p 2; Set-UniqueWallets $p 30 } }
    @{ name = 'U3 uw60 >= 20 INSTEAD of gross'; buy = 0.30
       apply = { param($p) Use-64hpGeometry $p; Set-Liquidity $p 40 75; Set-Dip $p 25
                 Set-ArmAbove $p 2; Remove-GrossGate $p; Set-UniqueWallets $p 20 } }
    @{ name = 'U4 uw60 >= 30 INSTEAD of gross'; buy = 0.30
       apply = { param($p) Use-64hpGeometry $p; Set-Liquidity $p 40 75; Set-Dip $p 25
                 Set-ArmAbove $p 2; Remove-GrossGate $p; Set-UniqueWallets $p 30 } }
    # A gate that only helps by firing less is not an edge - it is a smaller sample.
    # This one is deliberately loose: if the lift is real it should survive here too.
    @{ name = 'U5 add uw60 >= 10'; buy = 0.30
       apply = { param($p) Use-64hpGeometry $p; Set-Liquidity $p 40 75; Set-Dip $p 25
                 Set-ArmAbove $p 2; Set-UniqueWallets $p 10 } }
  )
  # Round 2 of the crowd gate, same OOS window. `uw13` established two things: at 10-30
  # the crowd gate is LOOSER than `gross60 >= 45` (adding it changes nothing at all -
  # U0/U1/U2/U5 are the same 127 episodes), and replacing gross with it fires MORE
  # (u20 = 151, u30 = 144). So the SQL's "crowd dominates volume" claim was never
  # tested by those rows: they compare different sample sizes.
  #
  # This round tightens the crowd gate until it fires like the volume gate, which is
  # the only comparison that means anything - a gate that just fires less is not an
  # edge, it is a smaller sample. Target n ~ 127.
  uw13c = @(
    @{ name = 'W1 uw60 >= 40 instead of gross'; buy = 0.30
       apply = { param($p) Use-64hpGeometry $p; Set-Liquidity $p 40 75; Set-Dip $p 25
                 Set-ArmAbove $p 2; Remove-GrossGate $p; Set-UniqueWallets $p 40 } }
    @{ name = 'W2 uw60 >= 60 instead of gross'; buy = 0.30
       apply = { param($p) Use-64hpGeometry $p; Set-Liquidity $p 40 75; Set-Dip $p 25
                 Set-ArmAbove $p 2; Remove-GrossGate $p; Set-UniqueWallets $p 60 } }
    @{ name = 'W3 uw60 >= 80 instead of gross'; buy = 0.30
       apply = { param($p) Use-64hpGeometry $p; Set-Liquidity $p 40 75; Set-Dip $p 25
                 Set-ArmAbove $p 2; Remove-GrossGate $p; Set-UniqueWallets $p 80 } }
    # And the ADD arm at a threshold that can actually bind, now that 10-30 cannot.
    @{ name = 'W4 add uw60 >= 60'; buy = 0.30
       apply = { param($p) Use-64hpGeometry $p; Set-Liquidity $p 40 75; Set-Dip $p 25
                 Set-ArmAbove $p 2; Set-UniqueWallets $p 60 } }
    @{ name = 'W5 add uw60 >= 100'; buy = 0.30
       apply = { param($p) Use-64hpGeometry $p; Set-Liquidity $p 40 75; Set-Dip $p 25
                 Set-ArmAbove $p 2; Set-UniqueWallets $p 100 } }
  )
  # THE SCALE-OUT RE-MEASURE. Contract: docs/plans/strategies/partial-exits.md.
  #
  # Base = `fs3-00 dev13 base`, which IS the fp13 `N1` row (64hP geometry, liq 40-75,
  # dip 25, arm 2, trail 7, held 90) - a bare armed trail, i.e. exactly the "gives back
  # 25-30% of every winner" shape a banked tranche is supposed to fix. Run it with the
  # big-dev-buy fingerprint and the WIDE window, because N1 closed only 74 episodes on
  # 07-25..07-28 and the per-episode sd is 9-15%: a scale-out delta on 74 episodes sits
  # inside the noise.
  #
  #   ./hunter/scripts/flow-scalper-ladder.ps1 -Plan so13 `
  #       -Since 2026-07-22T00:00:00Z -Until 2026-07-28T00:00:00Z `
  #       -FingerprintId 49471cf5-41d5-4929-973a-4bc0afc63754
  #
  # Two families, and they are NOT comparable to each other - only within a family:
  #   S0-S4, S7  bank a tranche while the global trail stays exactly N1's. ONE knob vs
  #              S0, so the delta is attributable to the tranche alone.
  #   S5-S6      does the STUB want its own policy? The global `held >= 90` has to go,
  #              or it closes the stub before a remainder stage can act - and dropping
  #              it leaves an armed trail with no floor below +2% (the V64b-1 trap:
  #              win rate up, PnL down, losers unbounded), so a `stop_loss` comes back
  #              in. That is two knobs, hence S5 as the family's own control.
  so13 = @(
    @{ name = 'S0 N1 control (no ladder)'; buy = 0.30
       apply = { param($p) Use-64hpGeometry $p; Set-Liquidity $p 40 75; Set-Dip $p 25
                 Set-ArmAbove $p 2 } }
    # Where is "strength"? N1's median closed episode is +2.9%, so +10 already banks
    # well into the right tail and +25 will fire on few positions.
    @{ name = 'S1 bank 50% @ +10'; buy = 0.30
       apply = { param($p) Use-64hpGeometry $p; Set-Liquidity $p 40 75; Set-Dip $p 25
                 Set-ArmAbove $p 2; Set-ScaleOut $p @((New-TpStage 5000 10)) } }
    @{ name = 'S2 bank 50% @ +15'; buy = 0.30
       apply = { param($p) Use-64hpGeometry $p; Set-Liquidity $p 40 75; Set-Dip $p 25
                 Set-ArmAbove $p 2; Set-ScaleOut $p @((New-TpStage 5000 15)) } }
    @{ name = 'S3 bank 50% @ +25'; buy = 0.30
       apply = { param($p) Use-64hpGeometry $p; Set-Liquidity $p 40 75; Set-Dip $p 25
                 Set-ArmAbove $p 2; Set-ScaleOut $p @((New-TpStage 5000 25)) } }
    # Tranche SIZE at the middle level - how much tail is worth keeping.
    @{ name = 'S4 bank 30% @ +15'; buy = 0.30
       apply = { param($p) Use-64hpGeometry $p; Set-Liquidity $p 40 75; Set-Dip $p 25
                 Set-ArmAbove $p 2; Set-ScaleOut $p @((New-TpStage 3000 15)) } }
    # --- stub-policy family: its own control first ---
    @{ name = 'S5 stub-family control (stop 15, no held cap)'; buy = 0.30
       apply = { param($p) Use-64hpGeometry $p; Set-Liquidity $p 40 75; Set-Dip $p 25
                 Set-ArmAbove $p 2; Set-Held $p $null; Set-StopLoss $p 15 } }
    @{ name = 'S6 bank 50% @ +15, stub = pure time stop 300'; buy = 0.30
       apply = { param($p) Use-64hpGeometry $p; Set-Liquidity $p 40 75; Set-Dip $p 25
                 Set-ArmAbove $p 2; Set-Held $p $null; Set-StopLoss $p 15
                 Set-ScaleOut $p @((New-TpStage 5000 15), (New-HeldStage 300)) } }
    # The bar that decides real money: live paper books `worst`. Every extra leg pays
    # the fixed per-leg cost again, so an adversarial fill hurts a ladder more than it
    # hurts S0 - if the tranche survives here it survives.
    @{ name = 'S7 bank 50% @ +15 WORST fill'; buy = 0.30; fill = 'worst'
       apply = { param($p) Use-64hpGeometry $p; Set-Liquidity $p 40 75; Set-Dip $p 25
                 Set-ArmAbove $p 2; Set-ScaleOut $p @((New-TpStage 5000 15)) } }
  )
}

if (-not $ladders.ContainsKey($Plan)) {
  throw "unknown -Plan '$Plan'. Known: $($ladders.Keys -join ', ')"
}

$outDir = Split-Path -Parent $OutCsv
if (-not (Test-Path $outDir)) { New-Item -ItemType Directory -Force -Path $outDir | Out-Null }

# Append each row the moment it lands, rather than batching at the end. The lab bin
# is a shared singleton on :8140 and another session restarting it kills the in-flight
# run - batching meant one such restart discarded every COMPLETED row too.
function Add-LadderRow ($row) {
  if (Test-Path $OutCsv) { $row | Export-Csv -Path $OutCsv -Append -NoTypeInformation -Encoding utf8 }
  else                   { $row | Export-Csv -Path $OutCsv          -NoTypeInformation -Encoding utf8 }
}

$rows = @()
foreach ($step in $ladders[$Plan]) {
  $p = New-AnchorParams
  if ($step.apply) { & $step.apply $p }
  $fill = if ($step.fill) { $step.fill } else { 'first' }
  $conc = if ($step.conc) { $step.conc } else { 4 }
  $buy  = if ($step.buy)  { $step.buy  } else { 1.0 }
  # `pumpfun_impact` is the honest default: it charges the 125 bps/leg fee, the fixed
  # tip/priority, AND our own buy_amount/reserve_sol price impact, so the cost finally
  # responds to `buy`. `pumpfun_fee_only` is size-blind (every pre-2026-07-28 row in the
  # CSV used it, at 1.0 SOL, and is therefore ~3pp/round-trip optimistic); keep it only
  # for an explicit old-vs-new comparison. `pumpfun_default` would double-count, since
  # its flat slippage_bps is a stand-in for the impact we now compute exactly.
  $cost = if ($step.cost) { $step.cost } else { 'pumpfun_impact' }
  $row = Invoke-SimRun -Label $step.name -Params $p -Fill $fill `
           -Cost $cost -Concurrency $conc -BuySol $buy
  Add-LadderRow $row
  $rows += $row
}

$rows | Format-Table label, n_closed, win_rate, total_pnl_sol, median_pnl_pct, median_hold_s -AutoSize
Write-Host "appended $($rows.Count) rows -> $OutCsv"
