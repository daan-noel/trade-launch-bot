# Strategies — the generic fingerprint + metrics engine

The named tpsl1/tpsl2/swing1 strategies were retired in Phase 7. There is now **one
generic engine**: a rule = a `fingerprint_id` (a token-creation shape) + `params`
(strict TP/SL + `entry`/`exit` metric-condition groups). The decision core is a
**pure fold** in the `hunter-engine` crate; the live and lab bins are thin adapters
that produce events and consume effects. A decision fix lands in exactly one place.

Deep-dive detail: flow metrics + classifier in
[`plans/strategies/metrics-reference.md`](../plans/strategies/metrics-reference.md);
what a round trip **costs** (fee 125 bps/leg, our own `buy_amount/reserve_sol` impact,
the U-shaped optimal buy size) in
[`plans/strategies/execution-costs.md`](../plans/strategies/execution-costs.md);
exit-condition traps that are invisible from the rule JSON — the unarmed `retrace`
and the `stall` hold-cap — in
[`plans/strategies/armed-trailing-stop.md`](../plans/strategies/armed-trailing-stop.md)
and [`plans/strategies/flow-scalper-findings.md`](../plans/strategies/flow-scalper-findings.md);
the external-wallet reverse-engineering behind all of the above in
[`plans/strategies/wallet-analysis.md`](../plans/strategies/wallet-analysis.md);
broader redesign history in [`docs/roadmap/`](../roadmap/).

## The pure engine — `hunter/engine` (crate `hunter-engine`)

`reduce(&mut EngineState, Event) -> Vec<Effect>` — no clock, no I/O, no randomness
(a purity guard test scans the crate manifest). Everything is a replay of the same
fold; live, sweep, and the replay debugger all drive it.

