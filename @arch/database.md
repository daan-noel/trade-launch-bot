# Database — schema & repositories

> Deep-dive schema reference lives in `@plans/database/`: [token-storage.md](@plans/database/token-storage.md), [trades-storage.md](@plans/database/trades-storage.md), [raw-txs-storage.md](@plans/database/raw-txs-storage.md), [strategy-storage.md](@plans/database/strategy-storage.md). The overview below is the canonical map; the plans have column rationale, index decisions, TimescaleDB config, and open design questions.

sqlx + Postgres. Raw SQL lives **only** in `trading_core/src/storage/repositories/*`. **Two migration sets, applied by separate runners so lab-only tables never reach EC2/live:**

- **Shared core** — `trading_core/migrations/` (`0001_init.sql` = clean TimescaleDB baseline; add further `00NN_*.sql`). Runner: `sqlx::migrate!("./migrations")` in `storage/postgres.rs::connect()` (then `timescale::setup_caggs`). Run by **both** bins on boot.
- **Lab-only** — `lab/migrations/` (`0001_grouped_sweep.sql` = the `tpsl{1,2}_grouped_sweep_*` tables). Runner: `lab::storage::lab_migrations::run()`, called from `lab/main.rs` after `connect()`. Tracked in a lab-private **`_lab_migrations`** ledger (its own checksum table; reuses `migrate!` only as an embedder, never `.run()`, so core's `_sqlx_migrations` is untouched). Run by **`lab` only** → these tables never exist on EC2. Add a lab-only table = drop `NNNN_*.sql` into `lab/migrations/`.
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
  (`sol_to_lamports`/`lamports_to_sol`) happens at the repo boundary — exactness lives
  in the column (`trades.sol_amount`, `tokens.initial_buy_sol`, `*.entry_sol/exit_sol`).
- **Prices/stats** → `f64` (genuine ratios: SOL per raw token unit; PnL %, win rate,
  volume). Any `price × tokens` casts the `u64` count `as f64` at the multiply.
- **Views** divide lamports back to SOL (`strategy_position_pnl.realized_pnl_sol`,
  `trades_priced.price_per_token`). **Frontend** receives integer JSON numbers and
  scales for display ("store integer, display float").

## Tables

### Core trading

- `tokens` — mint_address UNIQUE, creator_wallet, name/symbol, bonding_curve_address, initial_buy_sol(BIGINT lamports), cu_limit/price, is_mayhem_mode, ix_labels(JSONB), creation_slot(BIGINT), created_at
- `trades` *(TimescaleDB hypertable on block_time, ~1mo retention)* — mint, wallet, trade_type, sol_amount(BIGINT lamports) / token_amount(BIGINT raw units), tx_signature(BYTEA), slot, block_time, virtual_*_reserves(BIGINT), venue(`curve`/`amm`); price derived in `trades_priced` view. PK `(block_time, tx_signature, leg_index)`. **This table = the LaserStream feed.**
- `raw_txs` *(TimescaleDB hypertable on block_time; compress 2d, retain 7d)* — tx_signature(BYTEA), slot, block_time, tx_index, payload(BYTEA = verbatim protobuf wire bytes, parse in Rust), source(SMALLINT: 0=live 1=sync). PK `(block_time, tx_signature)`. Source-of-truth feed; `trades` is a typed projection. Written by `RawTxRepo` from both the live ingest db_writer and the token_sync backfill.

### Token analysis

- `tokens_info` — ATH, age, volume, market_cap, trade_count, is_dead, is_migrated, first_slot_buy_sol/first_slot_sell_sol(BIGINT lamports — same-creation-slot buy/sell totals, streamed in `TokenState`), sync watermarks

### Strategy (unified across all strategies — rows not tables per strategy)

- `strategy_rules` — `strategy_id` discriminator, `buy_amount`, `trade_mode`, `is_active`, `max_concurrent_tokens`, `max_total_tokens`, `params`(JSONB with strategy-specific gates). See [strategy-storage.md](@plans/database/strategy-storage.md).
- `strategy_runs` — one activation session; `run_seq` monotonic per `(rule, mode)`; `params_snapshot` frozen at activation
- `strategy_run_metrics` — 1:1 finalize-time rollup (`win_rate`, `total_pnl_sol`, exit-reason mix, etc.)
- `strategy_positions` — one bot-opened position; `status`(`Arming`/`BuySubmitted`/`Holding`/`ExitPending`/`End`/`ExitFailed`); amounts as BIGINT (lamports/raw units); `submitted_buy_signatures TEXT[]` for in-flight recovery; `token_account TEXT` (nullable, `0002_*.sql`) — the wallet's token account for the mint, persisted on the entry fill so a re-buy reuses one account and the sell reads it from the row (restart-safe, no in-memory-cache dependency)

### Grouped param-sweep (per-strategy triples; generic table-name-driven repo)

- `tpsl{1,2}_grouped_sweep_runs` — run metadata, status(`running`/`completed`/`cancelled`), groups_done, corpus filters, label
- `tpsl{1,2}_grouped_sweep_groups` — one per fingerprint group; best_combo_id, best_expectancy_sol, best_score
- `tpsl{1,2}_grouped_sweep_combos` — per-run combo→params dictionary (deduped; JOINed back on read)
- `tpsl{1,2}_grouped_sweep_results` — per (group, combo): score, win_rate, PnL metrics, exit-reason mix. Retention-filtered to ~660 rows/group max

### Wallets / settings

- `wallet_profiles`, `wallets`, `wallet_profile_tags` — profile/wallet/tag CRUD
- `app_settings` — key/value(JSONB); keys: `ingest.*`, `ui.*`, `trade.*`

## Repositories (`storage/repositories/`)

| File | Table(s) | Notable fns |
| --- | --- | --- |
| `token_repo.rs` | tokens (+tokens_info) | `find_list_rows` (DB base for /api/tokens; `TokenListRow` carries the `tokens_info` metrics incl. `first_slot_buy_sol`/`first_slot_sell_sol`, divided to human SOL in the SELECT), `find_page_before` (keyset page for analysis scans), `find_by_mints` (chunked mint=ANY) |
| `trade_repo.rs` | trades | `find_fill_by_signature`, `sum_legs_by_signatures` (per-sig attribution), `for_each_seed_mint` (cold-start seed), `find_by_mints_all` (batched per-mint grouped reads for backtests; **reconstructs** the dropped `real_sol_reserves` via `approx_real_sol_reserves(reserve_sol, venue)` — backtest-only, never the live path) |
| `raw_tx_repo.rs` | raw_txs | `insert`, `insert_many` (ON CONFLICT DO NOTHING), `find_by_signature` (PK lookup) |
| `token_info_repo.rs` | tokens_info | `upsert_metrics`, `get/update_sync_watermark` |
| `creation_stats_repo.rs` | tokens (+tokens_info) | `heatmap`, `trend`, `grouped` — TZ-aware SQL, bucket granularities, per-field corpus filters |
| `settings_repo.rs` | app_settings | `load_all`, `get_one`, `set_one`, `set_many` |
| `grouped_sweep_repo.rs` | `<strategy>_grouped_sweep_*` | incremental writes: `insert_run`, `append_group`, `finalize_completed`, `mark_cancelled`; compaction: `fetch_combo_metrics_for_group`, `delete_combos_except`, `vacuum_full_results` |
| `wallet_repo.rs` | wallets | `touch_last_seen_many` |
| `wallet_profile_repo.rs` / `wallet_profile_tag_repo.rs` | wallet_profiles, tags | CRUD |
| `strategy_repo.rs` | strategy_rules, strategy_runs, strategy_run_metrics, strategy_positions | `find_rule`, `insert_run`, `insert_position`, `update_position_status`, `mark_buy_submitted`, `find_all_holding`, `find_all_exit_pending`, `find_all_buy_submitted`, `find_reusable_token_account`, `fail_stale_exit_pending`, `finalize_run`, `find_positions_by_{run,rule}_paged` + `count_positions_by_{run,rule}` (page + total for the positions table's `X-Total-Count`), `positions_summary_by_{run,rule}` (single `COUNT/SUM FILTER` aggregate for the Positions Summary panel — win = `status='End' AND exit_sol>entry_sol`, mirrors `StrategyPosition::is_win`; SOL sums cast BIGINT→human), `rule_counters_for_latest_paper_runs` (one batched `GROUP BY` — per-rule open/pending/total/win/loss/win_rate/avg_pnl_pct/pnl over each rule's latest paper run; same win predicate; feeds the **lab** rules-table counters, which have no runtime cache) |

## Rules

- Always bound queries — paginate/time-window/stream. Never `SELECT *` full `trades`/`raw_txs`.
- New high-volume tables → TimescaleDB hypertable with `add_compression_policy` + `add_retention_policy` (see `@plans/database/trades-storage.md` for the pattern). `maintenance.rs` is deleted.
- Bulk-insert must chunk by `floor(65535 / binds_per_row)` — sqlx 0.6 has no guard against the 65535 bind-param ceiling.
