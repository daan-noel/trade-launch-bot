# forge — data-layer architecture

The data layer is the **`forge-core`** crate (Cargo package `forge-core`, **lib name
`platform_core`**, dir `forge/core/`) plus the SQL in `forge/migrations/`. It is
deliberately **solana-free** — every on-chain address is `TEXT`/`String`; on-chain
types live behind the live-side crates (`launcher`/`ingest-host`), never here.

Shipped to **both** bins (`forge-live`, `forge-lab`). No `ingest`/`trader`/`lake` deps.

## Architecture

Ideas carried from hunter, generalized off SOL/pump.fun:

- **Raw feed + typed projection.** `raw_txs` (unparsed BYTEA source-of-truth) →
  `trades` (typed, quote/venue-generalized projection). Both are **TimescaleDB
  hypertables** partitioned on `block_time` with declarative compression + retention;
  the dedup key **is** the PK, so a replayed feed is idempotent (`ON CONFLICT DO NOTHING`).
- **Dimensions, not columns.** `quote_assets` + `launchpads` are small interned
  small-int dimension tables stamped denormalized onto hot rows. Native SOL is just
  the `quote_assets` row with `is_native, decimals 9`. **A new launchpad or quote
  asset is a new dimension ROW, never a migration.**
- **Integer base units, unit-in-name.** Amounts are exact `BIGINT` base units named
  by the referenced asset as a suffix: `amount_quote`/`amount_base`,
  reserves `reserve_quote`/`reserve_base`. `crate::units` is the only base↔display
  converter, applied at the repo/view boundary with the row's decimals.
- **Prices are RAW RATIOS.** Stored/aggregated as `amount_quote / amount_base`
  (decimals-agnostic). Decimals + USD are applied **only in derived views**
  (`trades_priced`, `token_overview`) — never stored.
- **Identity SSOT keys:** `mint_address`, `launchpad_id`, `quote_asset_id`, `market_id`.
- **Metadata SSOT:** token identity (name/symbol/uri) lives in ONE `metadata_templates`
  row; `launch_templates.metadata_template_id` (FK, `ON DELETE SET NULL`) references it.
- **CHECK-constrained vocab enums** in `platform_core::models::status` are the Rust
  half of each SQL `CHECK`; `as_str()` must be byte-equal, guarded by roundtrip tests.
- **Workload-isolated pools** (`hot`/`api`/`batch`) — connection counts are load-bearing
  on the 2vCPU/4GB EC2 box.

## Crate module map

| File | Responsibility |
| --- | --- |
| `core/src/lib.rs` | crate root: `units`, `config`, `models`, `storage`, `venue` |
| `core/src/units.rs` | base-unit ↔ display SSOT (`to_base_units`/`to_display`, `sol_to_lamports` = decimals-9 case); rounds so display→base→display round-trips |
| `core/src/config.rs` | `Settings::from_env` — `DATABASE_URL` (required) + hot/api/batch pool sizes + acquire timeout |
| `core/src/venue.rs` | `MarketKind` enum + `LaunchpadAdapter` trait (the launchpad seam) |
| `core/src/models/` | typed rows / DTOs, one module per domain; amount fields are `i64` base units |
| `core/src/models/status.rs` | CHECK-constrained vocab enums (SSOT of the status strings) |
| `core/src/storage/postgres.rs` | `DbPools{hot,api,batch}` + `connect()` (runs `sqlx::migrate!("../migrations")` + cagg teardown on `hot`) |
| `core/src/storage/timescale.rs` | `teardown_dead_caggs` — drops the dead OHLCV CAggs at boot (idempotent) |
| `core/src/storage/repositories/` | one repo struct per table; the DB↔model boundary |

## Postgres schema (`migrations/0001_init.sql`, single squashed init)

`0001` is the full end-state (SLP `0001..0013` + forge `0002..0007` folded in),
including the `trades_priced` view's `wallet_address` (LEFT JOIN `wallet_dict` +
`#<ref>` COALESCE fallback), which was the last on-disk `0002`.

Squashing changes version 1's checksum and drops version 2 from
`_sqlx_migrations`, so `sqlx` refuses to boot against a database that already ran
the chain. Reconcile it once with
`scripts/consolidate-migration-ledgers.ps1 -Ledger forge -Apply` (ledger-only — no
schema, no data).

