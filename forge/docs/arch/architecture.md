# Architecture — forge workspace skeleton

File-level map of the `forge` backend. Read this instead of re-exploring source, and
instead of `forge/README.md` / `forge/CLAUDE.md`, whose crate tables are **stale** (they
still list `ingest-host` / `lake` as crates and omit `orchestrator`). Ground truth is the
root `Bot/Cargo.toml` `members`.

`forge` is one product folder inside the `Bot/` monorepo — a single Cargo `[workspace]`
(`resolver = "1"`, root `Bot/Cargo.toml`) shared with `hunter/` and `shared/`. Forge's bins
are **not** workspace `default-members`; build/run them with `-p forge-live` / `-p forge-lab`.
Deep-dive detail: [../decisions.md](../decisions.md), [../roadmap-plan.md](../roadmap-plan.md),
[../plans/launcher/dev-buy-variants.md](../plans/launcher/dev-buy-variants.md).

## Crate map — 5 forge crates (3 lib + 2 bin) + 3 shared crates

| Crate | Dir | Lib/bin name | Kind | Owns | Ships to |
| --- | --- | --- | --- | --- | --- |
| `forge-core` | `forge/core/` | lib `platform_core` | lib | Data layer: `config` (DB pools), `units`, `models`, `storage` (postgres + repos + timescale), `venue` trait. **solana-free** — addresses are TEXT/String | both |
| `forge-orchestrator` | `forge/orchestrator/` | lib `orchestrator` | lib | **The brain:** `plan` (Operation/Plan model), `provider` (catalog-gated validation), `macros` / `personas` / `disguise` / `audit` / `rng`. Zero-SOL. **forge-only, LIVE-only** | forge-live |
| `forge-launcher` | `forge/launcher/` | lib `launcher` | lib | Launch/fund/manage orchestration: create + dev-buy + bundle + confirm + wallet-pool + funding + management, all through the plan gate. **LIVE-only** | forge-live |
| `forge-live` | `forge/live/` | bin `forge-live` | **bin** | LIVE composition root: `ingest/` (host adapter) + `sse` + `sol_price` + `http` → EC2. Links launcher + orchestrator + ingest-pumpfun | — (→ EC2) |
| `forge-lab` | `forge/lab/` | bin `forge-lab` | **bin** | ANALYSIS composition root: `lake/` (cold columnar tier) + thin read `http` → workstation. NO keys / NO gRPC | — (→ workstation) |
| `executor-core` | `shared/executor/core/` | — | shared lib | Venue-agnostic write stack; neutral `VenueId` seam | both products |
| `executor-pumpfun` | `shared/executor/pumpfun/` | lib `pump_trader` (dep key `pump-trader`) | shared lib | pump.fun venue adapter: ix builders, catalog, `PumpFunTrader`, Jito. Canonical pump IDLs at `idl/` | both products |
| `ingest-core` | `shared/ingest/core/` | — | shared lib | Venue-agnostic read stack | both products |
| `ingest-pumpfun` | `shared/ingest/pumpfun/` | lib `ingest_pumpfun` | shared lib | pump.fun ingest venue + the assembly root over `ingest-core` / `ingest-laserstream` / `ingest-nats`; raw `Ingest`/`IngestHandle`/`IngestEvent` API | both products |
| `http-auth` | `shared/http-auth/` | — | shared lib | `ApiAuth` + `require_bearer_auth` fail-closed bearer gate | both products |

**In-crate modules, not crates:** `lake` → `forge/lab/src/lake/`; `ingest-host` →
`forge/live/src/ingest/`. `forge/keystore/` and `forge/wallet-backups/` are **data dirs**,
not crates. `forge/frontend/` is the React SPA (React-Router + RTK-Query + Tailwind).

**Dep edges:** `orchestrator` → `executor-core` + `pump-trader` (+ `solana-sdk` for address
parsing at the provider boundary). `launcher` → `platform-core` + `orchestrator` +
`pump-trader` + `executor-core`. `forge-live` → `platform-core` + `launcher` +
`ingest-pumpfun` + `http-auth` (pulls `orchestrator`/`pump-trader` transitively via
`launcher`). `forge-lab` → `platform-core` + `http-auth` **only**.

## Architecture — process topology

