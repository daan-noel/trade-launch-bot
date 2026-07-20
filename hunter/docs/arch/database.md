# Database — schema & repositories

> Deep-dive schema reference lives in `@plans/database/`: [token-storage.md](@plans/database/token-storage.md), [trades-storage.md](@plans/database/trades-storage.md), [raw-txs-storage.md](@plans/database/raw-txs-storage.md), [strategy-storage.md](@plans/database/strategy-storage.md). The overview below is the canonical map; the plans have column rationale, index decisions, TimescaleDB config, and open design questions.

sqlx + Postgres. Raw SQL lives **only** in `trading_core/src/storage/repositories/*`. **Two migration sets, applied by separate runners so lab-only tables never reach EC2/live:**

- **Shared core** — `trading_core/migrations/` (`0001_init.sql` = clean TimescaleDB baseline; add further `00NN_*.sql`). Runner: `sqlx::migrate!("./migrations")` in `storage/postgres.rs::connect()` (then `timescale::setup_caggs`). Run by **both** bins on boot.
- **Lab-only** — `lab/migrations/` (`0001_grouped_sweep.sql` … `0005_retire_legacy_sweep_tables.sql` = the `grouped_sweep_*` tables). Runner: `lab::storage::lab_migrations::run()`, called from `lab/main.rs` after `connect()`. Tracked in a lab-private **`_lab_migrations`** ledger (its own checksum table; reuses `migrate!` only as an embedder, never `.run()`, so core's `_sqlx_migrations` is untouched). Run by **`lab` only** → these tables never exist on EC2. Add a lab-only table = drop `NNNN_*.sql` into `lab/migrations/`.
Deep-dive detail: `@plans/database/db-pool-routing.md`, `@plans/database/db-patterns.md`.

## Connection pools

`storage/postgres.rs::connect()` builds three workload-isolated `PgPool`s (`DbPools { hot, api, batch }`):

- **hot** (default 64 conn) — ingest DbWriter, StrategyRunner, maintenance, seed. In `main.rs` as `db`.
- **api** (default 32 conn) — `CoreState.db()`; fast HTTP handlers; 8s `statement_timeout`.
- **batch** (default 16 conn) — `CoreState.batch_db()`; grouped sweeps, backtests, token_sync backfill. No statement timeout.

## Amount typing — "type by real-world meaning"

Every on-chain quantity is stored as an **exact integer**; only ratios/statistics are
float. This holds across `trades`, `tokens`, and `strategy_positions`:

- **Token amounts** → raw units, `BIGINT` column **and** `u64` in the model end-to-end
  (`Trade.token_amount`, token reserves, `strategy_positions.*_token_amount`,
  `SigLegs`). The old `f64` model field silently lost precision above 2^53 on large
  legs — that was the bug this convention fixes.
- **SOL** → lamports, `BIGINT` column. The model keeps SOL as human `f64`; conversion
  happens at the repo boundary via **one shared pair** —
  `config::constants::{sol_to_lamports, lamports_to_sol}` (in `token_math.rs`; rounds,
  so a value round-trips exactly). Every repo (`trade`/`token`/`token_info`/`strategy`)
  imports these instead of a private copy; `pump-trader` keeps its own `u64` truncating
  variant by design. Exactness lives in the column (`trades.amount_lamports`,
  `tokens.initial_buy_lamports`, `strategy_positions.entry_lamports/exit_lamports`).
- **Unit-in-the-name rule (no exceptions):** every field/column/variable that denotes an
  amount of SOL names its unit. `_lamports` = exact integer (`BIGINT`/`i64`/`u64`); `_sol`
  = human `f64`. Same base concept, unit-only suffix differs by layer: the DB stores
  `entry_lamports`, the model exposes `entry_sol`. If a name contained `sol` but held
  lamports the word is dropped (`reserve_sol` → `reserve_lamports`, not `reserve_sol_lamports`).
  Ratio/rate fields (`*_price`, `price_per_token`, `*_pct`, `cu_price`) are **not** amounts
  and keep their `_price`/`_pct` names. See migration `0009_sol_lamports_naming.sql`.
- **Prices/stats** → `f64` (genuine ratios: SOL per raw token unit; PnL %, win rate,
  volume). Any `price × tokens` casts the `u64` count `as f64` at the multiply.
- **Views** divide lamports back to SOL (`strategy_position_pnl.realized_pnl_sol`,
  `trades_priced.price_per_token`). **Frontend** receives integer JSON numbers and
  scales for display ("store integer, display float").

### Derived-value single sources

- **Market cap** = spot price × supply, defined once per surface: `MARKET_CAP_SQL`
  (`storage::token_enrichment` — `current_price × initial_supply_token`) is spliced into
  every SQL projection/sort/filter (`ENRICH_SELECT`, `token_repo`, `handlers::tokens`,
  `sql`); the live in-RAM path uses `config::constants::market_cap_sol` (same per-token
  supply, falling back to the mayhem-aware constant only when unknown), so the two agree.
  `ENRICH_SELECT` is pinned to `MARKET_CAP_SQL` by a guard test.
- **Token-list filter/sort grammar** has two backends (live SQL `handlers::tokens::sql`,
  lab in-RAM `TokenQuery`); `tokens::grammar_parity_tests` (no DB) asserts they recognize
  the same column keys, and `token_repo::parity_tests` (auto-runs when `DATABASE_URL` is
  set, self-skips otherwise) asserts identical ordered rows. Requested via the unified
  `POST /api/tokens` [`TableRequest`] body (same contract as the strategy tables); the
  global filter panel + per-column filters arrive as ONE `filters: {col → FilterSpec}` map
  and `TokenQuery::from_table_request` **lowers** each spec back onto the internal panel-map
  / per-column-predicate representation both engines already evaluate — so the fold added no
  new eval code and the parity guarantee still holds. `ath_price`/`current_price` are
  numeric-filterable; free-text search is mint/symbol only.
- **`TokenDetail`** coalesces `trade_count`/`volume_sol_total` to 0 (non-null), matching
  the list endpoint's `TokenSummary` — the detail modal and the list agree on those two.

## Tables

### Core trading

- `tokens` — mint_address UNIQUE, creator_wallet, name/symbol, bonding_curve_address, initial_buy_lamports(BIGINT), cu_limit/price, is_mayhem_mode, ix_labels(JSONB), initial_buy_instruction(JSONB; keys `max_cost_lamports`/`spendable_lamports_in`), creation_slot(BIGINT), created_at
- `trades` *(TimescaleDB hypertable on block_time, ~1mo retention)* — mint, wallet, trade_type, amount_lamports(BIGINT) / token_amount(BIGINT raw units), reserve_lamports/reserve_token(BIGINT venue-neutral pair), tx_signature(BYTEA), slot, block_time, venue(`curve`/`amm`), **`ix_labels`(JSONB, migration 0002, forward-only)**; price derived in `trades_priced` view (`price_per_token` = SOL/token). PK `(block_time, tx_signature, leg_index)`. **This table = the LaserStream feed.**
- `raw_txs` *(TimescaleDB hypertable on block_time; compress 2d, retain 7d)* — tx_signature(BYTEA), slot, block_time, tx_index, payload(BYTEA = verbatim protobuf wire bytes, parse in Rust), source(SMALLINT: 0=live 1=sync). PK `(block_time, tx_signature)`. Source-of-truth feed; `trades` is a typed projection. Written by `RawTxRepo` from both the live ingest db_writer and the token_sync backfill.

### Token analysis

- `tokens_info` — ATH, age, volume_sol(DOUBLE PRECISION), market_cap, trade_count, is_dead, is_migrated, first_slot_buy_lamports/first_slot_sell_lamports(BIGINT — same-creation-slot buy/sell totals, streamed in `TokenState`), sync watermarks

### Strategy (unified across all strategies — rows not tables per strategy)

- `fingerprints` — shared match axes (`cu_limit`/`cu_price`/amount buckets/`ix_labels`/…); **`metric_config` JSONB NOT NULL DEFAULT '{}'** (migration 0006) — top-level keys = metric group names (e.g. `m_flow_split.volume_ix_patterns`). Identity predicate for `find_or_create` **ignores** `metric_config` (patterns are config, not identity).
- `strategy_rules` — `fingerprint_id` FK + `buy_amount_lamports`, `trade_mode`, `is_active`, `max_concurrent_tokens`, `max_total_tokens`, `params`(JSONB: TP/SL + entry/exit metric groups). See [strategy-storage.md](@plans/database/strategy-storage.md).
- `strategy_runs` — one activation session; `run_seq` monotonic per `(rule, mode)`; `params_snapshot` frozen at activation
- `strategy_run_metrics` — 1:1 finalize-time rollup (`win_rate`, `total_pnl_sol`, exit-reason mix, etc.)
- `strategy_positions` — one bot-opened position; `mint_address` (the SPL mint — renamed from `mint` in `0002_strategy_positions_mint_address.sql` so the physical column matches the token-data SSOT key); `status`(`Arming`/`BuySubmitted`/`Holding`/`ExitPending`/`End`/`ExitFailed`); amounts as BIGINT (lamports/raw units); `submitted_buy_signatures TEXT[]` for in-flight recovery; `token_account TEXT` (nullable) — the wallet's token account for the mint, persisted on the entry fill so a re-buy reuses one account and the sell reads it from the row (restart-safe, no in-memory-cache dependency)

### Analysis lake (lab Parquet — not Postgres)

Sealed-day trade files (`lab/src/lake/`) carry optional per-trade **`ix_labels`**
(JSON-string, dict-encoded) and **`wallet`** (address; export LEFT JOINs
`wallet_dict` with `unknown:{id}` COALESCE). Loaded only when
`Selection.with_flow` (flow metrics / discovery). Pre-V0 sealed days stay NULL.
See [lake-pg-read-paths.md](@plans/database/lake-pg-read-paths.md).

### Grouped param-sweep (generic table-name-driven repo)

One family since the Phase 7 retirement — the per-strategy `{tpsl1,tpsl2,swing_1}_grouped_sweep_*`
tables were dropped in `lab/0005`:

- `grouped_sweep_runs` — run metadata, status(`running`/`completed`/`cancelled`), groups_done, corpus filters, label
- `grouped_sweep_groups` — one per fingerprint group; best_combo_id, best_expectancy_sol, best_score
- `grouped_sweep_combos` — per-run combo→params (`RuleParams`) dictionary (deduped; JOINed back on read)
- `grouped_sweep_results` — per (group, combo): score, win_rate, PnL metrics, exit-reason mix (incl. `n_exit_metrics`). Retention-filtered to ~660 rows/group max

### Wallets / settings

- `wallet_profiles`, `wallets`, `wallet_profile_tags` — profile/wallet/tag CRUD
- `app_settings` — key/value(JSONB); keys: `ingest.*`, `ui.*`, `trade.*`

## Repositories (`storage/repositories/`)

| File | Table(s) | Notable fns |
| --- | --- | --- |
| `token_repo.rs` | tokens (+tokens_info) | `find_list_rows` (DB base for /api/tokens; `TokenListRow` carries the `tokens_info` metrics incl. `first_slot_buy_sol`/`first_slot_sell_sol`, divided to human SOL in the SELECT), `find_page_before` (keyset page for analysis scans), `find_by_mints` (chunked mint=ANY) |
| `trade_repo.rs` | trades | `find_fill_by_signature`, `sum_legs_by_signatures` (per-sig attribution), `avg_entry_by_wallet_and_mints` (**manual-buy cost-basis SSOT** → `AvgEntry {avg_entry_price (SOL/raw), total_token_amount, total_cost_lamports}` per mint, `SUM(amount_lamports)/SUM(token_amount)` over `trade_type='buy'`; bounded by the held-mint set; feeds the portfolio service), `wallet_traded_mints` (**Trader Analysis** → `WalletTradedMint {mint_address, last_trade_at, buy_count, sell_count}` per distinct mint the wallet traded in a `block_time>=since` window, `GROUP BY mint_address ORDER BY MAX(block_time) DESC LIMIT n` + `COUNT(*) FILTER (WHERE trade_type=…)`; both buy+sell count; empty for an unknown wallet; feeds lab `GET /api/wallets/:wallet/tokens`, which merges each with the full token row via `TokenRepo::find_list_rows_for_mints` → `WalletTokenRow` = flattened `TokenSummary` + `wallet_{last_trade_at,buy_count,sell_count}`. Rides the **lab-only** covering index `idx_trades_wallet_time (wallet_id, block_time DESC, mint_address, trade_type)` — index-only, zero EC2 ingest cost), `for_each_seed_mint` (cold-start seed), `sig_bytes_to_base58` (`pub`; BYTEA→base58, reused by the lake export), `find_by_mints_all` (batched per-mint grouped reads; **reconstructs** the dropped `real_reserve_sol` via `approx_real_sol_reserves(reserve_sol, venue)`, never the live path — but the **simulate/backtest path no longer calls it**: single-rule simulate now reads the Parquet lake like the sweep, which bakes that same value in at export/read. Retained for ad-hoc grouped reads / tests) |
| `raw_tx_repo.rs` | raw_txs | `insert`, `insert_many` (ON CONFLICT DO NOTHING) |
| `token_info_repo.rs` | tokens_info | `upsert_metrics`, `get/update_sync_watermark` |
| `creation_stats_repo.rs` | tokens (+tokens_info) | `heatmap`, `trend`, `grouped` — TZ-aware SQL, bucket granularities, per-field corpus filters |
| `settings_repo.rs` | app_settings | `load_all`, `set_one`, `set_many` |
| `grouped_sweep_repo.rs` | `<strategy>_grouped_sweep_*` | incremental writes: `insert_run`, `append_group`, `finalize_run`, `mark_cancelled` |
| `wallet_repo.rs` | wallets | `touch_last_seen_many` |
| `wallet_profile_repo.rs` / `wallet_profile_tag_repo.rs` | wallet_profiles, tags | CRUD |
| `fingerprint_repo.rs` | fingerprints | `find_or_create` (identity axes only — **not** `metric_config`), `insert`/`update`/`list`/`delete`; persists `metric_config` |
| `strategy_repo.rs` | strategy_rules, strategy_runs, strategy_run_metrics, strategy_positions | `find_rule`, `insert_run`, `insert_position`, `update_position_status`, `mark_buy_submitted`, `find_all_holding`, `find_all_exit_pending`, `find_all_buy_submitted`, `find_reusable_token_account`, `fail_stale_exit_pending`, `finalize_run`, `find_positions_by_{run,rule}_paged` + `count_positions_by_{run,rule}` (page + total for the positions table's `X-Total-Count`), `find_tokens_by_mints_paged` + `count_tokens_by_mints` (the **matched** table: `tokens t LEFT JOIN tokens_info i` scoped to a materialized `mint = ANY($set)`, selecting the shared `token_enrichment::ENRICH_SELECT` → full `TokenEnrichmentRow`, so the response carries all ~28 enrichment fields with no client merge), `positions_summary_by_{run,rule}` (single `COUNT/SUM FILTER` aggregate for the Positions Summary panel over an optional `PositionQuery` filter — same JOIN + WHERE as the paged list; win = `status='End' AND exit_sol>entry_sol`, mirrors `StrategyPosition::is_win`; SOL sums cast BIGINT→human), `rule_counters_for_latest_paper_runs` (one batched `GROUP BY` — per-rule open/pending/total/win/loss/win_rate/avg_pnl_pct/pnl over each rule's latest paper run; same win predicate; feeds the **lab** rules-table counters, which have no runtime cache). `avg_pnl_pct` here **and** in `positions_summary_by_*` **and** the live runtime cache is the one canonical **capital-weighted return** = `Σ realized_pnl_lamports / Σ entry_lamports (closed) × 100` via `strategies::kernel::weighted_return_pct` (SSOT) — NOT a mean of per-trade price %, so its sign is always locked to `total_pnl_sol` (the old mean could show `+%`/`−◎` on the same rule), `find_open_positions` (all unsettled positions cross-rule), `managed_mints(real_only)` (projection-only "who manages this mint" — open positions `LEFT JOIN strategy_rules` for the rule name; `→ ManagedMint`; backs the portfolio bot badge / double-sell interlock), `realized_pnl_lamports_since(ts)` (real `End`-position `SUM(exit_lamports−entry_lamports)` since a boundary — the "realized today" KPI) |

## Rules

- Always bound queries — paginate/time-window/stream. Never `SELECT *` full `trades`/`raw_txs`.
- New high-volume tables → TimescaleDB hypertable with `add_compression_policy` + `add_retention_policy` (see `@plans/database/trades-storage.md` for the pattern). `maintenance.rs` is deleted.
- Bulk-insert must chunk by `floor(65535 / binds_per_row)` — sqlx 0.6 has no guard against the 65535 bind-param ceiling.
- **Server-side table filters are structured + type-checked.** The strategy token tables take a unified `TableRequest` (POST/JSON, `trading_core::api::table_query`); per-column filters are `{op, val}` (`FilterOp`: contains/eq/gt/gte/lt/lte/between). `strategy_repo` splits its whitelist into typed `(sql_expr, FilterKind::{Text,Numeric})` rows: numeric cols return the **uncast** expr so `gt`/`between` compare numerically (operand bound as `f64`), text cols keep `ILIKE`. `push_filter_predicate` lowers each op to a bound predicate; an illegal pairing (numeric op on a text col, non-number operand) is **dropped**, like an unknown key — every operand `push_bind`s (injection-safe). No user text ever reaches an identifier.