### Domain A — dimensions (interned, row-extensible)

| Table | Purpose |
| --- | --- |
| `quote_assets` | SMALLINT-PK interned quote asset (SOL id 1, USDC id 2); `mint`, `decimals`, `is_native`, `usd_rate` numeraire. Seeded. |
| `launchpads` | SMALLINT-PK interned launchpad (`pump_fun` id 1); `default_quote_asset_id`, JSONB `meta` (program ids/fees/curve consts). Seeded. |

### Domain B — token identity + market state (observed universe)

| Table | Purpose |
| --- | --- |
| `tokens` | static creation facts, PK `mint_address`; FK → launchpad/quote asset; `is_own_launch`; `initial_supply_base`/`initial_buy_quote` (base units) |
| `markets` | per-token tradeable venue instance(s); `market_kind` CHECK (`bonding_curve`/`amm`/`clmm`/`orderbook`); BIGINT identity PK `market_id`; UNIQUE(mint,launchpad,kind) |
| `token_market_state` | 1:1 hot live metrics (PK=FK `mint_address`); `current_price_quote`/`ath_price_quote` RAW ratios, `volume_quote`, `trade_count`, `is_dead`/`is_migrated` |
| `token_sync_state` | per-(mint,market) ingest watermark (`last_sig`/`last_slot`); PK(mint,market_id) |

### Domain C — the feed (high-volume hypertables)

| Table | Purpose |
| --- | --- |
| `wallet_dict` | wallet interning map (int id ↔ address); **soft** ref from `trades`, no FK on hot insert |
| `raw_txs` | **hypertable** — unparsed BYTEA payload source-of-truth; PK(block_time,tx_signature); compress after 2d, retain 7d |
| `trades` | **hypertable** — typed projection; `wallet_ref` (soft), denormalized `launchpad_id`/`market_kind`/`quote_asset_id`; `amount_quote`/`amount_base`, `reserve_quote`/`reserve_base`; PK(block_time,tx_signature,leg_index); segmentby `mint_address`, compress after 7d, retain 30d |

### Derived views (decimals + USD applied HERE, never stored)

| View | Purpose |
| --- | --- |
| `trades_priced` | `trades` + quote-asset join + `wallet_dict`; adds `wallet_address`, `exec_price_quote` (amount ratio), `spot_price_quote` (reserve ratio), `amount_quote_display`, `amount_usd` |
| `token_overview` | `tokens` LEFT JOIN `token_market_state` + quote asset; adds `age_secs`, `price_quote_display`/`price_usd`, `market_cap_quote`/`market_cap_usd` |

### Domain D — own-launch + wallet pool + management

| Table | Purpose |
| --- | --- |
| `managed_wallets` | OUR wallets (UUID PK); `role` CHECK, `status` lifecycle CHECK; **`key_ref` is a reference only, never key bytes**; `balance_lamports` (native SOL bookkeeping); `reserved_by_launch_id` circular FK → launches |
| `metadata_templates` | token-identity SSOT: name/symbol/`uri` (pinned JSON); `image_uri` nullable |
| `launch_templates` | authored launch specs; `variant`, JSONB `params`; `metadata_template_id` FK → metadata_templates (`ON DELETE SET NULL`) is the name/symbol/uri SSOT |
| `launches` | executed launch record; `status` CHECK (`pending`/`created`/`failed`); `dev_wallet_id` FK; `bundle_id` **soft** back-ref |
| `bundles` | atomic Jito bundle of create+buy legs; `status` CHECK (7 states); `tip_quote`, JSONB `legs`/`plan`/`audit`/`create_args`, `leg_signatures` TEXT[], `submit_attempts` (re-bid level) |
| `token_positions` | per-wallet holdings read model; `managed_wallet_id` FK; `balance_base`/`cost_quote`/`realized_quote`; `status` CHECK (`open`/`closed`/`dropped`); no FK on `mint_address` |
| `manage_actions` | audit log of executed management; `kind`/`sizing`/`status` CHECKs; JSONB `selection`/`plan`/`plan_orchestrator`/`audit` |
| `sell_ladders` | threshold sell ladders; JSONB `rungs`; `status` CHECK (`armed`/`done`/`cancelled`) |
| `volume_bots` | volume-making bots; JSONB `config`; `spent_quote`/`volume_quote`; `status` CHECK (`running`/`paused`/`stopped`) |

