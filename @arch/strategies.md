# Strategies — TPSL snipers

File-level map of `backend/src/strategies/`. Two **intentional clones**: `tpsl_sniper_1` (canonical/live) and `tpsl_sniper_2` (clone + scalp-continuation entry gates). A fix in one usually belongs in both.
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
| `kernel.rs` | shared metric-aggregation primitives: `RunAgg` (streaming fold of per-token `TokenOutcome`s) → `RunMetrics` ≡ the `strategy_run_metrics` columns; `CostModel`/`round_trip_with_costs` + `QuantileSketch`. `lab/sweep` folds into the same `RunAgg` via its `ComboAgg` wrapper, so backtest/live/paper metrics share one copy of the sketch/robust-score math |
| `runtime_cache.rs` | strategy-agnostic `StrategyRuntimeCache`: active rules + parsed params, holding-by-mint index, per-rule cap counters, `RuleClosedStats`, run pointer, `ExitGuard`/`EntryGuard` RAII, **the per-position clock exit-state memo + time-exit index** (`exit_state_advance_and_find_exit` / `exit_state_clock_check`, Phase-3), and the bounded until-dead armer set. DB loaders + run lifecycle (`load_from_db`/`reload_rules`/`start_run`/`stop_run`) over `StrategyRepo`; SSE-free (the live edge emits) |
| `exit_state.rs` | `CachedExitStateImpl` enum-wrapped per-strategy exit memo + `clock_entry_time` — the incremental trade-gate + clock-sweep the cache drives |
| `rules.rs` | `strategy_id`-dispatched rule-CRUD domain: `validate` (percent ranges + tpsl2 entry-window/scalp-gate), `build_rule(RuleDraft) -> StrategyRule`, `create`/`save` against `StrategyRepo`. Replaces the cloned `tpsl_rules_core` split. The calling **edge** (live `api/handlers/strategies/rules.rs`) appends cache `reload_rules` + `rules_changed` SSE via the service |

Universal knobs (`buy_amount_sol`, `trade_mode`, caps) are the typed columns on `StrategyRule`; only the
strategy-specific gates live in `params`. `StrategyPosition` gained PnL/status helpers
(`is_win`/`realized_pnl_sol`/`pnl_pct`/`is_closed`/…) mirroring the `strategy_position_pnl` view.

## Dispatch — `live/src/strategies/runner.rs` (Phase 3)

`StrategyRunner` consumes `strategy_rx` in a `select!` loop and routes to the **one** unified
`StrategyService` (which dispatches each per-rule decision through `StrategyImpl` keyed by
`rule.strategy_id` — no more per-strategy services):

- `on_token_created(mint)` → entry gating (`matches_entry` / deferred `pending_first_slot`) + run ensured + inline cap/holding-index claim → tpsl1 immediate buy / tpsl2 scalp-arm then buy / swing1 phase-entry-arm then buy
- `on_trade_executed(mint)` → **deferred first-slot gate resolve** (when window closes) + exit evaluation via the core exit-state memo → paper close / real sell
- 1s clock tick → `sweep_time_exits()` (deadline exits that come due in silence) + `sweep_first_slot_pending()` (5s backstop for deferred entry gates)

The `select!` **serializes** all position transitions (no Holding→ExitPending interleave). In-memory transitions happen inline; slow DB/chain work is spawned.

**Position lifecycle:** `Arming → BuySubmitted → Holding → ExitPending → End/ExitFailed`

**Live orchestration files** (`live/src/strategies/`): `service.rs` (the unified service: entry gate,
trade/time exit ladder, real sell, manual-sell close, recovery reapers, rule CRUD + activate/pause/
stop lifecycle), `runner.rs` (the `select!` dispatch), `execution/{real,paper,scalp,swing}.rs` (real
on-chain exec + double-buy/sell invariants · paper mirror fill-poll · tpsl2 scalp-arming · swing1
phase-entry arming). `scalp.rs`/`swing.rs` are thin live-watch wrappers around the shared
`tpsl_sniper_2::entry::find_scalp_entry` / `swing_1::entry::find_phase_entry` gates — the same fns the
paper entry resolvers and backtests call, so live honors the identical entry moment.

