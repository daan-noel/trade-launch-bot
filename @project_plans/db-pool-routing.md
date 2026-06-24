# DB Pool Routing — Current Code Status

## The three pools

The backend connects three workload-isolated Postgres pools (see
`backend/src/storage/postgres.rs`). Only two are exposed on `AppState`; the third
wires directly into the ingest pipeline.

| Pool | `AppState` field | Statement timeout | Purpose |
|---|---|---|---|
| `hot` | — (not on AppState) | none | Ingest `DbWriter`, `StrategyRunner`, maintenance, seed. Wired directly in `main.rs`. |
| `api` | `state.db` | 8 s | Fast dashboard reads, single-row mutations. The 8 s ceiling kills a pathological query before it drains the pool. |
| `batch` | `state.batch_db` | none | Sweep corpus load/write, backtests, bulk deletes. Long by design; bounded at the job level instead. |

## Where `state.db` (API pool) is used

Every usage is a fast, bounded operation — a single-row read/write or a paginated
list query. None should approach the 8 s limit under normal load.

| File | Operation |
|---|---|
| `state/app_state.rs` | `token_repo()` accessor — fast reads |
| `strategies/tpsl_sniper_{1,2}/lifecycle.rs` | `find_by_id` rule read, paper position close, live sell spawn repos (sell confirm arrives via gRPC feed, not DB polling — no long query) |
| `strategies/tpsl_sniper_{1,2}/backtest.rs` | One-time rule fetch before heavy work starts (single row) |
| `api/handlers/strategies/grouped_sweep.rs` | `list_runs`, `list_groups`, `count_results`/`list_results` (paginated by `run_id`+`group_id`), `update_label`, `get_run`/`get_group`/`get_combo_params` |
| `api/handlers/tokens/sync.rs` | Preview read (fast, interactive; bulk insert uses `batch_db`) |

## Where `state.batch_db` (batch pool) is used

All heavy, potentially long-running DB work routes here.

| File | Operation |
|---|---|
| `api/handlers/strategies/grouped_sweep.rs` | Corpus load, `insert_run`, sweep writer task, `delete_run` (CASCADE), `prune_runs` (CASCADE), targeted + full corpus reload for `list_token_results` |
| `strategies/tpsl_sniper_{1,2}/backtest.rs` | Whole-table token scan (`collect_matching_tokens`) + batched trade fetch chunks |
| `api/handlers/tokens/sync.rs` | Bulk token insert |

## Known past bug (fixed 2026-06-24)

`delete_run` and `prune_runs` in `grouped_sweep.rs` were originally on `state.db`.
A run with 331,776 rows in `tpsl2_grouped_sweep_combos` (1.7 GB) triggered the 8 s
`statement_timeout` on its CASCADE delete → HTTP 500. Fixed by routing both to
`state.batch_db`.
