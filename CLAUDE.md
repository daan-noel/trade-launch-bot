# CLAUDE.md — Bot monorepo (super-root)

Rules and layout that hold for the **whole `Bot/` workspace**. Product-specific
guidance lives one level down and loads when you work in that subtree:

| Working in… | Also read |
| --- | --- |
| `hunter/**` | [hunter/CLAUDE.md](hunter/CLAUDE.md) — meme-coin trading bot |
| `forge/**` | [forge/CLAUDE.md](forge/CLAUDE.md) — launch + trading + analytics platform |
| `shared/**` | both — these crates feed **both** products; a change ripples to each |

> **Docs are tiered to save context.** A `CLAUDE.md` is paid on every session, so it
> stays a thin **index + hard rules**. Explanations live in `docs/` and cost nothing
> until Read. See *Docs discipline* below.

## Layout

One Cargo `[workspace]` (`resolver = "1"`, root [Cargo.toml](Cargo.toml)) spanning two
products plus their shared crates:

```text
Bot/
├── CLAUDE.md            this file (super-root)
├── Cargo.toml           the ONE [workspace]; default-members = hunter/live, hunter/lab
├── RUN.md               how to run the stacks locally
├── docs/                monorepo-wide docs (refactor-plan.md, history/)
├── deploy/             per-family docker/nginx (deploy/hunter, deploy/forge)
├── shared/              standalone drop-in crates — consumed by BOTH products
│   ├── executor/{core,pumpfun}   venue-agnostic write stack + pump.fun venue (was pump-trader)
│   ├── ingest/{core,pumpfun}     venue-agnostic read stack + pump.fun venue (was ingest-laserstream)
│   └── http-auth                 fail-closed bearer gate for real-money bins
├── hunter/              product: meme-coin trading bot (hunter-live / hunter-lab)
└── forge/               product: launch + trading + analytics (forge-live / forge-lab)
```

**Build/run targeting** — a bare `cargo build` at the root builds only the hunter bins
(`default-members`). Everything else is `-p`:

```powershell
cargo build                       # hunter-live + hunter-lab only
cargo check -p forge-live         # a forge crate
cargo check -p executor-pumpfun   # a shared crate
```

