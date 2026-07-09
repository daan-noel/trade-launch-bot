# Monorepo Structure Plan — `hunter` / `forge`

Target folder + naming layout for the two-product monorepo. Purely about **where files
live and what they're called** — no build/behavior redesign (that's
`executor-redesign-plan.md` for the write side and `ingest-redesign-plan.md` for the read
side). This is the layout to converge on; it renames and reshuffles. The crate-graph change
it anticipates is the split of `shared/` into **two totally separate stacks** —
`shared/executor/` (write: intents → chain) and `shared/ingest/` (read: chain → events),
each a `core/` engine + per-venue leaf, with **no crate crossing the line** between them.
The two redesign plans own the internals; this file just names the target folders so the
rename map is complete.

## Products

| Folder | Was | Purpose |
| --- | --- | --- |
| `hunter/` | `meme-trading` | Trade **others'** tokens with defined strategies for profit (market **taker**). |
| `forge/`  | `solana-launch-platform` | Create **our own** tokens + self-trade / make volume for profit (market **maker**). |

Names are venue- and quote-agnostic on purpose — we go multi-launchpad (pump.fun today; Bonk,
Raydium, Meteora later) and multi-quote (SOL today; USDC later). The stable identity is the
**behavior** (hunt vs. forge), never the venue.

---

## The tree

```
bot/                                # monorepo root
├── Cargo.toml                      # the ONE [workspace]
├── CLAUDE.md                       # monorepo conventions; points into each product
├── docs/                           # cross-product docs (this file, migration plan)
├── scripts/                        # cross-product tooling only
│
├── shared/                         # product-agnostic — TWO totally separate stacks; no crate crosses the line
│   ├── executor/                   # WRITE: intents → chain  (never pulls gRPC/proto)
│   │   ├── core/                   #   crate: executor-core    — venue-AGNOSTIC engine: send/sign/confirm/nonce/tip/retry/sim + Venue trait
│   │   └── pumpfun/                #   crate: executor-pumpfun — pump.fun builders + pricing + own protocol consts (deps executor-core)
│   │
│   └── ingest/                     # READ: chain → events  (never pulls signing/keystore)
│       ├── core/                   #   crate: ingest-core      — venue-AGNOSTIC engine: transport/pipeline/reconnect + Decoder trait + neutral IngestEvent; provider = config
│       ├── pumpfun/                #   crate: ingest-pumpfun   — pump.fun decoders + own protocol consts (deps ingest-core)
│       └── websocket/              #   crate: ingest-websocket — stub second transport (only if needed)
│
├── hunter/                         # trade others' tokens
│   ├── core/                       # crate: hunter-core            (was trading_core)
│   ├── live/                       # crate: hunter-live   → Docker → EC2
│   ├── lab/                        # crate: hunter-lab    (analysis; lake lives here)
│   ├── frontend/                   # two vite apps (live/lab) over shared src
│   ├── migrations/
│   ├── scripts/                    # e.g. db-incremental-sync.ps1
│   └── docs/{arch,plans}/          # was @arch / @plans + loose *-plan.md
│
├── forge/                          # launch own tokens, make volume
│   ├── core/                       # crate: forge-core             (was platform-core)
│   ├── live/                       # crate: forge-live             (was slp-live)
│   │   └── src/ingest/             # host adapter (was the ingest-host crate) — a module,
│   │                               #   like hunter/live/src/ingest/. NOT a separate crate.
│   ├── lab/                        # crate: forge-lab              (was slp-lab)
│   │   └── src/lake/               # cold tier (was the lake crate) — a module, like
│   │                               #   hunter/lab/src/lake. Heavy deps stay out of live
│   │                               #   via the BIN boundary, not a crate. NOT a crate.
│   ├── launcher/                   # crate: forge-launcher — the ONE product-owned crate
│   │                               #   that isn't `*-core`. Live-only execution/signing
│   │                               #   domain (create/fund/bundle/keystore). Earns a crate
│   │                               #   on the isolation-seam clause, NOT on 2 consumers —
│   │                               #   see rule 2. Hunter has no equal: its trader is a
│   │                               #   ~45-line module (taker, not maker).
│   ├── frontend/
│   ├── migrations/
│   ├── idl/
│   └── docs/{arch,plans}/
│
└── deploy/                         # ONE folder per shipped artifact — build-machine + EC2
    ├── hunter-live/                # Dockerfile + compose.yml + nginx.conf
    ├── hunter-lab/                 # compose.yml (local only)
    ├── forge-live/
    └── forge-lab/
```