## Shared — `sim_progress.rs` + `sim_fetch.rs` + token-enrichment SSOT

`SimProgress` — per-backtest progress reporter shared by all three backtests' `backtest.rs`. Simulate is a **start→wait→fetch** job (POST → 202; the detached run stores its per-token results as `Vec<serde_json::Value>` in `LocalState.sim_results`; client collects via the `simulation_finished` SSE).

`sim_fetch.rs::fetch_sim_histories(mints, curve_only) -> HashMap<mint, Arc<Vec<SweepTrade>>>` (+ `fetch_sim_history_one(mint, curve_only)` single-mint wrapper) — the **single shared lake read** behind tpsl1/tpsl2/swing1 backtests **and the per-token `swing1-detect` endpoint**. All keep their PG candidate scan (`collect_matching_tokens`, the `tokens` table) but pull trade histories from the **same Parquet lake, same loader, same `SweepTrade`** the grouped sweep uses — just with `Selection::with_signatures = true` so `SweepTrade::tx_signature` (`Option<Box<str>>`) is populated for the result tables' Solscan links (the sweep leaves it `None`). So a rule prices identically whether swept or drilled into, by construction. Uncapped per-mint (full history, matching the old `find_by_mints_all`), with a stale-lake warn (the lake is sealed-days-only → keep `lake-export --include-today` on a cadence). `curve_only` is a **load-time** `Selection` filter (the projected `SweepTrade` drops `venue`, so a venue filter must precede projection): backtests pass `false`; the detect endpoint threads the request flag. The old per-chunk PG fetch + `BacktestTradeCache` is gone; the flag's isolation is guarded by `lake::duck::parity_tests` (`--ignored`).

**Swing1 detection is lake-backed + funnel-shared.** `POST /api/tokens/{mint}/swing1-detect` (`lab/src/api/handlers/tokens/swing1_detect.rs`) reads the **same uncapped lake corpus** (`fetch_sim_history_one`) — not PG, and NOT the old `MAX_TRADES_RETAINED` cap (that constant is the live in-RAM cache trim, never an analysis bound). It runs the shared `trading_core::strategies::swing_1::funnel::build_swing1_funnel<T: TradeRow>(trades, rule) -> Swing1Funnel { gate_configured, legs, lows, latch }`. **Two levels of single-source-of-truth:** (1) the *decision* logic — `entry::find_phase_entry` / `exit::find_trade_driven_exit` — is the one place entry/exit resolve, and the grouped sweep, backtest, detect handler and probe all call it, so a rule prices identically however run; (2) the *diagnostic* layer (legs + per-low verdicts + latch) is the funnel, whose per-low walk is extracted to `funnel::classify_swing_lows(legs, profile) -> Vec<Swing1LowVerdict>` and shared by `build_swing1_funnel`, `lab swing-probe`, and `lab swing-census` (no caller re-implements the gate loop). `funnel_matches_leg_primitive` (`funnel.rs`) pins the funnel's legs to the exact `detect_swing_legs_raw` call the backtest carries. The swing1 **backtest** (`swing_1::backtest`) carries the exact legs it priced entry/exit against in its result row (`BacktestTokenResult { #[serde(flatten)] base: BacktestBase, swing_legs: Option<Vec<SwingLeg>> }`) — sourced from that same leg primitive, not the full funnel, so the multi-token sim payload isn't bloated with per-token lows/latch the table never shows — and the single-rule inspect chart (`Swing1Page` `SwingRuleInspectModal`) draws them with no separate detect round-trip. **Caveat:** the chart *candles* still come from PG `GET /trades`; for a sealed (past-day) token PG≡lake so legs align, but a today-token's lake read is stale (the `warn_if_stale` log). The **generic** `swing.rs` endpoints (`detect_swings`, batch) remain on PG + the `MAX_TRADES_RETAINED` cap — a separate analyzer, not migrated.

**Simulated table = in-memory server-side paging** (`lab/src/strategies/sim_query.rs`). The finished backtest's rows are already resident (lab is single-user, workstation RAM), so `POST …/rules/{id}/simulate/result` (unified `TableRequest`) pages/sorts/filters them in Rust — numeric operators compare numerically, matching the SQL path's semantics — with a whole-run `GET …/simulate/result/summary` aggregate. No `sim_result_tokens` table: an in-RAM query fits the data. The legacy whole-blob `GET /api/jobs/simulations/{rule_id}/result` still serves (re-serializes on take).

