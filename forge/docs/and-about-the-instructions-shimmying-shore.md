# New Platform — Data Infrastructure Design (Foundation)

## Context

Starting a **new, separate project**: a Solana launch + trading + analytics platform that
begins with token creation on pump.fun and grows into a multi-launchpad (incl. USDC-quoted),
multi-wallet, multi-RPC, strategy + analytics ecosystem.

The **current step is the data-infrastructure foundation** — a concrete schema designed
*generalized for multi-venue + non-SOL quote from day one*, learning from (not copying) the
existing `hunter` repo's data layer. That layer is well-built but **SOL-and-pump.fun
locked** (`amount_lamports`, `reserve_lamports`, `venue IN ('curve','amm')`). This redesign's
job is to lift those two hard-coded assumptions into first-class dimensions.

The launcher execution (create tx, dev-buy, bundler) is a **later** step; here we only design
the schema seams it will need.

**Operational constraint (carried from `hunter`):** production ships to a **2vCPU / 4GB
RAM EC2** box. Ingest + launch + trading and lake/sweep analytics are **disjoint workloads** —
the architecture mirrors the existing `live`/`lab` split from day one (see §1b), not as a later
refactor.

## Locked decisions

| Decision | Choice |
| --- | --- |
| Reuse | New Cargo workspace; borrow the two standalone crates (`pump-trader`, `ingest-laserstream`) via **`path` dep now → pinned `git` rev when stable**. `trading_core`'s data layer is redesigned fresh, not carried. |
| Data scope now | The extensible **foundation**: identity (mint/launchpad/quote SSOT), tokens, launches, wallets, and the trades/ingest feed — generalized for multi-launchpad + non-SOL quote. Launch tables implemented first. |
| Storage | Carry the proven **two-tier** stack: Postgres/TimescaleDB (source-of-truth + hot path) + Parquet/DuckDB lake (cold OLAP). |
| **Deployment** | **Two bins from day one** (mirror `hunter`'s `live`/`lab` split): **`live` → EC2** (ingest + launch + trade + thin HTTP); **`lab` → workstation** (lake export, sweeps, backtests, wallet analytics). Lake/DuckDB/`arrow`/`parquet`/`rayon` **never** ship to EC2. Server PG = hot rolling buffer; analysis via DB sync to local PG. |
| Bundler variation | Per-leg structure variation surface = **variant + params + budget/tip** (smallest safe surface). |

---

## 1. Reuse strategy

New workspace (fresh git repo). Two crates referenced, not copied:

| Crate | Why reused | Venue-specificity |
| --- | --- | --- |
| `pump-trader` | Execution layer: tx build / sign / submit / confirm / retry / nonce / Jito / slippage — real-money-hardened. The launch dev-buy *is* a pump-trader buy. | pump.fun-specific → used as the **pump.fun venue adapter**; gets a new `create.rs` (token-create builder doesn't exist yet). |
| `ingest-laserstream` | Ingestion layer: Helius gRPC transport + curve/AMM/create decoders + truncated-log recovery + reconnect watchdog. Foundational to *all* analytics. | decoders are pump.fun/PumpSwap-specific → extend per launchpad; transport/pipeline/watchdog are venue-agnostic. |

- **Path deps** (`{ path = "../hunter/pump-trader" }`) during co-development, so edits to
  the crate (e.g. adding `create.rs`) are seen by both projects immediately; switch to a
  **pinned `git` rev** once stable for reproducible builds.
- **pump-trader gets a targeted extension (you own it):** expose each buy/sell **variant** as an
  audited public builder (pump.fun has ~5 buy discriminators) + a primitive to assemble + sign a
  tx from an *ordered ix list with a chosen signer*. The per-leg composition/randomization for
  bundlers lives in the NEW project, not in pump-trader — another reason path deps matter.
- **`trading_core` is NOT reused** — it encodes the current app's SOL/pump domain and is the
  very thing being redesigned. Lift only tiny pure SSOT files by copy: the checked-in IDLs, the
  `lamports↔sol` conversion constants, the naming-rule docs.
- **Extraction trigger:** if a third consumer appears, promote the two crates into a shared git
  repo both depend on.

### 1a. Folder structure & dev workflow

New repo sits NEXT TO the existing one (path deps reach sideways). `★` = build now
(foundation), `○` = scaffolded now / filled later.

```text
your-code/
├── hunter/                  ← EXISTING (borrowed from, not modified except pump-trader ext.)
│   ├── pump-trader/               ← borrowed: execution
│   └── ingest-laserstream/        ← borrowed: data feed
│
└── forge/        ← NEW repo + Cargo workspace
    ├── Cargo.toml                 ← members + path deps to ../hunter/*
    ├── docker-compose.yml         ← local Postgres + TimescaleDB
    ├── .env.example
    ├── migrations/
    │   └── 0001_init.sql          ★ data foundation (this step)
    └── crates/
        ├── platform-core/  (lib)  ★ data layer: config, models,
        │                              storage/{postgres,timescale,repositories/*}, venue/ (trait)
        ├── ingest-host/    (lib)  ○ borrowed ingest → PG trades       (next; **live only**)
        ├── launcher/       (lib)  ○ orchestrator + pumpfun adapter + bundler seam (later; **live only**)
        ├── lake/           (lib)  ○ Parquet/DuckDB cold tier          (later; **lab only**)
        ├── live/           (bin)  ○ ingest + launcher + trading + HTTP → **EC2**
        └── lab/            (bin)  ○ lake + sweeps + backtests + analytics → **workstation**
```

Each crate = one job: **platform-core** (all tables + types + read/write — this step) ·
**ingest-host** (translate the borrowed feed into DB rows) · **launcher** (create/buy/bundle
via pump-trader) · **lake** (cold columnar copy for backtests) · **live** / **lab** (two
composition roots — never one monolithic `app` bin).

**Dep partition (enforced from scaffold — same lesson as `hunter`'s crate split):**

| Crate | `live` | `lab` | `platform-core` |
| --- | --- | --- | --- |
| `ingest-host` | ✓ | ✗ | ✗ |
| `launcher` (+ `pump-trader`) | ✓ | ✗ | ✗ |
| `lake` (+ `duckdb`/`arrow`/`parquet`/`rayon`) | ✗ | ✓ | ✗ |
| `platform-core` | ✓ | ✓ | — |

Verify with `cargo tree -p live` (no `duckdb`/`arrow`/`parquet`) and `cargo tree -p lab`
(no `pump-trader`/`ingest-laserstream`/`tonic`).

- **Dev loop (live):** `docker compose up -d` → `sqlx migrate run` → `cargo run -p live`.
- **Dev loop (lab):** same DB (local mirror) → `cargo run -p lab` (no gRPC, no keys).
- **Data flow (live box):** Helius gRPC → `ingest-laserstream` → `ingest-host` → PG
  `raw_txs`/`trades` (7-day `raw_txs` / 30-day `trades` retention on EC2).
- **Data flow (analysis box):** EC2 PG → **DB sync** (`scripts/db-incremental-sync.ps1` or
  equivalent) → local PG → `lab -- lake-export` → sealed-day Parquet → DuckDB reads/sweeps.
- **Launch flow:** template → `launcher` → `pump-trader` → chain → confirm → `launches` row
  (live only).
- **Reuse flow:** edit `../hunter/pump-trader`, both projects see it (path dep).

### 1b. Deployment topology — EC2 vs workstation

Hard constraint inherited from `hunter`: production runs on **2vCPU / 4GB RAM EC2**.
This platform adds launcher + bundler on top of ingest/trading — the box gets *tighter*, not
looser. The live/lab split is a **resource partition**, not a naming preference.

| | **EC2 (`live`)** | **Workstation (`lab`)** |
| --- | --- | --- |
| **Ship** | `live` bin + `ingest-laserstream` + `pump-trader` | `lab` bin + `lake` crate |
| **Never ship** | `lake`, DuckDB, `arrow`/`parquet`/`rayon`, sweep engine | `ingest-laserstream`, `pump-trader`, signing keys, gRPC |
| **Postgres role** | Source-of-truth + hot path; **rolling buffer** (`raw_txs` 7d, `trades` 30d) | Long-retention mirror via incremental DB sync |
| **In-RAM caches** | **Tracking tokens only** (bounded cap); token list = SQL-paged, no full-universe snapshot | Full token-list snapshot OK (`LAB_TOKEN_LIST_LIMIT` / window-days) |
| **Lake** | Schema + export contract only — **no export cron, no DuckDB process on EC2** | `lake-export` seals days locally; all OLAP reads here |
| **Analysis** | Live positions, thin inspect endpoints | Sweeps, backtests, wallet/bot analytics, simulate |
| **Infra spend** | Fixed — no bigger box, no second server for analytics |

**RAM / connection guardrails on EC2 (carry verbatim, tune names only):**

- Connection pool sizes are load-bearing; new pools require shrinking something else.
- In-RAM trade cache trim cap (like `MAX_TRADES_RETAINED`) — live hot path only, never an
  analysis read bound.
- Tracking-cache seed cap (like `SEED_TRACKING_LIMIT`) — not the token-list page cap.
- Don't raise cache TTLs or caps on server to "make analysis easier" — sync to lab instead.
- Sell-confirm stays feed-based (no RPC poll on hot path).

**Why two bins in the scaffold, not later:** a single `app` bin that eventually links ingest +
launcher + lake + DuckDB repeats the pre-split mistake — EC2 drags in `arrow`/`parquet` it
never runs, and binary size/RSS creep silently. Split at the `Cargo.toml` member graph from
commit 1; lake crate can stay empty (`○`) but `lab` must exist as the only consumer.

---

## 2. Design principles carried from the current schema

The current repo's genuinely good ideas — keep verbatim:

1. **Raw feed + typed projection.** `raw_txs` (unparsed BYTEA source-of-truth, short retention)
   → `trades` (typed projection). Replayable; the feed table *is* the transport.
2. **Integer base units, "type by meaning."** On-chain quantities stored as exact integers
   (BIGINT); human values derived. Prices stay `f64`.
3. **Identity SSOT.** One key per concept everywhere (`mint_address`), extended with
   `launchpad_id` / `quote_asset_id` / `market_id`.
4. **Extensibility via ROWS, not COLUMNS.** A new launchpad or quote asset is a new dimension
   *row*, never a schema migration.
5. **Interning for hot-path perf.** `wallet_dict` (4-byte `wallet_id` in `trades`, soft ref, no
   FK on the hot insert). Add small-int `quote_assets`/`launchpads` dimensions for the same reason.
6. **Hypertables + declarative retention/compression** for high-volume tables; continuous
   aggregates for OHLCV.
7. **Typed columns + JSONB "brain"** for extensible config (rules, launch templates, settings).
8. **Derived-never-stored** as views (`market_cap`, `age`, PnL); add USD projections here.
9. **Dedup-key-as-PK** on hot tables (the ordering/dedup key IS the PK; no surrogate).
10. **Two composition roots.** Live workloads (ingest, launch, trade, sell-confirm) and analysis
    workloads (lake, sweep, backtest) link **disjoint dep graphs** — enforced by crate boundaries,
    not feature flags. Storage is two-tier; **runtime** is two-tier too.

---

## 3. Core redesign — venue + quote-asset generalization

### 3a. Quote asset becomes first-class (the biggest change)

- `*_quote` = BIGINT, exact **base units of the quote asset** (was `amount_lamports`).
- `*_base` = BIGINT, exact **base units of the token** itself (was `token_amount`).
- The quote's `mint` + `decimals` come from a **`quote_assets`** dimension. **Native SOL is just
  the QuoteAsset whose base unit is the lamport** (`is_native`, decimals 9); USDC is
  non-native, decimals 6.
- Evolution of the SOL/lamports rule: the "unit-in-name, store-integer/display-float" discipline
  is preserved, but the unit is now the *referenced quote/base asset*, not a hard-coded lamport.

### 3b. Launchpad + market kind become first-class

`venue IN ('curve','amm')` → two orthogonal dimensions: **`launchpad_id`** (which platform, a
row in `launchpads`) and **`market_kind`** (`bonding_curve | amm | clmm | orderbook`). A token
moves across markets over its life (curve → graduates → AMM), so it has 1..n **markets**.

### 3c. Own vs. observed

The platform both **creates** tokens (own launches/wallets) and **observes** the whole market
(ingested trades). `tokens.is_own_launch` flags ours, so "eat-bots" analytics can compare our
launches against the world.

### 3d. USD as the cross-quote numeraire

To compare a SOL-quoted token against a USDC-quoted one, every quote carries a USD rate
(`quote_assets.usd_rate`, poller-updated; USDC≈1, SOL from the price poller). USD values are
**derived in views**, never stored.

### 3e. Bundler leg structure (dynamic, composed from audited parts)

A bundle's N buy txs must NOT look uniform, or a decoder like our own `ingest-laserstream`
fingerprints the bundle and bots won't ape in. But an instruction's **account list is fixed by
the on-chain program** — you cannot reshape it (arbitrary accounts just revert). The safe
variation surface (chosen level: **variant + params + budget/tip**) is: **which audited buy/sell
variant** (~5 pump.fun buy discriminators), **amount/slippage**, and **compute-budget + Jito-tip
values/placement**. Each leg is composed per-tx from these audited parts
(correct-by-construction), NOT authored as an arbitrary account list — the *safe* extension of
"selectable variants," not a runtime interpreter. Persisted as a **per-leg structure
descriptor** (§4 Domain D). Executes in phase 2; the schema seam lands now.

---

## 4. Concrete schema — by domain

### Domain A — Dimensions (small, slow-changing, interned)

```sql
CREATE TABLE quote_assets (            -- SOL, USDC, …
    id            SMALLINT PRIMARY KEY,          -- interned, stamped on hot rows
    mint          TEXT NOT NULL UNIQUE,          -- WSOL mint for SOL
    symbol        TEXT NOT NULL,
    decimals      SMALLINT NOT NULL,             -- 9 SOL, 6 USDC
    is_native     BOOLEAN NOT NULL,              -- true → lamports + wrap/unwrap path
    usd_rate      DOUBLE PRECISION,              -- poller-updated; USDC≈1
    usd_rate_at   TIMESTAMPTZ
);

CREATE TABLE launchpads (              -- pump.fun, raydium_launchlab, …
    id                     SMALLINT PRIMARY KEY,
    key                    TEXT NOT NULL UNIQUE,  -- 'pump_fun'
    display_name           TEXT NOT NULL,
    default_quote_asset_id SMALLINT NOT NULL REFERENCES quote_assets(id),
    meta                   JSONB NOT NULL DEFAULT '{}',   -- program ids, fee accts, curve consts
    created_at             TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE markets (                 -- a token's tradeable venue instance (curve, then amm)
    id             BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    mint_address   TEXT NOT NULL REFERENCES tokens(mint_address) ON DELETE CASCADE,
    launchpad_id   SMALLINT NOT NULL REFERENCES launchpads(id),
    market_kind    TEXT NOT NULL CHECK (market_kind IN ('bonding_curve','amm','clmm','orderbook')),
    program_id     TEXT NOT NULL,
    quote_asset_id SMALLINT NOT NULL REFERENCES quote_assets(id),
    pool_address   TEXT,
    created_slot   BIGINT,
    UNIQUE (mint_address, launchpad_id, market_kind)
);
```

Seed rows: `quote_assets` = {SOL, USDC}; `launchpads` = {pump_fun}.

### Domain B — Token identity + market state (observed universe)

```sql
CREATE TABLE tokens (                  -- static creation facts (write-once), PK = mint
    mint_address          TEXT PRIMARY KEY,
    launchpad_id          SMALLINT NOT NULL REFERENCES launchpads(id),
    quote_asset_id        SMALLINT NOT NULL REFERENCES quote_assets(id),
    creator_wallet        TEXT NOT NULL,
    is_own_launch         BOOLEAN NOT NULL DEFAULT FALSE,     -- did WE launch it
    name                  TEXT NOT NULL,
    symbol                TEXT NOT NULL,
    decimals              SMALLINT NOT NULL,                  -- token base decimals (6 on pump)
    token_program_id      TEXT,
    initial_supply_base   BIGINT,                             -- base units
    initial_buy_quote     BIGINT,                             -- quote base units
    creation_slot         BIGINT,
    creation_tx_signature TEXT NOT NULL,
    ix_labels             JSONB NOT NULL DEFAULT '[]',
    meta                  JSONB NOT NULL DEFAULT '{}',
    created_at            TIMESTAMPTZ NOT NULL
);

CREATE TABLE token_market_state (      -- hot-updated live metrics (was tokens_info)
    mint_address     TEXT PRIMARY KEY REFERENCES tokens(mint_address) ON DELETE CASCADE,
    current_price_q  DOUBLE PRECISION,          -- price in QUOTE units (quote per base)
    ath_price_q      DOUBLE PRECISION,
    ath_at           TIMESTAMPTZ,
    volume_quote     BIGINT NOT NULL DEFAULT 0, -- quote base units
    trade_count      BIGINT NOT NULL DEFAULT 0,
    last_trade_at    TIMESTAMPTZ,
    is_dead          BOOLEAN NOT NULL DEFAULT FALSE,
    is_migrated      BOOLEAN NOT NULL DEFAULT FALSE,
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE token_sync_state (        -- per-(mint, market) ingest watermark (rows, not cols)
    mint_address TEXT NOT NULL REFERENCES tokens(mint_address) ON DELETE CASCADE,
    market_id    BIGINT NOT NULL REFERENCES markets(id) ON DELETE CASCADE,
    last_sig TEXT, last_slot BIGINT, last_synced_at TIMESTAMPTZ,
    PRIMARY KEY (mint_address, market_id)
);
```

`token_overview` view joins the two + derives `market_cap`, `age_secs`, and a USD projection.

### Domain C — The feed (high-volume hypertables)

```sql
CREATE TABLE raw_txs (                  -- carried ~as-is: source-of-truth unparsed feed
    tx_signature BYTEA NOT NULL, slot BIGINT NOT NULL, block_time TIMESTAMPTZ NOT NULL,
    tx_index INTEGER NOT NULL, payload BYTEA NOT NULL, source SMALLINT NOT NULL DEFAULT 0,
    PRIMARY KEY (block_time, tx_signature)
);  -- hypertable(block_time,1d); compress@2d; retain@7d

CREATE TABLE trades (                   -- typed projection; quote/venue-generalized
    mint_address    TEXT NOT NULL,
    wallet_id       INTEGER NOT NULL,               -- interned soft ref → wallet_dict
    launchpad_id    SMALLINT NOT NULL,              -- denormalized (hot path, no join)
    market_kind     TEXT NOT NULL,                  -- 'bonding_curve'|'amm'|…
    quote_asset_id  SMALLINT NOT NULL,              -- denormalized
    trade_type      TEXT NOT NULL CHECK (trade_type IN ('buy','sell')),
    quote_amount    BIGINT NOT NULL,                -- quote base units (was amount_lamports)
    base_amount     BIGINT NOT NULL,                -- token base units (was token_amount)
    reserve_quote   BIGINT,                         -- venue-neutral price pair
    reserve_base    BIGINT,
    slot BIGINT NOT NULL, tx_index INTEGER NOT NULL, leg_index SMALLINT NOT NULL DEFAULT 0,
    block_time TIMESTAMPTZ NOT NULL, tx_signature BYTEA NOT NULL,
    PRIMARY KEY (block_time, tx_signature, leg_index)   -- dedup key = PK
);  -- hypertable(block_time,1d); segmentby mint_address; compress@7d; retain@30d
    -- idx (mint_address, slot, tx_index, leg_index)
```

`wallet_dict` (interning) carried verbatim. `trades_priced` view derives `price_quote =
quote_amount/base_amount` (÷ decimals) and `price_usd` via the quote's USD rate. OHLCV
continuous aggregates are per-quote.

### Domain D — Own-launch domain (NEW)

```sql
CREATE TABLE managed_wallets (          -- OUR wallets (dev / bundler / treasury)
    id             UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    address        TEXT NOT NULL UNIQUE,
    label          TEXT,
    role           TEXT NOT NULL CHECK (role IN ('dev','bundler','treasury','trading')),
    key_ref        TEXT NOT NULL,        -- SECURITY: reference to external keystore/KMS/encrypted
                                         -- blob, NEVER the raw private key (see §5)
    derivation_index INTEGER,
    is_active      BOOLEAN NOT NULL DEFAULT TRUE,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE launch_templates (         -- authored launch specs (typed + JSONB brain)
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    template_name TEXT NOT NULL, launchpad_id SMALLINT NOT NULL REFERENCES launchpads(id),
    variant TEXT NOT NULL,               -- 'pumpfun.create_v1' | '…create_v2'
    quote_asset_id SMALLINT NOT NULL REFERENCES quote_assets(id),
    params JSONB NOT NULL DEFAULT '{}',   -- name/symbol/uri/image/dev_buy + leg_structures pool
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(), updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE launches (                 -- executed launch record
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    template_id UUID REFERENCES launch_templates(id) ON DELETE SET NULL,
    mint_address TEXT NOT NULL, launchpad_id SMALLINT NOT NULL REFERENCES launchpads(id),
    variant TEXT NOT NULL, quote_asset_id SMALLINT NOT NULL REFERENCES quote_assets(id),
    dev_wallet_id UUID REFERENCES managed_wallets(id),
    create_signature TEXT, dev_buy_quote BIGINT,   -- quote base units
    bundle_id UUID,                                 -- phase-2 seam
    status TEXT NOT NULL DEFAULT 'pending',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

Phase-2 seam — `bundles` (atomic Jito bundle for a launch's buy legs): `id, launch_id, status,
tip_quote,` and `legs JSONB = [{ wallet_id, quote_amount, structure }]` where `structure` is the
per-leg descriptor (surface = variant + params + budget/tip):

```text
{ variant:'buy'|'buy_exact_sol_in'|'buy_v2'|…,   -- audited pump.fun discriminator
  slippage_bps, cu_limit, cu_price,               -- randomized within ranges
  tip_account_ix, tip_quote, ix_order }           -- decoration/ordering knobs
```

Reusable recipes live in `launch_templates.params.leg_structures` (a pool the composer draws
from). Every field selects an AUDITED builder/param — never an arbitrary account list.

### Domain E — Strategy + analytics (carry the pattern)

Carry `strategy_rules` / `strategy_runs` / `strategy_run_metrics` / `strategy_positions` (typed +
JSONB brain), generalizing amount columns to `*_quote`/`*_base` and PnL to quote→USD in views.
Carry the wallet directory (`wallet_profiles` / `wallets` / `wallet_profile_tags`) — central to
"analyze wallets / eat bots" (seed tags already include `Bot`, `Sniper`, `Bundler`, `MEV`).

### Domain F — Multi-RPC (light)

`rpc_providers` (id, url, role `send|read|grpc`, is_active) as config-ish reference; defer a
latency/health metrics hypertable until observability is actually needed.

---

## 5. Cross-cutting rules

- **Secrets never in Postgres.** `managed_wallets.key_ref` is a *reference* (keystore path / KMS
  key id / envelope-encrypted blob with an external KEK), never a plaintext private key. Signing
  goes through `pump-trader`'s `Arc<dyn Signer>` (HSM/remote-ready). `.env` holds connection
  secrets only.
- **SSOT keys:** `mint_address`, `launchpad_id`, `quote_asset_id`, `market_id`. A new
  launchpad/quote = a new dimension **row**, never a migration.
- **Derived-never-stored:** `market_cap`, `age`, PnL, and all **USD** projections live in views.
- **Data-scale guardrails carried:** bound every query; hypertables + declarative
  retention/compression on `raw_txs`/`trades`; the lake is sealed-days-only with a PG fresh tail.
- **Deployment guardrails carried:** `live` on EC2 only; `lab` + lake on workstation only; DB
  sync for analysis; token list SQL-paged on live / in-RAM snapshot on lab; no DuckDB on server.
- **Instruction variation ≠ full stealth (parallel workstream).** Per-leg structure variation
  (§3e) defeats naive "identical-tx" detection but NOT funding-graph/timing analysis (N fresh
  wallets funded from one source, same mint, same slot). Wallet-funding obfuscation is a separate
  concern `managed_wallets` must eventually support (funding source, hop graph) — flag now.

---

## 6. Cold tier — Parquet/DuckDB lake (lab / workstation only)

Mirror `trades` + a `tokens` dimension to sealed-day Parquet files with generalized columns
(`quote_amount`, `base_amount`, `reserve_quote`, `reserve_base`, `launchpad_id`,
`quote_asset_id`). Carry the pattern: DuckDB name-based reader, **column names single-sourced in
one `lake/schema.rs`** with a writer/reader guard test, sealed-days-only + PG fresh-tail union
for recent tokens, parity test vs. PG.

**Runtime placement (non-negotiable):**

- The `lake` crate is a **`lab` dependency only** — `live` must not appear in its reverse dep
  chain.
- `lake-export` runs on the **workstation** against the **synced local PG** mirror, on a cron
  (e.g. nightly + `--include-today`). EC2 retains only enough PG history for live trading +
  the sync watermark; it does not host Parquet files or run export jobs.
- Simulate/sweep/backtest endpoints live in **`lab`** and read lake (+ PG fresh tail for tokens
  newer than the last export). Single-rule simulate reads Parquet, not PG — same as
  `hunter`.
- EC2's contribution to the lake is **being the upstream PG source** that gets synced — not
  running the cold tier.

Scaffold the `lake` crate + empty `lab` bin early (`○`); fill after the feed is live.

---

## 7. Implementation order (this step = foundation)

1. **Scaffold** — new repo; workspace `Cargo.toml` with path deps on `pump-trader` +
   `ingest-laserstream`; **two bin members (`live`, `lab`) from commit 1** with dep-partition
   stubs (even if bodies are `todo!()`); `docker-compose.yml` (Postgres + TimescaleDB);
   `.env.example`; copy the `lamports↔sol` SSOT constants + the IDLs; add `cargo tree` dep-check
   notes to README.
2. **Migration `0001_init.sql`** — Domains A–C (dimensions, tokens, feed) + seeds (SOL, USDC,
   pump_fun) + hypertables/policies. Boot-time continuous-aggregate setup in `timescale.rs`.
3. **`platform-core`** — models + one repo per table; config; TimescaleDB boot setup;
   `venue/` trait contracts.
4. **Prove the generalization (key test)** — insert a mock USDC-quoted token + a SOL-quoted
   token; confirm the same `trades`/views handle both (USD-comparable) with **no schema change**.
5. **Domain D** — `managed_wallets` (key_ref only), `launch_templates`, `launches` (the tables
   token creation needs first).
6. **`ingest-host` + `live` scaffold** — bridge borrowed `ingest-laserstream` events →
   `raw_txs`/`trades` with the pump.fun/SOL adapter (`launchpad_id=pump_fun`,
   `quote_asset_id=SOL`); round-trip check via `cargo run -p live`.

7. **`lab` scaffold** — empty `lab` bin + stub `lake` crate (no DuckDB yet); document DB-sync +
   `lake-export` workflow; port or rewrite `db-incremental-sync.ps1` for the new schema.

Domains E–F and the lake fill follow on the workstation side as the platform grows.

---

## 8. Verification (zero real SOL)

- **Migrations apply clean** on a fresh Postgres + TimescaleDB (`sqlx migrate run`); hypertables +
  policies present (`SELECT * FROM timescaledb_information.hypertables`).
- **Generality proof:** the mock USDC + SOL token test in step 4 — the core proof the venue/quote
  generalization needs no schema change.
- **Ingest round-trip:** run `ingest-laserstream` against live pump.fun; confirm decoded events
  land in `trades` with the right `launchpad_id`/`quote_asset_id` and `reserve_quote/base`
  populated; spot-check `trades_priced` price vs. a known trade.
- **Dep partition:** `cargo tree -p live` shows no `duckdb`/`arrow`/`parquet`; `cargo tree -p lab`
  shows no `pump-trader`/`ingest-laserstream`.
- **Lake parity (lab / workstation):** export a sealed day from synced local PG; DuckDB read
  matches PG for the same window. Not run on EC2.
- Read-only Postgres inspection via the `postgres` MCP tool throughout.

---

## 9. Open decisions — **RESOLVED** (see [`forge/docs/decisions.md`](forge/docs/decisions.md))

1. **Amount naming** → **LOCKED:** asset-referenced `*_quote`/`*_base` (dual `*_lamports`
   vocab rejected). Already implemented across migrations + models.
2. **`markets` table vs. denormalized-only** → **LOCKED (hybrid):** keep the `markets`
   dimension for metadata + denormalize `launchpad_id`/`market_kind`/`quote_asset_id` onto
   `trades` for the hot read. Already implemented.
3. **Managed-wallet keystore backend** → **CHOSEN:** envelope-encrypted file + pluggable KEK
   trait (env/passphrase now → AWS KMS later); ed25519 signs in-process (KMS can't sign it).
   Builds in the phase-2 launcher; schema already stores `key_ref` only.
4. **USD rate source** → **CHOSEN:** USDC pinned to 1.0 + SOL price poller (carried from
   hunter); USD derived in views only; live oracle deferred.
5. **Wallet-funding obfuscation** → **DEFERRED** (parallel workstream, post-launcher);
   `managed_wallets` must eventually record funding source + hop graph.
