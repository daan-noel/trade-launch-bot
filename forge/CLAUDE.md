# CLAUDE.md

Guidance for Claude Code working in **forge** — a venue- and
non-SOL-quote-generalized Solana launch + trading + analytics platform (sibling to
`../hunter` inside the shared `Bot/` monorepo — one Cargo workspace, no longer
separate git repos). Starts with pump.fun token creation; grows into
multi-launchpad / multi-quote / multi-wallet.

**Full design:** [docs/and-about-the-instructions-shimmying-shore.md](docs/and-about-the-instructions-shimmying-shore.md).
**Phases & tasks:** [docs/roadmap-plan.md](docs/roadmap-plan.md).
**Decisions (ADR):** [docs/decisions.md](docs/decisions.md).
**Overview + layout:** [README.md](README.md). **Analysis pipeline:** [docs/analysis-workflow.md](docs/analysis-workflow.md).

## Status

Data-infrastructure **foundation is complete** (7 phase commits): 6 crates, 2 bins,
migrations `0001` (Domains A–C) + `0002` (Domain D). §9 open decisions are **resolved**
(see the ADR). **Phase-2 launcher:** create (`v1`/`v2` + dev-buy), keystore,
`wallet-encrypt` CLI, launch failure rollback, bundle leg composer (`planned` bundles),
`POST /api/bundles/{id}/execute` (Jito submit), **feed-based bundle-landing
confirmation** (migration `0003`; always-on watcher in `live/main.rs` checks leg
signatures against ingested `trades`, no RPC poll; `GET /api/bundles/{id}` for status).
**Auto-submit** after launch, multi-variant bundle legs, SOL/USD poller. Phase-2b:
ingest round-trip test + dep-partition CI + [`docs/live-verify.md`](docs/live-verify.md)
mainnet checklist.
**Wallet pool (parallel workstream, plan finished & retired — see
[docs/roadmap-plan.md](docs/roadmap-plan.md) or `git show 7f0526f:docs/wallet-pool-plan.md`
for full history):**
Phase 1 done — migration `0004` replaces `managed_wallets.is_active` with an
explicit `status` lifecycle (`generated`/`funded`/`reserved`/`used`/`retired`) +
`funding_source`/`reserved_by_launch_id`/`reserved_at`/`balance_lamports`;
`launcher::wallet_pool` adds batch generation, a balance poller, and a reservation
TTL sweep; `ManagedWalletRepo` adds the atomic `claim_funded` (`FOR UPDATE SKIP
LOCKED`) + `mark_used` transitions. Phase 2 done — `GET /api/wallet_pool` +
`POST /api/wallet_pool/generate`; the Wallet Pool page (list, generate, status
counts, low-pool banner). **Frontend note:** `frontend-launch` was rebuilt as a
React-Router + RTK-Query + Tailwind operator dashboard (`src/app` shell, `src/shared`
ui-kit/store/lib, `src/features/*` pages — e.g. `features/wallets/WalletPoolPage.tsx`,
`features/launches/{LaunchesPage,TokenDetailPage}.tsx`); the old single `App.tsx`/`api.ts`
is gone. New `GET /api/launches` (paged enriched `LaunchListRow`) backs the launched-tokens
list. See `docs/roadmap-plan.md` Phase 5+.
Phase 3 done — launch flow now consumes the pool: dev-wallet dropdown filters to
`funded`, bundler legs are claimed via `claim_funded` (no more template
`bundle_wallet_ids`), token identity is a single `metadata_template_id` choice
(the launch template links one; the console can override it per launch — see
Metadata SSOT below), and `mark_used` for bundler legs moved to `launcher::confirm`'s
landed/dropped/partial outcomes (bundle *planning* also moved outside
`execute_launch`'s failure-reset scope, fixing a pre-existing bug where a
post-create bundle problem could flip an already-succeeded launch to `failed`).
Phase 4 done (wallet-pool plan fully implemented) — `launcher::spawn_dust_sweep`
(hourly: sweeps `used` wallets' balance to a `role=treasury` wallet via a plain
`solana-client` transfer, then retires them); `launcher::run_backup` (opt-in via
`WALLET_BACKUP_DIR`, fires after every generate batch — non-retired keystore
blobs + a full `managed_wallets.json` export, KEK never included); new
`wallet-verify` CLI backs the restore runbook (decrypt + confirm a derived
address before trusting a restored pool).
**Next:** see [docs/roadmap-plan.md](docs/roadmap-plan.md) — Phase 3 live trading.
Wallet-pool Phase 5+ (automated multi-hop funding, fingerprint picker UI, KMS
KEK) is explicitly deferred — see `docs/roadmap-plan.md`'s Phase 5+ Growth section.

## Priorities

