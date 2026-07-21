# Strategies — the generic fingerprint + metrics engine

The named tpsl1/tpsl2/swing1 strategies were retired in Phase 7. There is now **one
generic engine**: a rule = a `fingerprint_id` (a token-creation shape) + `params`
(strict TP/SL + `entry`/`exit` metric-condition groups). The decision core is a
**pure fold** in the `hunter-engine` crate; the live and lab bins are thin adapters
that produce events and consume effects. A decision fix lands in exactly one place.

Deep-dive detail: flow metrics + classifier in
[`plans/strategies/metrics-reference.md`](../plans/strategies/metrics-reference.md);
broader redesign history in [`docs/roadmap/`](../roadmap/).

## The pure engine — `hunter/engine` (crate `hunter-engine`)

`reduce(&mut EngineState, Event) -> Vec<Effect>` — no clock, no I/O, no randomness
(a purity guard test scans the crate manifest). Everything is a replay of the same
fold; live, sweep, and the replay debugger all drive it.

| Module | Role |
| --- | --- |
| `event.rs` | `Event` (TokenCreated / FirstSlotSettled / Trade / Tick / FillConfirmed / FillFailed / Migrated / RulesReloaded / ManualClose / **ExternallyCleared**), `Effect` (SubmitBuy / SubmitSell / PositionUpdate / ArmedChanged), `PositionDelta` / `ArmedDelta`, `Fill`, `ExitReason` (metric exits = spaced `name op value`), `LoadedRule`, derived ids (`RuleId`/`PositionId`/`IntentId` = `(rule, mint, seq)`) |
| `state.rs` | `EngineState` (compiled rules / tracked tokens / open positions / per-rule cap counters, intent+position seqs); `TokenState` (metric track + `last_meaningful_at` + per-rule arms); `PositionRef` |
| `arm.rs` | `CompiledRule` pre-chews a rule into flat `MetricReq`s + windows + `MonoBound`s (recompiled on reload, never per event); `can_enter` = entry AND holds and exit OR does not; `ArmState` machine `PendingFirstSlot → Armed → EntryPending → Entered → ExitPending → Done \| Disarmed` |
| `reduce.rs` | the fold: arm on fingerprint match, disarm (dead / migration / derived-unsatisfiable), enter via `CompiledRule::can_enter` (entry holds **and** exit metrics do not — refuses buy into an immediately-exitable state), caps checked at entry, fill retry policy (`Reverted` bounded; `Fatal` immediate give-up; exit `Unconfirmed`/`Fatal` terminal — never resold), exit priority **Dead > StopLoss > TakeProfit > Metrics**, metric exits persist as spaced `name op value` (`stall > 3`) via `ExitReason::Metrics { metric, operator, value }`, `ManualClose` (sell), `ExternallyCleared` (book closed, no sell) |
| `fingerprint.rs` | `Fingerprint` (criteria; lamports at rest) + `match_all` / `MatchPhase` (Instant vs Full — the two-phase first-slot split) |
| `metrics/` | the metric registry + `TokenTrack` (in-memory per-token metric state) + `MetricSeries` (sweep precompute) + `evaluator` (Operator/Condition/eval). Flow groups (`m_flow_split` / `m_flow_window`) live in `metrics/flow_split.rs` — fingerprint-scoped classifier state + SSOT `ix_hash`/`wallet_hash` |
| `rule_params.rs` | `RuleParams` registry-guided parse → canonical `to_value` + validation |
| `grouping.rs` | bucket matching (`same_bucket`) for the SOL fingerprint axes |
| `deadness.rs` | `is_dead_verdict` + `DEAD_*` consts — the ONE deadness SSOT (core + live + sweep re-export it) |
| `kernel.rs` | `CostModel` / `round_trip_with_costs` + `RunAgg` → `RunMetrics` (≡ `strategy_run_metrics` cols) + the quantile sketch / robust score — one copy of the PnL+summary math shared by live/paper/sweep |
| `event_log.rs` | `LoggedEvent` — the on-disk JSONL format, SSOT for the live recorder (writer) + the lab replay inspector (reader) |

## Live adapters — `live/src/strategies/engine/`

The live composition root around the fold: it **produces** events (ingest pings + a
`TICK_MS` clock tick + confirmed fills) and **consumes** effects (submit on-chain /
paper, persist to PG, push SSE). All decision logic is in the fold; these are
side-effects only.