```text
                       ┌──────────────────── shared/ (both products) ─────────────────────┐
                       │  executor/core  executor/pumpfun(pump_trader)                     │
                       │  ingest/{core,laserstream,nats,pumpfun}              http-auth   │
                       └───────────────────────────────────────────────────────────────────┘
                                    ▲                     ▲                      ▲
      forge-live (EC2) ────────────┼─────────────────────┼──────────────────────┘
      ┌───────────────────────────────────────────────────────────────────────────────┐
      │ main.rs  tokio::select! over long-lived tasks:                                 │
      │   • ingest/ (host adapter) ── ingest_pumpfun::Ingest ─────► DbWriter ─► PG    │
      │   • bundle-confirm watcher   • wallet-lifecycle   • ladder   • volume          │
      │   • SOL/USD poller           • actix HTTP (:8230, bearer-gated) + SSE hub      │
      │                                                                                │
      │  launcher ──assembles──► orchestrator::Plan ──gate(validate+disguise+audit)──► │
      │           plan_exec ──builds txs──► pump_trader (PumpFunTrader, Jito)          │
      └───────────────────────────────────────────────────────────────────────────────┘
                                    │ platform_core (config·models·storage·venue)
                                    ▼
                              Postgres / TimescaleDB (hot: raw_txs 7d, trades 30d)
                                    │  (server→local DB sync)
                                    ▼
      forge-lab (workstation) ──────┴────────────────────────────────────────────────
      ┌───────────────────────────────────────────────────────────────────────────────┐
      │ main.rs  actix HTTP (:8240, read-only) over local PG mirror; NO keys/NO gRPC   │
      │   lake/ (schema SSOT; Parquet/DuckDB export pipeline lands later)              │
      └───────────────────────────────────────────────────────────────────────────────┘
```

## Composition roots — the two `main.rs` files

Each bin is a composition root that builds `Settings` → `connect()` DB pools → shared
services → long-lived tokio tasks. Both wrap actix with the shared `require_bearer_auth`
middleware. `platform_core::storage::connect` applies migrations at boot.

### `forge/live/src/main.rs` — LIVE (EC2, 4 worker threads)

Modules: `http`, `ingest`, `sol_price`, `sse`. **CLI subcommands** (return before any
HTTP/ingest startup, via `launcher::`): `wallet-encrypt`, `wallet-verify`, `wallet-export`,
`launch-probe`, `launch-sim-matrix`, `bundle-simulate`, `create-alt`. Long-lived tasks
joined in one `tokio::select!` (any task ending ⇒ process exits):

| Task | Source | Role |
| --- | --- | --- |
| SOL/USD poller | `sol_price::run_poller` | keeps `quote_assets.usd_rate` fresh for `trades_priced` |
| bundle-confirm watcher | `launcher::spawn_bundle_confirm_watcher` | feed-based bundle-landing confirm + auto re-bid; always on |
| wallet-lifecycle | `launcher::spawn_wallet_lifecycle` | DB-only reservation-TTL safety sweep (60s); needs launcher cfg. Fund/balance are operator-triggered (buttons) — no idle Helius RPC |
| ladder evaluator | `launcher::spawn_ladder_evaluator` | fires sell-ladder rungs (self-skips until `MANAGE_ENABLED`) |
| volume scheduler | `launcher::spawn_volume_scheduler` | volume-bot scheduler (self-skips until `MANAGE_ENABLED`) |
| ingest | `ingest::spawn_ingest` | **optional** — only with `HELIUS_LASERSTREAM_URL` + `HELIUS_API_KEY` |
| HTTP server | `http::configure` | actix on `HOST`/`PORT` (fallback `127.0.0.1:8230`) |

Boot wiring: a `tokio::sync::Notify` (`trades_notify`) lets the ingest DbWriter ping the
confirm watcher (notify-over-poll); an `sse::SseHub` is shared by the DbWriter and the HTTP
`/api/stream`; the launcher's workers push status through the crate-neutral
`launcher::EventSink` seam (the SseHub is its impl). `LauncherSettings::from_env()` is built
**once** at boot and shared with both background tasks and HTTP handlers (via `app_data`).
`API_AUTH_TOKEN` is **required** — the live server refuses to boot without it. Ingest and the
launcher tasks are each optional; absent, the box still boots and serves HTTP.

### `forge/lab/src/main.rs` — ANALYSIS (workstation, 2 worker threads)

Modules: `http`, `lake`. No trader, no ingest, no launcher, no signing keys, no gRPC. Takes
**no subcommands** (there is no `lake-export`; the column-SSOT seam lives in
`lake::schema`). Builds DB pools over the local mirror, then serves one actix HTTP server on
`HOST`/`PORT` (fallback `127.0.0.1:8240`) and `await`s it — no `tokio::select!` fan-out.
`API_AUTH_TOKEN` is **optional** here (lab moves no SOL; reads pass regardless).

