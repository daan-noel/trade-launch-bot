# CLAUDE.md — forge

A venue- and non-SOL-quote-generalized Solana **launch + trading + analytics** platform
(sibling to `../hunter` in the shared `Bot/` monorepo — one Cargo workspace). Starts with
pump.fun token creation; grows into multi-launchpad / multi-quote / multi-wallet.

Read [../CLAUDE.md](../CLAUDE.md) first for the monorepo-wide rules (SSOT,
backend-latency-first, EC2 constraint, `.env`, docs discipline). This file is
forge-specific only.

- **Roadmap (open phases & tasks):** [docs/roadmap-plan.md](docs/roadmap-plan.md)
- **Decisions (ADR):** [docs/decisions.md](docs/decisions.md) · **Overview + layout:** [README.md](README.md)

## Status (one-line — detail in the roadmap)

Foundation + Phase-2 launcher + wallet-pool (Phases 1–4) **done**: create/dev-buy/keystore,
atomic Jito bundles with tip-floor + drop re-bid + multi-region submit, feed-based landing
confirmation, wallet-pool lifecycle, dust sweep, operator transfers, and the React operator
dashboard. **Next:** Phase 3 live trading (buy/sell executor + feed-based sell-confirm).
Phase 4 lab/lake is scaffold-only. What shipped, phase by phase: [docs/history/shipped-phases.md](docs/history/shipped-phases.md).

## Architecture — 5 crates, 2 bins

`trading_core` is **NOT** reused (its SOL/pump domain is the thing being redesigned); the
only SSOT lifted by copy is the unit consts (generalized to quote/base). The pump.fun IDLs
are the single canonical copy at `shared/executor/pumpfun/idl/`. forge's bins are
`forge-live` / `forge-lab` (not workspace `default-members` — build/run with `-p`).

| Crate | Kind | lib / dir | Role | Ships to |
| --- | --- | --- | --- | --- |
| `forge-core` | lib | `platform_core`, `forge/core/` | data layer: config, models, storage, repositories, `venue/` trait. **solana-free** (addresses are TEXT/String) | both |
| `forge-orchestrator` | lib | `orchestrator`, `forge/orchestrator/` | **the brain:** `Operation`/`Plan` model + provider catalog gate + persona/disguise + fingerprint audit + dry-run, over the executor stack | **forge-live** |
| `forge-launcher` | lib | `launcher`, `forge/launcher/` | create / dev-buy / bundle / confirm / wallet-pool / funding / manage; routes every write through the orchestrator gate, executes via `shared/executor` | **forge-live** |
| `forge-live` | **bin** | `forge/live/` | ingest (`forge/live/src/ingest`) + launcher + trading + thin HTTP → **EC2** | — |
| `forge-lab` | **bin** | `forge/lab/` | lake (`forge/lab/src/lake`, scaffold) + sweeps/backtests/analytics HTTP → **workstation** | — |

**In-crate modules, not crates:** `lake` → `forge/lab/src/lake` module · `ingest-host` →
`forge/live/src/ingest` module. **Shared drop-in crates** (in `Bot/shared/`, consumed by
both products): `shared/executor/{core,pumpfun}` (write stack; dep key `pump-trader`, lib
`pump_trader`), `shared/ingest/{core,pumpfun}` (read stack; dep key `ingest-laserstream`,
lib `ingest_laserstream`), `shared/http-auth`.

**Read `docs/arch/` instead of re-exploring source. Deep-dive detail lives in `docs/plans/`.**

| Doc | Covers |
| --- | --- |
| [docs/arch/architecture.md](docs/arch/architecture.md) | crate/bin map, both `main.rs` wiring, module topology, dep partition |
| [docs/arch/database.md](docs/arch/database.md) | `platform_core` schema, hypertables, repos, `venue/` trait, status enums |
| [docs/arch/launcher.md](docs/arch/launcher.md) | launch flow (create→dev-buy→bundle→submit→confirm), wallet-pool lifecycle, tip re-bid |
| [docs/arch/orchestrator.md](docs/arch/orchestrator.md) | `Operation`/`Plan`, provider gate, persona/disguise, fingerprint audit, dry-run |
| [docs/arch/ingest.md](docs/arch/ingest.md) | `forge/live/src/ingest` host adapter over `shared/ingest`, bundle-landing watcher |
| [docs/arch/lake.md](docs/arch/lake.md) | `forge/lab/src/lake` cold tier (scaffold today: `schema.rs` only) |
| [docs/arch/frontend.md](docs/arch/frontend.md) | operator dashboard: app shell, RTK-Query, feature pages, SSE |

**Dep partition (load-bearing — enforce from the scaffold):**

- `forge-live` must NOT pull `duckdb`/`arrow`/`parquet` (the lake stack). Verify
  `cargo tree -p forge-live` (rayon *is* present, but only as a Solana transitive via the
  executor — not the lake crate).
- `forge-lab` must NOT pull the executor / ingest / `tonic` / solana stack. Verify
  `cargo tree -p forge-lab`.

## Schema conventions (locked)

- **Amounts** name their unit as a SUFFIX: `amount_quote` / `amount_base` (exact BIGINT base
  units), reserves `reserve_quote` / `reserve_base`. The unit is the *referenced quote/base
  asset*, **never** a hard-coded lamport — native SOL is just the `quote_assets` row with
  `is_native, decimals 9`. (Evolution of hunter's SOL/lamports rule; `*_lamports` dual-vocab
  was **rejected** — see ADR D1.)
