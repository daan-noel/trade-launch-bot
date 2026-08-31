# Database — schema & repositories

> Deep-dive schema reference lives in `@plans/database/`: [token-storage.md](@plans/database/token-storage.md), [trades-storage.md](@plans/database/trades-storage.md), [raw-txs-storage.md](@plans/database/raw-txs-storage.md), [strategy-storage.md](@plans/database/strategy-storage.md). The overview below is the canonical map; the plans have column rationale, index decisions, TimescaleDB config, and open design questions.

sqlx + Postgres. Raw SQL lives **only** in `trading_core/src/storage/repositories/*`. **Two migration sets, applied by separate runners so lab-only tables never reach EC2/live:**

- **Shared core** — `trading_core/migrations/` (`0001_init.sql` = the whole chain squashed into one end-state TimescaleDB baseline; add further `00NN_*.sql`). Runner: `sqlx::migrate!("./migrations")` in `storage/postgres.rs::connect()` (then `timescale::setup_caggs`). Run by **both** bins on boot.
- **Lab-only** — `lab/migrations/` (`0001_init.sql` = the `grouped_sweep_*` tables + the covering `idx_trades_wallet_time`, likewise squashed; `0002` = `ix_pattern_sets`, the Trader Analysis flow lens). Runner: `lab::storage::lab_migrations::run()`, called from `lab/main.rs` after `connect()`. Tracked in a lab-private **`_lab_migrations`** ledger (its own checksum table; reuses `migrate!` only as an embedder, never `.run()`, so core's `_sqlx_migrations` is untouched). Run by **`lab` only** → these tables never exist on EC2. Add a lab-only table = drop `NNNN_*.sql` into `lab/migrations/`.

**Squashing a chain breaks every already-migrated database.** Both ledgers key on `(version, SHA-384 of the file bytes)`, so collapsing `00NN_*.sql` into `0001_init.sql` changes version 1's checksum *and* orphans the recorded versions above it — the runner then refuses to boot on both counts. Reconcile each database ONCE with `scripts/consolidate-migration-ledgers.ps1` (ledger-only; touches no schema and no data), and do the **EC2 box before the next `db-incremental-sync.ps1`** — that script copies the server's `_sqlx_migrations` rows into the local mirror, so a stale server ledger re-pollutes a cleaned local one. `.gitattributes` pins `**/migrations/*.sql` to `eol=lf` so the checksum is identical on the workstation and in the Linux build container.

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
- **`strategy_position_pnl.pnl_pct` is a MONEY return, not a price ratio** (mig `0006`):
  `realized_pnl_lamports / entry_lamports × 100`, same numerator as `realized_pnl_sol`
  right above it, so the two can never disagree in sign. Three spellings of the one
  formula — this view, `StrategyPosition::pnl_pct`, and `PNL_PCT_SQL`/`PNL_SOL_SQL`
  (the positions table's sort+filter expressions, guarded by
  `pnl_sql_columns_share_one_numerator`). Rationale + the failure it fixes:
  [docs/plans/strategies/pnl-percent-definition.md](../plans/strategies/pnl-percent-definition.md).

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
  page/wrapper structured filters + per-column filters arrive as ONE
  `filters: {col → FilterSpec}` map and `TokenQuery::from_table_request` **lowers** each
  spec back onto the internal panel-map / per-column-predicate representation both engines
  already evaluate — so the fold added no new eval code and the parity guarantee still
  holds. `ath_price`/`current_price` are numeric-filterable; free-text search is
  mint/symbol only.
- **`TokenDetail`** coalesces `trade_count`/`volume_sol_total` to 0 (non-null), matching
  the list endpoint's `TokenSummary` — the detail modal and the list agree on those two.

## Tables

### Core trading

- `tokens` — mint_address UNIQUE, creator_wallet, name/symbol, bonding_curve_address, initial_buy_lamports(BIGINT), cu_limit/price, is_mayhem_mode, ix_labels(JSONB), initial_buy_instruction(JSONB; keys `max_cost_lamports`/`spendable_lamports_in`), creation_slot(BIGINT), created_at
- `trades` *(TimescaleDB hypertable on block_time, ~1mo retention)* — mint, wallet, trade_type, amount_lamports(BIGINT) / token_amount(BIGINT raw units), reserve_lamports/reserve_token(BIGINT venue-neutral pair), tx_signature(BYTEA), slot, block_time, venue(`curve`/`amm`), **`ix_labels`(JSONB, migration 0002, forward-only)**, **`fee_lamports`(BIGINT NULL, migration 0005, forward-only)**, **`cu_limit`/`cu_price`/`tip_lamports`(BIGINT NULL, migration 0013, forward-only)**; price derived in `trades_priced` view (`price_per_token` = SOL/token). PK `(block_time, tx_signature, leg_index)`. **This table = the LaserStream feed.**
  - **`fee_lamports` is per-TRANSACTION, the table is per-LEG.** It is the on-chain
    `meta.fee` (base signature fee + priority fee) read straight off the feed at decode
    — no RPC, no Helius credits — and denormalized onto every leg of its tx, so a bare
    `SUM(fee_lamports)` over multi-leg transactions over-counts by the leg multiplier.
    Collapse first: `SUM(fee) FROM (SELECT DISTINCT tx_signature, fee_lamports FROM trades …)`.
    Denormalizing beats a per-signature side table here: a second table would add a
    write per tx to the hot ingest path (the write-amplification shape that froze
    ingest before), whereas this is one more bind on an insert that already runs.
  - **NULL is load-bearing.** Pre-0005 rows have no fee and cannot get one (`raw_txs`
    is not persisted and is dropped after 3 days), so NULL means "not captured" — never
    coalesce it to 0 or average it in as 0. A landed tx always pays the base fee, so a
    genuine zero does not exist; `ingest_core::event::fee_lamports_opt` is the ONE
    reader that folds the protobuf's ambiguous `0` back to NULL at the source.
  - **Excludes** the Jito tip (a transfer instruction — absent from `meta.fee`) and the
    venue's own protocol/LP fee (already inside `amount_lamports`).
  - **`cu_limit` / `cu_price` / `tip_lamports` are the fee BUDGET the sender chose**,
    beside the fee the chain took. A sender picks one thing — how much to spend to land
    early — and pays it on either of two rails, so the quantity is the SUM and the
    columns are its parts:
    `priority_lamports = ceil(cu_limit * cu_price / 1e6) + tip_lamports`.
    `cu_price` is priced per compute unit, so it is **not** comparable across rows on
    its own: the same spend at half the limit reads as double the price. Group by the
    sum, never by a part. All three carry the same per-TRANSACTION-on-a-per-LEG
    attribution as `fee_lamports` — and the tip makes it sharper, since one tx selling
    four wallets' bags emits four legs and pays ONE tip.
  - **`tip_lamports` has three states and `0` is not NULL.** NULL = the tx carries no
    top-level system transfer; `0` = it carries one but none reached a recognised tip
    account (a router paying its own rake, or a tip rail the decoder's list does not
    know yet); `> 0` = tipped. The `0` bucket is the coverage meter for
    `TIP_ACCOUNT_IDS` in `shared/ingest/pumpfun/src/protocol.rs` — when it grows
    against the other two, that list is behind the market. Only top-level transfers
    count: an inner CPI transfer is the venue moving its own protocol fee, not the
    sender buying priority.
- `raw_txs` *(TimescaleDB hypertable on block_time; compress 2d, retain 7d)* — tx_signature(BYTEA), slot, block_time, tx_index, payload(BYTEA = verbatim protobuf wire bytes, parse in Rust), source(SMALLINT: 0=live 1=sync). PK `(block_time, tx_signature)`. Source-of-truth feed; `trades` is a typed projection. Written by `RawTxRepo` from both the live ingest db_writer and the token_sync backfill.

### Token analysis

- `tokens_info` — ATH, age, volume_sol(DOUBLE PRECISION), market_cap, trade_count, is_dead, is_migrated, first_slot_buy_lamports/first_slot_sell_lamports(BIGINT — same-creation-slot buy/sell totals, streamed in `TokenState`), sync watermarks

### Strategy (unified across all strategies — rows not tables per strategy)

- `fingerprints` — one `criteria` JSONB map of axis → inclusive integer range / label sequence (migration 0009; the per-axis columns and the row-wide bucket width are gone); **`metric_config` JSONB NOT NULL DEFAULT '{}'** (migration 0006) — top-level keys = metric group names (e.g. `m_flow_ix.ix_patterns`). Part of ROW identity for `find_or_create` and the `fingerprints_identity_uniq` index, though NOT of match identity: it selects no token, but it compiles into that row's live `m_flow_ix` patterns, so two rows matching the same tokens with different config are different fingerprints (the eleven `8dtx · <router>` carriers share `{}` + `wildcard` and differ only here).
- `strategy_rules` — `fingerprint_id` FK + `buy_amount_lamports`, `trade_mode`, `is_active` (Active/Idle live arming), `is_enabled` (soft-archive; Disabled stays in DB but is hidden from default lists and cannot Activate), `max_concurrent_tokens`, `max_total_tokens` (both `0 = unlimited`, decoded by the one `hunter_engine::Cap` reader), `params`(JSONB: TP/SL + entry/exit metric groups), `tags`(TEXT[], 0002: free-form Rules-board labels — presentational, canonicalized by `strategies::rules::normalize_tags`, never read by the engine; deliberately NOT a `params` key, which is re-serialized on write, is identity, and is frozen into `params_snapshot` — see [rule-tags.md](@plans/strategies/rule-tags.md)). Create/save refuse an **identity-identical** duplicate (`fingerprint_id` + `trade_mode` + sizing/caps + canonical `params`; `rule_name`/`tags`/`is_active`/`is_enabled` are not identity — same pattern as fingerprint `find_or_create`). See [strategy-storage.md](@plans/database/strategy-storage.md).
- `strategy_runs` — one activation session; `run_seq` monotonic per `(rule, mode)`; `params_snapshot` frozen at activation, plus `config_hash` + `config_edits` (0012) — the digest of the config the run is running under **now** and the append-only `[{at, changed[]}]` log of the edits that landed mid-run. A rule edited while active keeps its run (rotating it would split the rule's open positions across two), so those two columns are what say the row's numbers span more than one config; they also cover the fingerprint (`m_flow_ix.ix_patterns`, identity axes), which `params_snapshot` does not carry at all. Written by `record_run_config`, read by the run navigator and by `running_run_config_edits` for the Rules board — see [strategies.md](strategies.md#a-run-says-what-config-it-is-running-under). `status` is a real lifecycle, not a constant: `Running` until the rule stops being active, then `Stopped` + `finished_at` (an activation that never held a position is deleted, not kept). The engine sink owns both ends — see [strategies.md](strategies.md#run-lifecycle-what-current-run-vs-history-actually-splits-on). Exception: the ONE manual run (`strategy_id='manual'`, `rule_id` NULL) stays `Running` forever — `ensure_manual_run` finds it **by** that status
- `strategy_run_metrics` — 1:1 finalize-time rollup (`win_rate`, `total_pnl_sol`, exit-reason mix, etc.), written when a run is finalized and re-rolled while stragglers settle. Absent ⇒ the run is still live (that absence is `has_metrics` in the run navigator). 0004 added `n_exit_dead`/`n_exit_metrics`/`n_exit_manual`/`n_exit_migrated` — without them a generic-engine run's histogram was all-zero, since every one of its exits is `ExitReason::Metrics`
- `strategy_positions` — one opened position (bot or manual); `mint_address` (the SPL mint — renamed from `mint` so the physical column matches the token-data SSOT key); `status` (`BuySubmitted`/`Holding`/`ExitPending`/`ExitUnconfirmed`/`ExitStuck`/`End`/`EntryFailed` — the EntryFailed/ExitStuck status split; open partition = NOT IN (`End`,`EntryFailed`)); `origin` (`bot`/`manual`) + `manual_exit` JSONB (per-position TP/SL); `exit_redrive_count`/`exit_parked` (reaper-owned — never written by `update_position`); `last_entry_error TEXT` (executor-owned via the sole writer `note_last_entry_error`, likewise never written by `update_position`) — the cause of the most recent buy attempt that did NOT fill, i.e. the send error or the Anchor custom code, which is what makes an `EntryFailed` row self-explaining and tells a 6002/6042 slippage revert (tuning) from a structural one without pulling container logs; amounts as BIGINT (lamports/raw units); `submitted_buy_signatures TEXT[]` for in-flight recovery; `token_account TEXT` (nullable) — the wallet's token account for the mint, persisted on the entry fill so a re-buy reuses one account and the sell reads it from the row (restart-safe, no in-memory-cache dependency). Manual positions hang off ONE `strategy_runs` row (`strategy_id='manual'`, `rule_id` NULL); their `rule_id` is a fresh per-episode uuid (no FK)
- `strategy_arms` (0002) — the **arm ledger**: one row per `(rule, mint)` arming episode, `armed_at` → `ended_at`/`end_reason` (`entered` \| `dead` \| `migrated` \| `unsatisfiable` \| `paused` \| `duplicate_identity`, CHECK-constrained; the engine's `DisarmReason` set plus the sink's own `entered`). `position_id` is set only on `entered`, and `end_detail` (0003) only on `unsatisfiable` — the one reason that names a mechanism rather than a cause, so the fold records the entry conditions still unmet at the disarm instant (`{blocked_by, killed_by, unmet}`; `blocked_by` is the filter/group key, lifted by ONE shared SQL expression). `position_id` carries **no FK** — this table has a retention policy and `strategy_positions` does not, so neither may pin the other. Natural PK `(armed_at, rule_id, mint_address)`; `ended_at IS NULL` = the episode is still live. Written at the arm and updated at the end, so a restart cannot swallow every in-flight episode. Volume is **unbounded** (an arm costs nothing on chain, so a loose fingerprint arms on most launches) ⇒ hypertable + compression/retention on the same footing as `trades` (7d/30d), and every write is batched off the decision fold. The in-RAM `ArmedRegistry` stays the SSOT for "armed right now"; this answers "armed over a window". See [arm-ledger.md](@plans/strategies/arm-ledger.md).

### Analysis lake (lab Parquet — not Postgres)

Sealed-day trade files (`lab/src/lake/`) carry optional per-trade **`ix_labels`**
(JSON-string, dict-encoded) and **`wallet`** (address; export LEFT JOINs
`wallet_dict` with `unknown:{id}` COALESCE). Loaded only when
`Selection.with_flow` (flow metrics / discovery). Pre-V0 sealed days stay NULL.
See [lake-pg-read-paths.md](@plans/database/lake-pg-read-paths.md).

### Grouped param-sweep (generic table-name-driven repo)

One family — there are no per-strategy sweep tables, only these four:

- `grouped_sweep_runs` — run metadata, status(`running`/`completed`/`cancelled`), groups_done, corpus filters, label; `buy_amount_sol` is **DOUBLE PRECISION** (`lab/0007`, not REAL — an f32 widen pollutes a promoted fingerprint). `partition` (`lab/0002`) is JSONB: `[[field, spec], …]`, the explicit edges a run partitions by. A run carrying `[]` reads as one-group-per-value; re-run it to promote a group. <!-- pt-ok: `[]` is what rows written before lab/0002 hold, and the reader has to know -->
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
| `trade_repo.rs` | trades | `find_fill_by_signature`, `sum_legs_by_signatures` (per-sig attribution), `avg_entry_by_wallet_and_mints` (**manual-buy cost-basis SSOT** → `AvgEntry {avg_entry_price (SOL/raw), total_token_amount, total_cost_lamports}` per mint, `SUM(amount_lamports)/SUM(token_amount)` over `trade_type='buy'`; bounded by the held-mint set; feeds the portfolio service), `wallet_traded_mints` (**Trader Analysis** → `WalletTradedMint {wallet_address, mint_address, first_trade_at, last_trade_at, buy_count, sell_count, buy_sol, sell_sol, buy_token_amount, sell_token_amount, entry_at, entry_slot, entry_tx_index, exit_at, exit_slot, exit_tx_index, entry_curve_sol, exit_curve_sol}` per distinct mint the wallet traded in a `since..=until` `block_time` window (`until: Option` — `None` ⇒ open-ended, bound as a NULL `$3::timestamptz` so one SQL string serves both shapes; EVERY aggregate below is scoped to the same window, so a closed upper bound reads the wallet exactly as it looked at that instant), `GROUP BY wallet_id, mint_address ORDER BY MAX(block_time) DESC` + optional `LIMIT n` (`limit<=0` ⇒ unbounded, the page default) + `COUNT(*)`/`SUM(amount_lamports)`/`SUM(token_amount)` each `FILTER (WHERE trade_type=…)`; both buy+sell count; the entry (first **buy**) and exit (last **sell**) legs come from `(ARRAY_AGG(col ORDER BY slot, tx_index, leg_index) FILTER (WHERE trade_type=…))[1]` — per-side first/last without a second scan, ordered by the execution key, not `block_time`. Each leg's `reserve_lamports` and own `amount_lamports` go through `pre_trade_real_sol` → the depth **before** that leg landed (the reserve snapshot is post-trade, so a leg's own impact is inside it) and then the `approx_real_sol_reserves` SSOT, giving `entry_curve_sol`/`exit_curve_sol`; the handler divides by `PUMP_GRADUATION_REAL_SOL` for the wire `wallet_*_curve_pct`. All four are `Option` — an exit-only window has no entry, a still-held bag no exit, and a missing reserve snapshot reads unknown, never 0; empty for an unknown wallet; feeds lab `GET /api/wallets/:wallet/tokens` (`?days=N` for the rolling window, or `?from=&to=` RFC3339 for an explicit range — `from` present ⇒ `days` ignored, `to` alone anchors the rolling span to that instant; `resolve_window` swaps a reversed pair and clamps the span to 90d, keeping the UPPER bound), which merges each with the full token row via `TokenRepo::find_list_rows_for_mints` and runs `strategies::kernel::wallet_mint_pnl` (avg-cost realized/unrealized/total PnL, net-of-fee via the shared `FEE_BPS_PER_LEG`) → `WalletTokenRow` = flattened `TokenSummary` + `wallet_*` interaction + PnL fields. Rides the **lab-only** covering index `idx_trades_wallet_time (wallet_id, block_time DESC, mint_address, trade_type)` — index-only, zero EC2 ingest cost. The entry/exit legs also carry their `(slot, tx_index)` tape position: `block_time` is second-precision and ties across a whole slot, so it is the only key that can order two wallets' entries against each other), `wallets_traded_mints_on` (**Trader Analysis co-trade** → the SAME rollup for a SET of wallets restricted to an explicit `mints` slice, `GROUP BY wallet_id, mint_address`, unbounded — the caller already bounded the mints. Both readers share ONE private `traded_mints_agg` (and so one SQL string), which takes the resolved `wallet_dict` id→address map, an optional `$4::text[]` mint scope and the `LIMIT` bind; `limit` is meaningful for a SINGLE wallet only, since across several `ORDER BY last_trade_at DESC LIMIT n` would cut through the union. Untracked addresses and `(wallet, mint)` pairs with no leg in the window drop out — never a zero row. Feeds `?with=<csv>` on `GET /api/wallets/:wallet/tokens`, capped at `MAX_COMPARISON_WALLETS`=8; absent `with` ⇒ not one extra query. The handler measures each comparison entry against the primary's → `CoTrader {entry_lag_slots, entry_lag_tx, bucket}`, `bucket` = `co-slot`/`leads`/`follows`/`independent` by `CO_TRADE_FOLLOW_SLOTS`=3 — same-slot means both wallets reacted to the same tape event, NOT that one copied the other. A co-trade DB error degrades to the single-wallet page rather than failing the read), `for_each_seed_mint` (cold-start seed), `sig_bytes_to_base58` (`pub`; BYTEA→base58, reused by the lake export), `find_by_mints_all` (batched per-mint grouped reads; **reconstructs** the dropped `real_reserve_sol` via `approx_real_sol_reserves(reserve_sol, venue)`, never the live path — the **simulate/backtest path does not call it**: single-rule simulate reads the Parquet lake like the sweep, which bakes that same value in at export/read. Retained for ad-hoc grouped reads / tests) |
| `raw_tx_repo.rs` | raw_txs | `insert`, `insert_many` (ON CONFLICT DO NOTHING) |
| `token_info_repo.rs` | tokens_info | `upsert_metrics`, `get/update_sync_watermark` |
| `creation_stats_repo.rs` | tokens (+tokens_info) | `heatmap`, `trend` — TZ-aware SQL, bucket granularities, per-field corpus filters; every cell/point carries censored outcome (`matured`/`known`/`migrated`/`dead`) **and** trade columns (`trades`, `volume_sol`, `trades_per_day` = age-normalized `SUM(trade_count/age_days)`, `trades_avg` = `SUM/COUNT` mean, `NULL`-safe via `NULLIF`) off `tokens_info.trade_count`/`volume_sol`, all reusing the same maturity-censoring predicate (`trade_metrics_sql`/`MATURED_PRED`). `grouped`/`grouped_scoped` add the same group-level `trades`/`trades_avg`, and `grouped`'s `rank_by` (`count` default \| `trades` \| `trades_per_token`) picks the top-N ranking criterion (`rank_by_order_sql`, whitelisted) |
| `settings_repo.rs` | app_settings | `load_all`, `set_one`, `set_many` |
| `ix_pattern_set_repo.rs` (lab) | ix_pattern_sets | `list` / `find` / `insert` / `update` / `delete` + `sanitize_patterns` / `validate`. Analysis-owned `ix_labels` pattern sets — the Trader Analysis flow lens' twin of a fingerprint's `ix_patterns`, for tokens that belong to no cohort. Patterns are `[{ group, ix_labels }]`; identity is the ordered `ix_labels` array alone (the same key the classifier matches), so the same sequence under two group labels collapses to one. `MAX_PATTERNS` caps a set at 500. Unique on `lower(name)` → a name clash is a 409, not a 500. Lab-only: never on EC2 |
| `grouped_sweep_repo.rs` | `<strategy>_grouped_sweep_*` | incremental writes: `insert_run`, `append_group`, `finalize_run`, `mark_cancelled` |
| `arm_repo.rs` | strategy_arms | `insert_arms` / `end_arms` (both take a SLICE and issue ONE statement — the writer batches a flush window, because a per-episode round trip would put the arm rate straight onto the pool; the end write is `WHERE ended_at IS NULL`, so it is idempotent and keeps the FIRST ending), `arms_paged` + `count_arms` (page + total under one `ArmQuery`, so the pager total tracks the page exactly), `arm_funnel` (armed/entered/live + a count per `end_reason` + median wait, all from ONE scan), `arm_blocked_by` (the `unsatisfiable` count grouped by `end_detail ->> 'blocked_by'` — its own statement because it groups by a per-row value a fixed-shape aggregate cannot carry, under the same JOIN + WHERE so it counts the funnel's population). `waited_sec` = `EXTRACT(EPOCH FROM (COALESCE(ended_at, now()) - armed_at))`, defined once and shared by the projection, the sort whitelist and the filter whitelist |
| `wallet_repo.rs` | wallets | `touch_last_seen_many` |
| `wallet_profile_repo.rs` / `wallet_profile_tag_repo.rs` | wallet_profiles, tags | CRUD |
| `fingerprint_repo.rs` | fingerprints | `find_or_create` (`criteria` + `wildcard` + `metric_config` — **not** `name`), `insert`/`update`/`list`/`delete`; persists `metric_config`; `name` is `Fingerprint::auto_name` when blank or a retired generator shape, and `list`/`find` rewrite those leftover labels in place |
| `rule_repo.rs` | strategy_rules | `insert`/`update`/`find`/`list`/`list_by_fingerprint`/`list_active` (`is_active AND is_enabled`)/`delete`, `find_identical` (trading identity — **not** `rule_name`/`tags`/`is_active`/`is_enabled`; feeds the create/save Duplicate gate) |
| `strategy_repo.rs` | strategy_rules, strategy_runs, strategy_run_metrics, strategy_positions | `find_rule`, `insert_run`, `insert_position`, `update_position_status`, `mark_buy_submitted`, `find_all_holding`, `find_all_exit_pending`, `find_all_buy_submitted`, `find_reusable_token_account`, `fail_stale_exit_pending`, the run-lifecycle set — `finalize_run` (rollup + terminal status, or delete when the activation held nothing; `allow_delete=false` while an insert may be in flight), `roll_up_run` (idempotent re-roll as a finalized run's stragglers settle), `orphan_running_runs` + `draining_finalized_runs` (the two bounded boot-reconcile queries) — `find_positions_by_{run,rule}_paged` + `count_positions_by_{run,rule}` (page + total for the positions table's `X-Total-Count`), `find_positions_all_paged` + `count_positions_all` (the same page/count with the scope predicate dropped — the cross-rule Console History view; the scope is an `Option<(&str, Uuid)>` on the ONE shared `find_positions_paged`/`count_positions`, deliberately not a forked query, and the cohort narrows through `PositionQuery`'s `mode`/`rule_id`/`status`/`exit_reason` filters + its `time_from`/`time_to` window over `COALESCE(exit_time, entry_time, created_at)`, `from` inclusive / `to` exclusive), `closes_series` + `entry_failed_count` (the per-close chart series: every `End` row's `{exit_time, rule_id, pnl_sol, entry_sol, win}` in a mode/window/rule scope, oldest first, with `EntryFailed` counted separately because it deployed no SOL; `pnl_sol` picks its exit figure through the shared `models::strategy::realized_exit_sol` so a scale-out position matches `realized_pnl_sol`), `find_tokens_by_mints_paged` + `count_tokens_by_mints` (the **matched** table: `tokens t LEFT JOIN tokens_info i` scoped to a materialized `mint = ANY($set)`, selecting the shared `token_enrichment::ENRICH_SELECT` → full `TokenEnrichmentRow`, so the response carries all ~28 enrichment fields with no client merge), `positions_summary_by_{run,rule}` (single `COUNT/SUM FILTER` aggregate for the Positions Summary panel over an optional `PositionQuery` filter — same JOIN + WHERE as the paged list; win = `status='End' AND exit_sol>entry_sol`, mirrors `StrategyPosition::is_win`; SOL sums cast BIGINT→human), `rule_counters_for_latest_paper_runs` (one batched `GROUP BY` — per-rule open/pending/total/win/loss/win_rate/avg_pnl_pct/pnl over each rule's latest paper run; same win predicate; feeds the **lab** rules-table counters, which have no runtime cache). `return_pct` here **and** in `positions_summary_by_*` **and** the live runtime cache is the one canonical **capital-weighted return** = `Σ realized_pnl_lamports / Σ entry_lamports (closed) × 100` via `strategies::kernel::weighted_return_pct` (SSOT) — NOT a mean of per-trade price %, so its sign is always locked to `total_pnl_sol` (the old mean could show `+%`/`−◎` on the same rule). Both surfaces also ship `closed_entry_sol`, that ratio's own denominator, so a caller spanning several scopes (the Rules TOTAL tile, the Portfolio window) re-weights by capital instead of by trade count, `find_open_positions` (all unsettled positions cross-rule), `managed_mints(real_only)` (projection-only "who manages this mint" — open positions `LEFT JOIN strategy_rules` for the rule name; `→ ManagedMint`; backs the portfolio bot badge / double-sell interlock), `realized_pnl_lamports_since(ts)` (real `End`-position `SUM(exit_lamports−entry_lamports)` since a boundary — the "realized today" KPI) |

## Rules

- Always bound queries — paginate/time-window/stream. Never `SELECT *` full `trades`/`raw_txs`.
- New high-volume tables → TimescaleDB hypertable with `add_compression_policy` + `add_retention_policy` (see `@plans/database/trades-storage.md` for the pattern). Chunk lifecycle is declarative — there is no `maintenance.rs` partition loop to extend. <!-- ref-ok: absence is the rule -->
- Bulk-insert must chunk by `floor(65535 / binds_per_row)` — sqlx 0.6 has no guard against the 65535 bind-param ceiling.
- **Server-side table filters are structured + type-checked.** The strategy token tables take a unified `TableRequest` (POST/JSON, `trading_core::api::table_query`); per-column filters are `{op, val}` (`FilterOp`: contains/eq/gt/gte/lt/lte/between). `strategy_repo` splits its whitelist into typed `(sql_expr, FilterKind::{Text,Numeric,Bool})` rows: numeric cols return the **uncast** expr so `gt`/`between` compare numerically (operand bound as `f64`), text cols keep `ILIKE`, and **flag** (All/Yes/No) cols bind a real `bool` for `eq`/`neq` only — never a `::text` compare, which matches only whichever spelling the producer sent. Every flag operand reads through the one `table_query::as_flag` vocabulary (`yes`/`no`, `true`/`false`, `1`/`0`), so the DataTable dropdown and the summary tiles narrow the same column identically; the in-memory twin (`table_eval::ColKind::Bool`) and the Tokens tri-state (`lower_flag`) read it too. `push_filter_predicate` lowers each op to a bound predicate; an illegal pairing (numeric op on a text col, non-number operand, unrecognized flag word) is **dropped**, like an unknown key — every operand `push_bind`s (injection-safe). No user text ever reaches an identifier.