Only `live` (hunter or forge) links the ingest stack, and that stack lives in `shared/`.
So forge does exactly what hunter does: **`forge/live` depends on `shared/ingest/core` +
`shared/ingest/pumpfun` directly**, and the mapping code (the old `ingest-host` crate's
`consumer.rs` / `map.rs` / `pumpfun.rs`) collapses into `forge/live/src/ingest/` as a
**module**, not a standalone crate. Both products then read identically: shared ingest stack +
a per-product `src/ingest/` bridge in the live bin. `lab` links neither.

There are two things called "ingest," and they land differently for the *same* reason:
the `shared/ingest/` stack (`core` engine + `pumpfun` decoder) stays crates because **both**
products' `live` bins consume them — two consumers = a crate. The per-product **mapping** was
a single consumer, so it becomes a module. "Ingest is only for live" doesn't make the mapping
a crate; the shared half is shared, the live-specific half is a module.

`forge/lake` follows the *single-consumer* half of that rule and becomes a **module inside
`forge/lab`** too — matching `hunter/lab/src/lake`. The subtle part: the load-bearing
partition (DuckDB/arrow/parquet/rayon **never** reach EC2) is enforced by the **bin
boundary**, not the crate boundary. `forge-live` doesn't depend on `forge-lab`, so a
`lab`-only *module* keeps those heavy C-deps out of `cargo tree -p forge-live` exactly as a
`lab`-only *crate* would — the crate adds zero partition safety. Keep `lake` a crate **only**
if you specifically want the DuckDB compile-isolation seam or a standalone parity test that
compiles without the `lab` HTTP shell; that is the entire case, and it is a convenience, not
a correctness, argument.

---

## The rules that make it work (more important than the tree)

1. **Root has exactly four kinds of thing:** products (`hunter/`, `forge/`), `shared/`,
   `deploy/`, and repo meta (`docs/`, `scripts/`, `Cargo.toml`, `CLAUDE.md`). A new product =
   one new top-level folder with the same skeleton. Nothing else lands at root — this is the
   fix for the current root sprawl (loose `*-plan.md`, `nginx/`, `run.bat`, `*.pem`, etc.).

2. **Every product has the same skeleton:** `core / live / lab / frontend / migrations /
   docs`. Learn one product, you know both. Extra crates (`launcher`) are allowed but obey
   "one folder = one crate = one responsibility."

   **A crate is justified by ≥2 consumers or a deliberate compile/test-isolation seam —
   never by a folder deserving a name.** Single-consumer code that ships in one bin is a
   **module**, not a crate (`live/src/ingest/`, `lab/src/lake/`). The `live`/`lab` dep
   partition is guaranteed by the **bin boundary** — `live` never depends on `lab` — so a
   `lab`-only module is exactly as safe as a `lab`-only crate; the crate buys only optional
   compile isolation, not partition safety. This is why the `shared/ingest/` stack stays
   crates (both products' `live` link them) while `ingest-host` and `lake` become per-product
   modules.

   Applying the rule to the whole roster produces one clean shape — **per product: exactly
   one `*-core` crate (the ≥2-consumer data layer both bins link) + the `live`/`lab` bins +
   single-consumer modules.** Cross-product reuse lives in `shared/`'s two stacks
   (`executor/{core,pumpfun}`, `ingest/{core,pumpfun}`), never duplicated into a product. The
   **only** product-owned crate that breaks the symmetry is **`forge/launcher`**, and it earns
   its exception on the *isolation-seam* clause, not the 2-consumer one:
   - It **can't fold into `forge/core`** — `core` is solana-free and ships to **both** bins;
     `launcher` pulls `executor-pumpfun`/`solana-sdk`/`aes-gcm`/`zeroize`/`reqwest` (execution +
     **signing + crypto**), which must never reach the analysis box.
   - It's a **large, security-sensitive, live-only** surface (keystore, funding, bundle
     composition, manage ladders) whose pure logic you want to unit-test and compile without
     booting the live bin's HTTP + tokio + gRPC runtime — the textbook isolation seam.
   - **Hunter has no equal.** Its execution analog `live/src/trader` is a ~45-line module
     over shared `executor-pumpfun` (hunter is a *taker*; forge is a *maker* that creates/funds/bundles).
     So the asymmetry is a real domain difference, not structural drift. If hunter ever grows
     a comparable live-only execution domain, extract one the same way — otherwise don't.

   Corollary — things that are **NOT** crates despite looking like candidates: `hunter`'s
   `lake` (1.3k-line `lab` module, single consumer — the precedent for demoting `forge/lake`),
   and the `swing_1`/`tpsl_sniper_*` strategies that appear under `core` **and** both bins —
   that's correct layering (shared logic in the `*-core` crate, per-bin execution vs.
   simulation adapters in the bins), not duplication to extract.