| Module | Role |
| --- | --- |
| `event.rs` | `Event` (TokenCreated / FirstSlotSettled / Trade / Tick / FillConfirmed / FillFailed / Migrated / RulesReloaded / ManualClose / **ExternallyCleared**), `Effect` (SubmitBuy / SubmitSell / PositionUpdate / ArmedChanged), `PositionDelta` / `ArmedDelta`, `Fill`, `ExitReason` (metric exits = spaced `name op value`), `LoadedRule`, derived ids (`RuleId`/`PositionId`/`IntentId` = `(rule, mint, seq)`) |
| `state.rs` | `EngineState` (compiled rules / tracked tokens / open positions / per-rule cap counters, intent+position seqs); `TokenState` (metric track + `last_meaningful_at` + per-rule arms + `episodes: BTreeMap<RuleId, u32>` — the re-entry episode counter, a parallel map so no `ArmState` transition has to carry it forward); `PositionRef` |
| `arm.rs` | `CompiledRule` pre-chews a rule into flat `MetricReq`s + windows + `MonoBound`s (recompiled on reload, never per event); `can_enter` = entry AND holds and exit OR does not; `ArmState` machine `PendingFirstSlot → Armed → EntryPending → Entered → ExitPending → Done \| Disarmed`, plus `Cooldown { until: Ts }` — a normal exit (TP/SL/Metrics, never Dead/Manual/Migrated) with `RuleParams.reentry { cooldown_sec, max_episodes_per_token }` configured re-arms into `Cooldown` instead of `Done`; `evaluate_token` promotes `Cooldown → Armed` once `now >= until` (trade/tick-driven, no new timer) and treats `Cooldown` as active so the token isn't pruned; absent `reentry` ⇒ today's one-shot behavior. Live boot seeds `episodes` from a batched closed-position count over adopted mints. Also carries `exclusive` / `priority` (`RuleParams`) — the single-position-per-token toggle |
| `reduce.rs` | the fold: arm on fingerprint match, disarm (dead / migration / derived-unsatisfiable), `exclusive` rules stand down (stay `Armed`, never disarmed) while ANY other arm on the token holds a position — in-flight buys/sells and manual arms included, since `evaluate_token` reads the shared `token.arms` map; that sweep is visited in `(priority desc, RuleId asc)` order so whichever exclusive rule sorts first claims the token and later ones see the claim in the *same* event, enter via `CompiledRule::can_enter` (entry holds **and** exit metrics do not — refuses buy into an immediately-exitable state), caps checked at entry, fill retry policy (`Reverted` bounded; `Fatal` immediate give-up; exit `Unconfirmed`/`Fatal` terminal — never resold), exit priority **Dead > `exit_fired`** (TP/SL desugar into prepended `m_position.pnl` reqs, so the old `SL > TP > Metrics` tiebreak is preserved inside the one exit loop), metric exits persist as spaced `name op value` (`retrace >= 3`) via `ExitReason::Metrics { metric, operator, value }` while a desugared TP/SL keeps its `TakeProfit`/`StopLoss` label (origin tag), `ManualClose` (sell), `ExternallyCleared` (book closed, no sell) |
| `cap.rs` | `Cap` — a governance limit with its `0 = …` storage encoding already decoded. `Cap::zero_unlimited` (`max_total_tokens`: `0 ⇒ UNLIMITED`) / `Cap::zero_defaults_to` (`max_concurrent_tokens`: `0 ⇒ 1`) are the ONE readers of those sentinels; `CompiledRule` carries both already decoded so the fold just asks `allows(count)`. `UNLIMITED = u32::MAX`, so the hot path is a single `<` with no branch |
| `fingerprint.rs` | `Fingerprint` (criteria; lamports at rest) + `match_all` / `MatchPhase` (Instant vs Full — the two-phase first-slot split) |
| `metrics/` | the metric registry + `TokenTrack` (in-memory per-token metric state) + `MetricSeries` (sweep precompute) + `evaluator` (Operator/Condition/eval). Aggregate flow: `metrics/flow_lifetime.rs` (`m_flow_lifetime` — lifetime `buy`/`sell`/`net_flow`/`gross_flow`) + `metrics/flow_window.rs` (same names over a trailing window). Classified flow (`m_flow_split` / `m_flow_split_window`) lives in `metrics/flow_split.rs` — fingerprint-scoped classifier state + SSOT `ix_hash`/`wallet_hash`. Price groups: `metrics/price_lifetime.rs` (lifetime extrema — `stall`/`trail`/`rise`), `metrics/price_window.rs` (`m_price_window` — rolling-window `trail`/`rise` via monotonic deques; the dip trigger), and `metrics/position.rs` (`m_position` — **position-scoped** `retrace`/`bounce`/`pnl`/`held`, exit-only, read from a `PositionCtx` on `ArmState::Entered`; TP/SL desugar into `pnl`; strict param `arm_above_pct` holds the **trailing** metrics (`retrace`/`bounce`, per the one reader `position::is_trailing`) off until the position is that far in profit — the peak seeds at the entry fill, so an unarmed `retrace` doubles as a hard stop from entry, and the exit combinator ORs across metrics so `retrace >= 3 AND pnl >= 2` is otherwise unauthorable. Absent ⇒ prior behavior; `0` is a real value (arm at break-even), which is why `StrictParamSpec` carries `allows_zero`. Design + the measurement behind it: [../plans/strategies/armed-trailing-stop.md](../plans/strategies/armed-trailing-stop.md)). Dynamic windows split flow vs price on `TokenTrack` (`ensure_window` / `ensure_price_window`) so a rule pays only for the buffers it reads. `GroupSpec.scope` (Token/Position) gates the entry side |
| `rule_params.rs` | `RuleParams` registry-guided parse → canonical `to_value` + validation (incl. `scale_out: Vec<ExitStage>` — ordered partial-exit ladder; see `docs/roadmap/partial-exits-plan.md`) |
| `grouping.rs` | bucket matching (`same_bucket`) for the SOL fingerprint axes |
| `deadness.rs` | `is_dead_verdict` + `DEAD_*` consts — the ONE deadness SSOT (core + live + sweep re-export it) |
| `kernel.rs` | `CostModel` / `round_trip_with_costs` / `round_trip_multi_leg` (+ `ExitLeg`) + `RunAgg` → `RunMetrics` (≡ `strategy_run_metrics` cols) + the quantile sketch / robust score — one copy of the PnL+summary math shared by live/paper/sweep. Fixed per-leg cost (tip + CU priority) comes from process-wide [`FeeTuning`](../../core/src/config/fee_tuning.rs) (`JITO_MIN_TIP_SOL` + `CU_PRICE_MICRO_LAMPORTS`), installed at boot by both bins. **`FEE_BPS_PER_LEG = 125`** — measured, not assumed (was 100 until 2026-07-28, making every earlier backtest 0.5 pp/round-trip optimistic). Three `CostModelKind`s: `pumpfun_default` (flat `slippage_bps`, legacy), `pumpfun_fee_only` (size-blind), and **`pumpfun_impact`** — the only one that charges our own `buy_amount_sol / reserve_sol` price impact, so the only one whose cost responds to buy size. Callers pass entry pool depth; `None` ⇒ no impact, never a guess. Scale-out prices through `round_trip_multi_leg` (fixed cost × leg count); the single-exit wrapper stays for legacy / sweep until the staged resolver lands |
| `event_log.rs` | `LoggedEvent` — the on-disk JSONL format, SSOT for the live recorder (writer) + the lab replay inspector (reader) |

