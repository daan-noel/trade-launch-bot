# Strategies — TPSL snipers

File-level map of `backend/src/strategies/`. Two **intentional clones**: `tpsl_sniper_1` (canonical/live) and `tpsl_sniper_2` (clone + cohort scalp gates). A fix in one usually belongs in both.
Logic explainers: `@project_plans/tpsl-strategy/*`.

## Dispatch — `runner.rs`
`StrategyRunner` (`new`, `run`) consumes `strategy_rx` in a `select!` loop and routes to both strategy services:
- `on_token_created(mint)` → entry gating → spawn buy.
- `on_trade_executed(mint)` → exit evaluation → spawn sell.
- 1s clock tick → `sweep_time_exits()` (deadline exits that come due in silence). Const `TIME_EXIT_SWEEP_INTERVAL = 1s`.

The single `select!` loop **serializes** all position transitions across both strategies (no Holding→ExitPending interleave on the same position).

## Shared — `sim_progress.rs`
`SimProgress` (`new`, `start`, `tick`) — per-backtest progress reporter shared by both snipers' `backtest.rs`. Counts completed candidate futures atomically and broadcasts throttled `SseEvent::SimulationProgress { rule_id, processed, total }` (~100 frames/run + a final `processed == total`). The dashboard's `SimProgressBar` subscribes via `simulation_progress` SSE for a determinate bar.

## Per-strategy module map (`tpsl_sniper_1/`, mirrored in `tpsl_sniper_2/`)
| File | Key items | Responsibility |
|---|---|---|
| `mod.rs` | `TPSL1StrategyHandler`, `Tpsl1RuntimeCache`, `Tpsl1StrategyService`, `run_backtest`, `activate_rule`, `pause_rule`, `stop_and_close_rule`, `PaperActivation` | public surface |
| `entry/mod.rs` | `CriterionOutcome{NotConfigured,Satisfied,Rejected}`, `EntryFill`, `token_matches_buy_rule`, `find_first_matching_buy_rule`, `find_entry_fill_in_trades` | entry gating — **all** configured criteria must pass: initial_buy_sol, cu_limit, cu_price, max_sol_cost, spendable_sol_in, ix_labels |
| `exit/mod.rs` | `ExitReason`, `ExitFill`, `ExitWalkState`, `CachedExitState` (`build`, `build_unfolded`, `advance_and_find_exit`, tpsl2 `ensure_cohort_seeded`), `LadderParams`, `ladder_reason`, `find_trade_driven_exit`, `find_clock_driven_exit`, `should_position_exit_on_clock`, `clock_entry_time` | exit ladder; the live trade gate folds new trades + evaluates the ladder in one incremental pass (`advance_and_find_exit`) — no per-ping full re-walk. `ladder_reason` is the one per-trade decision shared by the full walk and the incremental gate so they can't drift. tpsl2 memoizes the E5 launch cohort (set + bag + running net) in `CachedExitState`, advanced incrementally instead of rebuilt per ping. `should_position_exit_on_trade`/`advance` retained as the test oracles. |
| `handler.rs` | `TPSL1StrategyHandler` (`new`,`check_buy_entry`,`get_rule`) | thin rule holder over `Arc<Vec<Rule>>`, rebuilt per token (atomic bump, not deep clone) |
| `execution/mod.rs` | retry consts (`BUY_MAX_ATTEMPTS`, `SELL_MAX_ATTEMPTS`, poll intervals, `PARTIAL_FILL_THRESHOLD`) | dispatch real vs paper by `rule.trade_mode` |
| `execution/real.rs` | `SnipeExecutor`, `BuyRetryCfg`, `classify_silent_send`, `buy_until_filled_or_give_up`, `sell_and_close_position`, `sell_until_balance_cleared` | live on-chain exec; double-buy/double-sell invariants |
| `execution/paper.rs` | `spawn_entry_fill_poll`, `spawn_exit_fill_poll`, `cache_trades` | paper exec: mirror feed, no tx sent; bounded 64 concurrent. Entry/exit fill confirmation reads the mint's trade window from the in-memory `token_cache` (`cache_trades`, kept current by the WS pipeline) on a bounded timer — **no** unbounded `find_by_mint_all` DB scan per tick. (tpsl2 real `await_scalp_entry_signal` likewise arms off the `token_cache` window.) Backtest still reads full history from the DB. |
| `service.rs` | `Tpsl1StrategyService` (`new`, `spawn_background_tasks`, `on_token_created`, `on_trade_executed`, `sweep_time_exits`) | wires entry/exit/execution; `selling: DashSet<Uuid>` guards no-double-sell |
| `lifecycle.rs` | `PaperActivation{Fresh,Continue}`, `activate_rule`, `pause_rule`, `stop_and_close_rule` | rule state transitions (single source of truth) |
| `runtime_cache.rs` | `Tpsl1RuntimeCache`, `PaperRunRef`, holding/position indexes, `time_exit_holding`, `exit_state_by_position`, `exit_state_build`/`exit_state_get`/`exit_state_advance_and_find_exit` | in-memory rules/positions/counters/paper-runs/exit-memos; loaded from DB at boot. The trade gate calls `exit_state_advance_and_find_exit` (incremental fold + ladder eval); the clock sweep reads `exit_state_get`, seeding via `exit_state_build` only on first sight. |
| `paper_run.rs` | `finish_paper_run_if_complete` | auto-finalize paper run when cap met + no open positions |
| `backtest.rs` | `BacktestTokenResult`, `run_backtest` | replay using the same `exit::find_trade_driven_exit` as live; candidates from the in-memory `token_cache` (not a full `tokens` scan). Trade history is served from the cross-run `app_state.backtest_trade_cache` (`BacktestTradeCache`, fresh iff `TokenState::trade_count` is unchanged) — re-running a tweaked rule re-fetches nothing for unchanged tokens. Cache misses are fetched in **batched chunks** (`trade_repo.find_by_mints_all`, `BACKTEST_FETCH_CHUNK` mints/query) with only `BACKTEST_FETCH_CONCURRENCY`≈3 chunk queries in flight, so a running sim holds ~3 of the ~20 shared PgPool conns instead of starving live ingest (was one query/token at concurrency 16) and skips the DB entirely when the whole chunk is cached; per-token resolution is synchronous over the in-hand `Arc<Vec<Trade>>`. Emits real progress via `SimProgress` — one `tick()` per resolved candidate (any outcome), broadcast as throttled `SseEvent::SimulationProgress` so the dashboard bar is determinate, not a fake trickle. **tpsl2 only:** `target_*` = the `find_scalp_entry` trigger; `entry_*` = the worst-case fill from `find_worst_case_paper_entry` (mirrors paper); exit/PnL/ATH are driven off the *fill* price, not the trigger; a token with no priced fill (`entry_fill.price <= 0`) is dropped, as paper deletes an un-filled position. `target_*` are `Option` on the struct (`None` only for legacy paper rows mapped by `paper_position_to_sim_result`). |
| `util.rs` | `none_if_zero_f64/u64` | 0 = "unset" sentinel for rule params |