3. **Folder name ≠ package name.** Folders stay short (`hunter/live`); Cargo packages carry
   the product prefix (`hunter-live`, `forge-live`). This permanently kills the `live`/`lab`
   name collision and makes `cargo run -p hunter-live` self-describing.

4. **Dependency arrows point one way:** `shared/` → nothing product-specific, ever. Products
   adapt shared crates via their own host adapters (keep the existing `live/src/ingest/`
   bridge pattern). `lab` never appears in a `live` dep graph — which is *also* what keeps
   the lake stack (DuckDB/arrow/parquet) off EC2, since `lake` lives inside `lab`. Guard with
   `cargo tree -p *-live`.

5. **Extensibility lives in the names, not extra nesting** — and each axis has a
   right-sized seam (smallest first):
   - New **provider** (Helius → Triton/Shyft; same Yellowstone gRPC) → **config** inside
     `shared/ingest/core` (endpoint · auth · capability flags). No new crate.
   - New **launchpad** (`bonk`, `raydium`) → up to two leaf crates, add only the side you
     use: `shared/executor/<venue>` (deps `executor-core`) if you **trade** it, and/or
     `shared/ingest/<venue>` (deps `ingest-core`) if you **watch** it. Each venue crate is
     self-contained (its own protocol consts) — no engine duplication, no shared kernel.
   - New **quote** (USDC…) → a **config dimension inside** a venue crate, not a folder.
     Structure encodes *venue*; config encodes *quote*.
   - New **transport** (WebSocket) → a `shared/ingest/<transport>/` sibling to `core`
     (only if you actually switch; the stub exists).
   - New **chain** (EVM…) → its own engine under `shared/ingest/` emitting the same neutral
     `IngestEvent`, so the DB/analytics stay unified — the engine internals don't cross chains.

6. **`deploy/` = deployable units, not technology.** Four artifacts, four folders, each
   self-contained (Dockerfile, compose, nginx). Build context stays the repo root (required
   anyway — live images pull `shared/` + product crates). "What do we ship and how" is one
   `ls`. **Build-time ≠ deploy-time** — see below; EC2 never checks out the repo.

7. **Docs are two-tier:** product deep-dives inside the product (`hunter/docs/arch`),
   monorepo-wide stuff at root `docs/`. Drop the `@` prefixes — `CLAUDE.md` `@`-imports can
   point at any path; the prefix was only a root-sort hack this structure removes.

---

## Rename / move map (from current state)