## Live adapters — `live/src/strategies/engine/`

The live composition root around the fold: it **produces** events (ingest pings + a
`TICK_MS` clock tick + confirmed fills) and **consumes** effects (submit on-chain /
paper, persist to PG, push SSE). All decision logic is in the fold; these are
side-effects only.

| Module | Role |
| --- | --- |
| `decision_loop.rs` | **THE** one serialized `select!` loop (command / fill / ingest ping / `TICK_MS` tick channels); every `reduce` call happens here; two-pass dispatch = registry/SSE first (BuySubmitted/ExitPending PG is async) then submit spawn. `spawn_engine` → `EngineHandles { handle, armed, positions, task }` |
| `producers.rs` | `StrategyPing` + `TokenCache` → `Event`s; first-slot settlement detection; the live freshness gate; feeds `real_reserve_sol` for deadness parity |
| `exec_real.rs` | `SubmitBuy`/`SubmitSell` → executor submit-and-return, then synthesize a definitive `FillConfirmed`/`FillFailed` from the **trades feed** (RPC watchdog fallback). SOL commit/release; M2 sync `SubmittedBuyJournal` + fire-and-forget bounded `mark_buy_submitted`; adopt skips PG when journal empty; curve sell uses cache reserves for min_out; `classify_swap_revert` heal; sell route re-read + rent reclaim. **Double-fire safe:** `FillFailed::Reverted` only when re-submitting is safe |
| `exec_paper.rs` | worst-case paper fill (`paper_fill`, slot window) → `FillConfirmed` (sim-parity) |
| `sinks.rs` | `PositionUpdate` → registry + SSE; `BuySubmitted` upserts registry then background `insert_position` (later transitions chain on the handle); `Holding` updates registry sync then backgrounds fill persist; `ExitPending` PG is fire-and-forget; terminal SSE emits **before** `registry.remove` (so `position_id` / frozen `trade_mode` stay on the wire); `warm_runs` on rule reload (`ensure_run` reuses latest still-`Running` DB run + collapses empty leading shells — does not mint a new `run_seq` on every restart); releases SOL on terminal unentered exits |
| `reapers.rs` | Boot+60 s: buy orphan adopt/drop/wait (never re-send; stale ⇒ `needs_review` SSE); **externally-cleared Holding** book-close (PG `trades` net, no RPC); exit orphan nudge via `FillFailed` or shared `orphan_exit`; **ExitStuck-with-bag** redrive (PG-gated, backoff, bounded-then-park); `ExitStuck`/`ExitUnconfirmed` bag-gone heal → End; stale `ExitPending` bag-check → `ExitStuck` (real) / breakeven End (paper). Skips `InFlightGuards`-held rows/mints |
| `orphan_exit.rs` | Shared direct-sell + PG book-close for registry-miss rows (Console close, ExitPending/ExitStuck reapers). Feed-confirm via `run_exit`; sibling mint clear → `ExternallyCleared` / PG End; boot adopts re-install manual TP/SL rules |
| `event_log.rs` | JSONL recorder (daily rotation + retention) + **conservative** boot-recovery replay (`recover_armed` = re-arm only; held/filled mints excluded; effects discarded). Dir = `EVENT_LOG_DIR` via `config::dir_from_env`: a relative value anchors to the loaded `.env`'s directory, never the CWD (see below) |
| `convert.rs` | DB model ↔ engine type converters (re-exports `fingerprint_axes::{fp_to_engine, observed_axes, rule_to_loaded}`) |

