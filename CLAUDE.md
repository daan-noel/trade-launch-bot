# CLAUDE.md — Bot monorepo (super-root)

Hard rules for the whole `Bot/` workspace. This file is paid on every session, so it stays
a thin **index + rules**; explanations live in `docs/` and cost nothing until Read.

| Working in… | Also read |
| --- | --- |
| `hunter/**` | [hunter/CLAUDE.md](hunter/CLAUDE.md) — meme-coin trading bot |
| `forge/**` | [forge/CLAUDE.md](forge/CLAUDE.md) — launch + trading + analytics platform |
| `shared/**` | both — a change there ripples to each product |

## Layout — one Cargo `[workspace]`, two products

Root [Cargo.toml](Cargo.toml) is the ONE workspace; `default-members` = the hunter bins.
`hunter/` · `forge/` · `shared/` (drop-in crates `executor/{core,pumpfun}`,
`ingest/{core,pumpfun}`, `http-auth`) · `deploy/` (per-family docker/nginx) · `docs/`
(monorepo-wide) · [RUN.md](RUN.md) (how to run the stacks).

**`shared/` crates depend on no product crate.** Their public APIs are contracts — a change
hits hunter *and* forge, so verify both consumers and keep each crate's decoupled
vocabulary (never leak a product's domain names in).

```powershell
cargo build                       # hunter-live + hunter-lab only (default-members)
cargo check -p forge-live         # everything else is -p
cargo check -p hunter-live --target-dir "C:/Users/User/Documents/Bot/target-check"
```

Pass `--target-dir` when a bin `.exe` is running (it locks `target/`) — **absolute path,
forward slashes, always the same dir**, else DuckDB rebuilds at ~20 GB a copy. Detail:
[docs/build-and-env.md](docs/build-and-env.md).

## Rules for both products

- **Single source of truth.** Before adding a constant, formula, SQL fragment, type, or
  column list, search for an existing one and reuse it. Watch for the same fact defined
  twice; where decoupling makes duplication unavoidable, add a no-DB guard test asserting
  the copies stay equal.
- **Backend latency first.** No blocking I/O, `.await`-on-lock, per-event alloc, redundant
  RPC/DB round-trips, or lock contention on a hot path. **Notify over poll**; sell-confirm
  stays feed-based.
- **Spend Helius sparingly** — a paid, capped budget. Prefer the push/stream feeds, cache
  and batch over polling; reuse a value already in hand; never call per-event. If a call
  doesn't change a decision, delete it.
- **Modular & concise.** handler → service → repo, one responsibility per module. Short
  answers; a non-trivial plan goes in a `*-plan.md`, not inline.
- **Deploy target is a 2vCPU / 4GB EC2 box.** Only `*-live` bins + their shared crates ship
  there; `*-lab` + DuckDB/arrow/parquet/rayon stay on the workstation. Never raise cache
  caps/TTLs or add a pool on the server — sync to the workstation instead.
- **No secrets in code.** `.env` is gitignored and stays key-for-key in sync with
  `.env.example`; back up first (`Copy-Item .env .env.backup -Force`). Both files' editing
  rules: [docs/build-and-env.md](docs/build-and-env.md).
- **Definition of done:** `cargo check` clean on touched bins; clippy on touched code; test
  when logic changed; stay in the owning crate; no new warnings; update the docs tier the
  change belongs to.

## Docs — write it in the tier that changed

| Changed | Goes in |
| --- | --- |
| A rule / command / constraint | the nearest **CLAUDE.md** |
| Module structure / data flow / behavior | `docs/arch/<subsystem>.md` |
| Algorithm / decision detail | `docs/plans/<subsystem>/<topic>.md` |
| Work still open | `docs/roadmap/<topic>.md` |
| Incident / RCA / refuted approach | `docs/history/<YYYY-MM-DD-slug>.md`, never linked from a top tier |

**Present tense only, everywhere outside `docs/history/`** — docs AND code. Write the rule,
never the story that produced it. A past fact stays only if it changes what someone does
today, rewritten present-tense, keeping a date only as a cutoff to check against. The one
exception is a code comment guarding a re-introduced bug.

`sh scripts/check-docs.sh --all` gates present tense + path resolution; CI runs the same.
The `pt-ok`/`ref-ok` escapes, the tier rationale, and the bar for a history entry:
[docs/docs-discipline.md](docs/docs-discipline.md).

## Environment

Windows 11 · **PowerShell primary** (Bash tool available for POSIX scripts) · git
`autocrlf=true` · every file plain **UTF-8, no BOM** — no smart quotes or em-dashes that a
tool can mangle.