## Repositories (repo → table → key fns)

All repos are zero-field structs with `async fn(pool, …)`; base-unit columns convert
at this boundary. `dimensions.rs`, `feed.rs`, `metadata.rs`, `token.rs`, `own_launch.rs`.

| Repo | Table | Key fns |
| --- | --- | --- |
| `QuoteAssetRepo` | `quote_assets` | `all`, `get`, `native`, `set_usd_rate` |
| `LaunchpadRepo` | `launchpads` | `all`, `get` |
| `MarketRepo` | `markets` | `upsert`, `by_mint` |
| `WalletDictRepo` | `wallet_dict` | `intern` (address → int ref, upsert) |
| `RawTxRepo` | `raw_txs` | `insert_batch` (UNNEST, ON CONFLICT DO NOTHING) |
| `TradeRepo` | `trades`/`trades_priced` | `insert_batch`, `find_signatures_present`, `sum_side_quote_by_address`, `sum_sells_by_address_for_mint`, `fills_for_mint_wallets`, `find_priced_by_mint`, `find_priced_page_with_count` |
| `TokenRepo` | `tokens`/`token_overview` | `insert`, `mark_own_launch`, `get`, `overview` |
| `TokenMarketStateRepo` | `token_market_state` | `upsert`, `get` |
| `TokenSyncStateRepo` | `token_sync_state` | `upsert`, `get` |
| `MetadataTemplateRepo` | `metadata_templates` | `insert`, `get`, `all`, `update`, `delete` |
| `LaunchTemplateRepo` | `launch_templates` | `insert`, `get`, `all`, `update`, `delete` |
| `LaunchRepo` | `launches` | `insert`, `get`, `get_many`, `find_by_mint`, `list_page`, `count`, `set_create_signature`, `set_created`, `set_failed`, `set_bundle_id` |
| `BundleRepo` | `bundles` | `insert`, `by_launch`, `get`, `set_status`, `claim_for_submitting`, `set_submitted`, `set_tip_quote`, `reset_for_rebid`, `set_confirmed`, `find_awaiting_confirmation` |
| `ManagedWalletRepo` | `managed_wallets` | `insert`, `by_role`, `get`/`get_many`, `list_all`, `find_by_status`, `find_pollable`, `record_balance`, `funded_count`, `claim_for_funding`/`claim_specific_for_funding`, `revert_funding`/`revert_stale_funding`, `claim_specific`, `claim_funded` (FOR UPDATE SKIP LOCKED), `release_expired_reservations`, `release_reservation`, `release_by_launch`, `mark_used`, `retire` |
| `TokenPositionRepo` | `token_positions` | `seed`, `seed_dropped`, `by_mint`, `managed_wallet_ids_with_open_positions`, `set_balance`, `set_realized`, `reconcile_batch` |
| `ManageActionRepo` | `manage_actions` | `insert_executing`, `set_result`, `by_mint` |
| `SellLadderRepo` | `sell_ladders` | `insert`, `by_mint`, `list_armed`, `update`, `cancel` |
| `VolumeBotRepo` | `volume_bots` | `insert`, `by_mint`, `list_due`, `record_cycle`, `pause`, `resume`, `stop` |

## `venue/` trait abstraction (`core/src/venue.rs`)

The seam that makes a new launchpad a `launchpads` **row + a trait impl**, not a
migration. `platform_core` defines only the CONTRACT and stays solana-free
(addresses are `&str`); the pump.fun impl lives live-side.

- **`MarketKind`** — enum mirroring the `market_kind` CHECK; `as_str()`/`FromStr`
  are the ONE Rust definition of `bonding_curve`/`amm`/`clmm`/`orderbook`.
- **`LaunchpadAdapter: Send + Sync`** — `key()`, `launchpad_id() -> i16`,
  `quote_asset_id(kind) -> i16`, `classify_market(program_id) -> Option<MarketKind>`.
  Decoding of raw txs stays in the live-side transport; the adapter only resolves
  identity so rows land with the right interned dimensions.