**Token enrichment is one SSOT across every token-result table** — `trading_core::storage::token_enrichment` (`ENRICH_SELECT` SQL fragment · `TokenEnrichmentRow` FromRow · `TokenEnrichment` `#[serde(flatten)]` struct · `fetch_by_mints` bounded batch). The Matched, Positions (live + lab), Simulated, and Sweep drill-in tables all render the same ~28-field token metadata set (mirrors the frontend `TOKEN_ENRICH_FIELDS`) **in the response body**, so sort/filter/search on those columns works server-side with **no client-side `mergeTokenData`**. Two attach mechanisms by table shape: SQL-paged tables (Matched: `find_tokens_by_mints_paged` selects `ENRICH_SELECT`; Positions: `enrich_position_responses` batch-fetches by the page's mints since `strategy_positions LEFT JOIN tokens` can't safely deserialize a missing token) and in-memory tables (Simulated/Sweep: `fetch_by_mints` after the rows are built in Rust). Each host row owns `mint`/`symbol` (identity) plus `ath_price`/`created_at` — those four are excluded from `TokenEnrichment`. `ath_price` is the uniform `tokens_info.ath_price` everywhere (Simulated no longer recomputes it from its corpus — it reads it off the enrichment row like Positions/Sweep); it stays row-owned only so a host struct that declares its own `ath_price` field doesn't collide with a flattened one under `#[serde(flatten)]`. `created_at` genuinely diverges (Positions keeps the position's, not the token's); token creation is aliased `token_created_at` in SQL so it never collides with `strategy_positions.created_at`. `lab/src/strategies/token_enrich.rs` is now a thin wrapper re-exporting the shared type for the Simulated path.