Use `--target-dir target-check` when a bin `.exe` is running (it locks `target/`).
**Always pass the ABSOLUTE path, with FORWARD slashes** —
`--target-dir "C:/Users/User/Documents/Bot/target-check"` — so every crate/CWD shares the
one dir instead of spawning a per-subdir copy (each copy re-compiles the giant
`libduckdb-sys` amalgamation, ~20 GB / minutes each). Forward slashes are mandatory:
cargo accepts them on Windows, and they survive the Bash tool. A **backslash** path
(`C:\Users\...`) works in PowerShell but the Bash tool eats the `\` escapes, collapsing it
to `C:UsersUserDocumentsBottarget-check`, which cargo then creates as a junk drive-relative
folder in the CWD.

A global **sccache** (`~/.cargo/config.toml`, `rustc-wrapper = "sccache"`) caches *rustc*
output across all target dirs and across `cargo clean`. It does **not** cache the DuckDB
C++ objects: those are built by cc-rs/`cl.exe`, and MSVC caching needs `cl.exe` on PATH
(a VS Developer prompt) plus `CC = CXX = "sccache cl.exe"`. Outside a dev prompt cc-rs
finds `cl.exe` by full VS path and a bare-`cl.exe` override fails with `ToolNotFound` and
breaks every MSVC build — so leave `CC/CXX` unset unless you always build from vcvars.
To avoid re-compiling DuckDB, prefer **not wiping** the DuckDB build (keep the persistent
`target-check`) over re-running it.

## Shared crates (`shared/`) — no workspace deps

`shared/executor/*`, `shared/ingest/*`, and `shared/http-auth` are **standalone
drop-in libraries**: they do NOT depend on any product crate. Both products consume
them as intra-workspace deps. Because a change hits hunter **and** forge, treat their
public APIs as contracts — verify both consumers before changing a signature, and keep
each crate's own decoupled vocabulary (don't leak a product's domain names into them).

## Rules for both products

- **Single source of truth.** Before adding a constant, formula, SQL fragment, type, or
  column list, search for an existing one and reuse it — never copy-paste a fact that
  must stay consistent. Actively watch for **SSOT violations** (the same fact defined
  twice that can silently drift). When duplication is genuinely unavoidable (deliberate
  crate decoupling), add a guard test that asserts the copies stay equal — prefer a
  no-DB guard so it runs on every `cargo test`.
- **Backend latency first.** Both products run hot ingest/strategy/sell-confirm paths.
  No blocking I/O, `.await`-on-lock, per-event alloc, redundant RPC/DB round-trips, or
  lock contention on a hot path. **Notify over poll**; sell-confirm stays feed-based.
- **Spend Helius sparingly.** Helius RPC calls (and credits) are a paid, capped budget.
  Before adding or keeping any Helius API call, prove it's worth it: prefer the existing
  push/stream feeds, cache, and batch requests over polling; reuse a value already in
  hand instead of re-fetching; never call per-event on a hot path. If an RPC call doesn't
  change a decision, delete it.
- **Modular & concise.** handler → service → repo; one responsibility per module. Short
  answers; non-trivial plans go to a `*-plan.md` file, not inline.
- **Deploy target is a 2vCPU / 4GB EC2 box** (IO-bound, RAM-constrained). Only the
  `*-live` bins + their required shared crates ship there; the `*-lab` bins +
  DuckDB/arrow/parquet/rayon stay on the workstation. Don't raise cache caps/TTLs or add
  a pool on the server to "make analysis easier" — sync to the workstation instead.
- **No secrets in code.** `.env` is gitignored; keep it in sync with `.env.example`, and
  **back up first** (`Copy-Item .env .env.backup -Force`) before applying new keys.
- **Definition of done (shared shape):** `cargo check` clean on the touched bins; clippy
  on touched code; test when logic changed; stay in the owning crate; no new warnings;
  update the docs tier the change belongs to (below) **and leave it present-tense** — a
  paragraph your change made inaccurate is part of that change, not a later cleanup.

## Docs discipline (where a change is written down)

Update the tier that matches what changed — this is what keeps `CLAUDE.md` thin:

| Changed | Write it in |
| --- | --- |
| A rule / command / constraint | the nearest **CLAUDE.md** (super-root here, else the product's) |
| Module structure / data flow / behavior | `docs/arch/<subsystem>.md` — high-level map (crates, files, flow) |
| Implementation detail / algorithm / decision | `docs/plans/<subsystem>/<topic>.md` — deep-dive reference |
| Work that is still open | `docs/roadmap/<topic>.md` — deleted or folded into a deep-dive once it lands |
| An incident/RCA, a superseded approach, a research journal | `docs/history/<YYYY-MM-DD-slug>.md` |

`docs/arch/` is the "read this instead of re-exploring source" tier; `docs/plans/` holds
permanent deep-dive references (column rationale, invariants, tuning constants, design
decisions), **not** throwaway plans.

<!-- pt-ok:begin — this section defines the rule, so it quotes the phrasing it forbids -->
## Present tense only (locked — applies to every edit, docs AND code)

**Everything outside `docs/history/` describes what the system does *now*.** Write the
**rule**, never the story that produced it. This is not a style preference: `CLAUDE.md` and
`docs/arch/` are paid on every session, and a paragraph about deleted code is pure cost
that also reads as if it were still true.

**The one test — does this past fact change what someone does today?**

- **Yes → it stays**, rewritten present-tense with the narrative stripped. Real examples:
  *"runs stored before 2026-07-28 were priced at 100 bps — they do not compare to a new
  run"* (that data is still on disk), *"grouped runs before 2026-07-26 carry poisoned
  aggregates — re-run them"*. Keep the date **only** because it is the cutoff someone has
  to check against, never as a timeline.
- **No → it moves to `docs/history/`, or is deleted.** "We used to do Y", "X was removed
  in Phase 7", "fixed on <date> after N hours of outage" — none of that changes an action.

### What that forbids, concretely

| Never write in `CLAUDE.md`, `docs/arch/`, or a code comment | Write instead |
| --- | --- |
| "X was deleted / retired / no longer exists" | describe what *does* exist; if X's absence is load-bearing, say "there is no X" and why re-adding it breaks something |
| An outage narrative — dates, durations, row counts, SOL figures | the invariant it produced, plus the measurement if the *number* is the rule (a threshold, a cost, a speedup) |
| "Phase N" / "step 2" labels from a plan doc | what the code does; a phase number outlives the plan and then points nowhere |
| A pointer to a `.md` that no longer exists | the doc that absorbed it, or nothing |
| "NOTE: the paths below are stale" | fix the paths and delete the note — a doc that warns it is wrong is worse than one that is right |
| A second copy of a tracked doc (e.g. under `_local/`) | one copy; two copies drift and each ends up holding what the other lost |

### `docs/history/` — the escape valve

One file per entry, `<YYYY-MM-DD-slug>.md`, shape: **Symptom** (with the numbers) →
**Cause** (the mechanism) → **Fix** → **The rule this produced** (one line + a link to
where that rule now lives). A `README.md` indexes each history dir.

- **Never linked** from a `CLAUDE.md` or an `arch/` link table. It is a grep target, so it
  costs nothing per session. An `arch/` doc may carry at most one inline link to a
  history entry, where a reader would otherwise ask "why is this odd rule here?".
- **The bar for writing one: the past left a live consequence.** Stored data is now wrong,
  a rule looks arbitrary without the story, or a whole approach was refuted (so nobody
  re-runs it). An ordinary bug fix gets a code comment and **nothing else** — history that
  grows per-commit is the same bloat in a new place.
- Every extraction is a **move**: delete from the source and write to history in the same
  change, so one diff shows both sides. Nothing is lost, ever.

### Code comments

Same rule, one exception. A comment whose job is to stop a future "simplification" from
reintroducing a bug **keeps its cautionary form** — `// \`?\` here used to strand a
wallet's SOL when a token-account close failed` earns its past tense, because deleting it
invites the bug back. That is a regression guard, not history. Everything else is
present-tense: no phase labels, no "previously", no describing code that was removed.
<!-- pt-ok:end -->

## Environment

Windows 11 · **PowerShell is the primary shell** (a Bash tool is available for POSIX
scripts — each takes its own syntax) · git `autocrlf=true`.

**File encoding.** Never write or edit a file with characters that can break — a BOM,
smart quotes/em-dashes pasted from elsewhere, or any non-ASCII punctuation a tool can
mangle. Save/edit as plain **UTF-8, no BOM**. If a file needs non-ASCII content, use
real UTF-8 characters directly, not escaped sequences.

## Env-file editing (`.env` / `.env.example`)

Each product owns its pair (`hunter/.env*`, `forge/.env*`) — there is no root `.env`.
When you touch either file, follow these rules:

- **Preserve the at-a-glance style.** Every var sits under a `# ===== Section (required/optional) =====`
  banner with one short comment above it saying what it is / why it matters; `VITE_*` vars are
  public (baked into the JS bundle) and must never hold a secret. Keep new entries in that exact
  shape so the file stays scannable.
- **Always edit `.env` and `.env.example` together — same keys, same order, comments in both;
  only the values differ.** `.env` gets **real, immediately-usable values**; `.env.example` gets
  **placeholder/example values** (e.g. `{your_helius_api_key_here}`, `change_me`) so copying it
  back over `.env` is a one-step recovery. A key that exists in one file must exist in the other.
- Back up before applying new keys: `Copy-Item .env .env.backup -Force`. `.env` is gitignored —
  never commit real secrets.
