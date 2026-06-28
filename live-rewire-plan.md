# Phase 3 · `live` rewire — registry-dispatched strategy orchestration

Sub-task decomposition for **Phase 3** of [live-lab-remake-plan.md](live-lab-remake-plan.md).
Phases 0–2 are done; core now owns the registry (`StrategyImpl`/`StrategyParams`), the
simulation kernel, the unified `StrategyRuntimeCache`, the unified rule-CRUD domain
(`strategies::rules`), and the ingest contract (`trading_core::ingest`). This phase makes
`live` **consume** them and deletes the hand-cloned tpsl1/tpsl2 orchestration.

## Goal

One **strategy-agnostic** runner + service + execution path in `live`, dispatching every
per-event decision through `StrategyImpl::from_id(rule.strategy_id)`. No `Tpsl1*`/`Tpsl2*`
types survive in `live`. Hot-path budgets preserved exactly: sell-confirm off the `trades`
feed (no new RPC), the per-second clock sweep never re-walks history (memoized exit state),
strategy eval reads the in-memory cache only.

## What `live` consumes from core (already built in Phase 2)

- Models: `StrategyRule` / `StrategyRun` / `StrategyRunMetrics` / `StrategyPosition`.
- Repo: `StrategyRepo` (rules/runs/metrics/positions CRUD).
- Registry: `StrategyImpl`, `StrategyParams`, `Tpsl1Params`/`Tpsl2Params`, `resolve_entry`/`resolve_exit`.
- Cache: `StrategyRuntimeCache` (rules+params, holding index, caps, closed-stats, paper-run ptr, exit/entry guards).
- Rule-CRUD: `strategies::rules::{validate, build_rule, create, save, RuleDraft, RuleError}`.
- Exit primitives (already in core): `tpsl_sniper_{1,2}::exit::{CachedExitState, ExitWalkState, LadderParams, ExitReason, clock_entry_time, should_position_exit_on_clock, find_trade_driven_exit}` — all generic over `TradeRow`; `TokenCache`/`CachedTrade` are in `trading_core::state`.

## The one design decision (locked here)

`tpsl1::CachedExitState` and `tpsl2::CachedExitState` are **distinct types**, and the live
incremental hot path (per-trade gate + per-second clock sweep) needs a *stateful* memo, not
the kernel's batch `resolve_exit`. So:

- Add an enum **`CachedExitStateImpl { Tpsl1(tpsl1::CachedExitState), Tpsl2(tpsl2::CachedExitState) }`**
  to `core::strategies` and store *that* in the unified cache's `exit_state_by_position`.
- Add incremental dispatch to `StrategyImpl` (registry): `ladder_params(&StrategyParams) -> LadderParamsImpl`,
  `clock_entry_time(&StrategyPosition)`, `exit_state_advance_and_find_exit(...)`,
  `exit_state_build/_get`, `should_exit_on_clock(...)` — each routing to the owning strategy's
  `exit` module. `LadderParams` is likewise enum-wrapped (`LadderParamsImpl`) or unified if the
  two are field-identical (verify: tpsl2 adds cohort-ratio only — keep enum-wrapped for safety).
