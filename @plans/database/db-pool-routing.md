# DB Pool Routing — Current Code Status

## The three pools

The backend connects three workload-isolated Postgres pools (see
`trading_core/src/storage/postgres.rs`). Only two are exposed on `CoreState`; the third
wires directly into the ingest pipeline.

| Pool | `CoreState` accessor | Statement timeout | Purpose |
|---|---|---|---|
| `hot` | — (not on CoreState) | none | Ingest `DbWriter`, `StrategyRunner`, seed. Wired directly in `live/src/main.rs`. |
| `api` | `state.db()` | 8 s | Fast dashboard reads, single-row mutations. The 8 s ceiling kills a pathological query before it drains the pool. |
| `batch` | `state.batch_db()` | none | Sweep corpus load/write, backtests, bulk deletes. Long by design; bounded at the job level instead. |

## Where `state.db()` (API pool) is used

Every usage is a fast, bounded operation — a single-row read/write or a paginated
list query. None should approach the 8 s limit under normal load.

| File | Operation |
|---|---|
| `trading_core/src/state/core_state.rs` | `token_repo()` + other accessor fns — fast reads |
| `live/src/api/handlers/strategies/rules.rs` | `find_by_id` rule read, cache reload; sell confirm arrives via gRPC feed, not DB polling |
| `lab/src/api/handlers/strategies/tpsl{1,2}.rs` | One-time rule fetch before heavy work starts (single row) |
| `lab/src/api/handlers/strategies/grouped_sweep.rs` | `list_runs`, `list_groups`, `count_results`/`list_results` (paginated by `run_id`+`group_id`), `update_label`, `get_run`/`get_group`/`get_combo_params` |
| `live/src/api/handlers/tokens/sync.rs` | Preview read (fast, interactive; bulk insert uses `batch_db`) |

## Where `state.batch_db()` (batch pool) is used

All heavy, potentially long-running DB work routes here.

| File | Operation |
|---|---|
| `lab/src/api/handlers/strategies/grouped_sweep.rs` | Lake/corpus load, `insert_run`, sweep writer task, `delete_run` (CASCADE), `prune_runs` (CASCADE), targeted + full corpus reload for `list_token_results` |
| `lab/src/api/handlers/strategies/tpsl{1,2}.rs` | Whole-table token scan (`collect_matching_tokens`) + batched trade fetch chunks for simulate |
| `live/src/api/handlers/tokens/sync.rs` | Bulk token insert |

## Known past bug (fixed 2026-06-24)

`delete_run` and `prune_runs` in `grouped_sweep.rs` were originally on `state.db`.
A run with 331,776 rows in `tpsl2_grouped_sweep_combos` (1.7 GB) triggered the 8 s
`statement_timeout` on its CASCADE delete → HTTP 500. Fixed by routing both to
`state.batch_db`.
