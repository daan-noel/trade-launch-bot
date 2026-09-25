# Strategies — the generic fingerprint + metrics engine

There is exactly **one generic engine** — no named per-strategy stacks. A rule =
a `fingerprint_id` (a token-creation shape, plus the trade lists its `tags` define) +
`params` (enter, signals, always lines, stages - [the rule model](#the-rule-model-enter-signals-always-stages)). The decision core is a
**pure fold** in the `hunter-engine` crate; the live and lab bins are thin adapters
that produce events and consume effects. A decision fix lands in exactly one place.

## ONE decision kernel — the root rule

Entry, exit, caps, re-entry and retries for **live-real, live-paper, and single-rule
simulate** are all decided inside `hunter-engine::reduce`. Real and paper fork *only* at
the fill layer (`exec_real.rs` vs `exec_paper.rs`); simulate forks *only* at who feeds the
events (`lab`'s `replay.rs`). Never add a second decision path or a per-strategy clone.
Live closes route through the engine (`EngineHandle::manual_close` / `reconcile_cleared`),
never a separate service.

The **grouped sweep** is the ONE sanctioned re-implementation — a precomputed
`MetricSeries` scan (`lab/src/sweep/generic/strategy.rs`) that trades exactness for speed.
Its contract has two halves:

- Every fact it *can* share with the engine is single-sourced from `hunter-engine`/`core`,
  never copied: deadness verdict, death-point, cost/PnL kernel, leaf-condition `eval`,
  `CompiledRule::compile` and its own entry and held-side decisions (`try_enter`,
  `held_line`, run over a series row through `CoinReads`), fill model, `TICK_MS`.
- Every deliberate divergence from `reduce` (bounded per-token tail, stripped concurrency
  caps, sketched quantiles) is recorded in [`../plans/sweep/sim-parity.md`](../plans/sweep/sim-parity.md)
  **and** locked by a `sweep/generic/guard.rs` parity test.

**Simulate is the PnL authority; a sweep result is a ranking screener, not a backtest.**
Its uncapped, per-token-tail numbers are optimistic upper bounds — always re-run a promoted
combo through simulate before trusting its PnL.

Deep-dive detail: flow metrics + classifier in
[`plans/strategies/_!___metrics.md`](../plans/strategies/_!___metrics.md);
what a round trip **costs** (fee 125 bps/leg, our own `buy_amount/reserve_sol` impact,
the U-shaped optimal buy size) in
[`plans/strategies/execution-costs.md`](../plans/strategies/execution-costs.md);
exit-condition traps that are invisible from the rule JSON - the ungated
`m_position.retrace_pct` and the `m_price.stall_sec` hold-cap - in
[`plans/strategies/armed-trailing-stop.md`](../plans/strategies/armed-trailing-stop.md);
the actor-model workflow that governs new rules in
[`plans/strategies/_!___strategy.md`](../plans/strategies/_!___strategy.md);
broader redesign history in [`docs/roadmap/`](../roadmap/).

## The pure engine — `hunter/engine` (crate `hunter-engine`)

`reduce(&mut EngineState, Event) -> Vec<Effect>` — no clock, no I/O, no randomness
(a purity guard test scans the crate manifest). Everything is a replay of the same
fold; live, sweep, and the replay debugger all drive it.

| Module | Role |
| --- | --- |
| `event.rs` | `Event` (TokenCreated / FirstSlotSettled / Trade / Tick / FillConfirmed / FillFailed / Migrated / RulesReloaded / ManualClose / **ExternallyCleared**), `Effect` (SubmitBuy / SubmitSell / PositionUpdate / ArmedChanged), `PositionDelta` / `ArmedDelta`, `Fill`, `ExitReason` (`TakeProfit` / `StopLoss` for the shortcuts, `Line(label)` for a rule line that sold, `Dead` / `Manual` / `Migrated`), `LoadedRule`, derived ids (`RuleId`/`PositionId`/`IntentId` = `(rule, mint, seq)`) |
| `state.rs` | `EngineState` (compiled rules / **the fingerprints those rules name** + each one's compiled `tags` (`fp_tags`) + the `TrackRequirements` every coin's track registers - the union of the rules' `Buffers` plus one entry per (fingerprint, tag) a rule reads / tracked tokens / open positions / per-rule cap counters, intent+position seqs); `TokenState` (metric track + `last_meaningful_at` + `last_trade_at` + per-rule arms + `episodes: BTreeMap<RuleId, u32>` - the re-entry episode counter, a parallel map so no `ArmState` transition has to carry it forward); `PositionRef`. Also the **settled-tick** machinery: `TokenState.settled: Option<Settled>` (this token is done changing on its own - see the `Tick` row below), `EngineState.cross_epoch` (bumped by `with_counters` / `record_identity` / `reload`, the ONE write paths for the three cross-token inputs a settled token's decision reads), the O(1) whole-map memo `all_settled_at`, `touch_token` (for boot adoption, which mutates a tracked token outside the fold), and the `dense_ticks` kill switch |
| `arm.rs` | `CompiledRule` pre-chews a rule at `RulesReloaded` into what the hot path reads, never parsing per event: flat `CondReq` lists for `enter.event` / `filters` / `final_filters`, `CompiledSignal`s, the `always` lines (stop loss, take profit, then the authored ones) and `CompiledStage`s with stage indices instead of names; one `MetricReq` per condition (its `MetricRef`, the DNF, and a `read_id` into `coin_reads`, the distinct coin reads a precomputed series must carry); the `Buffers` those reads need; the `MonoBound` entry kills; the `ClockHorizons`. **Every decision reads the coin through `CoinReads`** - `TokenTrack` in the fold, one series row in the lab sweep - so the entry and held-side walks are one generic body. Entry: `try_enter` -> `Enter` / `SpendSlot` / `Exhaust` / `No` under `enter.lock`, gated by the pre-entry veto; `can_enter` is the same gate for a buy retry. Held side: `held_step` / `held_line` (see [the rule model](#the-rule-model-enter-signals-always-stages)). `ArmState` machine `PendingFirstSlot -> Armed -> EntryPending -> Entered -> ExitPending -> Done \| Disarmed`, plus `Cooldown { until: Ts }` - a normal exit (TP / SL / a rule line, never Dead/Manual/Migrated) with `RuleParams.reentry { cooldown_sec, max_per_coin }` configured re-arms into `Cooldown` instead of `Done`; `evaluate_token` promotes `Cooldown -> Armed` once `now >= until` (trade/tick-driven, no new timer) and treats `Cooldown` as active so the token is not pruned; absent `reentry` => one position per coin. `EnteredCtx` carries the stage index and when it began (`move_to`), `sold_bps`, and the peak / trough. Live boot seeds `episodes` from a batched closed-position count over adopted mints. Also carries `exclusive` / `priority` (`RuleParams`) - the single-position-per-token toggle |
| `identity.rs` + `dupe_guard.rs` | the **copycat guard**. `token_identity_hash(name, symbol)` is the ONE hasher (normalize → FNV-1a → 63-bit, so an `i64` column holds it unchanged) for the live producer, the lake exporter, and replay; `DupeGuard` is a per-mode (paper/real) rolling memory `identity → [(mint, at)]` that `decide_arm` consults. Records at the entry **attempt** (a reverted buy still counts), exempts the recording mint (else a token blocks its own retry), and prunes on event time. Policy arrives via `EngineState::set_dupe_guard_policy` — a switch, not an `Event` |
| `reduce.rs` | the fold: arm on fingerprint match, disarm (dead / migration / derived-unsatisfiable - which captures the entry conditions still unmet at that instant via `CompiledRule::entry_blockers`, since the kill names only the monotonic bound that crossed / **duplicate-identity** - a different mint with the same `(name, symbol)` traded inside the guard's window; a `Disarm`, not `exclusive`'s wait, because the block outlives any curve token), `exclusive` rules stand down (stay `Armed`, never disarmed) while ANY other arm on the token holds a position - in-flight buys/sells and manual arms included, since `evaluate_token` reads the shared `token.arms` map; that sweep is visited in `(priority desc, RuleId asc)` order so whichever exclusive rule sorts first claims the token and later ones see the claim in the *same* event, enter via `CompiledRule::try_enter` (the entry holds **and** no sell line already holds - refuses to buy into an immediately-exitable state), caps checked at entry, fill retry policy (`Reverted` bounded; `Fatal` immediate give-up; exit `Unconfirmed`/`Fatal` terminal - never resold), held side **`Dead` first, then one `held_step`** (`decide_arm`): a full sell, a partial sell (`sell_pct` of the first bag, moving to the line's `go` stage when its fill lands; one that would take the bag past what is left sells the rest) or a stage move (`EnteredCtx::move_to`, read from the next print or tick). A rule-line exit persists as its label via `ExitReason::Line`; the TP/SL shortcuts keep `TakeProfit`/`StopLoss`; `ManualClose` (sell), `ExternallyCleared` (book closed, no sell) |
| `cap.rs` | `Cap` — a governance limit with its `0 = unlimited` storage encoding already decoded. `Cap::zero_unlimited` is the ONE reader of that sentinel for **both** caps (`max_total_tokens`, `max_concurrent_tokens` — a blank field in the rule editor stores `0` on either); `CompiledRule` carries both already decoded so the fold just asks `allows(count)`. `UNLIMITED = u32::MAX`, so the hot path is a single `<` with no branch |
| `fingerprint.rs` | `Fingerprint` (criteria; lamports at rest) + `match_all` / `MatchPhase` (Instant vs Full — the two-phase first-slot split). `wildcard` matches EVERY token and short-circuits every axis: a criterion-less row matches *nothing* on purpose (a half-filled form must not arm on everything), so a rule that decides purely on the tape has to say "any token" out loud or the two states are indistinguishable. A DB CHECK refuses a wildcard row that also carries axes, `Fingerprint::validate` refuses one at the write edge (a 400, not a DB error), and `wildcard` is part of match identity (`IDENTITY_WHERE` compares it) so a wildcard row never dedupes against an axis row. Every reader of the flag agrees on the same verdict: the matcher short-circuits to *matches all*, `has_any_criterion` counts it as the one criterion on both the model and engine sides, and the creation-stats SQL mirror (`fingerprint_scope_clauses`) emits **no** clause — never the `FALSE` fence a criterion-less row gets, which would show 0 matched tokens for the one fingerprint that arms on every launch. UI: a `match every token` checkbox on `FingerprintForm` that greys out and drops every axis, an `ALL tokens` chip in place of the axis chips, and an `all` badge in the fingerprints table (where the axis columns are all dashes) |
| `metrics/` | the registry, the per-coin state and the reads. **`registry.rs`**: `FAMILIES` + `METRICS`, one `MetricSpec` per quantity - path `m_family.quantity` (unit in the last word: `_sol`, `_count` for prints, `_tx_count` for transactions, `_pct`, `_sec`; none = a 0/1 flag), summary, example, note, the tags it accepts (`TagUse` none / optional / required, at the `TagLevel` trade / template / wallet class) and the spans (`SpanUse`), `monotonic`, `eq_tolerance`, hue - plus `RULE_PARTS`; `registry_json()` serves it all, pinned to `engine/fixtures/registry.json` by `registry_fixture_is_current`, and every UI surface renders a metric only from it. **`metric_ref.rs`**: `MetricRef` = one read, metric + tag + span (`m_flow.buy_sol @!volume [10s]`); `check()` refuses a tag or span the metric does not accept, `label()` is the one spelling in labels, readouts, ledgers and errors, `chart_reads` lists every read a chart draws. **`span.rs`**: `Span` - the life (default), a trailing window `10s` / `20sl` / `5p` closed `[now - N, now]` with an optional lag (`10s@2`), a since-age anchor (`age60s`), and the nested `slice` of the two `m_flow.slice_*_share_pct` reads; a span is part of a read's identity, so two conditions on `buy_sol [10s]` share one buffer. **`tags/`**: a fingerprint's named trade lists - `config.rs` (the `tags` document, `validate_tags`, `compile_tags`), `state.rs` (`TagState`, see [the rule model](#the-rule-model-enter-signals-always-stages)). **`buffers.rs`**: `Buffers`, the per-buffer registration a set of reads needs, derived by one walk that the rule compiler, the engine's per-coin registration and the sweep's series all share. **`track.rs`**: `TokenTrack`, all the metric state one coin carries and the router every read goes through (`value(MetricRef, fingerprint, now)`); a read with no registered state is `NaN`, which satisfies nothing. Compute, one module per subject and basis: `state.rs` (`m_state`: `age_sec`, `liquidity_sol` on either venue, `on_curve` = the venue of the last print), `price_lifetime.rs` / `price_window.rs` (`m_price` over the life / a trailing window via monotonic deques), `flow_lifetime.rs` / `flow_window.rs` / `flow_slice.rs` (untagged `m_flow`; the window stays lean at 8 bytes a print with no transaction flag, so the `_tx_count` quantities need a tag; the slice reads own no state), `tags/state.rs` (tagged `m_flow` and `m_holdings.profit_sol`), `holder_book.rs` (`m_holdings.bag_share_pct` over the built-in wallet classes: an exact per-wallet token book over `TradeLite::token_amount`, a holder classed at its first buy, the `public_app` test from `TradeLite::build_day_public`, which `reduce` stamps from `EngineState::public_recipes` - the recipes of the daily build-breadth table that pass `holder_book::is_public_app`, replaced by an `Event::BuildBreadthReloaded`: at 00:00 UTC in replay, by `breadth_refresh` off the decision loop in live), `crowd_window.rs` / `build_window.rs` / `crowd_after_age.rs` (`m_crowd`: distinct wallets, distinct ix shapes, buyers since an age; the two windowed counts hold a `distinct_window.rs` counter, the one O(1) distinct-count mechanism), `print_wallet.rs` (`m_print`: a wallet -> last-buy map a track opens only when a loaded rule reads it), `burst_slot.rs` (`m_slot`, this slot's buy prefix at the ix-template grain, and `m_crowd.unique_ix_templates`), `burst_wave.rs` (`m_wave`, the consecutive-slot buy run), `position.rs` (`m_position`, **position-scoped**, read from a `PositionCtx` on `ArmState::Entered` and `NaN` without one: `pnl_pct`, `held_sec`, `retrace_pct` / `bounce_pct` off the running peak / trough seeded at the entry fill, `stage_sec`, and `room_taken_pct` - `pnl` as a percent of the entry's room to the graduation wall (`position::GRADUATION_PRICED_RESERVE_SOL`), sized from the `vsol` of the last print folded at `FillConfirmed`, which the sink stores in `strategy_positions.extra` and boot adoption (`orphan_exit`) reads back). `trade_keys.rs` is the one hasher set for a trade's identity (ix shape, build recipe, marker bits, wallet) every adapter calls. **A rule pays only for the buffers its reads name**: `Buffers` registers one buffer per span per backing store (`ensure_window` / `ensure_price_window` / `ensure_crowd_window` / `ensure_build_window` / `ensure_crowd_after_age`, the print-wallet map, the holder book, the slot state) and one `TagState` (`ensure_tag`) or template view (`ensure_template_tag`) per (fingerprint, tag) read; `reload` narrows `fps` to the fingerprints the rules name and compiles their `tags` once, because the live edge hands over every `fingerprints` row and a tag state folds on every trade of every coin. Formulas + NaN rules: [`plans/strategies/_!___metrics.md`](../plans/strategies/_!___metrics.md) |
| `rule_params.rs` | `RuleParams` - the typed `strategy_rules.params` (`RULE_FORMAT_VERSION` 2): `enter { event, filters, final_filters, lock, size_pct_of_pool }`, the `take_profit` / `stop_loss` shortcuts, `signals`, `always`, `stages` (at most `MAX_STAGES`), `reentry`, `exclusive` / `priority`. Parsed and validated once at save and at load, never per event; `to_value` is the one canonical writer. A condition is `{metric, tag?, span?, slice?, is}` (a `MetricRef` and its DNF: a list is AND, a list of lists is OR) or `{signal, not?}`; a line is `{if: [cond], sell?: "label" \| true, sell_pct?, go?}`, `sell_pct` at most `MAX_SELL_PCT` of the first bag. **Any condition or line may carry `"off": true`** - parked: kept in place and validated exactly like a live one (so switching it back on can never produce an unsavable rule) but compiled by nothing, so the engine, sweep and simulate never read it and the hot path pays zero. A parked condition may duplicate a live one on the same read (park `trail_pct >= 12` while trying `trail_pct >= 20`) |
| `v1.rs` | The one reader of documents written before metric system v2 - rule params, fingerprint `metric_config` + criteria, rule bundles, stored sweep axes and ladders - into v2. A v1 (group, metric, window) becomes one `m_family.quantity @tag [span]` condition (`map_metric`); the fingerprint lists become tags (`m_flow_ix` = `volume`, `m_dump_ix` = `dump`, `m_copy` = `targets`, `m_burst_slot` = `working`); exit clauses become `always` lines; `arm_above_pct` on a lone trailing clause adds `m_position.pnl_pct >= X` to that line; the `armed` latch becomes stages `start` -> `armed` (`since_armed` = `m_position.stage_sec`); a `scale_out` ladder becomes one stage per rung; `disabled` becomes `"off": true`. A combination v2 cannot state exactly is refused. **Permanent**: the core and lab data migrations, rule-bundle import, the fingerprint PUT (a body may still send `metric_config`), every reader of a stored sweep combo (`parse_params_any`), and the golden tests, which keep their pinned v1 rules and so prove the conversion decision for decision |
| `grouping.rs` | bucket matching (`same_bucket`) for the SOL fingerprint axes |
| `deadness.rs` | `is_dead_verdict` + `DEAD_*` consts — the ONE deadness SSOT (core + live + sweep re-export it) |
| **`Tick` (in `reduce.rs`)** | Sweeps every tracked token - **except** those `Settled`. A token is settled once a sweep has run **at or past** its horizon (`arm::ClockHorizons`: widest trailing window from the last trade, `m_state.age_sec` thresholds and age deadlines from creation, `m_price.stall_sec` from the last trade, `m_position.held_sec` thresholds and held deadlines from the entry fill, `m_position.stage_sec` thresholds and stage deadlines from the stage start; plus the dead flip and any `Cooldown { until }`) **and** `cross_epoch` has not moved since. This exists because a token whose real reserves stayed above `DEAD_MAX_LIQUIDITY_SOL` (or that has no reserve reading at all) can never go dead, so without this it is never pruned and gets swept 5x/second forever - the dominant cost of a multi-day simulate. Skipping is decision-neutral and guarded differentially by `engine/tests/settled_ticks.rs`; measured ~180x on the quiet-token shape (`engine/tests/tick_bench.rs`). Full rationale: [../plans/strategies/tick-cost-and-settled-tokens.md](../plans/strategies/tick-cost-and-settled-tokens.md) |
| `kernel.rs` | `CostModel` / `round_trip_with_costs` / `round_trip_multi_leg` (+ `ExitLeg`) + `RunAgg` -> `RunMetrics` (== `strategy_run_metrics` cols) + the quantile sketch / robust score - one copy of the PnL+summary math shared by live/paper/sweep. Fixed per-leg cost (tip + CU priority) comes from process-wide [`FeeTuning`](../../core/src/config/fee_tuning.rs) (`JITO_MIN_TIP_SOL` + `CU_PRICE_MICRO_LAMPORTS`), installed at boot by both bins. **`FEE_BPS_PER_LEG = 125`** - measured, not assumed. **Runs stored before 2026-07-28 were priced at 100 bps with no impact charge: they understate cost and do not compare to a new run** (the constants are not persisted per run). <!-- pt-ok: cutoff, those runs are still in the DB --> Two `CostModelKind`s: **`pumpfun_impact`** (the default) - the only one that charges our own `buy_amount_sol / reserve_sol` price impact, so the only one whose cost responds to buy size - and `pumpfun_fee_only`, a size-blind zero-impact upper bound. Callers pass entry pool depth; `None` => no impact, never a guess, so `pumpfun_impact` without depth *is* `pumpfun_fee_only`. **`CostModel` carries no flat slippage field**: a third kind charged one, which double-counted what the fill model already prices and, being size-blind, had an error that changed sign with buy size. The kind, the field and its wire name are all deleted - `pumpfun_default` does not decode. An unrecognized `cost_model` therefore fails loudly instead of resolving to a default, which is correct: a run that reports a model it was not priced under is not comparable to anything, and the runs that named it are deleted rather than migrated. A position with partial sells prices through `round_trip_multi_leg` (fixed cost x leg count); the single-exit wrapper stays for legacy / sweep until the staged resolver lands |
| `event_log.rs` | `LoggedEvent` — the on-disk JSONL format, SSOT for the live recorder (writer) + the lab replay inspector (reader) |
| `readout.rs` | **Read-only** view of what the fold reads for one (token, rule), placed where each condition sits: `read_rule` walks the `CompiledRule`'s conditions through the same `MetricReq::read` + `evaluator` the decision uses and returns a `ConditionRead` per metric condition (its `ReadPart` - event / filter / final filter / signal group / always line / stage line, `on` or `at_end` - the `MetricRef`, the authored DNF, the value, `matched`, `ok`), a `SignalRead` per signal and a `LineRead` per line (`holds`, the label it `sells` with, `sell_bps`, `goes_to`). `read_state` resolves an arm straight off `EngineState` - including a **manual episode's** rule, which lives in `manual_rules` keyed by position and is invisible to a plain `state.rules` get - and names the held position's stage. `replay_readout` reconstructs the same thing for a closed position by folding stored trades to one instant, with its tag context (`ReplayTags`: fingerprint, compiled tags, creator). `RuleReadout.source` (`Engine` \| `Replay`) travels with the numbers because the two are not equally exact. See *Rule readout* below |

## The rule model: enter, signals, always, stages

A rule reads the coin through **conditions**: one `MetricRef` (`m_family.quantity @tag
[span]`) judged against a DNF, or a named signal. The grammar is `rule_params.rs`; every
part's one-line definition and example is the registry's `RULE_PARTS`, which the editors
and the Guide page render as-is.

**Entry.** `enter.event` is the print that triggers the buy, `enter.filters` must also
hold (a failure keeps watching), and `enter.final_filters` are checked on a print that
would otherwise buy (a failure ends the coin for this rule, `EntryVerdict::Exhaust`).
`enter.lock: "token"` gives the coin one decision, on the first PRINT that makes the
event true (a tick never is one, `evaluate_token`'s `on_print`); `"slot"` gives one per
slot, and a filter failure spends that slot. `enter.size_pct_of_pool` sizes the buy as a
percent of the pool's SOL, resolved in `reduce` at the entry instant. **The pre-entry
veto** (`sell_line_holding_before_entry`): the rule never buys while an `always` sell line
or a first-stage sell line already holds. Position metrics read `NaN` before the buy, so
a line on our position never vetoes.

**Held side: one step per print or tick** (`CompiledRule::held_step`). Each evaluation
takes at most one action:

1. the `always` lines in order - stop loss, take profit, then the authored ones; the
   first that holds acts;
2. else, when the current stage's deadline is reached, the first `at_end` line that holds
   acts, and when none does the rule moves to `then` (default: the next stage);
3. else the stage's `on` lines in order; the first that holds acts.

`Dead` outranks all three (`reduce::decide_arm`). A line sells the whole bag, or
`sell_pct` of the first buy's bag, and may move (`go`). A move takes effect from the next
print or tick, where the new stage's lines are first read; a partial sell moves when its
fill lands (`ArmState::ExitPending.then_stage`). `m_position.stage_sec` reads the time
since the current stage began, so "in the first 30 s of the ride" needs no deadline.

**Deadlines are clocks, so they join `ClockHorizons`.** A stage's `ends` is `age_sec`
(coin age), `held_sec` (since our buy) or `stage_sec` (since the stage began);
`absorb_deadline` widens `time_secs` / `held_secs` / `stage_secs` to one tick past it, so
the settled-token tick skip never skips a deadline (the `Tick` row above). The sweep's
frozen tail resolves deadlines the same way ([sweep.md](sweep.md)).

**Exit labels.** A sell line's label is authored (`"sell": "spike"`) or, absent, its first
live condition as written (`m_flow.buy_sol @!volume [10s] >= 2`; a signal's name on a
signal line). It travels as `ExitReason::Line(label)`, persists verbatim in
`strategy_positions.exit_reason`, and buckets as `n_exit_metrics` in every rollup. The
TP/SL shortcuts keep `TakeProfit` / `StopLoss`. A row closed before the v2 migration keeps
its v1 label (`stall > 3`), shown raw.

**Tags are per fingerprint.** A `@tag` read counts the trades of a list the rule's
fingerprint defines in its `tags` document (`metrics::tags`): `@volume` the trades that
carry it, `@!volume` the rest. A trade carries a tag when ANY of its `match` entries holds
(`program`, `ix_shape`, `ix_template`, `ix_contains`, `ix_lacks`, `wallet`, `creator`,
`cluster`), on the tag's `side` only; `sticky` keeps the tag on a wallet that carried it
once; `exclude_creation_slot` counts a creation-slot buyer that matches nothing on
neither side. The fold keeps one `TagState` per (coin, fingerprint, tag) a loaded rule
reads - lifetime totals for both halves, one window per span, the sticky set, the cluster
groups and each half's bag - and one template view per (fingerprint, tag) an `m_slot` /
`m_wave` / `m_crowd.unique_ix_templates` read uses, which sees only the tag's
`ix_template` and `program` matchers. A tag the fingerprint does not define opens no state
and reads `NaN` (`rule_tag_warning` says so at save). The built-in wallet classes
`bundled` and `public_app` need no config and serve `m_holdings.bag_share_pct` only. A
tags document is validated on save (`validate_tags`) and compiled once per reload
(`compile_tags`); a trade already folded keeps the half it was folded under when a tag is
edited. `TradeLite` carries the trade's identity hashes from the one `trade_keys` hasher
set (ix shape, marker bits, wallet), so every adapter classifies alike.

**Every driver that folds a track registers the tags off the one fingerprint row.**
`EngineState` (live + simulate), `replay_readout` / `replay_series` (`ReplayTags`), the
lab's `/metric-series` and the sweep's series (`register_tags`) all compile the same
`tags` document and seed the creator before the first fold (the sweep, whose lake has no
creator column, a first-slot stand-in: [sim-parity D8](../plans/sweep/sim-parity.md)); a
driver that skips either returns `NaN`, or books the dev's trades on the other side, for
a condition the live engine evaluates.

## Rule readout (what a position is waiting on / why it closed)

One question, two sources, and the answer says which it used. An **open** position is
read out of the decision loop's own `TokenTrack`, so a condition shown satisfied is one
the fold is acting on. A **closed** one has no engine state left and is reconstructed by
`replay_readout` — folding `TradeRepo::find_by_mint_until` rows through a fresh track to
the exit (or entry) fill, then reading there. For a rule that reads the holder book
(`m_holdings.bag_share_pct`), each buy is first stamped on its own UTC day's stored
build-breadth table (`BuildBreadthRepo::load_day`, read-only: a day never stored stays
unknown and a `@public_app` read is `null`).

A replay anchors its metric clock on the **token's own `created_at`**, the same instant
`TokenCreated` gives the live fold - never on the first retained trade. `m_state.age_sec`
is measured from it, `m_price.stall_sec` and the lifetime price/flow state are re-based
on it, and the tick grid is phased from it, so anchoring on `trades[0]` shifts all three
by however much of the token's head has aged out of the rolling ingest window. Its hashes
agree with the fold's for the same reason: `trades.ix_labels` goes through the engine's
shape-complete `ix_hash_from_labels_value`, so both persisted shapes hash alike and an
object-shaped row is not silently booked outside every tag.

**Two shapes, and the client picks by what it is asking.** `.../metrics` answers at one
instant, which is what makes it cheap enough to poll. `.../metric-series`
(`replay_series`) answers at *every* row of the engine's decision grid in one pass, for
a chart crosshair that would otherwise re-fold the token's history per hover — the fold
is O(all trades) and a pointer moves at frame rate. Both resolve their rule and tag
context through one shared path (`resolve_rule`, `load_tag_ctx`), because the
`params_snapshot` preference below is exactly what two copies must not disagree about.

The series is affordable where the lab's is not because it folds **only the columns
this rule's conditions read** (a 6-condition rule folds 6 columns, against the lab's
whole registry) and evaluates backend-side, so the client declares no windows or
horizons: the grid's density comes off `CompiledRule::clock_horizons`, with `held_secs`
and `stage_secs` riding on the `stall` horizon (measured from the last trade, which is at
or after the entry fill and any stage move, so it covers both). The tick grid is
load-bearing either way: every decaying metric advances only inside a tick, so a
trade-only fold never samples a between-trades crossing.

**The row cap is a coverage duration, not a payload size.** Any rule with a time stop
holds a horizon open longer than the gaps an actively-traded token leaves, so the grid
emits `1000/TICK_MS` = 5 rows per second of coverage almost regardless of how often the
token prints: `MAX_READOUT_SERIES_ROWS` = 8k buys **~22 minutes**, and a fold that runs
out reports `truncated` + `covered_until` rather than reading as a token that stopped
trading. Position windows past that are uncovered, which the strip surfaces as
`· past coverage`. Nothing at the call site reveals that the row count is really a clock.

**Armed rows get the same pair.** `GET /api/strategies/armed/metrics` and
`.../armed/metric-series?mint=&rule=[&armed_at=]` answer for a (token, rule) the engine is
armed on but has **not entered** — the Waiting modal's "how close is it". One
`SeriesAnchor` is the only difference between the two series routes: pre-entry there is no
entry fill and no stage, so the position-scoped conditions read `null` throughout,
which is exactly what the entry gate itself sees. The rule is taken **live**, with no
`params_snapshot` step — nothing has been decided yet, so the current thresholds are the
ones this row is being judged by, and reaching back for a frozen copy would draw a rule
nobody is applying.

**So the budget is spent on the anchor's window, not on the token's first N rows.**
`MetricSeries::set_record_from` withholds rows before an instant while the fold still runs
from creation, and the series routes set it to `anchor − max(widest window, 60 s)` — the
entry fill for a position, the arm instant for a Waiting row, which is what keeps a
long-tracked token's coverage on *why it is still waiting* rather than on its first
minutes. Raising the cap would only *move* the boundary - cost is linear in rows and this
ships to 2vCPU - whereas placing it covers the position that was actually opened for
inspection. Without it a position entered later than the cap's span falls entirely
outside coverage, which is the one case a post-mortem cares about.

**Recording later is not folding later, and the difference is a wrong answer rather than a
missing one.** `m_price` over the life and `m_state.age_sec` are defined from token
creation, so a fold that *starts* at the window reports different numbers; one that
starts at creation and discards early rows reports the same numbers over a narrower span.
Withheld rows cost no row budget.

Coverage is therefore a span with two ends, and the response carries both plus the reason:
`covered_from` / `covered_until`, and `record_from` (`null` when the whole history is
recorded). The strip needs the reason because the two ways a head can start late read
oppositely — with a window, rows to its left exist and were withheld (`· before coverage`);
without one, the token had not traded yet and there is nothing to miss.

The contract between the two routes is one test: **a series row at an instant equals
`replay_readout` at that instant**, condition for condition
(`a_series_row_equals_the_replay_at_its_instant`). It holds because they are the same
parts - the same track, tag and buffer registration, position ratchet, and the one
`judge` body. Per-row `ok` must never come from a second evaluation.

What a second reader of a rule gets wrong (the engine side is guarded in
`engine/src/readout_tests.rs`):

- **A condition is identified by its place.** `listed_reqs` walks entry (event, final
  filters, filters), signals, `always` lines, then each stage's `on` and `at_end` lines -
  the one order the point read and the series share, so column `i` and point read `i` are
  the same condition, and the wire names each by `part` (`event` \| `filter` \|
  `final_filter` \| `signal` \| `always` \| `stage`, with the signal group, stage and line
  index) (`every_condition_is_placed_where_it_is_written`).
- **Entry conditions read with no position**, even on a held position, exactly as the
  pre-entry decision does (`entry_reads_with_no_position`). Position-scoped columns are
  likewise blank before the entry fill: there is no position there, and the live fold
  reads `NaN` on an un-entered arm.
- **Every line is reported; only some decide.** `lines` carries each `always` and stage
  line with `holds`, its sell label, `sell_pct` and the stage it `goes_to`, and `stage`
  names where the position is. A line of another stage - or an `at_end` line before the
  deadline - shows what the fold *would* read there, never a decision it is making.
- **A replay's stage clock is an upper bound after a move.** `strategy_positions` keeps the
  stage index (`scale_stage`) but not when the stage began, so a replay reads
  `m_position.stage_sec` from the entry fill (`replay_stage`): exact in the first stage,
  an upper bound after a move.
- **A replay compiles `strategy_runs.params_snapshot`, not the rule's current params.**
  A rule edited after the position closed would otherwise draw thresholds that never
  applied to it — the most misleading thing a reconstruction can do, since every number
  beside them is real. Missing/unparseable snapshot falls back to the rule row, logged.

A replay is close to, not identical with, the live reading: `trades` carries no
real-reserve column, so a stored row's is *reconstructed* (`approx_real_sol_reserves`),
and any trade the feed observes without persisting is absent. `trades` retention on
the deploy box also bounds how far back one can go; past that the replay returns nothing
and says so, and the lake (lab-side) is the only answer.

## Live adapters — `live/src/strategies/engine/`

The live composition root around the fold: it **produces** events (ingest pings + a
`TICK_MS` clock tick + confirmed fills) and **consumes** effects (submit on-chain /
paper, persist to PG, push SSE). All decision logic is in the fold; these are
side-effects only.

| Module | Role |
| --- | --- |
| `decision_loop.rs` | **THE** one serialized `select!` loop (command / fill / **create ping** / trade ping / `TICK_MS` tick — create lane biased above trade pings); every `reduce` call happens here; two-pass dispatch = registry/SSE first (BuySubmitted/ExitPending PG is async) then submit spawn. `spawn_engine` → `EngineHandles { handle, armed, positions, task }` |
| `producers.rs` | `StrategyPing` + `TokenCache` → `Event`s; first-slot settlement detection (freshness-gated); the live freshness gate; **the restart rail** — `Produced { prime, events }` splits cached trades into history to observe vs signal to decide on (`started_at`), plus `prime_tracked` for mints that never ping; feeds `real_reserve_sol` for deadness parity; the rebuild reads (`hydrate_facts`, `hydration_history`, `history_trade_lite`) |
| `hydrate.rs` | **Rule activation**: after every reload, arms the rules that just became armable on every cached, alive token born since `started_at`, rebuilding a track the new rule set reads more of from its whole history (cache, or `trades` before the oldest cached trade spliced onto it) through `hunter_engine::hydrate_token`, one token per loop turn. Tokens that crossed a restart stay out: the downtime's trades are nowhere. [restart](../plans/strategies/restart-state-restoration.md) |
| `exec_real.rs` | `SubmitBuy`/`SubmitSell` → executor submit-and-return, then synthesize a definitive `FillConfirmed`/`FillFailed` from the **trades feed** (RPC watchdog fallback). SOL commit/release; M2 sync `SubmittedBuyJournal` + fire-and-forget bounded `mark_buy_submitted`; adopt skips PG when journal empty; curve sell uses cache reserves for min_out; `classify_swap_revert` heal; sell route re-read + rent reclaim. **Double-fire safe:** `FillFailed::Reverted` only when re-submitting is safe |
| `exec_paper.rs` | worst-case paper fill (`paper_fill`, slot window) → `FillConfirmed` (sim-parity). **`Fill::price` is SOL per RAW token unit**, so `token_amount = sol / price` with no decimals scaling — see below. Stashes the `PrintKey` (slot, tx_index, leg_index, block_time) of the print that priced each fill in `FillSigStore`; the token cache is signature-free, so the sink resolves the signature |
| `sinks.rs` | `PositionUpdate` → registry + SSE; `BuySubmitted` upserts registry then background `insert_position` (later transitions chain on the handle); `Holding` updates registry sync then backgrounds fill persist; `ExitPending` PG is fire-and-forget; **terminal writes (`End`/`EntryFailed`/`ExitStuck`/`ExitUnconfirmed`) chain-spawn too — NO sink transition awaits PG on the loop** (see below); terminal SSE emits **before** `registry.remove` (so `position_id` / frozen `trade_mode` stay on the wire); `warm_runs` on rule reload (`ensure_run` reuses latest still-`Running` DB run + collapses empty leading shells — does not mint a new `run_seq` on every restart); releases SOL on terminal unentered exits. Inside each write task, a `PrintKey` (a paper fill's print, and the trigger print in both modes) resolves to its signature via `TradeRepo::print_signature` (3 reads, 250 ms apart) and lands where a real fill's does (`entry_tx_signatures`, `exit_tx_signatures`, `target_tx`, the buy leg of `position_fills`); a paper SELL leg keeps no ledger signature, because `uq_position_fills_sell_tx` guards only OUR sells and two paper positions can exit on one print (`FillSigKind::Print`); a miss writes the row without one |
| `reapers.rs` | Boot+60 s: buy orphan adopt/drop/wait (never re-send; stale ⇒ `needs_review` SSE); **externally-cleared Holding** book-close (PG `trades` net, no RPC); exit orphan nudge via `FillFailed` or shared `orphan_exit`; **ExitStuck-with-bag** redrive (PG-gated, backoff, bounded-then-park); `ExitStuck`/`ExitUnconfirmed` bag-gone heal → End; stale `ExitPending` bag-check → `ExitStuck` (real) / breakeven End (paper). Skips `InFlightGuards`-held rows/mints |
| `orphan_exit.rs` | Shared direct-sell + PG book-close for registry-miss rows (Console close, ExitPending/ExitStuck reapers). Feed-confirm via `run_exit`; sibling mint clear → `ExternallyCleared` / PG End; boot adopts re-install manual TP/SL rules |
| `rule_readout.rs` (in `live/src/api/handlers/strategies/`) | The readout's HTTP surface. `GET .../positions/{id}/metrics[?at=exit\|entry]` answers from the live fold when `PositionRegistry` still holds the row, else replays the durable row + stored trades (fold under `web::block`); `GET /api/strategies/armed/metrics?mint=&rule=` and `.../armed/metric-series` do the armed pair. Both series routes share ONE body (`series_response`) differing only in a `SeriesAnchor`, so an armed row and a position row can never disagree about a grid row. Reaches the loop through `EngineCommand::ReadRule` (`oneshot` ack, 2 s - a UI poll must not queue behind trade decisions), **not** a per-tick publish, which would allocate on the hot path for a usually-closed modal. Each `404` keeps its own reason (manual position / deleted rule / trades aged out / never filled); a wedged loop is `503`. The wire carries names only - the registry path, the tag and span as authored, the full `label` (`m_flow.buy_sol @!volume [10s]`) and each condition's and line's `part` - never an engine ordinal, and non-finite readings serialize `null` |
| `event_log.rs` | JSONL recorder (day + size segmented rotation, age/byte retention) + **conservative, bounded** boot-recovery replay (`recover_armed` = re-arm only; held/filled mints excluded; effects discarded; reads only the recent tail — see below). Dir = `EVENT_LOG_DIR` via `config::dir_from_env`: a relative value anchors to the loaded `.env`'s directory, never the CWD (see below) |
| `convert.rs` | DB model ↔ engine type converters (re-exports `fingerprint_axes::{fp_to_engine, observed_axes, rule_to_loaded}`) |

`EngineHandle` (held by the HTTP layer, enqueues commands only): `reload_rules` (blocking, used by the background scheduler), `schedule_reload(sse_tx)` (HTTP rule/fingerprint mutations — PG write returns immediately; debounced reload + `tpsl_rules_changed` SSE on ack; coalesced reload acks in the decision loop),
`manual_close(pg_id, portion)` (per-row "Sell ALL" / "Sell N%"), `close_rule(rule_id)` (per-row Stop),
`close_mode(real)` (Stop All), `reconcile_cleared(pg_id, fill)` (externally-cleared
close — below), `manual_buy(pg_id, mint, lamports, exit)` (Console manual buy — a
fresh per-episode rule id + `Event::ManualBuy`), `set_manual_exit(pg_id, exit)`
(per-position TP/SL resynthesis). `DeployState` also holds the shared
`PositionRegistry` + `InFlightGuards` + engine `fill_tx` so Console orphan-close can
sell without the registry and still fold sibling clears.

**Paper/sim fill units (locked).** A `Fill::price` is the feed's `price_per_token` =
**SOL per RAW token unit** (`Trade::new`: `amount_sol / token_amount`, count in raw
units), the same convention `entry_price`/`exit_price` and the real executor use. A
synthesized paper/sim fill therefore sizes `token_amount = sol / price` and prices a
leg `sol = token_amount × price` — **never** through a `10^decimals` factor. Such a
factor cancels out of SOL PnL (so PnL and every ratio-based exit stay correct) while
silently inflating the *stored* token count, which then corrupts anything derived from
that count alone — see
[`@history/2026-08-04-token-scale-1e6-pnl.md`](../history/2026-08-04-token-scale-1e6-pnl.md).
Corollary for tests: a corpus priced at
`1.0` buys a **one-unit** bag, so any partial sell quantizes to 0/1 units - the
sweep parity guard prices its partial-sell corpora at `RAW_PX = 1e-6`.

**No PG write blocks the decision loop (locked).** Every sink transition, terminal
ones included, chain-spawns its write and keeps the handle in `pending_pg` so the
*next* write for that same position awaits it first — per-position order is total,
the loop never waits. **Terminal handlers are not an exception** — an inline
`await_pending_pg` + `record_sell_fill` + real-mode held-pool check is three round
trips, and a Stop closes every position of a rule at once, so they serialize
head-to-head while ingest is also writing PG; while the loop is blocked **nothing**
else folds — no ticks, no pings, no other fills. **A position's
`strategy_position_update` frame is chained onto the same handle** (`send_after_write`):
it is built at the transition but sent only once that position's write commits, so
a client may refetch the row on a frame and read the new status. Sending it at the
transition instead races the spawned write, and every refetching reader (Rules
Evidence, Console History, the fill ledger, Portfolio) shows the pre-transition row
with no later frame to correct it. `pending_pg` is pruned of finished handles on each
finalize (`prune_finished_pg`), else it would grow by one entry per closed position
for the life of the process.

**Position lifecycle:** `BuySubmitted → Holding → ExitPending → End`, with
`EntryFailed` (buy never filled, terminal) and the OPEN attention states
`ExitStuck` / `ExitUnconfirmed` (engine drops the arm; reaper + manual actions own
the row). Full map: [position-lifecycle.md](position-lifecycle.md). Manual positions
(`origin='manual'`) ride the same machine; their optional TP/SL compiles into a
per-position one-off rule (`EngineState::manual_rules`) — without it, tracked-only
(no auto-exit).

**Boot Holding adopt:** after event-log re-arm, PG `Holding` rows are loaded into
the in-memory engine (`Entered`) + registry (PG-only, no RPC) so TP/SL/Dead and
Ops `ManualClose` work after a process restart. The stage and `sold_bps` resume from the
row (`scale_stage`, the index the last partial fill recorded). A stage move with no sell
(`go` alone, a deadline's move to `then`) is not stored, so such a position resumes in the
last stage a partial fill recorded, and the stage's start time is not stored, so
`m_position.stage_sec` restarts from the entry (open:
[metric-system-v2-plan](../roadmap/metric-system-v2-plan.md)).

**Warm start: prime, never re-decide.** An adopted arm carries the entry price but
an *empty* metric track, while the async cache seed backfills up to
`SEED_TRADES_PER_MINT` (500) historical trades per mint — and the producer's trade
cursor is RAM-only, so every seeded row reads as new. `Producer::split_trade`
therefore routes each cached trade by chain time against the loop's `started_at`:
older ⇒ **primed** (`hunter_engine::prime_trade` — folds the track, the peak/trough
of every `Entered` arm, and the deadness clock, emitting nothing, and never
recorded to the event log), newer ⇒ a live `Event::Trade` the fold decides on. The
200 ms tick then decides against a warm track and the wall clock, so nothing is
lost — only re-based onto the present. `Producer::prime_tracked` runs the same path
from the tick for tracked mints that never get a ping (a quiet token's adopted bag),
retrying until the seed lands. A seeded row is read with the same `trades`
projection as a history read (labels, fee trio, `real_reserve_sol` rebuilt by
`approx_real_sol_reserves`), and `build_state` primes the reserve and the
meaningful-trade clock together, so a primed trade hashes like a live one and a
seeded token reads dead or alive as it would live. Why both halves are load-bearing:
[../plans/strategies/restart-state-restoration.md](../plans/strategies/restart-state-restoration.md).

**Boot recovery is bounded at both ends — never read the corpus.** `recover_armed`
needs only the last `MAX_SNIPE_AGE_SECS` (30 s) of events, and must stay O(that),
not O(log size): `recent_log_files` skips files whose date is wholly older than the
window, and `read_log_tail` reads each kept file **backwards** in 1 MiB chunks,
stopping at the first event older than it. A scan margin (`RECOVERY_SCAN_MARGIN_SECS`,
5 min) covers the fact that `at()` is *chain* time, so append order is only
approximately time-ordered and a settling fill must be seen before its mint is
re-armed. Retention is enforced by **bytes** (`EVENT_LOG_MAX_BYTES`, default 6 GiB,
oldest-evicted-first) as well as days — daily volume swings 5× (4.3 GB → 0.87 GB
across three days), so `EVENT_LOG_RETENTION_DAYS` alone cannot bound the directory.

**The day is split into segments, and only the OPEN one is un-evictable.** Files roll
on size (`EVENT_LOG_SEGMENT_BYTES`, default 256 MiB) as well as at midnight, named
`events-YYYY-MM-DD.NN.jsonl` — a day's first segment keeps the legacy un-suffixed
`events-YYYY-MM-DD.jsonl`, so old directories parse unchanged and sort first. The
name is parsed in ONE place, `hunter_engine::event_log::parse_log_file_name`, shared
by the recorder's prune/recovery scan and the lab inspector's `read_logs`; the sort
key is `(date, seq)`.

Segmentation exists because a byte cap can only be enforced by deleting something,
and the file open for append can never be deleted. With one file per day, "today's
file" *was* "the open file", so a day that outgrew the budget by itself left `prune`
nothing it was allowed to evict — it deleted every other file, reached today's,
stopped, and the directory grew unbounded (measured **11 GB against a 6 GiB cap**).
Two properties, both load-bearing:
`prune` guards the **open path** rather than "is it today", and it runs on **every**
rotation instead of only at the date change. Recovery is unaffected — the segment
size is ~17× the 5.5-minute recovery window, so a boot scan stays inside one file,
and `recover_armed` walks segments in order when it doesn't.

> **Boot recovery is bounded at both ends.** Recovery reads a *tail*, never a corpus: an
> unbounded front-to-back scan starves the 2-worker runtime, `DbWriter` stops landing
> flushes, and the ingest watchdog then force-exits the process **mid-recovery** — which
> turns a slow boot into an unbreakable crash loop rather than a recovery. Three guards
> keep that chain broken and none is optional: the bounded scan above, the watchdog
> `BootGate` (it must not police a booting process), and a **loud** shed warning in
> `consumer.rs` — a silent `try_send`-and-drop on the ping path makes the whole failure
> invisible from outside.

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
A sibling's bot exit queues behind the one in flight (FIFO, bounded by the holder's
worst case) and sends the moment it finishes. After a leader sell clears the wallet
mint net (PG), siblings are booked `ExternallyCleared` / End — no parallel sell
fan-out.

## Console close + externally-cleared reconcile

`POST …/positions/{id}/close?action=retry|dump|writeoff|verify[&sell_bps=N]` (per-status legality
matrix — see [position-lifecycle.md](position-lifecycle.md) §3):

1. Registry hit (Holding) → `manual_close(portion)` (engine `ManualClose`, SSE lifecycle).
   Optional `sell_bps` in `1..=9900` ⇒ partial (`Portion::BpsOfInitial`); omit / `10000` ⇒ Sell ALL.
   Partials reuse the partial-sell fill path (Holding preserved, `sold_bps` advances).
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

## Flow (`m_flow`)

Money moving: `buy_sol` / `sell_sol` / `net_sol` / `gross_sol`, the print counts
`buy_count` / `sell_count` / `trade_count` (every leg), and `buy_share_pct` (window only).
Untagged over the life it is two running counters on `TokenTrack`; over a span, a ring
buffer. Use the life for maturity / critical-mass gates, a window for hot-right-now;
`m_crowd.unique_wallets` is the how-many-people companion (it needs the wallet column).
With a tag the same quantities count one half of the split, and add `tag_share_pct` and
the transaction counts `buy_tx_count` / `sell_tx_count` (leg 0 only; `TradeLite::leg_index`
is what makes them possible, every adapter fills it from its own column).

**The slice reads** (`slice_trade_share_pct`, `slice_sol_share_pct`, written
`[30s, slice 2s]`) are the share of the span's trades / SOL that landed in a shorter
window nested at its end, PERCENT 0-100. The slice counts in the span's unit, takes the
span's lag, and may not be wider than it (`Span::parse`). They own **no state**: both
readings are the coin's own flow buffers, which `Buffers` registers for each axis. The
slice is part of the read's identity (`MetricRef`), so two reads differing only in the
slice never collide in the blocker / monotonic-kill maps or the sweep's columns, and the
exit label carries both (`[30s, slice 2s]`). Formulas, monotonic flags and the
young-token reading: [`plans/strategies/_!___metrics.md`](../plans/strategies/_!___metrics.md).

## Launch size is an axis

Total buy SOL in the token's creation slot is the fingerprint axis
`first_slot_buy_lamports`, and only that — no `m_state` metric mirrors it. A fact fixed
by the creation slot selects WHICH tokens arm, never when a rule fires, so it belongs to
one vocabulary; a threshold like `>= 6.41 SOL` is the open range
`{"kind": "range", "min": "6410000000"}`. The axis is deferred: the arm waits at
`PendingFirstSlot` until `FirstSlotSettled`, because the number is summed from that
slot's trades and does not exist at birth.

## Fingerprint axes (`hunter_engine::fingerprint::axis`)

A fingerprint is a `wildcard` flag or a **criteria map**: one predicate per configured
axis. A predicate is an inclusive integer `Range { min, max }` (exact match is the
degenerate `min == max`) or an ordered `Sequence` of instruction labels. An axis absent
from the map is not part of identity; a configured axis whose observed value is unknown
**fails**, so an unscreened token never arms a rule.

Everything derives from the `AXES` registry — the matcher loop, the criterion guards, the
auto-name and its grammar, validation, the dashboard's SQL mirror, the sweep partition,
and the UI form — so **adding an axis is one `AxisDef` plus one reader arm**. The
frontend keeps a mirror of that table, locked to it by a guard test that reads the Rust
source directly, so the two lists cannot drift.

Identity is **integer**: lamports, compute units and tallies, carried as `u128` in memory
and as decimal strings on the wire. No `f64` reaches a match, so there is no boundary
epsilon and no value that stops being representable past 2^53 (`max_sol_cost = u64::MAX`
is real launch data). SOL exists only at the display edge.

Two axes are not `tokens` columns. `ix_count` is derived (`ix_labels.len()`), so the two
can never disagree about one transaction. `prior_launches` is the engine's own tally —
`EngineState.creator_launches`, keyed by `creator_wallet_hash`, read strictly before its
own increment at `TokenCreated` and stamped onto the observed axes there, before the match
runs. It is primed from `TokenRepository::creator_launch_counts` over a trailing 30-day
window so a fresh process does not read every creator as new; an unknown creator leaves it
`None`, which fails a configured axis rather than reading as a first launch. The lake
corpus carries no creator column, so a sweep leaves it `None` too. The dashboard's SQL
mirror counts the same window off `tokens`.

Design + rationale:
[fingerprint-ranges.md](../plans/strategies/fingerprint-ranges.md).

## Tag authoring (lab)

A tag is a list, not a meaning: `volume` (the dev's volume-making trades), `dump` (the
dev's dump sells), `targets` (wallets to copy) and `working` (tool templates) are the same
thing with different matchers ([the rule model](#the-rule-model-enter-signals-always-stages)).
Tags live on the fingerprint (`fingerprints.tags`), never on the rule, and a series
column of a tagged read is `SeriesColumn::tagged(read, fingerprint)`.
`POST /api/strategies/flow-discovery` scores ix-structures per sweep `GroupKey`; its bind
writes the chosen ix shapes into one named tag's `match.ix_shape` on the find-or-created
fingerprint, keeping every other tag and that tag's other options (a new tag takes the
posted `side`), validated before the write.
`POST /api/strategies/rule-search` fills registry roles for one fingerprint and
datetime range and boards a champion `RuleParams` (Promote → inactive paper).
Governing workflow:
[`_!___strategy.md`](../plans/strategies/_!___strategy.md).

## Two-phase first-slot fingerprint gate

A fingerprint axis whose data settles after `TokenCreated` (`first_slot_{buy,sell}_
lamports` — the SOL summed across the creation slot's trades) can't match
synchronously. Instant axes match on `TokenCreated` (`MatchPhase::Instant`); a rule
with a first-slot axis arms `PendingFirstSlot` and resolves on `FirstSlotSettled`
(fired when the creation slot closes). No hot-path sleep/poll.

**The creation slot closes on a feed-wide slot watermark, not on the token's own next
trade.** `Producer` tracks the highest slot seen on any drained trade; the 200 ms tick
calls `Producer::settle_ready`, which settles every pending creation slot the watermark
has passed. The feed delivers per block, so seeing slot `S+1` anywhere proves every
slot-`S` trade has already arrived. Waiting for a later-slot trade *on the token itself*
instead costs however many slots the launch stays quiet — on a bundled launch that is
several slots of price movement, and the measured cost is the entire edge
(+0.48% vs +3.97% per trade on the same rule when settle waited on the token's own
next print). `Producer::on_trade` keeps the same-mint path as the fallback
for a token this process never saw created (adopted or log-re-armed), and the two paths
share one `settle_first_slot` body, so a mint settles exactly once whichever fires first.
The snipe-freshness rail applies to both — a restart never re-settles a slot that closed
hours ago. `replay.rs` mirrors it by stamping the settle at the **last creation-slot
trade**, so simulate and live resolve at the same point.

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
  `pumpfun_impact`). No selectable kind charges flat slippage, so no pairing can
  double-count it: impact is our own footprint on the curve, orthogonal to which
  print the fill model picks, and a live trade pays both.
- **`strategies/engine_sim.rs`** + **`api/handlers/strategies/engine.rs`** —
  `POST /api/strategies/simulate` (`rule_id` OR inline `draft`, `fill_model` +
  `cost_model`); reuses the fingerprint candidate scan + the analysis-cache
  single-flight; results served by the strategy-agnostic `positions::
  sim_result_{page,summary}`. Both pricing knobs persist on the saved-rule's
  `SimMeta` (`state/sim_results.rs`) and surface as the Simulate table's Fill/Cost
  columns, so a stored result always shows what it was priced under, and
  `SimMeta.rule_format` (`RULE_FORMAT_VERSION`) stamps the rule format: a cached result
  of an older format is not loaded, so its rule re-simulates. Loads `with_flow` (the
  wallet and `ix_labels` columns) when a read needs wallet identity or ix labels; a
  dry-run classifies with the rule's fingerprint `tags`.
- **`strategies/flow_discovery.rs`** + **`api/handlers/strategies/flow_discovery.rs`** —
  lab-only job: score trade ix-structures per fingerprint group -> bind them into a
  fingerprint tag (mutual `409` with sweep / metric-discovery / rule-search).
- **`sweep/generic/`** — the precompute-then-scan grouped sweep. `GenericSweepStrategy`
  implements the existing `sweep::strategy::Strategy` trait (so partition / two-phase
  pool / `GroupSink` persistence / refine / `ComboAgg` and the whole
  `start_grouped_sweep` handler are reused); only the per-combo simulation is
  replaced with a scan over precomputed `MetricSeries` that runs the engine's own
  `CompiledRule::try_enter` and `held_line` over each row (`CoinReads` for a series row),
  and the same `kernel` cost. `sweep/generic/guard.rs` asserts the scan is identical to a
  single-token `run_replay` (the real fold). Registry id `"generic"`, tables
  `grouped_sweep_*`.
- **Deadness:** sim/sweep book `Dead` (not `Open`) for a silent-death token at its
  death point, via the shared verdict — the same one the live fold uses. A dead
  **real** pool has no liquidity to sell into.
- **Wallet episode ledger (`core/src/strategies/wallet_ledger.rs`)** — Trader Analysis'
  one PnL source for a studied wallet. `wallet_episodes` folds one wallet's transactions
  on one mint (legs collapsed per transaction, tape order) into round trips: an episode
  opens on a transaction while none is held and closes on the sell that leaves at most
  `1 / DUST_DIVISOR` (0.1 %) of what it bought above what it came in with; a later sale
  of that dust joins it without moving its exit. Its SOL is each transaction's payer net
  flow (`trades.payer_net_lamports`), read by `TradeRepo::wallet_txs_on` only when the
  wallet paid and no other mint or wallet shares the transaction, otherwise `None` —
  there is no curve-side fallback. Status: `Closed` (sold down, every flow exact — the
  only one with `net_sol` / `pnl_pct`), `Open` (still holding; `mark_sol` is what the bag
  sells for now, `open_pnl_sol` adds the SOL moved so far — an estimate), `Incomplete` (`missing_flow`, or `unseen_buy`:
  it sold more than the ledger saw bought — consecutive such sells join one episode).
  The lab handler folds each mint from `EPISODE_LOOKBACK_DAYS` (30, the `trades`
  retention) before the window and keeps the episodes that close inside it plus the
  open one. The open mark (`wallet_ledger::open_mark_sol`) sells the held bag into the
  pool of the mint's newest priced trade (`TradeRepo::latest_pools`) through
  `CostModel::venue_only` — venue fee and impact, not the wallet's own transaction
  cost. The stored trades carry no per-swap PumpSwap fee, so a migrated pool marks at
  the curve fee. An episode with a missing flow keeps `held x spot`. A transfer out is invisible to `trades`, so tokens that left that way read
  as an open episode, which no PnL figure sums.

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
which silently drops every such `Trade` line from recovery/replay with `WARN
event log: skipping unparseable line ...: invalid type: null, expected f64`.
Any future non-`Option` `f64` field that can legitimately be
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
   silent still advances to `now` so stall/age/held/stage clocks, stage deadlines,
   decayed-flow conditions and the dead
   verdict fire. Price TP/SL still fire on Trade events (no tick wait).
5. **A tick may be skipped, a decision may not.** `Tick` skips `Settled` tokens
   (above). Anything that mutates a tracked token *outside* the evaluate sweep must
   drop that verdict — `TokenState::unsettle()` inside the fold,
   `EngineState::touch_token` from live's boot adoption — and any new cross-token
   input a decision reads must bump `cross_epoch` (an input read only at fold time,
   stamped onto a creation or a print like the launch-build stats and the build
   breadth, changes no tracked reading and needs none). A new metric that can move on a
   bare tick needs a matching `ClockHorizons` field, or it will be skipped past.
   `dense_ticks` disables the whole thing if you need to bisect.
6. **ONE serialized decision loop** — every `reduce` happens in `decision_loop.rs`;
   no mint sharding, no interleaved position transitions.
7. **Live-rule edit guard** — `fingerprint_id` is frozen post-create (PUT
   ignores it); `trade_mode` is editable via PUT but the editor locks it
   behind an unlock control. The rule's params lock in the UI while the rule
   is active. Entry dispatch + sells both route off the position's
   snapshotted `trade_mode`, never a mid-retry rule flip. That lock is also what
   makes the condition **park toggle** (`"off": true`) an *authoring* feature
   and not a live A/B knob: a live rule's conditions are frozen, because its
   run's `params_snapshot` has to keep matching the positions it produced.
   Toggling a condition off is a rule edit like any other — park, simulate,
   compare, then promote. What the UI does not lock — sizing, caps, the fingerprint
   behind the rule — still lands mid-run, and the run is stamped for it (below).

## Run lifecycle (what "current run" vs "history" actually splits on)

A `strategy_runs` row is **one activation of a rule**, not the rule's whole life.
The sink owns both ends of it and `strategy_rules.is_active` is the only input —
carried onto `LoadedRule::entry_enabled`, the same flag the arm gate reads, so
"may own a run" and "may take an entry" cannot disagree.

| Event | What happens |
| --- | --- |
| Rule becomes active (reload) | `warm_runs` → `ensure_run` reuses the latest still-`Running` run (restart continuity) else mints `run_seq + 1` |
| Rule stops being active (reload) | `close_stale_runs` evicts the cache entry and backgrounds `StrategyRepo::finalize_run` → metrics rollup + `status='Stopped'` |
| Rule's `trade_mode` is edited | same path — a run belongs to exactly one mode (`run_seq` is monotonic per `(rule, mode)`), so the old-mode run ends and the new mode opens its own |
| Any OTHER edit lands while active | the run is **kept and stamped**: `ensure_run` diffs the new [`RunConfigSig`](#a-run-says-what-config-it-is-running-under) against the cached one and appends `{at, changed[]}` to `strategy_runs.config_edits` |
| The activation caught nothing | the empty run is **deleted**, not kept — no empty "Run #N" in front of the real bag, and the `run_seq` is freed |
| A straggler of a finalized run settles | `reroll_draining_run` re-rolls that run's metrics (a run closed mid-drain has provisional numbers) |
| Boot | `close_orphan_runs` finalizes runs left `Running` by a deactivation this process never witnessed; `load_draining_runs` rebuilds the re-roll set |

Every deactivation path (pause, disable, per-rule Stop, Stop All, delete) already
ends in `schedule_engine_reload`, so the reload hook covers all of them — there is
no per-handler run bookkeeping to keep in sync.

Consequences worth knowing:

- **Metrics are written at finalize, not continuously.** A `Running` run has no
  `strategy_run_metrics` row, which is what `RuleRunListRow::has_metrics` reports —
  the Evidence pane shows a status for the current run and real PnL for prior ones.
  If a finalized run's membership changes afterwards, `hunter-lab -- reroll-run
  <uuid>` recomputes it through the same kernel (it refuses a `Running` run, since
  that would advertise a live activation as a settled result).
- **`?score_scope=current` resets on re-activation** — `rule_counters_for_latest_runs`
  scopes to the newest run, which is now the new one. That is the point of the
  scope; all-time counters are the other chip.
- **The run cache is keyed by rule *and mode*** (`CachedRun`). It was rule-only,
  which handed a paper position the id of the rule's *real* run whenever a
  `trade_mode` edit landed — 17 rows in the local DB ended up with
  `strategy_positions.mode <> strategy_runs.mode`, mis-scoping every run-scoped read
  and hiding real-money positions from the real scoreboard.
- **Positions never migrate between runs.** A position keeps the `run_id` it was
  born with (the registry carries it), so pausing a rule with open bags leaves them
  reporting into the run that opened them, and re-activating opens a fresh run
  beside them.
- **Two guards exist because the writes are backgrounded** (the rollup reads every
  position of the run, and this runs on the serialized decision loop): `closed_runs`
  (RAM) stops a fast pause→activate from re-adopting a run whose `Stopped` write has
  not landed; and a run is never deleted while the engine still holds positions for
  it, because a `BuySubmitted` insert naming that `run_id` may still be in flight and
  would fail its FK.

### A run says what config it is running under

A rule edit does **not** restart the run. `schedule_engine_reload` → `reload_rules`
folds `RulesReloaded`, the fold decides on the new config from that instant, and the
open positions stay where they are — rotating the run mid-flight would split one
rule's bags across two of them, which is a worse lie than one run spanning two
configs. So the run is **stamped** instead, and every surface that shows its numbers
can say so.

`live::strategies::engine::run_config::RunConfigSig` is the diffable digest: one FNV
hash per part — `params`, `buy size`, `caps`, `identity`, `ix structure` — so the
stamp names *what* moved, not just that something did. `Sink::set_rules` computes it
per rule on every reload; `ensure_run` compares it against `CachedRun::config` (or,
adopting across a restart, against the row's own `config_hash`, which is the parts
concatenated so it still diffs part-wise) and writes through
`StrategyRepo::record_run_config`. A failed write warns and the cache still advances:
the marker is display, and losing one entry must not retry on the buy path.

Two things make this the only trace of the edit that matters most:

- **`params_snapshot` cannot say it.** It is written once at launch and describes the
  config the run *started* with — which stops being true the moment the rule is
  edited, without anything on the row changing.
- **A fingerprint edit touches no rule row at all.** Its `tags` and the
  identity axes live on `fingerprints`, and one fingerprint is shared by every rule
  pointing at it — so an ix-structure edit re-defines several live rules at once and
  never appears in any `params_snapshot`. `RunConfigSig` hashes the fingerprint, so
  each of those runs is stamped.

What is deliberately **not** a config change: `rule_name` and the rule's `tags` (Rules-board labels the
kernel never reads — a rename that cried wolf would teach the operator to ignore the
mark), and `trade_mode`, which already mints its own run one row above.

Reading it: `strategy_runs.config_edits` rides on the run navigator rows
(`RuleRunListRow`) and on the Rules board via `running_run_config_edits`, which only
reports **`Running`** runs with a non-empty log — a finished run cannot answer "is
this still scoring under the config it started with", and an unedited one answers by
absence. The log keeps the newest `MAX_RUN_CONFIG_EDITS`; it is a marker, not an
audit trail.

**A pre-0012 run has no baseline.** Its `config_hash` is NULL, so the first reload
after deploy *observes* the live config onto the row without dating an edit —
inventing one would report a change that may never have happened.

Rollup arithmetic is not re-derived: `strategies::run_rollup` maps a PG position
onto the kernel's `TokenOutcome` and folds through the same `exact_run_metrics` the
sweep and simulate use, so a finished run compares to a backtest of the same rule.
The one PG-specific decision is `ExitCode::from_closed_reason`: a row known to be
closed never buckets to `Open`, because `RunAgg` splits realized from unrealized on
exactly that test and a `Manual`/unknown label would otherwise drop a settled
trade's PnL out of every realized figure.

## Persistence

Generic rules → `strategy_rules` (`RuleRepo`); fingerprints → `fingerprints`
(`FingerprintRepo`); runs / run metrics / positions → `strategy_runs` /
`strategy_run_metrics` / `strategy_positions` (`StrategyRepo`). See
[@arch/database.md](database.md). There is no `strategy_rules_legacy` table and no
per-strategy `*_grouped_sweep_*` family.