High token + trade volume; production runs on a **2vCPU / 4GB EC2** box. Performance
and modularity outrank everything.

- **Backend latency first.** Hot paths (ingest, trade projection, sell-confirm): no
  blocking I/O, `.await`-on-lock, per-event alloc, or redundant RPC/DB round-trips.
  Notify over poll. Sell-confirm stays **feed-based** (no RPC poll on the hot path).
- **Single source of truth.** Before adding a constant, formula, SQL fragment, type,
  or column list, search for an existing one and reuse it. Watch for **SSOT
  violations** — the same fact defined twice that can silently drift.
- **Modular** (handler→service→repo; one job per crate/module) and **concise** (short
  answers; non-trivial plans to a `*-plan.md`).

## Architecture — 6 crates, 2 bins

**Monorepo:** `forge/` is now one product folder inside the `Bot/`
monorepo — a single Cargo `[workspace]` (`resolver = "1"`, root `Bot/Cargo.toml`) shared
with `hunter/`. The two standalone crates (`pump-trader`, `ingest-laserstream`) are
**borrowed from the neutral `shared/` home** (`Bot/shared/…`) as intra-workspace deps
(`{ workspace = true }`), NOT a cross-repo path dep any more. **forge's bins are named
`forge-live` / `forge-lab`** (hunter owns `hunter-live`/`hunter-lab`); they are NOT workspace
`default-members`, so build/run them with `-p forge-live` / `-p forge-lab`. `trading_core` is
**NOT** reused (its SOL/pump domain is the thing being redesigned); only tiny pure SSOT
files were copied (IDLs, unit consts).

| Crate | Kind | Role | Ships to |
| --- | --- | --- | --- |
| `platform-core` | lib | data layer: config, models, storage, repositories, `venue/` trait. **solana-free** (addresses are TEXT/String) | both |
| `ingest-host` | lib | borrowed `ingest-laserstream` events → PG `raw_txs`/`trades` | **forge-live** |
| `launcher` | lib | create / dev-buy / bundle via `pump-trader` | **forge-live** |
| `lake` | lib | Parquet/DuckDB cold tier (sweeps/backtests) | **forge-lab** |
| `forge-live` (`crates/forge-live`) | **bin** | ingest + launcher + trading + thin HTTP → **EC2** | — |
| `forge-lab` (`crates/forge-lab`) | **bin** | lake + sweeps + backtests + analytics → **workstation** | — |

**Dep partition (load-bearing — enforce from the scaffold):**

