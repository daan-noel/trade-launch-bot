<#
.SYNOPSIS
    Reclaims disk space from Cargo build caches and Docker cruft for the Bot monorepo.

.DESCRIPTION
    Two independent, safe-by-default cleanup passes:
      1. Cargo: wipes Bot/target-check (the duplicate check cache) by default, and
         keeps Bot/target so its DuckDB (libduckdb-sys) build is preserved and the
         next build stays incremental. Pass -DeepClean to also wipe Bot/target
         (pure rebuild cache; nothing is lost, but you pay a full DuckDB rebuild).
         Skips files locked by any running hunter-*/forge-* binary instead of failing.
      2. Docker: removes dangling volumes (0 container links - orphaned, e.g.
         leftovers from old project names), prunes stale build-cache layers, and
         removes exited containers. NEVER touches a volume that's attached to a
         container (so live dev DBs like hunter-pgdata/forge-pgdata are untouched).

.PARAMETER WhatIf
    Preview what would be removed without deleting anything.

.PARAMETER SkipDocker
    Only run the Cargo target cleanup.

.PARAMETER SkipCargo
    Only run the Docker cleanup.

.PARAMETER DeepClean
    Also wipe Bot/target (not just target-check). Reclaims the most space but forces
    a full rebuild afterward, including the expensive DuckDB C++ amalgamation.

.EXAMPLE
    .\cleanup-disk.ps1 -WhatIf
    .\cleanup-disk.ps1
    .\cleanup-disk.ps1 -DeepClean
#>
param(
    [switch]$WhatIf,
    [switch]$SkipDocker,
    [switch]$SkipCargo,
    [switch]$DeepClean
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot

function Get-DirSizeGB($path) {
    if (-not (Test-Path $path)) { return 0 }
    $size = (Get-ChildItem -Path $path -Recurse -Force -ErrorAction SilentlyContinue |
        Measure-Object -Property Length -Sum -ErrorAction SilentlyContinue).Sum
    return [math]::Round(($size / 1GB), 2)
}

function Remove-TargetDir($path, $label) {
    if (-not (Test-Path $path)) {
        Write-Host "  $label`: not present, skipping"
        return
    }
    $before = Get-DirSizeGB $path
    Write-Host "  $label`: $before GB"
    if ($WhatIf) {
        Write-Host "  [WhatIf] would delete $path"
        return
    }

    # Try a fast full delete first; if something's locked (e.g. a running .exe
    # from this dir), fall back to per-item deletion so unlocked files still go.
    try {
        Remove-Item -Path $path -Recurse -Force -ErrorAction Stop
        Write-Host "  $label`: fully removed"
        return
    } catch {
        # fall through to per-item cleanup below
    }

    $items = Get-ChildItem -Path $path -Recurse -Force -ErrorAction SilentlyContinue |
        Sort-Object -Property FullName -Descending
    $removed = 0; $skipped = 0
    foreach ($item in $items) {
        try {
            Remove-Item -LiteralPath $item.FullName -Force -ErrorAction Stop
            $removed++
        } catch {
            $skipped++
        }
    }
    $after = Get-DirSizeGB $path
    Write-Host "  $label`: removed $removed items, skipped $skipped locked items (after: $after GB)"
}

Write-Host "=== Bot monorepo disk cleanup ===" -ForegroundColor Cyan
if ($WhatIf) { Write-Host "(dry run - nothing will be deleted)" -ForegroundColor Yellow }

if (-not $SkipCargo) {
    Write-Host "`n--- Cargo target caches ---" -ForegroundColor Cyan
    $locking = Get-Process | Where-Object { $_.Path -like "$repoRoot\target*" }
    if ($locking) {
        Write-Host "  Note: these processes are running from target/ and may lock some files:"
        $locking | ForEach-Object { Write-Host "    PID $($_.Id): $($_.ProcessName) ($($_.Path))" }
    }
    # Default: wipe only the duplicate `target-check`. Keeping `target` preserves
    # its libduckdb-sys (DuckDB) build — a ~20 GB / multi-minute C++ amalgamation
    # that sccache does NOT cache (MSVC/cl.exe) — so the next build stays
    # incremental instead of recompiling DuckDB from scratch. Use -DeepClean to
    # also wipe `target` (you'll pay a full DuckDB rebuild afterward).
    Remove-TargetDir "$repoRoot\target-check" "target-check"
    if ($DeepClean) {
        Remove-TargetDir "$repoRoot\target" "target"
    } else {
        Write-Host "  target: kept (preserves DuckDB build; pass -DeepClean to wipe)"
    }
}

if (-not $SkipDocker) {
    Write-Host "`n--- Docker ---" -ForegroundColor Cyan
    $dockerOk = $true
    try { docker info *> $null } catch { $dockerOk = $false }

    if (-not $dockerOk) {
        Write-Host "  Docker not running/available, skipping."
    } else {
        $dangling = docker volume ls -qf dangling=true
        if ($dangling) {
            Write-Host "  Dangling (orphaned, 0-link) volumes:"
            $dangling | ForEach-Object { Write-Host "    $_" }
            if ($WhatIf) {
                Write-Host "  [WhatIf] would run: docker volume rm $($dangling -join ' ')"
            } else {
                docker volume rm $dangling
            }
        } else {
            Write-Host "  No dangling volumes."
        }

        if ($WhatIf) {
            Write-Host "  [WhatIf] would run: docker builder prune -af"
            Write-Host "  [WhatIf] would run: docker container prune -f"
        } else {
            docker builder prune -af | Out-Null
            docker container prune -f | Out-Null
            Write-Host "  Build cache pruned, exited containers removed."
        }
    }
}

Write-Host "`n=== Done ===" -ForegroundColor Cyan
Get-PSDrive -Name C -PSProvider FileSystem | ForEach-Object {
    Write-Host ("C: {0} GB used, {1} GB free" -f [math]::Round($_.Used/1GB,2), [math]::Round($_.Free/1GB,2))
}