| Module | Role |
| --- | --- |
| `decision_loop.rs` | **THE** one serialized `select!` loop (ingest ping / `TICK_MS` tick / fill / command channels); every `reduce` call happens here; two-pass dispatch = registry/SSE first (BuySubmitted/ExitPending PG is async) then submit spawn. `spawn_engine` → `EngineHandles { handle, armed, positions, task }` |
| `producers.rs` | `StrategyPing` + `TokenCache` → `Event`s; first-slot settlement detection; the live freshness gate; feeds `real_reserve_sol` for deadness parity |
| `exec_real.rs` | `SubmitBuy`/`SubmitSell` → executor submit-and-return, then synthesize a definitive `FillConfirmed`/`FillFailed` from the **trades feed** (RPC watchdog fallback). SOL commit/release; M2 sync `SubmittedBuyJournal` + fire-and-forget bounded `mark_buy_submitted`; adopt skips PG when journal empty; curve sell uses cache reserves for min_out; `classify_swap_revert` heal; sell route re-read + rent reclaim. **Double-fire safe:** `FillFailed::Reverted` only when re-submitting is safe |
| `exec_paper.rs` | worst-case paper fill (`paper_fill`, slot window) → `FillConfirmed` (sim-parity) |
| `sinks.rs` | `PositionUpdate` → registry + SSE; `BuySubmitted` upserts registry then background `insert_position` (later transitions await the handle); `ExitPending` PG is fire-and-forget; terminal SSE emits **before** `registry.remove` (so `position_id` / frozen `trade_mode` stay on the wire); `warm_runs` on rule reload; releases SOL on terminal unentered exits |
| `reapers.rs` | Boot+60 s: buy orphan adopt/drop/wait (never re-send); exit orphan nudge via `FillFailed` or direct `run_exit`; then stale `ExitPending` fail + stale `Arming` delete. Skips `InFlightGuards`-held rows |
| `event_log.rs` | JSONL recorder (daily rotation + retention) + **conservative** boot-recovery replay (`recover_armed` = re-arm only; held/filled mints excluded; effects discarded) |
| `convert.rs` | DB model ↔ engine type converters (re-exports `fingerprint_axes::{fp_to_engine, observed_axes, rule_to_loaded}`) |

`EngineHandle` (held by the HTTP layer, enqueues commands only): `reload_rules`,
`manual_close(pg_id)` (per-row "Sell ALL"), `close_rule(rule_id)` (per-row Stop),
`close_mode(real)` (Stop All), `reconcile_cleared(pg_id, fill)` (externally-cleared
close — below).

**Position lifecycle:** `BuySubmitted → Holding → ExitPending → End | ExitFailed | ExitUnconfirmed`.

## Externally-cleared close (manual wallet-sell reconcile)

When a manual wallet sell (`POST /api/solana/wallet/sell`, `solana.rs`) empties a
held mint's bag, the handler confirms via the trades feed that the balance is
actually cleared, then drives each open **real** position through
`EngineHandle::reconcile_cleared`. That folds `Event::ExternallyCleared { position,
fill }`, which closes `Entered → Closed` (reason `Manual`) **without** a `SubmitSell`
— the twin of `ManualClose` minus the sell (a sell into an already-empty wallet just
reverts and would leave the row `ExitUnconfirmed`). The handler resolves the exit
`fill` from the wallet's last sell (or the entry as a fallback). This is a
real-money path; the exit is book-only, no new tx.

## Volume/organic flow split (`m_flow_split` / `m_flow_window`)

Split every trade's SOL into **volume-side** (creator tooling + contagion + creator
wallet) vs **organic**, exposed as ordinary registry metrics. Patterns live on
`fingerprints.metric_config.m_flow_split.volume_ix_patterns` (not on the rule).
`TradeLite` carries `ix_hash` / `wallet_hash`; adapters call the engine SSOT hashers
only. Flow state keys by `FingerprintId` on `TokenTrack`. Unconfigured fingerprint
⇒ NaN. Full formulas / NaN rules / discovery scoring:
[`plans/strategies/metrics-reference.md`](../plans/strategies/metrics-reference.md).

Lab authoring: `POST /api/strategies/flow-discovery` scores ix-structures per
sweep `GroupKey`; the Flow discovery page toggles patterns into `metric_config`.

## Two-phase first-slot fingerprint gate