- `forge-live` must NOT pull `duckdb`/`arrow`/`parquet` (the `lake` stack). Verify:
  `cargo tree -p forge-live` (rayon *is* present, but only as a Solana transitive via
  `pump-trader` — not the lake crate; that's expected).
- `forge-lab` must NOT pull `pump-trader`/`ingest-laserstream`/`tonic`/solana. Verify:
  `cargo tree -p forge-lab`.

## Schema conventions (locked)

- **Amounts** name their unit as a SUFFIX: `amount_quote` / `amount_base` (exact BIGINT
  base units), reserves `reserve_quote` / `reserve_base`. The unit is the *referenced
  quote/base asset*, **never** a hard-coded lamport — native SOL is just the
  `quote_assets` row with `is_native, decimals 9`. (Evolution of hunter's
  SOL/lamports rule; `*_lamports` dual-vocab was **rejected** — see ADR D1.)
- **Prices are RAW RATIOS** stored/aggregated (`amount_quote / amount_base`,
  decimals-agnostic). Decimals + USD are applied **only in derived views**
  (`trades_priced`, `token_overview`) — never stored. Ratios keep `_price`/`_pct`.
- **SSOT keys:** `mint_address`, `launchpad_id`, `quote_asset_id`, `market_id`. A new
  launchpad or quote asset is a new dimension **ROW**, never a schema migration.
- **Metadata SSOT:** token identity (name/symbol/uri) lives in
  ONE place — a `metadata_templates` row. `launch_templates.metadata_template_id`
  (FK, `ON DELETE SET NULL`) references it; `launcher::service::execute_launch`
  (and `probe`) resolve name/symbol/uri from that row at create time. NEVER inline
  name/symbol/uri in `launch_templates.params` or a launch request — the per-launch
  override is a *different* `metadata_template_id`, not free text. `image_uri` is
  nullable (a preset authored/backfilled outside the pin flow embeds the image in
  the JSON at `uri`). Migration `0007`; the frontend `Metadata` dropdown is the
  single authoring surface.
- **CHECK-constrained vocabularies are enums, never loose strings:** `LaunchStatus`
  / `BundleStatus` / `WalletRole` / `WalletStatus` (`platform_core::models::status`)
  own the `launches.status` / `bundles.status` / `managed_wallets.role` /
  `managed_wallets.status` values; each `as_str()` must equal the SQL `CHECK`
  (launch/bundle → migration `0006`; wallet role → `0002`, wallet status → `0004`;
  roundtrip tests pin the strings), same pattern as `MarketKind`. A new value is a
  code + CHECK edit — never a bare string literal at a call site.
- **Extensibility via rows, not columns**; interned small-int dimensions
  (`quote_assets`, `launchpads`) + `wallet_dict` (soft ref, no FK on the hot insert;
  read paths LEFT JOIN with a COALESCE fallback). Hot tables (`raw_txs`, `trades`) are
  **hypertables** with declarative compression + retention; the dedup key IS the PK.

## Security (locked)

**No secret material in Postgres.** `managed_wallets.key_ref` is a *reference* only
(keystore path / KMS id / envelope-encrypted blob), and the model marks it
`#[serde(skip_serializing)]`. Signing goes through `pump-trader`'s `Arc<dyn Signer>`.
Keystore backend (ADR D3) = **envelope-encrypted file + pluggable KEK trait**
(env/passphrase now → AWS KMS later; ed25519 signs in-process — KMS can't sign it).

**HTTP: fail-closed bearer gate on mutating routes.** `forge-live` moves treasury SOL
(launch / fund / manage), so — like hunter — every POST/PUT/DELETE/PATCH requires
`Authorization: Bearer $API_AUTH_TOKEN`; safe reads + preflight pass. `API_AUTH_TOKEN`
is **required** for the live HTTP server to boot (a missing token refuses to serve,
never serves open). The gate is the shared `http-auth` crate (`ApiAuth` +
`require_bearer_auth`, `shared/http-auth`) — ONE copy across hunter live/lab + forge
live, so the auth decision can't drift between products. In deploy, nginx (`forge-live`
UI, `default.conf.template`) bridges the operator's Basic-auth login to the bearer
token via envsubst so only the proxy can reach `/api`; the token never reaches the
browser. Defense in depth, not a substitute for keeping the box off the public
internet where possible.

## Deployment (EC2: 2vCPU / 4GB — IO-bound, RAM-constrained)

- Ship **`forge-live` + borrowed crates** to EC2 only. **`forge-lab` + `lake` + DuckDB/arrow/
  parquet/rayon stay on the workstation** — never deploy them.
- EC2 PG = hot rolling buffer (`raw_txs` 7d, `trades` 30d). Analysis is via DB sync to
  a local mirror → `lake-export` → Parquet. No DuckDB/export cron on the server.
- Connection-pool sizes are load-bearing; a new pool requires shrinking another. Don't
  raise caps/TTLs on the server to "make analysis easier" — sync to lab instead.

## Commands

```powershell
# Run from the monorepo root (Bot/) or this folder; the workspace is Bot/Cargo.toml.
docker compose up -d                 # local Postgres + TimescaleDB (host port 5556); adds forge-live service
sqlx migrate run                     # apply migrations/0001,0002
cargo check -p forge-live -p forge-lab   # typecheck the forge bins (use --target-dir target-check if a bin is running)
cargo tree -p forge-live               # dep-partition check (no duckdb/arrow/parquet)
cargo tree -p forge-lab                # dep-partition check (no pump-trader/ingest-laserstream/tonic)
cargo run -p forge-live                # LIVE box: needs Postgres + Helius gRPC/keys; HTTP :8230
cargo run -p forge-live -- wallet-encrypt <keypair.json> <key_ref>  # envelope-encrypt dev wallet (needs WALLET_KEYSTORE + LAUNCHER_KEK_PASSPHRASE)
cargo run -p forge-live -- wallet-verify <key_ref> <expected_address>  # restore runbook: confirm a keystore blob decrypts to the expected pubkey
cargo run -p forge-lab                 # ANALYSIS box: needs Postgres only; NO keys / NO gRPC; HTTP :8240
```

**DB-gated tests** (generality proof, repo round-trips) self-skip unless
`PLATFORM_TEST_DATABASE_URL` points at a **dedicated throwaway** DB (they run their own
migrations). Ports: DB **5556**, live **8230**, lab **8240**.

## Definition of done

- `cargo check -p forge-live -p forge-lab` clean; clippy on touched code; test when logic changed.
- Dep partition still holds (`cargo tree -p forge-live`/`-p forge-lab`).
- Stayed in the owning crate; no secrets in code; `.env` synced with `.env.example`.
- **Docs updated:** rules/commands → this file; schema/data-flow → `README.md` +
  `docs/`; decisions/rationale → `docs/decisions.md`.

## .env

`.env` is gitignored; keep in sync with `.env.example` (`Copy-Item .env .env.backup
-Force` before applying new keys). Secrets/keys live there only, never in code.