| From | To |
| --- | --- |
| `meme-trading/trading_core` | `hunter/core` (pkg `hunter-core`) |
| `meme-trading/live` | `hunter/live` (pkg `hunter-live`) |
| `meme-trading/lab` | `hunter/lab` (pkg `hunter-lab`) |
| `meme-trading/ingest-websocket` | `shared/ingest/websocket` (pkg `ingest-websocket`) |
| `meme-trading/frontend-react` | `hunter/frontend` |
| `meme-trading/{@arch,@plans,*-plan.md}` | `hunter/docs/{arch,plans}` |
| `meme-trading/scripts` | `hunter/scripts` |
| `shared/pump-trader` | split → `shared/executor/core` (engine, pkg `executor-core`) + `shared/executor/pumpfun` (venue, pkg `executor-pumpfun`) — see `executor-redesign-plan.md` |
| `shared/ingest-laserstream` | split → `shared/ingest/core` (engine, pkg `ingest-core`) + `shared/ingest/pumpfun` (venue decoder, pkg `ingest-pumpfun`) — see `ingest-redesign-plan.md` |
| `solana-launch-platform/crates/platform-core` | `forge/core` (pkg `forge-core`) |
| `solana-launch-platform/crates/slp-live` | `forge/live` (pkg `forge-live`) |
| `solana-launch-platform/crates/slp-lab` | `forge/lab` (pkg `forge-lab`) |
| `solana-launch-platform/crates/lake` | fold into `forge/lab/src/lake` module (mirror hunter) |
| `solana-launch-platform/crates/launcher` | `forge/launcher` (pkg `forge-launcher`) |
| `solana-launch-platform/crates/ingest-host` | fold into `forge/live` host adapter (mirror hunter) |
| `solana-launch-platform/frontend-launch` | `forge/frontend` |
| `{meme,slp}` Dockerfiles + compose + `nginx/` | `deploy/{hunter,forge}-{live,lab}` |

Use `git mv` for every move so history follows. Update `[workspace] members`, all
`{ workspace = true }` / path deps, `[[bin]]` names, deploy `--bin` flags, and every doc
`@`-import path in the same commit as each move.

---

## Deploy: build once, ship an image, pull on EC2

EC2 (2vCPU/4GB) must **never build** — Rust release builds OOM/thrash it. Build-context =
repo root is a *build-machine* requirement only; deploy-time needs just the image + a compose
file + `.env`.

```
Workstation/CI                     Registry (ECR)              EC2
─────────────                      ─────────────              ───
docker compose build (ctx=root) →  hunter-live:<sha>  ────→   docker compose pull && up -d
docker compose push                                            (only compose.yml + .env here)
```

One compose file per artifact carries **both** keys so it builds on the workstation and
deploys on EC2 unchanged:

```yaml
services:
  hunter-live:
    image: <acct>.dkr.ecr.<region>.amazonaws.com/hunter-live:${TAG}
    build:                     # used ONLY by `compose build` on the workstation
      context: ../..           # repo root
      dockerfile: deploy/hunter-live/Dockerfile
```

- Workstation: `docker compose build && docker compose push`
- EC2: `docker compose pull && up -d` — Docker sees `image:`, ignores `build:`, touches no
  source. You `scp` only `compose.yml` + `.env` to the box (ECR = IAM auth, no extra creds).

`deploy/*-lab/` compose files omit the registry (local build/run only; lab never ships).

---

## Security cleanup (do this during the move)

Move **out of the repo entirely**, never re-commit:
- `meme-trading/aws-ec2-key.pem` → `~/.ssh/`
- `solana-launch-platform/keystore/`, `wallet-backups/` → offline / OS keychain

A restructure is exactly when a `.gitignore` slip leaks a key. Verify with `git status` after
every move phase.

---

## Deliberately NOT doing (yet)

- **`shared/lake-core`:** both products' lakes are now modules (`hunter/lab/src/lake`,
  `forge/lab/src/lake`) — symmetric, no longer a crate on either side. If they converge on the
  same Parquet/DuckDB tooling, promote a shared `shared/lake-core` crate **then** (≥2
  consumers = the crate rule earns it) — not preemptively.
- **`forge/orchestrator` + the executor behavior redesign** (variant catalog, `Operation`/`Plan`,
  personas, auditor): owned by `executor-redesign-plan.md`. This file names the target folders
  (`shared/executor/{core,pumpfun}`) but does not design their internals.
- **The ingest behavior redesign** (`Decoder` trait, neutral `IngestEvent`, provider-as-config
  with pluggable `Auth` + capability flags): owned by `ingest-redesign-plan.md`. This file names
  the target folders (`shared/ingest/{core,pumpfun,websocket}`) but does not design their
  internals. The two stacks are **totally separate** — no crate in `shared/executor/` depends on
  anything in `shared/ingest/` or vice-versa; each is an independent, self-contained drop-in.
