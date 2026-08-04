<#
.SYNOPSIS
  Reconcile a migration ledger with the squashed single-file `0001_init.sql`.

.DESCRIPTION
  The three migration chains (hunter/core, hunter/lab, forge) were collapsed into
  one `0001_init.sql` each. A database that already ran the old chain therefore
  disagrees with the code in two ways:

    1. version 1 holds the OLD file's checksum -> sqlx aborts with
       "migration 1 was previously applied but has been modified";
    2. versions 2..N are recorded but no longer exist on disk -> sqlx aborts with
       "migration N was previously applied but is missing in the resolved
       migrations".

  This script rewrites the ledger to match: it deletes every row above version 1
  and stamps version 1 with the checksum of the new file. It touches NO schema and
  NO data -- the tables/columns those migrations created are already there, which
  is exactly why the squashed init reproduces them and must not be re-run.

  Checksum = SHA-384 over the migration file's raw bytes, which is what sqlx 0.6
  computes at compile time (`Sha384::digest(fs::read_to_string(path).as_bytes())`).
  `.gitattributes` pins `**/migrations/*.sql` to LF so this value is identical on
  the Windows workstation and in the Linux build container; the script refuses to
  run against a CRLF checkout rather than write a checksum only one of them agrees
  with.

  LEDGERS
    core   `_sqlx_migrations` <- hunter/core/migrations/0001_init.sql (both hunter bins)
    lab    `_lab_migrations`  <- hunter/lab/migrations/0001_init.sql  (hunter-lab only)
    forge  `_sqlx_migrations` <- forge/migrations/0001_init.sql       (both forge bins)

  A ledger whose table does not exist is skipped (that database never ran the
  chain, so the squashed init applies normally on the next boot).

.PARAMETER DatabaseUrl
  libpq connection URI, e.g. postgres://user:pw@127.0.0.1:5555/hunter_bot
  Defaults to $env:DATABASE_URL.

.PARAMETER Ledger
  Which ledgers to reconcile in this database. Default: core + lab (a hunter DB).
  Use -Ledger forge for a forge_bot database. Use -Ledger core alone for the EC2
  live box (it has no `_lab_migrations`; the script would skip it anyway).

.PARAMETER Apply
  Actually write. Without it the script only reports what it WOULD change.

.EXAMPLE
  # Local hunter DB (dry run first, then apply)
  .\scripts\consolidate-migration-ledgers.ps1 -DatabaseUrl 'postgres://postgres:pw@127.0.0.1:5555/hunter_bot'
  .\scripts\consolidate-migration-ledgers.ps1 -DatabaseUrl 'postgres://postgres:pw@127.0.0.1:5555/hunter_bot' -Apply

.EXAMPLE
  # forge DB
  .\scripts\consolidate-migration-ledgers.ps1 -DatabaseUrl 'postgres://postgres:pw@127.0.0.1:5556/forge_bot' -Ledger forge -Apply

.NOTES
  ORDER MATTERS for hunter. `hunter/scripts/db-incremental-sync.ps1` copies the
  SERVER's `_sqlx_migrations` rows into the local mirror (ON CONFLICT DO NOTHING),
  so reconcile the EC2 database BEFORE the next sync -- otherwise the sync
  re-inserts versions 2..21 into a local ledger you just cleaned.

  Requires `psql` on PATH.