`tpsl_sniper_2/` adds `cohort.rs` (launch-cohort primitive) and `entry/scalp.rs` (cohort-based scalp entry gates). `find_scalp_entry` runs in **one O(n) forward pass** — the launch cohort is fixed by the slice's first trade, so cohort/outside flows, the trailing alive-window sum, last-seen real reserves, and the higher-low confirmation index (`higher_low_confirmed_index`) are carried forward instead of recomputing `scalp_features` per candidate (was O(n²)). `scalp_features` / `higher_low_confirmed` / `cohort::held_ratio` / `cohort::outside_net_sol` are retained as the per-prefix oracles the linearized path is tested against.

**tpsl2 target (trigger-trade) capture.** The scalp-entry signal trade is the *target*; `EntryFill` carries its `amount_sol` so it can be persisted. Real: `await_scalp_entry_signal` returns the qualifying `EntryFill`; `service.rs` writes it via `position_repo.update_target` **before** sending the buy, then the wallet's own on-chain fill fills `entry_*` independently (a true gap). Paper: `spawn_entry_fill_poll` writes the trigger via `update_target`, then records `entry_*` from `entry/scalp.rs::find_worst_case_paper_entry` — the highest-priced (worst-case) trade in the trigger's slot S or S+1, strictly after it, non-dust/priced; ties → latest; empty → the trigger itself. `entry_amount` stays `rule.buy_amount` (the worst-case fill supplies only the entry tx/price/time); the trigger trade's SOL is the `target_amount`. **Backtest** now mirrors this same trigger→worst-case-fill pair (see `backtest.rs` above), so a tpsl2 simulation reproduces the live target↔entry slippage gap; the **paper-test-run** view surfaces the stored `target_*` straight off the `Position` via `paper_position_to_sim_result`. The gap vs. `entry_*` is derived client-side, not stored.

## Exit ladder (priority order)
`LiquidityExit → StopLoss → TakeProfit → TrailingStop → Stall → TimeStop`.
- Trade-driven (`find_trade_driven_exit`, each new trade) **and** clock-driven (`find_clock_driven_exit`, 1s sweep, Stall/TimeStop only) share feature predicates so they never drift.
- Optional features (0/None = off): `p_exit_trailing_stop_pct` (E1), `p_exit_time_stop_secs` (E2), `p_exit_stall_secs` (E3), `p_exit_liquidity_drop_pct` (E4). TakeProfit + StopLoss always on.

## Real vs paper
- `rule.trade_mode` selects path; same service, branches at execute. Real counters are all-time; paper counters are scoped to the current run.
- Entry recorded only from an **indexed** trade (no synthetic create-time fill), real and paper alike.

## Persistence (see [database.md](database.md))
`tpsl{1,2}_strategy_rules`, `tpsl{1,2}_real_positions`, `tpsl{1,2}_paper_test_run`, `tpsl{1,2}_paper_positions`. Repos: `storage/repositories/tpsl{1,2}_{strategy_rule,position,paper_trading}_repo.rs`.

## Invariants (preserve when editing)
1. No double-buy — only a confirmed on-chain revert (`classify_silent_send`) re-sends.
2. No double-sell — `selling` DashSet gates in-flight sells.
3. Sell-confirm via the `trades` feed, **no new RPC poll**; poll the full window before concluding a retry (see CLAUDE.md "Gotchas" — naively flipping confirm off fires duplicate sells). The confirm loop registers its `TradeSignals` guard once per exit and re-runs the `net_token_amount_by_wallet_and_mint` aggregate **only when the guard's `seq` advanced** (a new trade landed for this wallet+mint); bare fallback ticks skip the scan. SQL stays the authoritative "cleared" gate (deduped by PK) — don't replace it with a pure in-memory balance: feed redelivery would double-count and over-sell.
4. Time exits fire on silence (1s sweep).
5. Strategy eval reads `runtime_cache.rs` only — never queries DB per trade event.
