# Database — schema & repositories

> **Live/lab remake Phase 1 (clean rebuild) applied.** `trading_core/migrations/0001_init.sql`
> was rewritten as a **TimescaleDB** clean rebuild; the per-table designs below are superseded by
> the four storage plans (now the source of truth): `token-storage-plan.md`, `trades-storage-plan.md`,
> `raw-txs-storage-plan.md`, `strategy-storage-plan.md` + `timescaledb-plan.md`. Highlights:
> `tokens`(mint PK)/`tokens_info`/`token_sync_state`; `trades` & `raw_txs` are **hypertables** on
> `block_time` with `add_compression_policy`/`add_retention_policy` (trades 7d/30d, raw 2d/7d) — the
> old `maintenance.rs` partition loop is deleted; integer base units, **BYTEA** signatures, wallet
> interning (`wallet_dict` → `trades.wallet_id`), `real_*_reserves` dropped; unified
> `strategy_rules`/`strategy_runs`/`strategy_run_metrics`/`strategy_positions` replace the tpsl1/2
> tables; views `token_overview`/`trades_priced`/`strategy_position_pnl`; OHLCV continuous aggregates
> (`trades_ohlcv_1m`/`_5m`/`_1h`) created at boot by `trading_core::storage::timescale::setup_caggs`.
> sqlx is **runtime-checked** (no compile-time SQL validation). The prose below is the pre-rebuild
> shape and is being migrated; trust the storage plans + `0001_init.sql`.

sqlx + Postgres. Raw SQL lives **only** in `trading_core/src/storage/repositories/*`. **Two migration sets, applied by separate runners so lab-only tables never reach EC2/live:**

