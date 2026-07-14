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
├── docs/                monorepo-wide docs (e.g. refactor-audit-*)
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
  update the docs tier the change belongs to (below).

## Docs discipline (where a change is written down)

Update the tier that matches what changed — this is what keeps `CLAUDE.md` thin:

| Changed | Write it in |
| --- | --- |
| A rule / command / constraint | the nearest **CLAUDE.md** (super-root here, else the product's) |
| Module structure / data flow / behavior | `docs/arch/<subsystem>.md` — high-level map (crates, files, flow) |
| Implementation detail / algorithm / decision | `docs/plans/<subsystem>/<topic>.md` — deep-dive reference |

`docs/arch/` is the "read this instead of re-exploring source" tier; `docs/plans/` holds
permanent deep-dive references (column rationale, invariants, tuning constants, design
decisions), **not** throwaway plans.

## Environment

Windows 11 · **PowerShell is the primary shell** (a Bash tool is available for POSIX
scripts — each takes its own syntax) · git `autocrlf=true`.

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
