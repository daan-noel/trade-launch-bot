# Strategies — TPSL snipers

File-level map of `backend/src/strategies/`. Two **intentional clones**: `tpsl_sniper_1` (canonical/live) and `tpsl_sniper_2` (clone + cohort scalp gates). A fix in one usually belongs in both.
Logic explainers: `@plans/tpsl-strategy/` (strategy-invariants.md, tpsl2-entry-exit-params.md, pumpfun-sniper-strategy-research.md).

Backtesting is strategy-agnostic — the sweep engine only sees `Strategy`/`ParamSpace` returning `TokenOutcome`. TPSL2's impl (`sweep/strategies/tpsl2.rs`) reuses the same live entry/exit pure fns. A new strategy adds only its `Strategy`+`ParamSpace`+`AxesSpec` impl + a `registry.rs` arm + migration. See [@arch/sweep.md](@arch/sweep.md).

## Dispatch — `runner.rs`

`StrategyRunner` consumes `strategy_rx` in a `select!` loop and routes to both strategy services:

- `on_token_created(mint)` → entry gating + inline cap/holding-index claim → **spawn** DB insert + buy
- `on_trade_executed(mint)` → exit evaluation → spawn sell
- 1s clock tick → `sweep_time_exits()` (deadline exits that come due in silence)

The `select!` **serializes** all position transitions (no Holding→ExitPending interleave). In-memory transitions happen inline; slow DB/chain work is spawned.

**Position lifecycle:** `Arming → BuySubmitted → Holding → ExitPending → End/ExitFailed`

## Shared — `sim_progress.rs`

`SimProgress` — per-backtest progress reporter shared by both snipers' `backtest.rs`. Simulate is a **start→wait→fetch** job (POST → 202, result stored in `AppState.sim_results`, client collects via SSE + `GET /api/jobs/simulations/{rule_id}/result`).

## Per-strategy module map (`tpsl_sniper_1/`, mirrored in `tpsl_sniper_2/`)

| File | Responsibility |
| --- | --- |
| `mod.rs` | `TPSL1StrategyHandler`, `Tpsl1RuntimeCache`, `Tpsl1StrategyService`, `run_backtest`, `activate_rule`, `pause_rule`, `stop_and_close_rule` |
| `entry/mod.rs` | `token_matches_buy_rule`, `find_first_matching_buy_rule`, `find_entry_fill_in_trades` — all configured criteria must pass |
| `exit/mod.rs` | `ExitWalkState`, `CachedExitState`, `LadderParams`, `find_trade_driven_exit`, `find_clock_driven_exit` — incremental ladder eval |
| `handler.rs` | Thin rule holder over `Arc<Vec<Rule>>`, rebuilt per token |
| `execution/mod.rs` | Dispatch real vs paper by `rule.trade_mode`; retry consts |
| `execution/real.rs` | Live on-chain exec; double-buy/double-sell invariants; write-ahead persist; sell fee guard; per-signature attribution |
| `execution/paper.rs` | Mirror feed, no tx sent; bounded 64 concurrent; count-gated re-walk |
| `service.rs` | Wires entry/exit/execution; `ExitGuard`/`EntryGuard` RAII interlocks; recovery reapers; SOL balance-floor guard |
| `lifecycle.rs` | `activate_rule`, `pause_rule`, `stop_and_close_rule` — rule state transitions |
| `runtime_cache.rs` | In-memory rules/positions/counters/paper-runs/exit-memos; `exiting`/`entering` guard sets; `ladder_params_by_id` (no full Rule clone on hot path) |
| `paper_run.rs` | `finish_paper_run_if_complete` |
| `backtest.rs` | `run_backtest` — replay using same exit fns as live; cross-run `BacktestTradeCache`; detached + 202 |
| `util.rs` | `none_if_zero_f64/u64` |

`tpsl_sniper_2/` adds `cohort.rs` (launch-cohort primitive) and `entry/scalp.rs` (cohort-based scalp entry gates).

## Exit ladder (priority order)

`LiquidityExit → StopLoss → TakeProfit → TrailingStop → Stall → TimeStop`

- Trade-driven (`find_trade_driven_exit`, each new trade) and clock-driven (`find_clock_driven_exit`, 1s sweep, Stall/TimeStop only) share feature predicates.
- Optional features (0/None = off): trailing stop (E1), time stop (E2), stall (E3), liquidity drop (E4), cohort ratio (E5, tpsl2 only). TakeProfit + StopLoss always on.
- All `p_*_pct` params stored whole-percent, divided by 100 at the comparison site.

## Real vs paper

- `rule.trade_mode` selects path; same service, branches at execute.
- Real counters are all-time; paper counters are scoped to the current run.
- Entry recorded only from an **indexed** trade (no synthetic fill).

## Persistence

Tables: `tpsl{1,2}_strategy_rules`, `tpsl{1,2}_real_positions`, `tpsl{1,2}_paper_test_run`, `tpsl{1,2}_paper_positions`.
Repos: `storage/repositories/tpsl{1,2}_{strategy_rule,position,paper_trading}_repo.rs`. See [@arch/database.md](@arch/database.md).

## Invariants (preserve when editing)

See `@plans/tpsl-strategy/strategy-invariants.md` for the full list. Summary:

1. No double-buy — write-ahead persist before submit; boot reaper adopts/waits/drops, never re-sends
2. No double-sell — `ExitGuard` RAII gates all exit paths
3. Sell-confirm via `trades` feed, no new RPC; per-signature attribution (not shared net balance)
4. Time exits fire on silence (1s sweep)
5. Strategy eval reads `runtime_cache.rs` only — never DB-per-event
6. Live-rule edit guard — frozen fields reject changes while rule is live (409)
7. Clear-results guard — paper-only, idle-only