`EngineHandle` (held by the HTTP layer, enqueues commands only): `reload_rules`,
`manual_close(pg_id, portion)` (per-row "Sell ALL" / "Sell N%"), `close_rule(rule_id)` (per-row Stop),
`close_mode(real)` (Stop All), `reconcile_cleared(pg_id, fill)` (externally-cleared
close — below), `manual_buy(pg_id, mint, lamports, exit)` (Console manual buy — a
fresh per-episode rule id + `Event::ManualBuy`), `set_manual_exit(pg_id, exit)`
(per-position TP/SL resynthesis). `DeployState` also holds the shared
`PositionRegistry` + `InFlightGuards` + engine `fill_tx` so Console orphan-close can
sell without the registry and still fold sibling clears.

**Position lifecycle:** `BuySubmitted → Holding → ExitPending → End`, with
`EntryFailed` (buy never filled, terminal) and the OPEN attention states
`ExitStuck` / `ExitUnconfirmed` (engine drops the arm; reaper + manual actions own
the row). Full map: [position-lifecycle.md](position-lifecycle.md). Manual positions
(`origin='manual'`) ride the same machine; their optional TP/SL compiles into a
per-position one-off rule (`EngineState::manual_rules`) — without it, tracked-only
(no auto-exit).

**Boot Holding adopt:** after event-log re-arm, PG `Holding` rows are loaded into
the in-memory engine (`Entered`) + registry (PG-only, no RPC) so TP/SL/Dead and
Ops `ManualClose` work after a process restart.

**Where the log lives (one contract, three readers).** `EVENT_LOG_DIR` is resolved
by [`config::env_paths`](../../core/src/config/env_paths.rs), installed after
`dotenvy` in **both** bins' `main`: absolute ⇒ verbatim; relative ⇒ joined to the
directory of the `.env` that was loaded; relative with no `.env` ⇒ CWD-relative.
This matters because `dotenvy` searches *upward*, so a bare CWD-relative path let
the same `.env` produce a different log directory per launch dir — the recorder,
`recover_armed`, and the lab replay inspector (`replay_inspect::resolve_dir`) then
disagreed, and a boot that started from the "wrong" folder silently re-armed
nothing. In the container there is no `.env` (`.dockerignore` excludes it) and
compose passes an absolute `/var/lib/hunter/event_log` backed by the
`hunter-eventlog` volume — on the container's writable layer the log would be
destroyed by the same `up --build` that boot recovery exists to survive.

**Mint-level exit lock:** `InFlightGuards` serializes sells per mint (shared ATA).
After a leader sell clears the wallet mint net (PG), siblings are booked
`ExternallyCleared` / End — no parallel sell fan-out.

## Console close + externally-cleared reconcile

`POST …/positions/{id}/close?action=retry|dump|writeoff|verify[&sell_bps=N]` (per-status legality
matrix — see [position-lifecycle.md](position-lifecycle.md) §3):

1. Registry hit (Holding) → `manual_close(portion)` (engine `ManualClose`, SSE lifecycle).
   Optional `sell_bps` in `1..=9900` ⇒ partial (`Portion::BpsOfInitial`); omit / `10000` ⇒ Sell ALL.
   Partials reuse the scale-out fill path (Holding preserved, stage/sold_bps advance).
