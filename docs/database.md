# Database — schema & repositories

sqlx + Postgres. Raw SQL lives **only** in `backend/src/storage/repositories/*`. Migrations in `backend/migrations/` (`0001_init.sql` = consolidated baseline; add `00NN_*.sql`). Runner: `sqlx::migrate!("./migrations")` in `storage/postgres.rs`, invoked from `main.rs`.

## Migrations
| File | Content |
|---|---|
| `0001_init.sql` | baseline: all tables/indexes, `uuid-ossp`, `raw_transactions` partition fns, seed tags |
| `0002_perf_indexes.sql` | composite hot-path indexes (wallet+mint balance, slot ordering) |
| `0003_settings_kv.sql` | `app_settings` single-row JSONB → typed key-value store (dotted keys) |

## Tables
**Core trading**
- `tokens` — `mint_address` UNIQUE, creator_wallet, name/symbol, bonding_curve_address, initial_supply_token, initial_buy_sol, cu_limit, cu_price, is_mayhem_mode, is_cashback_enabled, token_program_id, initial_buy_instruction/ix_labels (JSONB), creation_tx_signature, created_at. Idx: creator, created_at, mayhem, token_program, cashback.
- `trades` — mint_address, wallet_address, trade_type(`buy`/`sell`), sol_amount, token_amount, price_per_token, tx_signature, slot, block_time, virtual/real sol+token reserves, ix_type, ix_labels(JSONB), leg_index, received_at, venue(`curve`/`amm`). **UNIQUE(tx_signature, leg_index)**. Idx: mint, wallet, block_time, (mint,block_time), (mint,venue,slot), (wallet,mint), (mint,slot,leg_index). **This table = the LaserStream feed.**
- `raw_transactions` *(PARTITIONED)* — id, signature, slot, block_time, raw_data(JSONB), received_at, source(`grpc`/`rpc`). PK (received_at,id). RANGE-by-week on received_at, ~9wk retention +2wk forward; fns `ensure_raw_partition`/`drop_raw_partition`. No UNIQUE (dupes tolerated).

**Token analysis**
- `tokens_info` — `mint_address` UNIQUE, ath_price/ts, age, volume, market_cap, trade_count, last_trade_at, current_price, is_rugged, is_migrated, per-venue sync watermarks (`last_synced_{curve,amm}_{sig,slot}`), created/updated_at.
- `tokens_analysis` — mint_address, analyzer_name, score, indicators(JSONB), computed_at. UNIQUE(mint_address, analyzer_name).
- `creator_profiles` — wallet_address UNIQUE, tokens_created, total_volume_sol, suspiciousness_score, wash_trade_score, indicators(JSONB), last_analyzed_at.

**TPSL strategy** (tpsl2 mirrors tpsl1 + extra scalp gate columns)
- `tpsl1_strategy_rules` — rule_name, entry params `p_token_*`, caps `p_max_{concurrent,total}_tokens`, exit params `p_exit_*` (trailing_stop_pct, time_stop_secs, stall_secs, liquidity_drop_pct, take_profit, stop_loss), buy_amount, tolerance_pct, trade_mode(`paper`/`real`), is_active.
- `tpsl1_real_positions` — mint, wallet, token_program_id, entry_{price,amount,time,tx}, exit_{price,amount,time,tx}, status(`Holding`/`ExitPending`/`End`/`ExitFailed`), strategy, rule_id (FK→rules, SET NULL), exit_reason. entry_tx/exit_tx UNIQUE. Idx: mint, wallet, status, rule_id, (mint,status), token_program_id.
- `tpsl1_paper_test_run` — rule_id (FK CASCADE), run_seq, status(`Running`/`Finished`/`Stopped`), max_total_tokens, started/finished_at. UNIQUE(rule_id, run_seq); one live run per rule.
- `tpsl1_paper_positions` — run_id (FK CASCADE) + same shape as real positions, no tx UNIQUE (token re-tradable across runs).
- `tpsl2_*` adds entry gates: `p_entry_{min_age_secs,min_alive_sol,min_organic_sol,pullback_pct,higher_low_secs,max_cohort_held,min_liquidity_sol,min_organic_liq}`, `p_exit_cohort_ratio`.

**Wallets / settings**
- `wallet_profiles` — name, type(`mine`/`trader`/`whale`/`dev`), tag_ids(UUID[]).
- `wallets` — profile_id (FK CASCADE), address UNIQUE, is_tracked, comment, last_seen_at.
- `wallet_profile_tags` — name UNIQUE, color, comment (seeded with ~20 labels).
- `app_settings` — PK `key`, value(JSONB), updated_at. Keys: `ingest.{track_mayhem,track_post_migration,live}`, `ui.{timezone,price_unit}`, `trade.slippage_bps`. New setting = new row (migration-free).

## Repositories (`storage/repositories/`)
| File | Table(s) | Notable fns |
|---|---|---|
| `token_repo.rs` | tokens | insert, upsert, find_by_mint, exists, find_all |
| `trade_repo.rs` | trades (+tokens_info) | insert(_many), latest_signature, find_by_mint_paged, net_token_amount_by_wallet_and_mint, real_sol_reserve_extremes, early_buyer_cohort_net, load_all_aggregates, for_each_chronological |
| `transaction_repo.rs` | raw_transactions | insert(_many), find_by_signature, exists |
| `token_info_repo.rs` | tokens_info | upsert_metrics, update_migration_status, get/update_sync_watermark, list_all |
| `analysis_repo.rs` | tokens_analysis, creator_profiles | upsert_result, list_results, upsert/find/list_creator_profile |
| `settings_repo.rs` | app_settings | load_all, get_one, set_one, set_many |
| `wallet_repo.rs` | wallets | insert/update/delete, find_by_address, list_by_profile(s), touch_last_seen(_many) |
| `wallet_profile_repo.rs` | wallet_profiles (+joins) | insert/update/delete, find_with_wallets, list_with_wallets |
| `wallet_profile_tag_repo.rs` | wallet_profile_tags | list, find_by_ids, insert/update/delete |
| `tpsl1_strategy_rule_repo.rs` / `tpsl2_*` | tpsl{1,2}_strategy_rules | insert, find_all, find_by_id, update, delete |
| `tpsl1_position_repo.rs` / `tpsl2_*` | tpsl{1,2}_real_positions | update_entry(RETURNING), insert, update, find_holding_by_{mint,wallet}, find_by_rule, find_all_holding, count_all_by_rule, fail_stale_exit_pending, delete_position |
| `tpsl1_paper_trading_repo.rs` / `tpsl2_*` | tpsl{1,2}_paper_{test_run,positions} | start/current/resume/mark run; position insert/update_entry/update_exit/mark_exit_failed; count_by_run, delete_stale_unentered |

## Rules
- **Always bound queries** — paginate / time-window / stream. Never `SELECT *` full `trades`/`raw_transactions` into memory.
- Server-side filter/sort/paginate in the repo; don't fetch-all-then-slice.
- New high-volume tables → follow the `raw_transactions` partition+retention pattern via `ingest_laserstream/maintenance.rs`.

## sqlx patterns
`query`/`query_as::<_,T>`/`query_scalar` + `.bind()`; `#[derive(sqlx::FromRow)]`; `sqlx::types::Json<T>` for JSONB; `QueryBuilder::push_values` for bulk; `pool.begin()`→`tx.commit()` for transactions.
