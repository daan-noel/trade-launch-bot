# Strategies — TPSL snipers

File-level map of `backend/src/strategies/`. Two **intentional clones**: `tpsl_sniper_1` (canonical/live) and `tpsl_sniper_2` (clone + cohort scalp gates). A fix in one usually belongs in both.
Logic explainers: `@plans/tpsl-strategy/` (strategy-invariants.md, tpsl2-entry-exit-params.md, pumpfun-sniper-strategy-research.md).

Backtesting is strategy-agnostic — the sweep engine only sees `Strategy`/`ParamSpace` returning `TokenOutcome`. TPSL2's impl (`sweep/strategies/tpsl2.rs`) reuses the same live entry/exit pure fns. A new strategy adds only its `Strategy`+`ParamSpace`+`AxesSpec` impl + a `registry.rs` arm + migration. See [@arch/sweep.md](@arch/sweep.md).

## Unified core domain — `trading_core::strategies` (Phase 2)

The strategy domain is unified behind one enum-dispatched registry in
`trading_core` (live-lab remake — see [live-lab-remake-plan.md](../live-lab-remake-plan.md)).
The decision logic in `tpsl_sniper_1`/`tpsl_sniper_2` is **unchanged** (still intentional clones); the
new modules only route by `strategy_id` and parse params once, so they run the identical code path
(exact parity). **Phase 3 rewired the live edge onto these** — the `live` bin now drives one
strategy-agnostic, registry-dispatched orchestration (`live/src/strategies/{service,runner,execution}`)
over the single `StrategyRuntimeCache` + `StrategyRepo`, and the cloned `live` `tpsl_sniper_{1,2}`
orchestration (caches, services, handlers, lifecycle, execution) was deleted. `lab` still consumes the
old per-strategy CRUD/backtest domain (`tpsl_rules_core`, `Tpsl1Rule`/`Tpsl2Rule`, the old repos) — its
migration is a later remake phase, so those core types survive until then.

| Module | Role |
| --- | --- |
| `registry.rs` | `StrategyImpl{Tpsl1,Tpsl2}` (`from_id`/`id`/`parse_params`); typed `Tpsl1Params`/`Tpsl2Params` (serde, `to_rule`/`from_rule`) parsed once from the `strategy_rules.params` JSONB; `StrategyParams`; enum-dispatched `matches_entry` / `resolve_entry` / `resolve_exit` over the unchanged tpsl1/2 fns |
| `kernel.rs` | `simulate_rule(StrategyImpl, &StrategyParams, &[&[T]], &SimConfig) -> RunMetrics` + `simulate_token`; `RunMetrics` ≡ the `strategy_run_metrics` columns (`to_run_metrics(run_id)`); `CostModel`/`round_trip_with_costs` + `QuantileSketch`/`RunAgg` (ported from `lab/sweep`; Phase 4 repoints the sweep here and drops the dup) |
| `runtime_cache.rs` | strategy-agnostic `StrategyRuntimeCache`: active rules + parsed params, holding-by-mint index, per-rule cap counters, `RuleClosedStats`, run pointer, `ExitGuard`/`EntryGuard` RAII, **the per-position clock exit-state memo + time-exit index** (`exit_state_advance_and_find_exit` / `exit_state_clock_check`, Phase-3), and the bounded until-dead armer set. DB loaders + run lifecycle (`load_from_db`/`reload_rules`/`start_run`/`stop_run`) over `StrategyRepo`; SSE-free (the live edge emits) |
| `exit_state.rs` | `CachedExitStateImpl` enum-wrapped per-strategy exit memo + `clock_entry_time` — the incremental trade-gate + clock-sweep the cache drives |
| `rules.rs` | `strategy_id`-dispatched rule-CRUD domain: `validate` (percent ranges + tpsl2 entry-window/scalp-gate), `build_rule(RuleDraft) -> StrategyRule`, `create`/`save` against `StrategyRepo`. Replaces the cloned `tpsl_rules_core` split. The calling **edge** (live `api/handlers/strategies/rules.rs`) appends cache `reload_rules` + `rules_changed` SSE via the service |

Universal knobs (`buy_amount`, `trade_mode`, caps) are the typed columns on `StrategyRule`; only the
strategy-specific gates live in `params`. `StrategyPosition` gained PnL/status helpers
(`is_win`/`realized_pnl_sol`/`pnl_pct`/`is_closed`/…) mirroring the `strategy_position_pnl` view.

## Dispatch — `live/src/strategies/runner.rs` (Phase 3)

`StrategyRunner` consumes `strategy_rx` in a `select!` loop and routes to the **one** unified
`StrategyService` (which dispatches each per-rule decision through `StrategyImpl` keyed by
`rule.strategy_id` — no more per-strategy services):

- `on_token_created(mint)` → entry gating (`matches_entry`) + run ensured + inline cap/holding-index claim → tpsl1 immediate buy / tpsl2 scalp-arm then buy
- `on_trade_executed(mint)` → exit evaluation via the core exit-state memo → paper close / real sell
- 1s clock tick → `sweep_time_exits()` (deadline exits that come due in silence)

The `select!` **serializes** all position transitions (no Holding→ExitPending interleave). In-memory transitions happen inline; slow DB/chain work is spawned.

**Position lifecycle:** `Arming → BuySubmitted → Holding → ExitPending → End/ExitFailed`

**Live orchestration files** (`live/src/strategies/`): `service.rs` (the unified service: entry gate,
trade/time exit ladder, real sell, manual-sell close, recovery reapers, rule CRUD + activate/pause/
stop lifecycle), `runner.rs` (the `select!` dispatch), `execution/{real,paper,scalp}.rs` (real on-chain
exec + double-buy/sell invariants · paper mirror fill-poll · tpsl2 scalp-arming).

