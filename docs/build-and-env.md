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

## sccache covers rustc, not DuckDB

A global sccache (`~/.cargo/config.toml`, `rustc-wrapper = "sccache"`) caches *rustc*
output across all target dirs and across `cargo clean`.

It does **not** cache the DuckDB C++ objects: those are built by cc-rs/`cl.exe`, and MSVC
caching needs `cl.exe` on PATH (a VS Developer prompt) plus `CC = CXX = "sccache cl.exe"`.
Outside a dev prompt, cc-rs finds `cl.exe` by full VS path, and a bare-`cl.exe` override
fails with `ToolNotFound` and breaks every MSVC build — so leave `CC`/`CXX` unset unless
you always build from vcvars.

Practical consequence: **keep the persistent `target-check` dir** rather than wiping and
re-running the DuckDB build.

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