- The unified runner/service is **one** generic struct; strategy-specific *orchestration*
  (tpsl2's until-dead armers + cohort entry watchers) is dispatched behind a small
  `StrategyImpl`-keyed hook, NOT cloned into a second service.

This keeps "fix the clone in both" satisfied at the logic level (the two `exit`/`entry`
modules stay separate behind the enum) while collapsing orchestration to one path.

**Wrinkles found while scoping T1 (must be honored for parity):**
- `t1::CachedExitState` and `t2::CachedExitState<W=u32>` are **not** symmetric:
  - t2 is **generic over the wallet type** (`W`, `=u32` for live `CachedTrade`) because cohort
    detection needs wallet identity; its methods bind `T: TradeRow<Wallet = W>`.
  - t2's `build_unfolded::<T>(trades, …)` **takes the trade slice** (and seeds the cohort);
    t1's `build_unfolded(base, entry_price, entry_time)` does **not**. t2 also has
    `ensure_cohort_seeded(...)` with no t1 equivalent. The enum's `advance_and_find_exit`
    dispatch must call each variant's real signature — not a lowest-common-denominator one.
  - Make the unified-cache dispatch **concrete to `CachedTrade`** (it lives in core and is the
    only feeder), so `CachedExitStateImpl::Tpsl2(t2::CachedExitState<u32>)` and the dispatch
    fns take `&[CachedTrade]` — no extra generics leak into the cache.
- The memo core (`CachedExitState`, `advance_and_find_exit`, `LadderParams`,
  `find_clock_driven_exit`) is **position-agnostic** (takes `entry_time`/`entry_price`/`state`).
  Only `clock_entry_time(&Position)` and `should_position_exit_on_clock(&Position, …)` bind the
  **old** `Position`. So in core: add a strategy-agnostic `clock_entry_time(&StrategyPosition)`
  (Holding/Arming/BuySubmitted + `entry_price.is_some()` → `entry_time`) and call
  `find_clock_driven_exit(state, entry_time, params, now)` directly — don't route through the
  Position-bound `should_position_exit_on_clock`.
- `ExitReason` is also two distinct enums (t1/t2); the enum dispatch must return the unified
  **`&'static str`** reason (`ExitReason::as_str()`), which is what `StrategyPosition.exit_reason`
  + `kernel::ExitCode::from_reason` already consume.

---

## Sub-tasks (ordered; each ends `cargo check` clean)

### T1 · Core cache: port the deferred exit-state memo + time-exit index
**Additive in `trading_core` — old `live` caches untouched, workspace stays green.**
- Add `CachedExitStateImpl` + `LadderParamsImpl` enums (+ `StrategyImpl` incremental dispatch).
- Extend `StrategyRuntimeCache` with: `exit_state_by_position: DashMap<Uuid, CachedExitStateImpl>`,
  `time_exit_holding: DashMap<Uuid, Arc<StrategyPosition>>`, `paper_poll_sem`, and methods
  `exit_state_get/_build/_advance_and_find_exit`, `time_exit_holding_positions`,
  `rebuild_time_exit_index`, `rule_has_time_exit` (via `LadderParamsImpl`).
- Wire the memo + time-exit index into `sync_position` / `remove_position` / `clear_rule`
  (drop memo + index entry when a position leaves the holding index), mirroring the old tpsl1 cache.
- Port the relevant unit tests (memo seed/advance, time-exit index membership).
- **Done when:** `cargo check -p trading_core` + new tests pass.

### T2 · Core cache: DB loaders + paper-run lifecycle over `StrategyRepo`
- `load_from_db` / `load_holdings` / `reload_holding` / `reload_rules` against `StrategyRepo`
  (active rules → `set_rules`; open positions → holding index; warm total/closed counters per
  `(rule_id, mode)`; paper runs → `paper_run_by_rule`).
- `start_paper_run` / `stop_paper_run` / `resume_paper_run` / `finish_paper_run` via
  `StrategyRepo` (`next_run_seq`, `set_run_status`, position deletes on fresh run).
- Optional SSE delta (`emit_position_changed`) behind a `broadcast::Sender<SseEvent>` the live
  edge injects — keep the cache itself decoupled (pass `Option` or a setter).
- **Done when:** `cargo check -p trading_core`; round-trip test against the new schema (ignored/DB).

### T3 · `live` generic StrategyService + execution

**Prerequisites discovered while reading the execution path (build these first, in `core`):**
- `StrategyPosition` is a **plain struct with no lifecycle mutators**. The old `Position` had
  `mark_entry_filled`/`mark_buy_submitted`/`mark_exit_pending`/`close`/`mark_exit_failed` +
  `Position::new`. Add the equivalent inherent methods/ctor to `StrategyPosition` (status-string
  transitions + fill stamping) so the unified execution path reads like the old one. Keep them
  pure (no DB).
- `StrategyRepo` lacks the **reaper/aggregate queries** the old `Tpsl{1,2}PositionRepo` had. Add:
  `find_open_by_run`(or reuse `find_positions_by_run`+filter), `find_all_exit_pending`,
  `find_all_buy_submitted`, `fail_stale_exit_pending(stale)`, `delete_stale_unentered(stale)`,
  `delete_position(id)`, `net_token_amount_by_wallet_and_mint` (this one lives on `TradeRepo`
  already — verify), `find_externally_cleared_holding_mints(threshold)`. Scope by current run /
  status / `mode='real'` and bound them (data-scale guardrails).
- Real-only double-sell guard is now the partial-unique indexes `uq_strategy_positions_entry_sig0`
  / `_exit_sig0` (mode='real') — keep insert/update paths using `entry_tx_signatures[0]` /
  `exit_tx_signatures[0]` so they engage.
- Replace `tpsl_sniper_1/service.rs` + `tpsl_sniper_2/service.rs` with **one**
  `live/src/strategies/service.rs` over `StrategyRuntimeCache` + `StrategyRepo`, dispatching
  entry/exit via `StrategyImpl`. Preserve every invariant: inline cap-claim then spawn,
  manual-sell detection, `trigger_real_exit`/`spawn_real_sell`, the ExitPending/BuySubmitted/
  unentered reapers, `reconcile_externally_cleared_holdings`.
- Collapse `execution/{real,paper}.rs` ×2 → one `execution/real.rs` + `execution/paper.rs`
  over `StrategyPosition`. Reuse `pump-trader` calls verbatim (sell-confirm stays feed-driven).
- tpsl2-only orchestration (until-dead armers, cohort entry watchers) behind a
  `StrategyImpl`-keyed hook, not a second service.
- **Done when:** `cargo check -p live`; trader path untouched.

### T4 · `live` runner + state + main wiring
- `runner.rs`: dispatch `StrategyPing` by `rule.strategy_id` (no hardcoded tpsl1/tpsl2);
  one `sweep_time_exits` over the unified cache.
- `deploy_state.rs`: replace `tpsl1_cache`/`tpsl2_cache` with one `strategy_cache: Arc<StrategyRuntimeCache>`.
- `main.rs`: build one cache, `load_from_db`, wire the single service; keep the `tokio::select!`
  task shape; ingest via `ingest_laserstream::spawn` (already `core::ingest`).
- Carry the deferred carve-out: the time-exit sweep now uses T1's memo + index against the live
  token-cache trade source.
- **Done when:** `cargo check -p live`; `probe ladder|simulate-sell|holdings` build.

### T5 · `live` API handlers (positions) + rule-CRUD edge
- Rewrite `api/handlers/strategies/{tpsl1,tpsl2}_positions.rs` → unified position-read handlers
  over `StrategyRepo` keyed by `strategy_id` path segment (keep route URLs stable for the FE).
- Rule-CRUD calling edge (create/update/activate/pause/stop): call `strategies::rules` + append
  live side effects (cache `reload_rules` + `rules_changed`/SSE). (Per Phase-2 note, CRUD lives
  in the calling bin's edge; wire it here for `live`.)
- **Done when:** `cargo check -p live`; routes register; FE contract unchanged.

### T6 · Delete the clones + docs
- Delete `live/src/strategies/tpsl_sniper_1`, `tpsl_sniper_2`, both runtime caches, old services.
- Delete core's now-orphaned old types: `Tpsl1Rule`/`Tpsl2Rule` models, `tpsl1/2_*_repo`,
  `tpsl_rules_core` — **only** once nothing references them (`grep` clean).
- Docs: `@arch/strategies.md` (registry runner + memo), `@arch/architecture.md` (one cache in
  DeployState), `CLAUDE.md` gotchas if the clone note changes.
- **Done when:** `cargo check --workspace` + `cargo test -p live` clean; `git commit` Phase 3.

---

## Verification (phase exit)
- `cargo check -p trading_core|live|lab` + clippy on touched code; `cargo test -p live`.
- `cargo run -p live` vs local PG + Helius gRPC; `probe ladder|simulate-sell|holdings`.
- Small real buy/sell: sell confirms off the `trades` feed (no extra RPC in logs).
- Rule CRUD via UI mutates `strategy_rules` and reloads the cache; positions render unchanged.
- No double-sell across the reaper + ladder + manual-sell paths (guards intact).

## Progress
- **T3 IN PROGRESS — core prerequisites DONE (workspace green):**
  1. `StrategyPosition` lifecycle ctor + mutators (`new`/`set_target`/`mark_buy_submitted`/
     `set_entry`/`mark_entry_filled`/`mark_exit_pending`/`mark_exit_failed`/`close` +
     `entry_tx_sigs`/`exit_tx_sigs`), mirroring old `Position` over the unified schema.
  2. `StrategyRepo` reaper/aggregate queries (mode-scoped): `delete_position`,
     `find_all_exit_pending`, `find_all_buy_submitted`, `find_open_by_mint`,
     `fail_stale_exit_pending`, `delete_stale_unentered`, `find_externally_cleared_holding_mints`
     (joins `wallet_dict`). `TradeRepo::net_token_amount_by_wallet_and_mint` /
     `sum_legs_by_signatures` / `find_fill_by_signature` already exist (reuse for sell-confirm).
  **UNITS — RESOLVED (non-issue):** `raw_to_f64` is identity (`v as f64`) and `trades.token_amount`
  was **raw-unit-valued f64 in BOTH schemas** (only the storage type changed f64→BIGINT, not the
  scale). So `PARTIAL_FILL_THRESHOLD=0.0001` is a near-zero dust check in raw units in both — it
  ports **verbatim**. entry/exit token amounts (from `SigLegs`) stay raw-unit f64 end to end.
  **Remaining T3 (next focused unit):** the unified `live` `service.rs` + `execution/{real,paper}.rs`
  over `StrategyPosition`/`StrategyRepo`/the unified cache, dispatched by `StrategyImpl` (tpsl2
  cohort + until-dead armers behind a hook). Read first: rest of `execution/real.rs` (buy/sell/
  recovery), `execution/paper.rs`, tpsl2 `service.rs`/entry/cohort orchestration. Write as ONE
  compiling unit (keep `cargo check -p live` green).

  **T3 PORT SPEC (mapped — the analytically hard part is done):**
  - **Shared, no divergence:** buy flow (`buy_until_filled_or_give_up` + `adopt_existing_fill_if_present`
    + `poll_feed_until_entry_fill` + `SnipeExecutor`/`BuyRetryCfg` + `classify_silent_send`), sell flow
    (`sell_and_close_position` → `sell_until_balance_cleared` → `SellOutcome`/`classify_sell_revert`),
    `close_externally_cleared_position`, `reconcile_externally_cleared_mint`, recovery reapers,
    lifecycle (`activate_rule`/`pause_rule`/`stop_and_close_rule` + `PaperActivation{Fresh,Continue}`),
    run-finish. Exit (trade-gate + clock sweep) already unified via the core memo.
  - **constants (`execution/mod.rs`):** BUY_MAX_ATTEMPTS=3, BUY_POLL_MAX_ATTEMPTS=12,
    BUY_POLL_INTERVAL_MS=1000, SELL_MAX_ATTEMPTS=6, SELL_POLL_MAX_ATTEMPTS=10,
    SELL_POLL_INTERVAL_MS=500, PARTIAL_FILL_THRESHOLD=0.0001 (**reinterpret in raw units!**),
    PAPER_EXIT_POLL_WINDOW_SECS=10, PAPER_EXIT_POLL_INTERVAL_MS=500, plus tpsl2
    SCALP_ENTRY_WAIT_INTERVAL_MS.
  - **tpsl2-ONLY divergence (dispatch via an `EntryOrchestration` enum/hook on the entry path):**
    1. Entry: tpsl1 buys immediately on match; tpsl2 first `await_scalp_entry_signal`
       (`ScalpWaitCfg`/`ScalpWatchWindow{MaxAge,UntilDead}`) → records `target_*` → then buys (drops
       the unentered position if no signal).
    2. Until-dead armers: `begin_until_dead_armer`/`UntilDeadArmerGuard`/`until_dead_armers` map, cap
       32, FIFO eviction via `seq`+`cancelled` AtomicBool. Add to the unified cache as **tpsl2-only
       ops (no-op for tpsl1)**.
    3. Paper entry poll: tpsl1 takes `buy_amount`; tpsl2 takes the rule, finds cohort
       (`scalp_cohort`/`find_scalp_entry_with_cohort_indexed`) + dual fill (target + worst-case entry
       in `[trigger, trigger+MAX_FILL_WAIT_SLOTS]`), persists target then entry.
    4. Paper exit fill: tpsl1 `find_trade_driven_exit`; tpsl2 `find_trade_driven_exit_with_slot`
       (slot-windowed) + tracks `max_slot_seen`/`fire_slot`; timeout → tpsl2 records `exit_price=0.0`
       vs tpsl1's trigger price.
    5. `target_*` persisted for tpsl2 only.
  - **Repo unification:** old paper `update_exit` (tpsl2 adds exit_token_amount) + `update_target`
    (tpsl2 only) fold into `StrategyRepo::update_position` (StrategyPosition carries target_* +
    exit_sol/token uniformly via the new mutators). Real/paper is the `mode` column, not separate repos.
  - **Build order within T3:** (a) `execution/mod.rs` constants ✅; (b) `execution/real.rs` (shared
    buy/sell/recovery over StrategyRepo/StrategyRuntimeCache/PumpFunTrader) ✅ — 11 invariant tests
    pass; (c) tpsl2 `await_scalp_entry_signal`/`ScalpWaitCfg` ✅ — in `execution/scalp.rs`;
    (d) `execution/paper.rs` ✅; (e) `service.rs` ✅; (f) lifecycle ✅. **T3 COMPLETE.**
- **T4 DONE** (`cargo check -p live` clean; 17 live tests pass incl. the 11 unified invariant
  tests). Rewrote `runner.rs` to dispatch ONE `StrategyService` (entry/exit/sweep) — no
  hardcoded tpsl1/tpsl2. `deploy_state.rs`: replaced `tpsl1_cache`/`tpsl2_cache` with
  `pub strategy: StrategyService` (+ `strategy_cache()` accessor → `strategy.runtime()`).
  `main.rs`: builds ONE `StrategyRuntimeCache` + `load_from_db(&StrategyRepo)`, constructs the
  `StrategyService` (+ `spawn_background_tasks()`), shares it with `DeployState` + the runner;
  eviction closure uses the one cache. `solana.rs` manual-sell reconcile collapsed to one unified
  `reconcile_externally_cleared_mint`. **Live-side of T6 brought forward** (forced by the removed
  `DeployState` fields): deleted `live/src/strategies/tpsl_sniper_{1,2}/` entirely (orchestration
  clones) + their `pub mod` decls — nothing outside referenced them (decision logic is in
  `trading_core`; handlers use core repos directly).
  - **KNOWN GAPS for T5/T6 (not regressions — expected mid-cutover):**
    1. ✅ **RESOLVED (T5)** — position-read handlers rewritten as one unified
       `api/handlers/strategies/positions.rs` over `StrategyRepo` keyed by the `{strategy}` path
       segment (`StrategyImpl::from_id` accepts `tpsl1`/`tpsl2` aliases + canonical ids). Added repo
       read views (`find_positions_by_run_paged`/`_by_rule_paged`/`_by_strategy`/`find_holding_by_mint`/
       `_by_wallet`). Old `tpsl{1,2}_positions.rs` deleted; routes use `{strategy}` (URLs stable).
    2. ✅ **RESOLVED (T5)** — rule-CRUD + lifecycle wired in `live` as one unified
       `api/handlers/strategies/rules.rs` (list/get/create/update/delete · activate/pause/stop) keyed
       by `{strategy}`, over the new `service.{create_rule,save_rule,delete_rule,activate_rule,
       pause_rule,stop_and_close_rule}` (which wrap core `strategies::rules` + append cache
       `reload_rules` + `rules_changed` SSE). Live-edit freeze guard (hot set only) in the edge.
       Stale "CRUD lives in lab" comment in `api/mod.rs` replaced. FE contract preserved (flat `p_*`
       body ⇄ params; response = params + universal cols + live counters + derived `lifecycle`).
    3. ✅ **RESOLVED (follow-up) — SSE `TpslPositionsChanged` delta restored.** Rather than thread a
       `broadcast::Sender` through every transition site (the boxed `on_signed` buy hook + deep
       real/paper helpers), the delta is emitted from the **cache's position-transition funnel**
       (`StrategyRuntimeCache::sync_position`/`remove_position`) via an *optional* sender installed at
       boot (`set_sse_sender`; unset in tests/`lab` ⇒ no-op). Every transition funnels through those two
       methods by construction, so coverage is guaranteed and future-proof (an edge that forgot to emit
       would silently drop one). **FE contract unchanged**: a new `impl From<&StrategyPosition> for
       Position` (core) adapts the row back to the legacy wire shape the stream bridge already renders
       via `PositionResponse`; the rule snapshot + cap counters are read from the cache. Module doc +
       `@arch/strategies.md` updated; 2 funnel unit tests added.
    4. **DEFERRED (blocked) — core orphan deletion.** `Tpsl1Rule`/`Tpsl2Rule`, the old per-strategy
       repos, and `tpsl_rules_core` are STILL consumed by `lab` (CRUD/simulate/backtest/paper-result)
       and core `seed.rs`/`wallet_reconcile.rs`. Cannot be deleted until lab is migrated off them (a
       later remake phase). T6's "delete only once grep-clean" guard correctly keeps them for now.
  - **DONE so far (live compiles + tests green):** `StrategyRepo` gained `#[derive(Clone)]` +
    `mark_buy_submitted` (atomic `WHERE entry_price IS NULL` RETURNING) + `record_entry_fill`
    (atomic entry RETURNING); new `live/src/strategies/execution/{mod,real,scalp}.rs` wired into
    `strategies/mod.rs` (`#![allow(dead_code)]` until T4 wires the runner).
    - **(c) tpsl2 scalp-arm DONE:** until-dead armer support ported into the **core**
      `StrategyRuntimeCache` (`UntilDeadArmerGuard`/`UntilDeadArmerSlot`, `MAX_UNTIL_DEAD_ARMERS=32`,
      `begin_until_dead_armer` with FIFO `seq`+`cancelled` eviction — strategy-agnostic mechanism,
      tpsl2-only user). New `execution/scalp.rs`: `ScalpWatchWindow`/`ScalpWaitCfg::for_params`/
      `await_scalp_entry_signal` over `Tpsl2Params` (builds `to_rule()` once) + the live `TokenCache`/
      `TradeSignals` mint lane, calling core `t2::entry::find_scalp_entry`. Sits ahead of the generic
      buy (per design). `cargo check -p live` clean; 12 core + 49 live tests pass.
    - **(d) `execution/paper.rs` DONE:** unified over `StrategyRepo`/`StrategyRuntimeCache`. Entry
      poll **forks** by strategy (`resolve_paper_entry_tpsl1` fixed-count cap-5 vs
      `resolve_paper_entry_tpsl2` scalp-watch + until-dead armer + indexed trigger/worst-case fill +
      target persist) with a shared persist/cleanup tail. Exit poll is **uniform** via new registry
      hook `StrategyImpl::resolve_paper_exit` → `(ResolvedExit, Option<fire_slot>)`: `None` records on
      first find (tpsl1), `Some(S)` keeps freshest worst-case until `max_slot_seen > S+MAX_FILL_WAIT_SLOTS`
      (tpsl2). Timeout fallback diverges (tpsl1 trigger price, tpsl2 `exit_price=0` total loss).
      `record_time_exit` + `finish_paper_run_if_complete` (cap-met + no holdings → `finish_run` +
      auto-deactivate rule + `PaperTestFinished` SSE) ported. Added `StrategyRepo::pool()`.
      `cargo check -p live` clean; 65 live + 7 registry tests pass. **Next: `service.rs` (e).**
- **Scoping DONE** (2026-06-27): mapped `live` orchestration (~7k LOC: runner + 2 services
  ~1k ea + 2 runtime caches ~1k ea + 2 execution layers ~1.7k ea) and core's Phase-2 API.
  Confirmed every exit-memo dependency (`CachedExitState`, `LadderParams`, `ExitReason`,
  `TokenCache`/`CachedTrade`) already lives in core → Phase 3 feasible as planned. Locked the
  enum-wrapped memo design + recorded the tpsl2 generic-wallet / cohort-seeding wrinkle above.
- **T1 DONE** — core cache memo + time-exit index ported. Added `strategies/exit_state.rs`
  (`CachedExitStateImpl` + `LadderParamsImpl` enums dispatching the per-strategy `exit` modules,
  concrete to `CachedTrade`; strategy-agnostic `clock_entry_time(&StrategyPosition)`); `Clone` on
  both `exit::LadderParams`. Extended `StrategyRuntimeCache` with `ladder_by_id` (built at
  `set_rules`), `exit_state_by_position`, `time_exit_holding`, `paper_poll_sem`, and methods
  `ladder_params_by_id` / `time_exit_holding_positions` / `exit_state_advance_and_find_exit`
  (trade gate, folds + fires; tpsl2 auto-seeds E5 cohort) / `exit_state_clock_check` (sweep) /
  `rebuild_time_exit_index`. Memo+index kept in lockstep via `sync_position`/`remove_position`/
  `upsert`/`remove`/`purge`. 5 new unit tests; `cargo check --workspace` clean; 12 runtime_cache
  tests pass. Additive — old live caches untouched.
- **T2 DONE** (build-verified; SQL is sqlx runtime-checked → validated on a real DB at the
  migration/cutover step). Generalized the in-memory run pointer `PaperRunRef`→`RunRef`,
  `paper_run_by_rule`→`current_run_by_rule`, `current_paper_run`/`set_paper_run`→
  `current_run`/`set_current_run` (both modes carry a run now). Added cache methods over
  `&StrategyRepo`: `load_from_db` (active rules → current `Running` run per rule → that run's
  positions → holding index + per-run total/closed counters; **caps are per current run for both
  modes**), `reload_rules`, `reload_holding`, private `set_holding_positions`, and run lifecycle
  `start_run`/`stop_run`/`resume_run`/`finish_run` (runs are immutable history — no delete).
  `cargo check --workspace` clean; 12 runtime_cache tests pass.
- **T3 next** — live generic `StrategyService` + unified execution.

  **Run-model decision (locked in T2, informs T3):** every position carries a non-null `run_id`;
  real rules get runs too. Total/concurrent caps are **per current run**. `start_run` resets
  in-memory state and points the rule at a fresh `Running` run; the service must `start_run` (or
  `resume_run`) a real rule on activation before inserting positions, and stamp new positions with
  `current_run(rule_id)`.