## Shared — `sim_progress.rs`

`SimProgress` — per-backtest progress reporter shared by both snipers' `backtest.rs`. Simulate is a **start→wait→fetch** job (POST → 202, result stored in `AppState.sim_results`, client collects via SSE + `GET /api/jobs/simulations/{rule_id}/result`).

## Per-strategy module map (`tpsl_sniper_1/`, mirrored in `tpsl_sniper_2/`)

> **Phase 3 note.** The **decision** modules (`entry/`, `exit/`, `cohort.rs`, `util.rs`) live in
> `trading_core::strategies::tpsl_sniper_{1,2}` and are consumed by both the registry (live) and the
> sweep/backtest (lab). The **orchestration** files below (`mod.rs` handler, `handler.rs`,
> `execution/`, `service.rs`, `lifecycle.rs`, `runtime_cache.rs`, `paper_run.rs`, `backtest.rs`) were
> **deleted from `live`** — that path is now the single unified `live/src/strategies/{service,runner,
> execution}`. They remain in `lab` (which still runs the old per-strategy CRUD + backtest) until lab's
> migration phase. The table describes those still-present (lab/decision) modules.

| File | Responsibility |
| --- | --- |
| `mod.rs` | `TPSL1StrategyHandler`, `Tpsl1RuntimeCache`, `Tpsl1StrategyService`, `run_backtest`, `activate_rule`, `pause_rule`, `stop_and_close_rule` |
| `entry/mod.rs` | `token_matches_buy_rule`, `find_first_matching_buy_rule`, `find_entry_fill_in_trades` — all configured criteria must pass. `token_is_fresh` (30s `MAX_SNIPE_AGE_SECS` gate) is **live-only**, applied by `find_all_matching_buy_rules`/`matches_entry`, NOT by the shared matcher — so the historical matched/simulate scan isn't emptied by token age |
| `exit/mod.rs` | `ExitWalkState`, `CachedExitState`, `LadderParams`, `find_trade_driven_exit`, `find_clock_driven_exit` — incremental ladder eval |
| `handler.rs` | Thin rule holder over `Arc<Vec<Rule>>`, rebuilt per token |
| `execution/mod.rs` | Dispatch real vs paper by `rule.trade_mode`; retry consts |
| `execution/real.rs` | Live on-chain exec; double-buy/double-sell invariants; write-ahead persist; sell fee guard; per-signature attribution |
| `execution/paper.rs` | Mirror feed, no tx sent; bounded 64 concurrent; count-gated re-walk |
| `service.rs` | Wires entry/exit/execution; `ExitGuard`/`EntryGuard` RAII interlocks; recovery reapers; SOL balance-floor guard |
| `lifecycle.rs` | `activate_rule`, `pause_rule`, `stop_and_close_rule` — rule state transitions |
| `runtime_cache.rs` | In-memory rules/positions/counters/paper-runs/exit-memos; `exiting`/`entering` guard sets; `ladder_params_by_id` (no full Rule clone on hot path); **emits `tpsl_positions_changed` SSE deltas from the `sync_position`/`remove_position` funnel** via an optional sender (`set_sse_sender`; unset in tests/`lab` ⇒ no-op) — every transition funnels here, so no delta is missed |
| `paper_run.rs` | `finish_paper_run_if_complete` |
| `backtest.rs` | `run_backtest` — replay using same exit fns as live; cross-run `BacktestTradeCache`; detached + 202 |
| `util.rs` | `none_if_zero_f64/u64` |

`tpsl_sniper_2/` adds `cohort.rs` (launch-cohort primitive) and `entry/scalp.rs` (cohort-based scalp entry gates).

## Buy guards (service.rs)

Checked sequentially for every real buy, inline before `sync_position`. If either fires: `continue` — no position is created, no runtime cleanup needed.

1. **SOL balance-floor** (`can_commit_buy`) — `wallet_balance − 0.02 SOL − committed_lamports ≥ buy_amount`. Fails open when the balance cache is cold (startup) so a stale cache never blocks all buys.
2. **`trade.max_committed_sol`** — if set, blocks buy when `committed + buy_amount > ceiling`. Optional (default: no ceiling). Configured live via the Settings API; no restart needed.

`committed_lamports` is shared across both TPSL1 and TPSL2 — the ceiling and floor apply to the wallet's total open exposure regardless of which strategy opened the position.

## Recovery reapers (service.rs)

Two background tasks fire once at boot then every 60 s:

- **`redrive_orphaned_buy_submitted`** — classifies in-flight `BuySubmitted` rows (adopt from feed / drop if all sigs reverted / wait if any sig pending); **never re-sends**.
- **`redrive_orphaned_exit_pending`** — re-drives `ExitPending` rows with no live `ExitGuard` (sell task gone); runs **before** the stale-fail sweep so recoverable bags retry before being marked `ExitFailed`.
- **`reconcile_externally_cleared_holdings`** — closes `Holding` rows whose bag was cleared **outside** the strategy exit path (a manual "Sell"/"Sell All"). One set-based candidate query (`find_externally_cleared_holding_mints`: entry-recorded `Holding` + a sell on record + net traded balance ≤ `PARTIAL_FILL_THRESHOLD`), then `execution::real::reconcile_externally_cleared_mint` drives each to `End` (reason `ManualSell`) with no new sell tx. Catch-all for the live-ping detector (`try_close_manually_sold`), which only fires on the *next* trade for a mint — a dead, manually-sold token never produces one, so without this the row stays `Holding` forever and boot seeding reloads it verbatim. The `manual_sell` API handler calls the same `reconcile_externally_cleared_mint` immediately (off the response path, brief retry to await sell indexing) so the UI updates without waiting for the 60 s tick.

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
