# Build, toolchain, and env files

The rules live in the root [../CLAUDE.md](../CLAUDE.md); this is the reasoning behind
them. See [../RUN.md](../RUN.md) for how to actually run the stacks.

## `--target-dir` — always absolute, always forward slashes

Use `--target-dir` when a bin `.exe` is running, because it locks `target/`:

```powershell
cargo check -p hunter-live --target-dir "C:/Users/User/Documents/Bot/target-check"
```

- **Absolute**, so every crate and CWD shares the one dir. A relative path spawns a
  per-subdir copy, and each copy re-compiles the giant `libduckdb-sys` amalgamation
  (~20 GB and minutes each).
- **Forward slashes**, because cargo accepts them on Windows and they survive the Bash
  tool. A backslash path works in PowerShell, but the Bash tool eats the `\` escapes and
  collapses `C:\Users\...` to `C:UsersUserDocumentsBottarget-check` — which cargo then
  creates as a junk drive-relative folder in the CWD.

## sccache stays OFF as a rustc wrapper

`~/.cargo/config.toml` keeps `rustc-wrapper = "sccache"` commented out, and re-enabling it
breaks every build in this workspace.

**Measured**, with the wrapper on: `toml`, `httpdate`, `itertools`, `rayon-core`, `rustls`
and `opaque-debug` all failed on a *cold* target dir with `can't find crate for std` /
`only metadata stub found for rlib dependency core`, while `sccache --show-stats` reported
1780 rust compile requests, **0 hits**, 19 compilation failures and 5 cache errors. The
same crates build clean with the wrapper off.

**Likely mechanism, not proven:** the toolchain's std ships with `-Zembed-metadata=no`, so
`libcore-*.rlib` carries a metadata stub and the real metadata sits in a sidecar
`libcore-*.rmeta` (2.3 MB vs 63 MB). sccache derives an invocation's emitted-file set from
`rustc --print file-names` and supports only `link`/`metadata`/`dep-info` emits, which
would put the sidecar outside its model. Treat the operational rule as settled and this
paragraph as the working theory.

Note the error text is **not** specific to sccache — any truncated or half-written artifact
produces it, including one left behind by an OOM-killed or interrupted build. Before
blaming the wrapper, check whether a previous build died mid-flight; the fix there is
`cargo clean` on the target dir and one uninterrupted run.

It also caches nothing worth having: the expensive half of a cold rebuild is the DuckDB
C++ objects, which are **not** cached. Those are built by cc-rs/`cl.exe`, and MSVC caching
needs `cl.exe` on PATH (a VS Developer prompt) plus `CC = CXX = "sccache cl.exe"`. Outside
a dev prompt, cc-rs finds `cl.exe` by full VS path, and a bare-`cl.exe` override fails with
`ToolNotFound` and breaks every MSVC build — so leave `CC`/`CXX` unset unless you always
build from vcvars.

To re-evaluate on a newer sccache, treat it as an experiment: `sccache --zero-stats`, one
full build, then `--show-stats`, and require a non-zero hit rate on the **second** build
before trusting it.

Practical consequence: **keep the persistent `target-check` dir** rather than wiping and
re-running the DuckDB build.

## Build parallelism is capped by the commit limit, not by cores

`~/.cargo/config.toml` sets `[build] jobs = 4`. The workstation has 16 logical cores but
16 GB of RAM and a **fixed** 48 GB pagefile, so the Windows commit limit is hard at ~64 GB
and cannot grow. Cargo's default is one rustc per core, and a desktop already near that
limit (rust-analyzer and WSL run ~4.7 GB each, plus VS Code, vite, and any running
`hunter-lab.exe`) has no headroom for 16 of them.

Windows denies an allocation once commit is exhausted **however much RAM is free**, so the
failure looks nothing like memory pressure: `memory allocation of N bytes failed`,
`STATUS_STACK_BUFFER_OVERRUN`, `STATUS_DLL_INIT_FAILED`, and cascading
`cannot determine resolution for the attribute macro` errors in whichever sibling crate
happened to be compiling. It lands in crates as small as `borsh` or `windows-sys`, which
reads as a corrupt toolchain. Check `Get-Counter '\Memory\Commit Limit'` against
`'\Memory\Committed Bytes'` before believing any of those errors.

For a from-scratch build of the whole graph, drop to `-j 2` or close rust-analyzer first.

## `[profile.dev] debug = "line-tables-only"`

Full `debug = true` puts rustc's peak memory on `hunter-lab` past what a 16 GB workstation
has free, and the OOM surfaces as `memory allocation of N bytes failed` plus a cascade of
unrelated-looking macro-resolution errors in sibling crates. `line-tables-only` keeps
file/line in backtraces and drops the type info. Any `[profile]` edit invalidates every
artifact once, DuckDB C++ included — set it and leave it.

## File encoding

Never write or edit a file with characters that can break — a BOM, smart quotes or
em-dashes pasted from elsewhere, or any non-ASCII punctuation a tool can mangle. Save as
plain **UTF-8, no BOM**. If a file needs non-ASCII content, use real UTF-8 characters
directly, not escaped sequences.

## Env files (`.env` / `.env.example`)

Each product owns its pair (`hunter/.env*`, `forge/.env*`) — there is no root `.env`.

- **Preserve the at-a-glance style.** Every var sits under a
  `# ===== Section (required/optional) =====` banner, with one short comment above it
  saying what it is and why it matters. Keep new entries in that exact shape so the file
  stays scannable.
- `VITE_*` vars are **public** — baked into the JS bundle, so they must never hold a
  secret.
- **Edit both files together: same keys, same order, same comments; only the values
  differ.** `.env` gets real, immediately-usable values; `.env.example` gets placeholders
  (`{your_helius_api_key_here}`, `change_me`) so copying it back over `.env` is a one-step
  recovery. A key in one file must exist in the other.
- Back up before applying new keys: `Copy-Item .env .env.backup -Force`. `.env` is
  gitignored — never commit real secrets.