- **Prices are RAW RATIOS** stored/aggregated (`amount_quote / amount_base`,
  decimals-agnostic). Decimals + USD are applied **only in derived views** (`trades_priced`,
  `token_overview`) — never stored. Ratios keep `_price`/`_pct`.
- **SSOT keys:** `mint_address`, `launchpad_id`, `quote_asset_id`, `market_id`. A new
  launchpad or quote asset is a new dimension **ROW**, never a schema migration.
- **Metadata SSOT:** token identity (name/symbol/uri) lives in ONE place — a
  `metadata_templates` row; `launch_templates.metadata_template_id` (FK, `ON DELETE SET
  NULL`) references it; `execute_launch` (and `probe`) resolve name/symbol/uri from that row
  at create time. NEVER inline name/symbol/uri in `launch_templates.params` or a launch
  request — a per-launch override is a *different* `metadata_template_id`, not free text.
  `image_uri` is nullable. Migration `0007`; the frontend `Metadata` dropdown is the single
  authoring surface.
- **CHECK-constrained vocabularies are enums, never loose strings:** `LaunchStatus` /
  `BundleStatus` / `WalletRole` / `WalletStatus` (`platform_core::models::status`) own the
  SQL `CHECK` values; each `as_str()` must equal the CHECK (launch/bundle → `0006`, wallet
  role → `0002`, wallet status → `0004`; roundtrip tests pin the strings), same pattern as
  `MarketKind`. A new value is a code + CHECK edit — never a bare string literal.
- **Extensibility via rows, not columns**; interned small-int dimensions (`quote_assets`,
  `launchpads`) + `wallet_dict` (soft ref, no FK on the hot insert; read paths LEFT JOIN with
  a COALESCE fallback). Hot tables (`raw_txs`, `trades`) are **hypertables** with declarative
  compression + retention; the dedup key IS the PK.

## Security (locked)

**No secret material in Postgres.** `managed_wallets.key_ref` is a *reference* only (keystore
path / KMS id / envelope-encrypted blob), marked `#[serde(skip_serializing)]`. Signing goes
through the executor's `Arc<dyn Signer>`. Keystore backend (ADR D3) = envelope-encrypted file
+ pluggable KEK trait (env/passphrase now → AWS KMS later; ed25519 signs in-process).

**HTTP: fail-closed bearer gate on mutating routes.** `forge-live` moves treasury SOL, so —
like hunter — every POST/PUT/DELETE/PATCH requires `Authorization: Bearer $API_AUTH_TOKEN`;
safe reads + preflight pass. `API_AUTH_TOKEN` is **required** for the live HTTP server to boot
(a missing token refuses to serve). The gate is the shared `http-auth` crate (`ApiAuth` +
`require_bearer_auth`) — ONE copy across hunter live/lab + forge live. In deploy, nginx bridges
the operator's Basic-auth login to the bearer token via envsubst; the token never reaches the
browser.

## Commands

```powershell
# Run from the monorepo root (Bot/) or this folder; the workspace is Bot/Cargo.toml.
docker compose up -d                 # local Postgres + TimescaleDB (host port 5556); adds forge-live service
sqlx migrate run                     # apply migrations
cargo check -p forge-live -p forge-lab   # typecheck the forge bins (use --target-dir target-check if a bin is running)
cargo tree -p forge-live               # dep-partition check (no duckdb/arrow/parquet)
cargo tree -p forge-lab                # dep-partition check (no executor/ingest/tonic)
cargo run -p forge-live                # LIVE box: needs Postgres + Helius gRPC/keys; HTTP :8230
cargo run -p forge-live -- wallet-encrypt <keypair.json> <key_ref>  # envelope-encrypt dev wallet (needs WALLET_KEYSTORE + LAUNCHER_KEK_PASSPHRASE)
cargo run -p forge-live -- wallet-verify <key_ref> <expected_address>  # restore runbook: confirm a keystore blob decrypts to the expected pubkey
cargo run -p forge-live -- create-alt <authority_key_ref>  # provision the persistent launch ALT (spends real SOL); paste output into PUMP_LAUNCH_ALT
cargo run -p forge-lab                 # ANALYSIS box: needs Postgres only; NO keys / NO gRPC; HTTP :8240
```

**DB-gated tests** self-skip unless `PLATFORM_TEST_DATABASE_URL` points at a **dedicated
throwaway** DB (they run their own migrations). Ports: DB **5556**, live **8230**, lab **8240**.

## Definition of done (forge-specific)

- `cargo check -p forge-live -p forge-lab` clean; clippy on touched code; test when logic changed.
- Dep partition still holds (`cargo tree -p forge-live` / `-p forge-lab`).
- Stayed in the owning crate; no secrets in code.
- **Docs — update the tier that changed** (see [../CLAUDE.md](../CLAUDE.md) docs discipline):
  rules/commands → this file; module structure/data-flow → `docs/arch/<subsystem>.md`;
  algorithm/decision detail → `docs/plans/<subsystem>/<topic>.md`; ADR → `docs/decisions.md`;
  open phases/tasks → `docs/roadmap-plan.md`; shipped narrative / incidents →
  `docs/history/`, never linked from here or from `docs/arch/`. **`docs/arch/` and this
  file are present-tense only** — see *Present tense only* in [../CLAUDE.md](../CLAUDE.md)
  for the test and what it forbids.