## Status enums (`core/src/models/status.rs`) — CHECK-string SSOT

Stored as `TEXT` (not PG enums) so a new value is a code + CHECK edit, never an
`ALTER TYPE`. Each `as_str()` must be byte-equal to the SQL `CHECK`; an `ALL` const
drives the roundtrip parity tests. `MarketKind` (in `venue.rs`) follows the same pattern.

| Enum | Column | Values |
| --- | --- | --- |
| `LaunchStatus` | `launches.status` | pending, created, failed |
| `BundleStatus` | `bundles.status` | planned, submitting, submitted, landed, dropped, partial, failed |
| `WalletRole` | `managed_wallets.role` | dev, bundler, treasury, trading |
| `WalletStatus` | `managed_wallets.status` | generated, funding, funded, reserved, used, retired |
| `PositionStatus` | `token_positions.status` | open, closed, dropped |
| `ManageKind` | `manage_actions.kind` | sell, buy, consolidate |
| `ManageSizing` | `manage_actions.sizing` | pct_of_holdings, to_sol_target, fixed_base, fixed_sol, sweep |
| `ManageStatus` | `manage_actions.status` | planned, executing, completed, partial, failed |
| `LadderStatus` | `sell_ladders.status` | armed, done, cancelled |
| `VolumeBotStatus` | `volume_bots.status` | running, paused, stopped |

## Pools + config (`core/src/config.rs`, `core/src/storage/postgres.rs`)

`Settings::from_env` loads `.env` (best-effort) and reads `DATABASE_URL` (required,
panics if absent) + pool sizes. `connect()` builds three workload-isolated pools and
runs migrations + cagg teardown on `hot`.

| Pool | Default max/min | Statement timeout | Use |
| --- | --- | --- | --- |
| `hot` | 12 / 1 | none | ingest writes, money-critical writes, caches; runs `migrate!` + cagg teardown |
| `api` | 8 / 1 | 8s | fast interactive HTTP reads |
| `batch` | 2 / 0 | none | long analysis jobs (bounded by job concurrency) |

Every connection also gets `idle_in_transaction_session_timeout = 30s` so a leaked
transaction can't permanently hold a slot. `connect()` runs
`sqlx::migrate!("../migrations")` (the same dir the `sqlx migrate` CLI uses) then
`timescale::teardown_dead_caggs` (drops the reader-less OHLCV CAggs).

## Key rules

- **solana-free data layer** — addresses are `TEXT`/`String`; no `solana-sdk` dep. On-chain
  types are live-side only.
- **Amounts name their unit as a suffix** (`amount_quote`/`amount_base`, `reserve_quote`/`reserve_base`),
  exact `BIGINT` base units. Native SOL = the `quote_assets` row with `is_native, decimals 9`;
  `*_lamports` dual-vocab was rejected (except native gas balances with no quote to reference,
  e.g. `managed_wallets.balance_lamports`).
- **Prices are RAW RATIOS** (`amount_quote/amount_base`); decimals + USD applied **only** in
  `trades_priced`/`token_overview`, never stored.
- **New launchpad / quote asset = a new dimension ROW**, not a migration.
- **Metadata SSOT** = a `metadata_templates` row referenced by
  `launch_templates.metadata_template_id`; never inline name/symbol/uri.
- **CHECK vocab = a Rust enum** in `models::status` (+ `MarketKind` in `venue`); `as_str()`
  byte-equal to the SQL CHECK, roundtrip-tested. A new value is a code + CHECK edit.
- **Hot tables (`raw_txs`, `trades`) are TimescaleDB hypertables** with declarative
  compression + retention; the dedup key IS the PK (idempotent replay via `ON CONFLICT DO NOTHING`).
- **`managed_wallets.key_ref` is a reference only** — no secret bytes in Postgres.
- **Soft refs on hot paths:** `trades.wallet_ref` → `wallet_dict` (no FK; read paths
  LEFT JOIN + COALESCE); `launches.bundle_id` bare UUID; management tables carry no FK on
  `mint_address` (a position can precede the token's create-tx ingest).
- **`forge-core` package, `platform_core` lib name** — don't trust "platform-core" in stale docs.