- **Shared core** — `trading_core/migrations/` (`0001_init.sql` = clean TimescaleDB baseline; add further `00NN_*.sql`). Runner: `sqlx::migrate!("./migrations")` in `storage/postgres.rs::connect()` (then `timescale::setup_caggs`). Run by **both** bins on boot.
- **Lab-only** — `lab/migrations/` (`0001_grouped_sweep.sql` = the `tpsl{1,2}_grouped_sweep_*` tables). Runner: `lab::storage::lab_migrations::run()`, called from `lab/main.rs` after `connect()`. Tracked in a lab-private **`_lab_migrations`** ledger (its own checksum table; reuses `migrate!` only as an embedder, never `.run()`, so core's `_sqlx_migrations` is untouched). Run by **`lab` only** → these tables never exist on EC2. Add a lab-only table = drop `NNNN_*.sql` into `lab/migrations/`.
Deep-dive detail: `@plans/database/db-pool-routing.md`, `@plans/database/db-patterns.md`.

## Connection pools

`storage/postgres.rs::connect()` builds three workload-isolated `PgPool`s (`DbPools { hot, api, batch }`):

- **hot** (default 64 conn) — ingest DbWriter, StrategyRunner, maintenance, seed. In `main.rs` as `db`.
- **api** (default 32 conn) — `AppState.db`; fast HTTP handlers; 8s `statement_timeout`.
- **batch** (default 16 conn) — `AppState.batch_db`; grouped sweeps, backtests, token_sync backfill. No statement timeout.

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

- `tokens` — mint_address UNIQUE, creator_wallet, name/symbol, bonding_curve_address, initial_buy_sol(BIGINT lamports), cu_limit/price, is_mayhem_mode, ix_labels(JSONB), created_at
- `trades` *(TimescaleDB hypertable on block_time, ~1mo retention)* — mint, wallet, trade_type, sol_amount(BIGINT lamports) / token_amount(BIGINT raw units), tx_signature(BYTEA), slot, block_time, virtual_*_reserves(BIGINT), venue(`curve`/`amm`); price derived in `trades_priced` view. PK `(block_time, tx_signature, leg_index)`. **This table = the LaserStream feed.**
- `raw_txs` *(TimescaleDB hypertable on block_time; compress 2d, retain 7d)* — tx_signature(BYTEA), slot, block_time, tx_index, payload(BYTEA = verbatim protobuf wire bytes, parse in Rust), source(SMALLINT: 0=live 1=sync). PK `(block_time, tx_signature)`. Source-of-truth feed; `trades` is a typed projection. Written by `RawTxRepo` from both the live ingest db_writer and the token_sync backfill.

### Token analysis

- `tokens_info` — ATH, age, volume, market_cap, trade_count, is_dead, is_migrated, sync watermarks
- `tokens_analysis` — mint_address, analyzer_name, score, indicators(JSONB)
- `creator_profiles` — wallet_address, tokens_created, suspiciousness_score, indicators(JSONB)

### TPSL strategy (tpsl2 mirrors tpsl1 + extra scalp gate columns)

- `tpsl1_strategy_rules` — entry params, exit params (trailing_stop_pct, time_stop_secs, stall_secs, liquidity_drop_pct, take_profit, stop_loss), buy_amount, trade_mode, is_active
- `tpsl1_real_positions` — mint, wallet, status(`Arming`/`BuySubmitted`/`Holding`/`ExitPending`/`End`/`ExitFailed`), entry/exit tx_signatures(JSONB arrays), submitted_buy_signatures(TEXT[]), token amounts (not SOL)
- `tpsl1_paper_test_run` / `tpsl1_paper_positions` — mirrors real, no tx uniqueness
- `tpsl2_*` adds scalp gate columns + `target_*` columns (trigger trade, distinct from entry fill)

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
| `token_repo.rs` | tokens (+tokens_info) | `find_list_rows` (DB base for /api/tokens), `find_page_before` (keyset page for analysis scans), `find_by_mints` (chunked mint=ANY) |
| `trade_repo.rs` | trades | `find_fill_by_signature`, `sum_legs_by_signatures` (per-sig attribution), `for_each_seed_mint` (cold-start seed), `find_by_mints_all` (batched per-mint grouped reads for backtests) |
| `raw_tx_repo.rs` | raw_txs | `insert`, `insert_many` (ON CONFLICT DO NOTHING), `find_by_signature` (PK lookup) |
| `token_info_repo.rs` | tokens_info | `upsert_metrics`, `get/update_sync_watermark` |
| `analysis_repo.rs` | tokens_analysis, creator_profiles | find/list |
| `creation_stats_repo.rs` | tokens (+tokens_info) | `heatmap`, `trend`, `grouped` — TZ-aware SQL, bucket granularities, per-field corpus filters |
| `settings_repo.rs` | app_settings | `load_all`, `get_one`, `set_one`, `set_many` |
| `grouped_sweep_repo.rs` | `<strategy>_grouped_sweep_*` | incremental writes: `insert_run`, `append_group`, `finalize_completed`, `mark_cancelled`; compaction: `fetch_combo_metrics_for_group`, `delete_combos_except`, `vacuum_full_results` |
| `wallet_repo.rs` | wallets | `touch_last_seen_many` |
| `wallet_profile_repo.rs` / `wallet_profile_tag_repo.rs` | wallet_profiles, tags | CRUD |
| `tpsl1_strategy_rule_repo.rs` / `tpsl2_*` | tpsl{1,2}_strategy_rules | `find_all`, `find_by_id`, `update`, `delete` |
| `tpsl1_position_repo.rs` / `tpsl2_*` | tpsl{1,2}_real_positions | `update_entry`, `mark_buy_submitted`, `find_all_holding`, `find_all_exit_pending`, `find_all_buy_submitted`, `fail_stale_exit_pending`, `delete_stale_unentered` |
| `tpsl1_paper_trading_repo.rs` / `tpsl2_*` | tpsl{1,2}_paper* | `start/current/resume/mark` run; position CRUD; `clear_runs` |

## Rules

- Always bound queries — paginate/time-window/stream. Never `SELECT *` full `trades`/`raw_txs`.
- New high-volume tables → partition+retention pattern via `ingest_laserstream/maintenance.rs`.
- Bulk-insert must chunk by `floor(65535 / binds_per_row)` — sqlx 0.6 has no guard against the 65535 bind-param ceiling.