2. Registry miss / `ExitStuck`/`ExitUnconfirmed` retry → if PG `trades` net ≤ 0, book
   End (no sell RPC); else `orphan_exit::spawn_orphan_sell` (same `run_exit` feed
   confirm). Retry on a parked bag un-parks it (fresh redrive budget). Partial
   `sell_bps` is rejected here (engine-held Holding only).
3. Never returns 202 on a silent ignore (409/404/500 with an error body).

When a manual wallet sell (`POST /api/solana/wallet/sell`) empties a held mint,
the handler confirms via PG `trades` net, then for each open **real** Holding:
registry hit → `reconcile_cleared` (`Event::ExternallyCleared`); miss → PG
book-close (`orphan_exit::book_externally_cleared_pg`). The 60 s reaper also runs
`find_externally_cleared_holding_mints` so a missed reconcile cannot leave a
ghost Holding.

## Aggregate flow (`m_flow_lifetime` / `m_flow_window`)

Classifier-free SOL totals: `buy` / `sell` / `net_flow` / `gross_flow`. Lifetime is
static (two running counters on `TokenTrack`); window is dynamic (`window_size_sec`,
ring buffer). Use lifetime for maturity / critical-mass gates; window for
hot-right-now. Formulas + monotonic flags:
[`plans/strategies/metrics-reference.md`](../plans/strategies/metrics-reference.md).

## Volume/organic flow split (`m_flow_split` / `m_flow_split_window`)

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
  are O(1) jumps). Fills price via the request's `FillModel` (`trading_core::
  strategies::paper_fill`, default `worst_case` — live-paper parity on pricing);
  both legs pass `market_fill_on_empty_window = true` so a sparse trigger/fire with
  an empty fill window still books at that trade; live paper entry matches
  (`true`) for the same taken-position set, while live paper exit keeps `false`
  and can fail closed.
  `outcome_to_row` then round-trips through the request's `CostModelKind` (default
  `pumpfun_default`) — the caller must pair a non-default fill model with
  `pumpfun_fee_only` or the round-trip double-counts slippage (same `Pricing`
  coherence rule as the sweep, below).
- **`strategies/engine_sim.rs`** + **`api/handlers/strategies/engine.rs`** —
  `POST /api/strategies/simulate` (`rule_id` OR inline `draft`, `fill_model` +
  `cost_model`); reuses the fingerprint candidate scan + the analysis-cache
  single-flight; results served by the strategy-agnostic `positions::
  sim_result_{page,summary}`. Both pricing knobs persist on the saved-rule's
  `SimMeta` (`state/sim_results.rs`) and surface as the Simulate table's Fill/Cost
  columns, so a stored result always shows what it was priced under. Loads
  `with_flow` when rule params reference `m_flow_*`; dry-run uses the rule's
  fingerprint `metric_config`.
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

`TradeLite::reserve_sol` carries a deliberate `NaN` sentinel ("no real reserve
decoded yet" — see `metrics::snapshot`); since JSON has no `NaN` literal, it
round-trips through the log via `metrics::finite_f64` (`#[serde(with = ...)]`),
which maps `NaN <-> null` on both sides. A bare `f64` derive only handles that
conversion one way (serialize NaN → `null`, but fail to deserialize `null` back),
which silently dropped every such `Trade` line from recovery/replay with `WARN
event log: skipping unparseable line ...: invalid type: null, expected f64`
(fixed 2026-07-28). Any future non-`Option` `f64` field that can legitimately be
non-finite needs the same treatment — never rely on the derive alone.

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
- **Closed-position PnL booking** — realized PnL is **`End`-only**
  (`CLOSED_PRED = entry_price IS NOT NULL AND status = 'End'`): an `EntryFailed`
  never deployed SOL (excluded), and a stuck/unconfirmed exit is OPEN (unrealized,
  marks to market) until it heals, sells, or is written off (`Dead` → `End` with
  exit 0 = the loss books then).

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
