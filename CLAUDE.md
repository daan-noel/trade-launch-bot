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
forward slashes, always the same dir**, else DuckDB rebuilds at ~20 GB a copy.
**Never re-enable `rustc-wrapper = "sccache"`** — measured at 0 cache hits and 19 failures
here, with an error that blames the sysroot. That same error also means "a previous build
died mid-flight"; `cargo clean` + one uninterrupted run before blaming anything else.
**A build OOM does not look like one**: 16 cores against a fixed ~64 GB commit limit means
cargo's default `-j` dies as `STATUS_STACK_BUFFER_OVERRUN` in some unrelated small crate —
hence `[build] jobs = 4` and `[profile.dev] debug = "line-tables-only"`. All of it:
[docs/build-and-env.md](docs/build-and-env.md).

## Rules for both products

- **Single source of truth.** Before adding a constant, formula, SQL fragment, type, or
  column list, search for an existing one and reuse it. Watch for the same fact defined
  twice; where decoupling makes duplication unavoidable, add a no-DB guard test asserting
  the copies stay equal.
- **The finding sets the metric — never the reverse.** Implement a derived rule in the terms
  it was derived in. Reuse an existing metric only when it expresses that term *exactly*; when
  the nearest one differs in time basis (slot vs second), granularity (per-print vs per-tick),
  or unit, **extend the metric system** rather than approximate — an approximation is a silent
  refutation: it changes the number that validates the rule, so what ships stops replicating
  what justifies it. Defer the extension and the rule stays unshipped, gap recorded.
- **Extend it, don't complicate it.** One metric = one quantity, named for what it measures.
  One group = one subject on one time basis (`m_flow_window` = flow, windowed) plus that
  basis's params. A new metric joins the group whose subject and basis it shares; a new group
  needs a subject or a basis that has none — never a second group for a family that exists,
  never an unrelated quantity folded into one for convenience. Add the smallest thing that
  carries the finding: a metric before a group, a group before a new window kind. A metric you
  cannot state in one line — what it measures, unit, basis — is not ready to add. Extension
  cost: [hunter/CLAUDE.md](hunter/CLAUDE.md#hot-path-landmines),
  [metrics-reference](hunter/docs/plans/strategies/metrics-reference.md).
- **A metric ships explained, and explained once.** Every metric carries one definition —
  what it measures, unit, time basis, window — written at the point it is defined and
  rendered into the UI from that same text. Adding or changing a metric updates that
  definition in the same commit: no second copy of the formula, no UI label that says
  something the code does not, no meaning that lives only in a doc.
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

`scripts/check-docs.sh` gates present tense + path resolution. The pre-commit hook runs it
over the STAGED files and CI runs `--all` over the tree, so a session needs neither — reach
for `sh scripts/check-docs.sh --all` only to sweep a docs change that spans many files.
The `pt-ok`/`ref-ok` escapes, the tier rationale, and the bar for a history entry:
[docs/docs-discipline.md](docs/docs-discipline.md).

## Environment

Windows 11 · **PowerShell primary** (Bash tool available for POSIX scripts) · git
`autocrlf=true` · every file plain **UTF-8, no BOM** — no smart quotes or em-dashes that a
tool can mangle.