A fingerprint axis whose data settles after `TokenCreated` (`first_slot_{buy,sell}_
lamports` — the SOL summed across the creation slot's trades) can't match
synchronously. Instant axes match on `TokenCreated` (`MatchPhase::Instant`); a rule
with a first-slot axis arms `PendingFirstSlot` and resolves on `FirstSlotSettled`
(fired when the creation slot closes). No hot-path sleep/poll.

## Analysis — replay, simulate, sweep (`lab`)

- **`strategies/replay.rs`** — expands the matched `ReplayToken`s into ONE globally
  time-ordered event stream (`(time, mint, kind)`) driven through the same `reduce`
  over one shared `EngineState`, so cross-token concurrency/lifetime caps apply
  exactly as live (not a post-hoc per-token select). Synthetic 500 ms ticks between
  event times, emission stopping the instant `state.tokens` empties (long quiet gaps
  are O(1) jumps). Sim fills mirror `exec_paper` (worst-case slot-window fill via
  `trading_core::strategies::paper_fill`).
- **`strategies/engine_sim.rs`** + **`api/handlers/strategies/engine.rs`** —
  `POST /api/strategies/simulate` (`rule_id` OR inline `draft`); reuses the
  fingerprint candidate scan + the analysis-cache single-flight; results served by
  the strategy-agnostic `positions::sim_result_{page,summary}`. Loads `with_flow`
  when rule params reference `m_flow_*`; dry-run uses the rule's fingerprint
  `metric_config`.
- **`strategies/flow_discovery.rs`** + **`api/handlers/strategies/flow_discovery.rs`** —
  lab-only job: score trade ix-structures per fingerprint group → toggle
  `volume_ix_patterns` (mutual `409` with sweeps; ephemeral in-RAM results).
- **`sweep/generic/`** — the precompute-then-scan grouped sweep. `GenericSweepStrategy`
  implements the existing `sweep::strategy::Strategy` trait (so partition / two-phase
  pool / `GroupSink` persistence / refine / `ComboAgg` and the whole
  `start_grouped_sweep` handler are reused); only the per-combo simulation is
  replaced with a scan over precomputed `MetricSeries`, reusing the same `evaluator`,
  the same `Dead > Unsat > Enter` / `Dead > SL > TP > Metrics` decision, and the same
  `kernel` cost. `sweep/generic/guard.rs` asserts the scan is identical to a
  single-token `run_replay` (the real fold). Registry id `"generic"`, tables
  `grouped_sweep_*`.
- **Deadness:** sim/sweep book `Dead` (not `Open`) for a silent-death token at its
  death point, via the shared verdict — the same one the live fold uses. A dead
  **real** pool has no liquidity to sell into.

## Event log + replay debugger (Phase 6)

The live engine records each folded event as JSONL (daily-rotated). `POST
/api/replay/inspect` (lab, `strategies/replay_inspect.rs`) re-loads a recorded log,
re-runs `reduce`, and dumps every `event → effects` decision as JSON — the
time-travel debugger. `LoggedEvent` (engine crate) is the SSOT format so recorder
and inspector can't drift; `Tick`/`RulesReloaded` are not logged (ticks regenerated
on replay, rules reloaded from PG).

## Shared result-table infrastructure

These surfaces are strategy-agnostic and unchanged by the retirement:

- **Token enrichment SSOT** — `trading_core::storage::token_enrichment` (`ENRICH_SELECT`
  SQL · `TokenEnrichmentRow` · `TokenEnrichment` `#[serde(flatten)]` · `fetch_by_mints`).
  Matched / Positions / Simulated / Sweep tables all render the same ~28-field token
  metadata **in the response body**, so sort/filter/search on token columns runs
  server-side with no client `mergeTokenData`.
- **Simulated table = in-memory server-side paging** (`lab/src/strategies/sim_query.rs`):
  the finished backtest's rows are already resident (lab is single-user), so
  `POST …/rules/{id}/simulate/result` (unified `TableRequest`) pages/sorts/filters
  in Rust, with a `…/simulate/result/summary` aggregate over the filtered cohort
  and a batch `POST …/simulate/summaries` for the Simulate page's multi-rule hydrate.
- **Positions table = Current run + Old runs** (`?scope=current|history` on
  `POST …/rules/{id}/positions[/summary]`): `current` pages the rule's latest run
  (`StrategyRepo::latest_run` → `find_positions_by_run_paged`); `history` pages every
  prior run stamped with `run_seq`. The rule's `trade_mode` (which selects the run) is
  read from the generic `strategy_rules` table via `RuleRepo::find`. Both live and lab
  handlers attach the shared enrichment per page.
- **Closed-position PnL booking** — a close is `End` (clean exit fill) or `ExitFailed`
  / `ExitUnconfirmed` (no realized `exit_sol`); the failed/unconfirmed cases book a
  **full loss of deployed capital** everywhere realized stats are summed
  (`positions_summary_by_run`, the sweep `RunAgg`), so row and card can't drift.

## Invariants (preserve when editing)

1. **No double-buy** — write-ahead persist the signed signature before submit; the
   boot reaper adopts-from-feed / waits / drops, never re-sends.
2. **No double-sell** — a submitted sell that neither confirmed a clear nor a revert
   is terminal (`ExitUnconfirmed`, alarmed); the fold only re-submits a sell on a
   proven on-chain revert.
3. **Sell-confirm via the `trades` feed**, no new RPC; per-signature attribution (a
   position confirms against its OWN sell sigs, not the shared net balance).
4. **Quiet/time exits fire on the `TICK_MS` clock tick (200 ms)** — a token that goes
   silent still advances to `now` so stall/time/decayed-flow conditions and the dead
   verdict fire. Price TP/SL still fire on Trade events (no tick wait).
5. **ONE serialized decision loop** — every `reduce` happens in `decision_loop.rs`;
   no mint sharding, no interleaved position transitions.
6. **Live-rule edit guard** — `fingerprint_id` is frozen post-create (PUT
   ignores it); `trade_mode` is editable via PUT but the editor locks it
   behind an unlock control. Entry/exit params lock in the UI while the rule
   is active. Entry dispatch + sells both route off the position's
   snapshotted `trade_mode`, never a mid-retry rule flip.

## Persistence

Generic rules → `strategy_rules` (`RuleRepo`); fingerprints → `fingerprints`
(`FingerprintRepo`); runs / run metrics / positions → `strategy_runs` /
`strategy_run_metrics` / `strategy_positions` (`StrategyRepo`). See
[@arch/database.md](database.md). (The legacy `strategy_rules_legacy` +
`{tpsl1,tpsl2,swing_1}_grouped_sweep_*` tables were dropped in Phase 7.2.)