## Module topology

### `platform-core` (`forge/core/src/`)

| Module | Owns |
| --- | --- |
| `config.rs` | `Settings` (DB URL + hot/api/batch pool sizing) `from_env` |
| `units.rs` | base-unit ↔ human conversion via the referenced asset's decimals (SSOT) |
| `models/` | `dimensions` (Launchpad/Market/QuoteAsset) · `token` · `trade` (RawTx/Trade/TradePriced) · `own_launch` (Launch/Bundle/ManagedWallet/SellLadder/TokenPosition/VolumeBot/…) · `metadata` · `status` (CHECK-backed enums: Launch/Bundle/Wallet/Position/Manage/Volume/Ladder) |
| `storage/postgres.rs` | `connect` → `DbPools { hot, api, batch }` + `sqlx::migrate!` |
| `storage/timescale.rs` | continuous-aggregate boot (idempotent, after `migrate!`) |
| `storage/repositories/` | one repo per table: `dimensions` (Launchpad/Market/QuoteAsset) · `feed` (RawTx/Trade/WalletDict) · `token` (Token/TokenMarketState/TokenSyncState) · `metadata` · `own_launch` (Bundle/Launch/LaunchTemplate/ManageAction/ManagedWallet/SellLadder/TokenPosition/VolumeBot) |
| `venue.rs` | venue/launchpad trait contract |

### `orchestrator` (`forge/orchestrator/src/`)

| Module | Owns |
| --- | --- |
| `plan.rs` | `Operation` / `Plan` — one uniform trade model keyed on orthogonal axes (mechanism ⊥ role ⊥ intent ⊥ venue ⊥ amount); serializable (carries base58 strings) |
| `provider.rs` | `prepare` → `PreparedPlan` — validates each op against the venue variant catalog (SSOT); an illegal tx is unrepresentable |
| `macros.rs` | plan builders: `bundle_launch` · `fund` · `volume_make` · `exit` · `consolidate` |
| `personas.rs` / `disguise.rs` | sticky per-wallet persona sampling → landing CU/tip/variant; pure/reproducible |
| `audit.rs` | fingerprint auditor — mandatory zero-SOL gate flagging on-chain linkage + bot tells |
| `rng.rs` | seeded RNG for reproducible persona/disguise draws |

### `launcher` (`forge/launcher/src/`)

Flat module set (see `lib.rs`). Key groupings:

| Area | Modules |
| --- | --- |
| Launch | `service` (`execute_launch`) · `bundle` · `bundle_execute` · `bundle_simulate` · `confirm` (`spawn_bundle_confirm_watcher`) · `alt` · `jito_leader` · `launch_sim_matrix` · `probe` |
| Plan gate | `plan_pipeline` (`gate` = validate+disguise+audit over an `orchestrator::Plan`) · `plan_exec` (builds real txs from a gated plan via `PumpFunTrader`) |
| Wallets | `wallet_pool` · `wallet_lifecycle` · `wallet_funding` · `funding_plan` · `wallet_sweep` · `dust_sweep` · `wallet_transfer` · `wallet_export` · `wallet_encrypt` · `wallet_verify` · `backup` · `keystore` (envelope-encrypt + `Kek` trait) |
| Manage (`manage/`) | `positions` (feed-derived holdings read model) · `model` · `plan` · `execute` · `ladder` · `volume` |
| Config / seams | `config` (`LauncherSettings`/`FundingConfig`/`ManageConfig`) · `events` (`EventSink`) · `metadata_upload` (Pinata) · `trader_config` |

### `forge-live` in-crate modules (`forge/live/src/`)

| Module | Owns |
| --- | --- |
| `ingest/` | host adapter (folded `ingest-host`): `consumer` (`spawn_ingest` + hot recv loop, no DB I/O) · `db_writer` (decoupled batched writer, all DB I/O + interning) · `map` (pure event→row) · `pumpfun` (`PumpFunAdapter` resolves interned dimension ids) · `watchdog` (OS-thread process watchdog on writer heartbeat) · `metrics` (`IngestMetrics`) |
| `sse.rs` | `SseHub` broadcast bus + `/api/stream` (`stream::unfold` over `futures-util`) |
| `sol_price.rs` | SOL/USD fetch + poller |
| `http.rs` | full LIVE route set (below) |

