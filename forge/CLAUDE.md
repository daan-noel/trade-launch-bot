# CLAUDE.md — forge

Venue- and quote-generalized Solana **launch + trading + analytics** platform, sibling to
`../hunter` in the same Cargo workspace. Starts at pump.fun token creation; grows into
multi-launchpad / multi-quote / multi-wallet.

Read [../CLAUDE.md](../CLAUDE.md) first (SSOT, latency, Helius budget, EC2, docs
discipline). This file is forge-specific only. Open phases and tasks:
[docs/roadmap-plan.md](docs/roadmap-plan.md) · ADRs: [docs/decisions.md](docs/decisions.md).

## Architecture — 5 crates, 2 bins

`trading_core` is **NOT** reused (its SOL/pump domain is the thing being generalized); the
only SSOT lifted by copy is the unit consts. The pump.fun IDLs have one canonical copy at
`shared/executor/pumpfun/idl/`. Both bins build with `-p` (not workspace `default-members`).

| Crate | Kind | Role | Ships |
| --- | --- | --- | --- |
| `forge-core` (`platform_core`) | lib | data layer: config, models, storage, repositories, `venue/` trait. **solana-free** (addresses are TEXT/String) | both |
| `forge-orchestrator` | lib | the brain: `Operation`/`Plan` + provider catalog gate + persona/disguise + fingerprint audit + dry-run | live |
| `forge-launcher` | lib | create / dev-buy / bundle / confirm / wallet-pool / funding / manage — every write routed through the orchestrator gate | live |
| `forge-live` | **bin** | ingest + launcher + trading + thin HTTP → **EC2** | — |
| `forge-lab` | **bin** | lake + sweeps/backtests/analytics HTTP → **workstation** | — |

`lake` and `ingest-host` are in-crate modules (`forge/lab/src/lake`, `forge/live/src/ingest`),
not crates. Shared drop-ins come from `Bot/shared/` (`executor`, `ingest`, `http-auth`).

**Dep partition (load-bearing):** `forge-live` must not pull `duckdb`/`arrow`/`parquet`;
`forge-lab` must not pull the executor / ingest / `tonic` / solana stack. Verify with
`cargo tree`. (rayon *is* in live — a Solana transitive, not the lake stack.)

**Read `docs/arch/` instead of re-exploring source; deep dives live in `docs/plans/`.**

| Doc | Covers |
| --- | --- |
| [docs/arch/architecture.md](docs/arch/architecture.md) | crate/bin map, both `main.rs` wirings, dep partition |
| [docs/arch/database.md](docs/arch/database.md) | schema, hypertables, repos, `venue/` trait, status enums |
| [docs/arch/launcher.md](docs/arch/launcher.md) | create→dev-buy→bundle→submit→confirm, wallet pool, tip re-bid |
| [docs/arch/orchestrator.md](docs/arch/orchestrator.md) | `Operation`/`Plan`, provider gate, persona, audit, dry-run |
| [docs/arch/ingest.md](docs/arch/ingest.md) | host adapter over `shared/ingest`, bundle-landing watcher |
| [docs/arch/lake.md](docs/arch/lake.md) | cold tier |
| [docs/arch/frontend.md](docs/arch/frontend.md) | operator dashboard: shell, RTK-Query, pages, SSE |

## Locked rules

- **Amounts name their unit as a suffix:** `amount_quote` / `amount_base` (exact BIGINT
  base units), `reserve_quote` / `reserve_base`. The unit is the *referenced quote/base
  asset*, never a hard-coded lamport — native SOL is just a `quote_assets` row. (ADR D1
  rejects hunter's `*_lamports` dual vocabulary here.)
- **Prices are raw ratios** (`amount_quote / amount_base`, decimals-agnostic). Decimals and
  USD apply **only in derived views**, never at rest. Ratios keep `_price`/`_pct`.
- **Extensibility via rows, not columns.** SSOT keys `mint_address`, `launchpad_id`,
  `quote_asset_id`, `market_id`; a new launchpad or quote asset is a new dimension **row**,
  never a migration. `wallet_dict` is a soft ref (no FK on the hot insert; read paths LEFT
  JOIN + COALESCE). Hot tables are hypertables and the dedup key IS the PK.
- **Token identity lives in ONE `metadata_templates` row**, referenced by
  `launch_templates.metadata_template_id` and resolved at create time. Never inline
  name/symbol/uri in a template's `params` or a launch request — a per-launch override is a
  *different* template id, not free text.
- **CHECK-constrained vocabularies are enums, never loose strings** (`LaunchStatus`,
  `BundleStatus`, `WalletRole`, `WalletStatus`). `as_str()` must equal the SQL CHECK, pinned
  by roundtrip tests. A new value is a code + CHECK edit.
- **No secret material in Postgres.** `managed_wallets.key_ref` is a *reference* only
  (keystore path / KMS id / envelope-encrypted blob) and is never serialized out; signing
  goes through the executor's `Arc<dyn Signer>`. Keystore = envelope-encrypted file +
  pluggable KEK trait (ADR D3).
- **Fail-closed bearer gate on every mutating route.** `forge-live` moves treasury SOL, so
  POST/PUT/DELETE/PATCH require `Authorization: Bearer $API_AUTH_TOKEN`; safe reads and
  preflight pass. The token is **required to boot**. The gate is the shared `http-auth`
  crate — one copy across hunter live/lab and forge live; in deploy, nginx bridges the
  operator's Basic-auth login to it so the token never reaches the browser.

## Commands

```powershell
docker compose up -d                      # local Postgres + TimescaleDB (host port 5556)
sqlx migrate run                          # apply migrations
cargo check -p forge-live -p forge-lab    # add --target-dir target-check if a bin is running
cargo tree  -p forge-live                 # dep-partition check (no duckdb/arrow/parquet)
cargo tree  -p forge-lab                  # dep-partition check (no executor/ingest/tonic)
cargo run   -p forge-live                 # LIVE: Postgres + Helius gRPC + keys   (HTTP :8230)
cargo run   -p forge-lab                  # ANALYSIS: Postgres only, no keys/gRPC (HTTP :8240)
cargo run   -p forge-live -- wallet-encrypt <keypair.json> <key_ref>   # needs WALLET_KEYSTORE + LAUNCHER_KEK_PASSPHRASE
cargo run   -p forge-live -- wallet-verify  <key_ref> <expected_address>  # restore runbook
cargo run   -p forge-live -- create-alt     <authority_key_ref>        # spends real SOL; output goes in PUMP_LAUNCH_ALT
```

DB-gated tests self-skip unless `PLATFORM_TEST_DATABASE_URL` points at a **dedicated
throwaway** DB (they run their own migrations).

## Definition of done

`cargo check -p forge-live -p forge-lab` clean; clippy on touched code; test when logic
changed; dep partition still holds; stayed in the owning crate; no secrets in code. Update
the docs tier that changed — rules/commands here, structure → `docs/arch/`, detail →
`docs/plans/`, decisions → `docs/decisions.md`, open work → `docs/roadmap-plan.md` — and
leave it present-tense (see [../CLAUDE.md](../CLAUDE.md)).