`BacktestTokenResult` (tpsl1/tpsl2/swing1's per-token row) flattens `token: TokenEnrichment`, filled in **once per backtest run** right after `select_simulated_tokens` narrows to the final result set (one bounded `fetch_by_mints`, keyed by mint, before the rows are stored in `SimResults`). `sim_query.rs::resolve()` whitelists both the sim-specific keys (`holding`→`holding_secs`, `reason`→`exit_reason`, ...) and the `appendedTokenColumns` display keys (`initial_buy`→`initial_buy_sol`, `cu_limit`, `migrated`→`is_migrated`, ...); booleans sort via `field_num`'s bool→0.0/1.0 coercion. The token's trade count comes solely from the shared enrichment `trade_count` (the "Token Trades" column); an earlier sim-only `total_trades` (the walked-corpus count) was removed as a duplicate of it. Paper-position rows (`paper_position_to_sim_result`) reuse the same shape but default `token` to empty — a one-off preview, not a full backtest, so no batch fetch runs for them.

**Matched table = materialize-then-page** (`lab/src/state/matched_cache.rs`). The match predicate is a Rust closure (not SQL), so `POST …/rules/{id}/matched` (unified `TableRequest`) runs `collect_matching_tokens` once per `(rule_id, from, to)` to get the matched **mint set**, caches it on `LocalState` (`MatchedCache`, TTL/GC like `sim_results`), and pages the DB restricted to `mint = ANY(set)` via `StrategyRepo::find_tokens_by_mints_paged` (token-scoped whitelist), which now selects the shared `ENRICH_SELECT` and returns a full `TokenEnrichmentRow` (the old sparse 6-field `MatchedTokenRow` + client `mergeTokenData` band-aid is gone). Removes the old 5,000-row display cap. tpsl2/swing1 delegate the paging to `tpsl1::matched_page_response` (strategy-agnostic).

**Positions table = Current run + Old runs** (`?scope=current|history` on `POST …/rules/{id}/positions` and its `/summary`). A rule's positions are split by run: `current` pages the rule's latest run (`StrategyRepo::latest_run` → `find_positions_by_run_paged` + `positions_summary_by_run`); `history` pages every prior run (`find_positions_by_rule_excluding_run_paged` + `positions_summary_by_rule_excluding_run`, excluding the latest run id), each row stamped with its `run_seq` from `run_seqs_for_rule` for a run column + per-run banding. Scope absent = legacy (paper = latest run, real = all runs). Both live (`live/src/api/handlers/strategies/positions.rs`) and lab (paper-only) handlers accept the param, and both attach the shared token enrichment to the page via `enrich_position_responses`/`enrich_positions` (one `fetch_by_mints` over the page's mints) — `PositionResponse` (the live-local + `trading_core` copies both) gained `symbol`/`ath_price` + `#[serde(flatten)] token: TokenEnrichment`, defaulted empty on the SSE-delta / single-position paths. Frontend: one shared `RunPositionsPanel` renders both sections — Current run keeps the live SSE/poll `useRulePositions` path; Old runs is a second `useRulePositions(…, live=false)` (fetch-only, immutable) and only shows when a prior run exists. Wired into all live + lab strategy pages, now consuming already-enriched rows (no `mergeTokenData`).

## Per-strategy module map (`tpsl_sniper_1/`, mirrored in `tpsl_sniper_2/`)

> **Phase 3 note.** The **decision** modules (`entry/`, `exit/`, `util.rs`) live in
> `trading_core::strategies::tpsl_sniper_{1,2}` and are consumed by both the registry (live) and the
> sweep/backtest (lab). The **orchestration** files below (`mod.rs` handler, `handler.rs`,
> `execution/`, `service.rs`, `lifecycle.rs`, `runtime_cache.rs`, `paper_run.rs`, `backtest.rs`) were
> **deleted from `live`** — that path is now the single unified `live/src/strategies/{service,runner,
> execution}`. They remain in `lab` (which still runs the old per-strategy CRUD + backtest) until lab's
> migration phase. The table describes those still-present (lab/decision) modules.

| File | Responsibility |
| --- | --- |
| `mod.rs` | `TPSL1StrategyHandler`, `Tpsl1RuntimeCache`, `Tpsl1StrategyService`, `run_backtest`, `activate_rule`, `pause_rule`, `stop_and_close_rule` |
| `entry/mod.rs` | `token_matches_buy_rule`, `token_matches_instant_criteria` (skips deferred first-slot checks), `find_first_matching_buy_rule`, `find_entry_fill_in_trades` — all configured criteria must pass. `p_token_first_slot_buy_sol` / `p_token_first_slot_sell_sol` are **deferred** live (see below). `token_is_fresh` (30s `MAX_SNIPE_AGE_SECS` gate) is **live-only**, applied by `find_all_matching_buy_rules`/`matches_entry`, NOT by the shared matcher — so the historical matched/simulate scan isn't emptied by token age |
| `exit/mod.rs` | `ExitWalkState`, `CachedExitState`, `LadderParams`, `find_trade_driven_exit`, `find_clock_driven_exit` — incremental ladder eval |
| `handler.rs` | Thin rule holder over `Arc<Vec<Rule>>`, rebuilt per token |
| `execution/mod.rs` | Dispatch real vs paper by `rule.trade_mode`; retry consts |
| `execution/real.rs` | Live on-chain exec; double-buy/double-sell invariants; write-ahead persist; sell fee guard; per-signature attribution |
| `execution/paper.rs` | Mirror feed, no tx sent; bounded 64 concurrent; count-gated re-walk |
| `service.rs` | Wires entry/exit/execution; `ExitGuard`/`EntryGuard` RAII interlocks; recovery reapers; SOL balance-floor guard |
| `lifecycle.rs` | `activate_rule`, `pause_rule`, `stop_and_close_rule` — rule state transitions |
| `runtime_cache.rs` | In-memory rules/positions/counters/paper-runs/exit-memos; `exiting`/`entering` guard sets; `ladder_params_by_id` (no full Rule clone on hot path); **emits `tpsl_positions_changed` SSE deltas from the `sync_position`/`remove_position` funnel** via an optional sender (`set_sse_sender`; unset in tests/`lab` ⇒ no-op) — every transition funnels here, so no delta is missed |
| `paper_run.rs` | `finish_paper_run_if_complete` |
| `backtest.rs` | `run_backtest` — replay using same exit fns as live; trade histories from the Parquet lake (`sim_fetch::fetch_sim_histories`, shared by all three backtests); detached + 202 |
| `util.rs` | `none_if_zero_f64/u64` |

`tpsl_sniper_2/` adds `entry/scalp.rs` (per-trade scalp-continuation entry gates — age/liveness/net-buy-demand/liquidity/pullback).

## Two-phase token fingerprint entry gate (first-slot SOL)

`p_token_first_slot_buy_sol` / `p_token_first_slot_sell_sol` live on all three strategies' fingerprint param group. Unlike `p_token_initial_buy_sol` (known at `TokenCreated`), first-slot totals stream in over same-slot trades and close when a later-slot trade arrives (`TokenState::first_slot_window_open` latch in `token_cache.rs`).

- **Instant criteria** (creation-time fingerprint axes) — still evaluated synchronously in `on_token_created` via `StrategyImpl::matches_instant_entry`.
- **Deferred criteria** (first-slot buy/sell) — when `StrategyParams::requires_first_slot_data()`, instant pass queues `(mint, rule_id)` in `StrategyRuntimeCache::pending_first_slot` (bounded like `until_dead_armers`, cap 32) instead of buying immediately.
- **Resolve triggers:** (1) primary — `on_trade_executed` when `first_slot_window_open` flips false for a mint with pending entries; (2) backstop — `sweep_first_slot_pending` on the existing 1s runner tick (`FIRST_SLOT_GATE_TIMEOUT_SECS = 5`). One-shot: resolved or expired entries never retry.
- **Backtest/matched/simulate** — no deferral; `TokenRepo::find_page_before` LEFT JOINs `tokens_info` so `Token.first_slot_buy_sol`/`first_slot_sell_sol` are populated before `token_matches_buy_rule` runs.

## Buy guards (service.rs)

Checked sequentially for every real buy, inline before `sync_position`. If either fires: `continue` — no position is created, no runtime cleanup needed.

1. **SOL balance-floor** (`can_commit_buy`) — `wallet_balance − 0.02 SOL − committed_lamports ≥ buy_amount_sol`. Fails open when the balance cache is cold (startup) so a stale cache never blocks all buys.
2. **`trade.max_committed_sol`** — if set, blocks buy when `committed + buy_amount_sol > ceiling`. Optional (default: no ceiling). Configured live via the Settings API; no restart needed.

`committed_lamports` is shared across both TPSL1 and TPSL2 — the ceiling and floor apply to the wallet's total open exposure regardless of which strategy opened the position.

## Recovery reapers (service.rs)

Two background tasks fire once at boot then every 60 s:

- **`redrive_orphaned_buy_submitted`** — classifies in-flight `BuySubmitted` rows (adopt from feed / drop if all sigs reverted / wait if any sig pending); **never re-sends**.
- **`redrive_orphaned_exit_pending`** — re-drives `ExitPending` rows with no live `ExitGuard` (sell task gone); runs **before** the stale-fail sweep so recoverable bags retry before being marked `ExitFailed`.
- **`reconcile_externally_cleared_holdings`** — closes `Holding` rows whose bag was cleared **outside** the strategy exit path (a manual "Sell"/"Sell All"). One set-based candidate query (`find_externally_cleared_holding_mints`: entry-recorded `Holding` + a sell on record + net traded balance ≤ `PARTIAL_FILL_THRESHOLD`), then `execution::real::reconcile_externally_cleared_mint` drives each to `End` (reason `ManualSell`) with no new sell tx. Catch-all for the live-ping detector (`try_close_manually_sold`), which only fires on the *next* trade for a mint — a dead, manually-sold token never produces one, so without this the row stays `Holding` forever and boot seeding reloads it verbatim. The `manual_sell` API handler calls the same `reconcile_externally_cleared_mint` immediately (off the response path, brief retry to await sell indexing) so the UI updates without waiting for the 60 s tick.

## Exit ladder (priority order)

`LiquidityExit → StopLoss → TakeProfit → TrailingStop → Stall → TimeStop`

- Trade-driven (`find_trade_driven_exit`, each new trade) and clock-driven (`find_clock_driven_exit`, 1s sweep, Stall/TimeStop only) share feature predicates.
- Optional features (0/None = off): trailing stop (E1), time stop (E2), stall (E3), liquidity drop (E4). TakeProfit + StopLoss always on.
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