### `forge-lab` in-crate modules (`forge/lab/src/`)

| Module | Owns |
| --- | --- |
| `lake/schema.rs` | Parquet/DuckDB column-name SSOT (the export/reader pipeline lands later) |
| `http.rs` | thin read-only route set (below) |

## HTTP surfaces

Each bin owns its **own** disjoint route config (no shared route module). Both prefix with
full `/api/...` paths and sit behind the bearer gate.

- **`forge-live`** (`http::configure`): `/health` · `/api/stream` (SSE) · ingest toggle
  (`GET`/`PUT /api/ingest`) · `/api/bootstrap` · dimensions (`quote_assets`, `launchpads`) ·
  `launch_templates` CRUD · `wallet_pool` (list/generate/fund/refresh_balances/
  transfer/sweep/consolidate/`{id}/export`) · `metadata_templates` CRUD · `launches`
  (list/`requirement`/`{id}`/`{id}/status`/`execute`) · `bundles/{id}`(+`/execute`) · per-token
  `overview`/`trades`/`positions`(+`/refresh`) · `manage` (preview/execute/actions/ladders/volume).
- **`forge-lab`** (`http::configure`): `/health` · `quote_assets` · `launchpads` ·
  `tokens/{mint}/overview` · `tokens/{mint}/trades` (uncapped mint-scoped analysis read).

## How forge consumes the shared crates

- **Write path:** `launcher`/`orchestrator` depend on `pump-trader` (dep key; package
  `executor-pumpfun`, lib `pump_trader`) + `executor-core`. Every forge write assembles an
  `orchestrator::Plan`, passes `launcher::plan_pipeline::gate` (validate against
  `pump_trader::catalog` → disguise → audit), then `plan_exec` builds the real txs through an
  initialized `PumpFunTrader`. The pump.fun IDLs live once at `shared/executor/pumpfun/idl/`.
- **Read path:** `forge-live` depends on `ingest-pumpfun` (`raw-tx` feature; the
  `nats` feature stays off, so the relay crate is never linked). The read-stack emits
  raw `IngestEvent`s; `forge/live/src/ingest/` is the host adapter that bridges them onto
  `platform_core`'s `raw_txs`/`trades`/`tokens` via the pump.fun/SOL venue adapter.
- **Auth:** both bins depend on `http-auth` for the one fail-closed `require_bearer_auth` gate.

## Key rules

- **Crate structure ground truth is `Bot/Cargo.toml`**, not the README/CLAUDE tables (stale).
  5 forge crates: 3 lib (`forge-core`, `forge-orchestrator`, `forge-launcher`) + 2 bin
  (`forge-live`, `forge-lab`). `lake`/`ingest-host` are folded modules, not crates.
- **Dep partition (load-bearing — enforce from the scaffold):**
  - `forge-live` must NOT pull `duckdb`/`arrow`/`parquet` (the lake stack). `lake` is
    deliberately absent from its `Cargo.toml`. Verify: `cargo tree -p forge-live`.
  - `forge-lab` must NOT pull `pump-trader`/any `ingest-*`/`launcher`/`orchestrator`/
    `tonic`/solana. It depends on `platform-core` + `http-auth` only. Verify:
    `cargo tree -p forge-lab`.
- **`platform-core` is solana-free** — addresses are TEXT/String; on-chain types live behind
  the live-side crates (`launcher`/`orchestrator`/`ingest`), never in the data layer.
- **`orchestrator` is forge-only + LIVE-only** — `hunter/live` calls the executor stack
  directly (lean snipe, no plan/disguise); neither lab links it.
- **The plan gate is mandatory** — no forge write flow hand-rolls instructions from free-text
  variant strings; every flow goes `Plan → gate (validate+disguise+audit) → plan_exec → txs`.
- **Two composition roots, disjoint HTTP surfaces** — no shared route config; `forge-live`
  fans out long-lived tasks under one `tokio::select!`, `forge-lab` serves a single
  read-only actix server.
- **Bins are not `default-members`** — build/run with `-p forge-live` / `-p forge-lab`. Ports:
  live `:8230`, lab `:8240`, DB `:5556`. `API_AUTH_TOKEN` required for live, optional for lab.
- **Ingest is notify-over-poll and off the DB hot path** — the recv loop does no DB I/O;
  `db_writer` is a decoupled batched task; the `watchdog` force-exits if writes stall while
  work is queued.
</content>
</invoke>