#>
[CmdletBinding()]
param(
    [string]   $DatabaseUrl = $env:DATABASE_URL,
    [ValidateSet('core', 'lab', 'forge')]
    [string[]] $Ledger = @('core', 'lab'),
    [switch]   $Apply
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

if (-not $DatabaseUrl) {
    throw "No connection string. Pass -DatabaseUrl or set `$env:DATABASE_URL."
}
if (-not (Get-Command psql -ErrorAction SilentlyContinue)) {
    throw "psql not found on PATH."
}

$repoRoot = Split-Path -Parent $PSScriptRoot

# ledger key -> (migration file, ledger table, name/description column)
$LEDGERS = @{
    core  = @{ File = 'hunter/core/migrations/0001_init.sql'; Table = '_sqlx_migrations'; NameCol = 'description' }
    lab   = @{ File = 'hunter/lab/migrations/0001_init.sql';  Table = '_lab_migrations';  NameCol = 'name' }
    forge = @{ File = 'forge/migrations/0001_init.sql';       Table = '_sqlx_migrations'; NameCol = 'description' }
}

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

# SHA-384 of the file's raw bytes, as a psql bytea hex literal (\x...).
function Get-MigrationChecksum([string]$path) {
    $bytes = [System.IO.File]::ReadAllBytes($path)

    if ($bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF) {
        throw "$path starts with a UTF-8 BOM; sqlx hashes it too. Re-save as UTF-8 without BOM."
    }
    for ($i = 1; $i -lt $bytes.Length; $i++) {
        if ($bytes[$i] -eq 10 -and $bytes[$i - 1] -eq 13) {
            throw ("$path has CRLF line endings, so its checksum would not match the " +
                   "Linux build container's. Re-checkout with the .gitattributes " +
                   "`**/migrations/*.sql text eol=lf` rule applied: " +
                   "git add --renormalize . (or: git rm --cached -r . ; git reset --hard).")
        }
    }

    $sha = [System.Security.Cryptography.SHA384]::Create()
    try   { $hash = $sha.ComputeHash($bytes) }
    finally { $sha.Dispose() }

    return '\x' + (($hash | ForEach-Object { $_.ToString('x2') }) -join '')
}

function Invoke-Scalar([string]$sql) {
    $errFile = [System.IO.Path]::GetTempFileName()
    try {
        $out  = & psql $DatabaseUrl -v ON_ERROR_STOP=1 -tAc $sql 2>$errFile
        $code = $LASTEXITCODE
        if ($code -ne 0) {
            throw "psql failed (exit $code) for: $sql`n$(Get-Content $errFile -Raw)"
        }
        return ($out | Out-String).Trim()
    }
    finally { Remove-Item $errFile -ErrorAction SilentlyContinue }
}

function Invoke-Sql([string]$sql) {
    $errFile = [System.IO.Path]::GetTempFileName()
    $sqlFile = [System.IO.Path]::GetTempFileName()
    try {
        # psql chokes on a UTF-8 BOM.
        [System.IO.File]::WriteAllText($sqlFile, $sql, (New-Object System.Text.UTF8Encoding $false))
        & psql $DatabaseUrl -v ON_ERROR_STOP=1 -f $sqlFile 2>$errFile | Out-Null
        $code = $LASTEXITCODE
        if ($code -ne 0) {
            throw "psql failed (exit $code):`n$(Get-Content $errFile -Raw)"
        }
    }
    finally {
        Remove-Item $errFile -ErrorAction SilentlyContinue
        Remove-Item $sqlFile -ErrorAction SilentlyContinue
    }
}

# ---------------------------------------------------------------------------
# Reconcile
# ---------------------------------------------------------------------------

Write-Host "Database : $($DatabaseUrl -replace '://[^@/]+@', '://***@')"
Write-Host "Mode     : $(if ($Apply) { 'APPLY' } else { 'DRY RUN (pass -Apply to write)' })"
Write-Host ""

$changed = 0

foreach ($key in $Ledger) {
    $spec  = $LEDGERS[$key]
    $table = $spec.Table
    $path  = Join-Path $repoRoot $spec.File

    Write-Host "[$key] $($spec.File) -> $table"

    if (-not (Test-Path $path)) { throw "  missing migration file: $path" }

    $exists = Invoke-Scalar "SELECT to_regclass('public.$table') IS NOT NULL"
    if ($exists -ne 't') {
        Write-Host "  $table does not exist -- nothing applied yet; the squashed init will run normally. Skipped."
        Write-Host ""
        continue
    }

    $rows = [int](Invoke-Scalar "SELECT count(*) FROM $table")
    if ($rows -eq 0) {
        Write-Host "  $table is empty -- the squashed init will run normally. Skipped."
        Write-Host ""
        continue
    }

    $checksum = Get-MigrationChecksum $path
    $current  = Invoke-Scalar "SELECT encode(checksum, 'hex') FROM $table WHERE version = 1"
    $stale    = [int](Invoke-Scalar "SELECT count(*) FROM $table WHERE version > 1")

    if (-not $current) {
        Write-Host "  WARNING: $table has $rows row(s) but no version 1. Not touching it -- inspect by hand:"
        Write-Host "    SELECT version, $($spec.NameCol) FROM $table ORDER BY version;"
        Write-Host ""
        continue
    }

    $wanted = $checksum.Substring(2)
    if ($current -eq $wanted -and $stale -eq 0) {
        Write-Host "  already reconciled (1 row, checksum matches). Nothing to do."
        Write-Host ""
        continue
    }

    Write-Host "  version 1 checksum : $($current.Substring(0,16))... -> $($wanted.Substring(0,16))..."
    Write-Host "  rows above v1      : $stale (will be deleted)"

    if (-not $Apply) {
        Write-Host "  (dry run -- not written)"
        Write-Host ""
        $changed++
        continue
    }

    # One transaction: the ledger is never left half-rewritten.
    $sql = @"
BEGIN;
DELETE FROM $table WHERE version > 1;
UPDATE $table
   SET checksum = '$checksum'::bytea,
       $($spec.NameCol) = 'init'
 WHERE version = 1;
COMMIT;
"@
    Invoke-Sql $sql
    Write-Host "  reconciled."
    Write-Host ""
    $changed++
}

if ($Apply) {
    Write-Host "Done ($changed ledger(s) rewritten). Boot the bin to confirm migrations validate."
} else {
    Write-Host "Dry run complete ($changed ledger(s) would change). Re-run with -Apply."
}
