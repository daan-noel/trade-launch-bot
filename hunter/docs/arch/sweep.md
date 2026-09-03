# Sweep — strategy-agnostic param-sweep engine

File-level map of `hunter/lab/src/sweep/`. The generic param-sweep & backtest stack.
Related: [@arch/strategies.md](@arch/strategies.md) (pure fns the sweep reuses), [@arch/database.md](@arch/database.md) (sweep tables), [@arch/frontend.md](@arch/frontend.md) (sweep page).
Deep-dive detail: `@plans/sweep/sweep-engine-detail.md` (engine internals + metrics reference), `@plans/sweep/ram-sizing.md` (RAM ladder + measured perf), `@plans/sweep/sim-parity.md` (sweep↔simulate divergences).

## Core idea

A backtest is a pure fn `simulate(trades, params) -> TokenOutcome`. A sweep loads the corpus **once**, calls `simulate` over combos × tokens in memory (no DB in the loop), folds per-(combo, token) outcomes into one ranked metrics row per combo. Engine/aggregate/grouping layers are strategy-blind — a new strategy adds only a `Strategy`/`ParamSpace` impl.

Two sweep shapes:

- **Flat sweep** (`run_sweep`) — one global ranked table. No longer exposed standalone; it is the per-group primitive the grouped sweep calls.
- **Grouped sweep** (`run_grouped_sweep`) — the primary entry point. Partition corpus by a **fingerprint key** (discrete fields exact-value; continuous SOL amounts binned into fixed-width ranges), run flat sweep per group, surface each group's best combo.

Decision parity: a strategy's `simulate` calls the same pure fns the live path uses.

## The sweep is an approximate ranking tool (deliberate)

**The sweep ranks candidates; `simulate` is the authority on any single combo's PnL.**
Several divergences from `simulate`/live are intentional trades of fidelity for speed —
they are listed here so a future session does not "fix" them:

| Divergence | Why it exists |
| --- | --- |
| **Tail-cap asymmetry.** Sweep caps a token's series at `last_trade + DEAD_QUIET + TAIL_MARGIN`; replay caps at the corpus-wide last trade. A quiet-but-liquid token therefore reads `Open` in sweep and closed `Metrics` in simulate. | Per-token tails keep the series short; a corpus-wide horizon would extend every token to the newest trade in the run |
| **Concurrency caps stripped.** Sweep runs `max_concurrent_tokens: u32::MAX`, so `n_fired` / `total_pnl_sol` are **upper bounds** vs. what a live rule under its own caps would achieve. | Caps make token outcomes order-dependent, which would serialize the rayon token fan-out |
| **Sketched quantiles.** Persisted quantiles come from a 64-bucket DDSketch (~15% rel. error); `simulate` and the sweep drill-in compute exact ones. **Ranking is unaffected** — `score` is exact. | O(1) memory per combo; exact quantiles would need every per-token value retained |
| **`pnl_percent` is not notional-invariant.** `fixed_cost_sol_per_leg` does not scale with trade size, so PnL% is only comparable across runs at the *same* `buy_amount_sol`. | The notional *chain* itself is consistent; this is the residual of a fixed per-leg cost, not a bug |

Ranked full list of sweep↔simulate divergences, including the not-yet-accepted ones:
[../plans/sweep/sim-parity.md](../plans/sweep/sim-parity.md).

## `hunter/lab/src/sweep/` (generic)

| File | Owns |
| --- | --- |
| `mod.rs` | Module map |
| `strategy.rs` | `Strategy` + `ParamSpace` traits; `SweepMethod` (Grid/Random/LHS/Refine); the **two-stage entry** (`entry_key` → `entry_candidates` shared per class → `resolve_entry_from` per combo) plus `prepare_token`, `build_exit_ctx`/`exit_ctx_key`, `resolve_exit`. **Re-exports** `CostModel`/`round_trip_with_costs`/`ExitCode` from `trading_core::strategies::kernel` — the sweep keeps no second copy of the cost/exit math |
| `projection.rs` | `SweepTrade` — slim per-trade row (wallet interned to `u32`); `WalletInterner`; `project_trades` |
| `corpus.rs` | `CorpusSource` trait + the corpus model (`TokenTrades`, `Corpus`, slim `SweepTrade` projection via `from_trades`); `Selection` (cap + time window + curve_only); `sweep_per_mint_cap` — **uncapped by default** (`SWEEP_DEFAULT_PER_MINT_CAP = i64::MAX`): analysis runs over each token's **entire** history, so the grouped sweep matches single-rule simulate for high-volume tokens. `SWEEP_PER_MINT_CAP` (≥1) is an opt-in bound to cut corpus weight (`tokens × trades/token`) for a lighter run. The sole impl is `LakeSource` (see `lake/duck.rs`) — there is no PG corpus source |
| `engine.rs` | `run_sweep` — rayon over tokens; per-token reuse of the entry-**candidate** walk (never a resolved entry — see below); `SweepObserver` cancel; buffer recycling |
| `progress.rs` | `SweepObserver` trait; `SweepProgress` (phase-tagged SSE); `NoopObserver` |
| `aggregate.rs` | `ComboAgg` (a thin wrapper over the core kernel's `RunAgg`) → `ComboMetrics` (= core `RunMetrics` + `combo_id`, via `from_run`). O(1) per combo via the core `QuantileSketch` (~0.6 KB, ~15% rel. error for median/p90) — the sketch/robust-score/exit-index math lives once in `trading_core::strategies::kernel` |
| `retention.rs` | `retained_combo_ids` — keeps per-metric-extreme combos + best_combo (~660 rows/group max); used write-time AND at compaction |
| `grouping.rs` | `TokenFingerprint`, `GroupField`, `GroupKey`; `normalize_label_vec` (shared with corpus filter). All `GroupField`s are `tokens`-creation facts **except** `FirstSlotBuySol`/`FirstSlotSellSol` — the first trade-derived fields, sourced from `tokens_info` (creation-slot buy/sell SOL). Their lake `fp_first_slot_*` cols + `creation_stats_repo::grouped()`'s `LEFT JOIN tokens_info` + `export_tokens`' join all exist to feed them. **Group-key rendering** (`render_field`) is exact-value for discrete fields but **bins** the continuous SOL amounts (`InitialBuySol`, `MaxCostLamports`, `SpendableLamportsIn`, `FirstSlot{Buy,Sell}Sol`) into `SOL_BUCKET_WIDTH` (0.1 SOL)-wide `"lo–hi"` ranges (`bucket_sol_label`); the dashboard SQL (`creation_stats_repo::sol_bucket_sql`) mirrors it byte-for-byte so both surfaces produce identical labels. Making the width runtime-configurable is a separate, not-yet-merged per-run knob. |
| `grouped_engine.rs` | `run_grouped_sweep`; two-phase driver (large groups serial, small groups parallel); `make_group_result`; coarse→refine (`run_grouped_with_refine`); partial persistence via `GroupSink` |
| `obs.rs` | Process RSS + host RAM reads; sweep milestone clock |
| `registry.rs` | `sweep_tables(strategy_id)` (one arm: `"generic"` → `grouped_sweep_*`), `run_grouped(...)`; `MAX_COMBOS`; resource fences (`bounded_threads` = cores/2 by default, host-RAM admission) |
| `generic/` | `GenericSweepStrategy` — the one sweep family; there are no per-strategy adapters. `axes.rs` = fingerprint + TP/SL + metric-condition axes → `RuleParams` combos; `strategy.rs` = `Strategy` impl (`TokenState = MetricSeries` precompute; `resolve_entry` mirrors `can_enter` / `resolve_exit` scan the series) + `Pricing` (notional + fill model + cost model) + `ExitClass` bind-time classification and its per-class / vector row finders; `exit_index.rs` = prefix-extrema hulls answering an arbitrary monotone predicate (`first_max_row` / `first_min_row`), plus the `at`-monotonicity flag a `held` binary search needs; `guard.rs` asserts scan ≡ `run_replay` (under every `FillModel`), index/SIMD ≡ scalar, and that a TP/SL rule actually *reaches* the index |

### The entry is exit-dependent — the fold caches candidates, not entries

The engine's `can_enter` gate refuses to buy **while the exit conditions already hold**,
and `resolve_entry` mirrors it. So the resolved entry is a function of the *whole* rule,
not just the entry axes: two combos with the same `entry_key` and different exits can
legitimately enter on different rows. The fold's single-slot cache is keyed on
`entry_key`, so caching the resolved entry there makes the first combo of each class
donate its entered set to every sibling — wrong `n_fired`, wrong entry rows and prices on
any grouped sweep with **exit-side metric axes**. Mechanism, proof and blast radius:
[../plans/sweep/sim-parity.md](../plans/sweep/sim-parity.md).

The fold now runs the entry in two stages:

* **Stage A** `entry_candidates` — the exit-independent walk (dead check, mono-kills,
  entry-condition eval), opened once per `entry_key` per token and **resumed** as combos
  ask for deeper candidates, so the short-circuit at the first admissible row survives.
* **Stage B** `resolve_entry_from` — per combo: walk the shared candidates applying that
  combo's veto, then price the first admissible row through a per-class fill memo.

Pure TP/SL sweeps (the 1M-combo shape) are untaxed: their exit reqs are position-scoped,
read `NaN` before entry, and so can never veto (`BoundCombo::entry_veto_possible`), making
Stage B a candidate lookup plus a memo hit. `ExitCtx` (the prefix-extrema hulls) is now
rebuilt on `exit_ctx_key` — the resolved `fill_row` — not on entry-key staleness.
Locked by `guard::fold_gives_each_exit_variant_its_own_entry` (fold ≡ per-combo `scan` ≡
`run_replay`, both combo orders) and `engine::tests::fold_reresolves_entry_per_exit_variant_within_one_class`.

**Operational caveat — stored runs.** Any grouped run with exit-side metric axes recorded
before 2026-07-26 carries poisoned aggregates for every combo that was not first in its <!-- pt-ok: cutoff, those runs are still in the DB -->
entry class. **Re-run them, and expect the crown to move.** Incident:
[`@history/2026-07-26-sweep-entry-cache-poisoning.md`](../history/2026-07-26-sweep-entry-cache-poisoning.md).

### Corpus scope: saved fingerprint vs manual filters

Two mutually-exclusive ways the start request narrows the loaded corpus, and they narrow at **different stages**. `fingerprint_id` resolves to an explicit `Selection::mints` *before* the lake load (rule 1 of the lake read discipline below), so the scope is part of the corpus identity and therefore part of the `sweep_corpus_cache` key — switching scopes costs a fresh Parquet read. The manual filters apply in-memory *after* the load, so one unfiltered corpus entry serves every filter value over the same selection:

- **`fingerprint_id`** (sweep form → *Group by fingerprint* → *Scope by saved fingerprint*, mirroring the Flow-discovery control) — keeps only tokens `hunter_engine::fingerprint::matches` accepts for that saved fingerprint: the **engine match SSOT**, exact axes exact and the continuous SOL axes by **bucket**, i.e. the token set the live entry gate would arm on. `group_by` still partitions *within* the matched slice (empty ⇒ one `ALL` group).
- **`ix_labels_filter` + `field_filters`** — the manual path: exact **ordered-sequence** label match (same on-chain order, same repeated labels, same length — the axis semantics `hunter_engine::fingerprint::matches` grades on, so `normalize_label_vec` never sorts or de-duplicates) and exact-value field pins. These cannot express a bucket axis, which is why scoping is a separate request field rather than a filter prefill. Ignored (and stored as `NULL`) whenever `fingerprint_id` is set.

  **`field_filters` values are parsed by the one shared `hunter_engine::grouping::parse_filter`**, which returns an `AxisPredicate` — the same type a fingerprint stores and the matcher grades. So the filter box, the group key and the live match speak one vocabulary by construction, not by three implementations agreeing. Three forms, read in the axis's own unit (human SOL for a lamports axis, the integer otherwise): a value (`"1.515"`), a half-open window (`"1.5-1.6"`, the span a chip shows, so a chip pastes straight into the box), or a bound (`">=1.5"`). Consumers: the resident-corpus retain `grouped_sweep::matches_field_filter`, the dashboard SQL `creation_stats_repo::field_filter_pred`, and the frontend's `buildFieldFilters` — all three now call the one parser rather than mirroring it.

  **Partitioning is a `PartitionSpec` per field**, separate from the filter: `Distinct` (one group per value — the default, and the answer to "what are the most common exact dev-buy sizes?") or `Ranges { edges }`, an explicit ascending list. Edge `i` opens the window `[edges[i], edges[i+1] - 1]`, open-ended below the first and above the last, so the edges tile the whole domain and no token is dropped.

  There is **no width**. A width is an infinite implicit lattice (`floor(v/w)`) that the sweep, the matcher and the dashboard SQL each had to re-derive identically down to a `1e-9` boundary epsilon, and whose `0` is a division by zero. A finite edge list travels with the run and means the same thing to everyone who reads it.

  **A group key carries predicates, not rendered labels.** `GroupValue::Window { min, max }` IS the predicate a promoted fingerprint stores, so promote is a copy — there is no `"lo–hi"` string to parse back, and the byte-identical-label lockstep between `render_field` and the SQL `to_char` masks is gone with it. Every card is promotable that names a criterion, the `max_sol_cost = u64::MAX` ceiling included: bounds are decimal strings over the full `u64` domain, so the value that no `BIGINT` axis could hold is now an ordinary bound.

**A group's selection is resolved ONCE, by `lab/src/sweep/selection.rs`** (`GroupSelection`), and every consumer reads that: promote materializes it into a fingerprint, `GET …/groups` serializes it onto each row so the card renders the real predicate (and the frontend compares the emitted `identity` instead of rebuilding one in TypeScript). A group's tokens are `window ∧ (scope fingerprint | run filters) ∧ group_key@precision`; each of those lives on the run or group row, so the resolver derives — never persists — a second copy, and legacy runs resolve identically.

**Why it must be resolved once, and centrally.** Re-deriving the selection from `group_key` alone loses clauses, and it loses them in the **widening** direction every time — `field_filters` get dropped, a scoped run that also groups by extra fields promotes the *scope* rather than the group, a `∅` key drops the axis (so the rule matches tokens that HAVE a value), and `token_program_id` / `is_cashback_enabled` have no axis to land in. Every one of those ships a rule that arms on a **superset** of the tokens whose numbers justified it. Detail: [`@history/2026-08-04-group-key-unit-drift.md`](../history/2026-08-04-group-key-unit-drift.md).

**Promote fails closed.** Three clause kinds still have no fingerprint expression: an **absent** (`∅`) axis (a fingerprint spells "unset" as "unconstrained", which matches tokens that HAVE a value — the opposite population), a **multi-value** filter (a range cannot express a disjunction), and the two **grouping-only** fields the matcher has no axis for. `materialize` returns those clauses and the endpoint answers **400 naming them** — it never drops a clause to produce a wider gate. The groups response carries the same verdict (`promotable` + `blockers`) so the card can say so before the click.

Nothing else blocks it. A per-axis predicate over a `u128` domain has no row-wide width to reconcile between two axes, no anchor to re-derive from a rendered label, and no value it cannot hold — so the promote path has three fewer failure modes than it has special cases. <!-- pt-ok: none -->

The scope is persisted on the run row (`grouped_sweep_runs.fingerprint_id`, `lab/migrations/0001_init.sql`, no FK) because it is not reconstructible from the filter columns: the token-results reload re-applies the same match and re-run restores it in the form. Promote reuses the **saved row itself** when the group is the whole scope unnarrowed (`is_scope_only` — materializing would re-anchor a bucketed axis on its bucket's lower edge, match-identical but a different `find_or_create` identity, i.e. a duplicate); a group that narrowed the scope promotes to its own narrower fingerprint, carrying the scope's `metric_config`. A newly materialized row gets `Fingerprint::auto_name` (`3ix:Buy · max=1 · bkt=1`); `find_or_create` keeps an existing nickname. Detail: [fingerprint-auto-name.md](../plans/strategies/fingerprint-auto-name.md).

### Metric scope: token-scoped columns vs position-scoped axes

Almost every metric is **token-scoped** — one value per token per event, so the
precompute records it as a `SeriesColumn` and the per-combo scan reads it off the
flat buffer. `m_position` (`retrace` / `pnl` / `held`) is **position-scoped**: its
value anchors on *your* entry fill, so it cannot be precomputed token-independently
(a static column would only ever record `NaN` — the track holds no position state).

Consequences, all enforced in code:

* `axes.rs` rejects an `m_position` axis on the **entry** side (it reads `NaN`
  before entry, so the condition could never fire) and contributes **no column**
  for it on the exit side.
* `strategy.rs::resolve_exit` carries a running since-entry peak/trough (seeded to
  the fill price, folded forward per row *before* that row's decision — mirroring
  `reduce.rs::evaluate_token`) and evaluates position-scoped exit reqs through
  `position_value(..)` against that `PositionCtx`, exactly as
  `CompiledRule::exit_fired` does. Token-scoped exit reqs still read their column.
* The desugared TP/SL `pnl` reqs (`ReqOrigin::{TakeProfit,StopLoss}`) go through that
  **same** walk. The scan must not re-derive them as an `entry_price · (1 ∓ pct/100)`
  price branch — that is a second representation of a fact the engine already
  desugars, and it compares in *price* space where the fold compares in *pnl* space.
  The exit label comes from the fired req's `ReqOrigin`, and priority
  (`Dead > SL > TP > authored`) is carried by `exit_reqs` **order**: `compile`
  prepends SL then TP. `CompiledRule::{take_profit,stop_loss}` survive as the
  authoring / DB / FE surface; only the sweep's *evaluation* of them is gone.
* `m_price_window` **is** token-scoped, so `trail`/`rise` precompute as an ordinary
  `SeriesColumn::Window` — routed to the price-extrema deque (`ensure_price_window`),
  not the flow ring buffer. Its window counts toward `SparseGrid::max_window_secs`:
  a rolling high decays as prints age out, so the decay-region ticks must be emitted
  exactly like a flow window's.

### The sparse tick grid lives in `hunter-engine`, not here

`SparseGrid` / `fold_sparse` / `estimate_sparse_rows` moved to
`hunter_engine::metrics::grid`; `generic/strategy.rs` re-exports
`SparseGrid` and calls `fold_sparse` for its precompute. The move is not cosmetic —
it makes the tick grid the **only** way to drive a `MetricSeries`. A trade-only fold
is not a coarser view of the same series but a different one: `m_flow_window` decay,
`m_price_window` extrema, `stall`/`time` and the dead verdict all advance solely
inside `TokenTrack::on_tick`, so with no ticks they are sampled exactly where a fresh
trade has just been folded back in. The chart endpoint had that bug and drew an exit
70 s late; sharing the loop is what stops a second caller re-acquiring it. Callers
declare what they will evaluate (`max_window_secs` + the `time`/`stall` ceilings) so
the grid knows how far into each quiet gap it must stay dense. Every
`guard.rs::scan_matches_replay_*` test covers the sweep's use of it unchanged.

Not wired: **re-entry** (`RuleParams.reentry`). The sweep's `TokenOutcome` is one
episode per (token, combo); multi-episode accumulation would change the outcome
model, aggregation and persistence. Re-entry validates via simulate/replay instead. Same for **exclusivity** (`RuleParams.exclusive` / `priority`) — it needs cross-*rule* state at one instant, which the per-combo fan-out has no place to keep; recorded as divergence D4 in [../plans/sweep/sim-parity.md](../plans/sweep/sim-parity.md) and locked by a `guard.rs` test.

**Scale-out** (`RuleParams.scale_out`) **is** wired on the exit scan:
`resolve_exit` delegates to `resolve_exit_staged` when the compiled ladder is
non-empty (`Dead > global exit > stage`, multi-leg PnL via
`round_trip_multi_leg`). Cost is ×(stages+1) resolve work only for those rules;
legacy combos keep the index/SIMD fast paths (`fast_exit` requires empty
`scale_out`). Deliberate residuals D5/D6 in sim-parity (no in-flight-sell
blindness; no frozen-tail stage advance).

**Axes do not sweep stage counts.** Optional **Pass-2 overlay** (run fields
`scale_out` + `scale_out_top_k`; `scale_out` = `ExitStage[][]` on the wire for
backend forward-compat, but the FE always sends exactly **one** user-authored
ladder as its sole entry): after each group's cheap fold, `GenericSweepStrategy::
post_group_rescore` re-scores its top-K combos against that ladder PLUS each
combo's own Pass-1 baseline, and keeps whichever wins **per combo** — never
forced onto a combo it doesn't help. The winning ladder (if any) is baked
directly into that specific combo's own `_combos.params` / `best_params` at
write time (`grouped_engine::retained_combo_params`); a combo the ladder
doesn't help keeps its own exit and carries no `scale_out`. Promote / drill-in
read `params` as-is — no run-level merge at read time. FE authors the ladder
via `ScaleOutBuilder` (same stage editor as the Rule Editor) rather than
picking from canned presets: Pass 2 is meant to test a hypothesis you already
believe in against the sweep's own top-K survivors, not to blind-search a grid
of guesses — comparing many arbitrary ladders per combo on a small sample is a
multiple-comparisons trap (looks good by chance, not real edge). The staged exit
semantics themselves are in
[`../plans/strategies/partial-exits.md`](../plans/strategies/partial-exits.md).

### Flow axes (`m_flow_ix` / `m_flow_ix_window`)

When axes reference a flow group, the corpus loads with `Selection.with_flow`: it
reads the trade `ix_labels` + `wallet` columns and resolves each row's
`projection::FlowKeys { ix_hash, wallet_hash }` **at the row decode**, through the
`flow_ix` SSOT hashers, then drops the strings. The same flag also reads
`cu_limit` + `cu_price` + `tip_lamports` into that row's `FeeKeys`, because a build
list entry may pin a budget and the classifier needs both halves or it matches on the
shape alone. `duck::FLOW_READ_COLS` names all five once — the three fee columns are
`Int64` neighbours, so a reorder on either side would decode cleanly into the wrong
field. `Selection.with_flow_text` keeps
the raw text as well and is set by exactly one caller — flow *discovery*, which
reports label shapes and groups by wallet address. Everything else (sweep, simulate,
metric-series, metric-discovery) classifies from the hashes, so its rows are the
slimmer shape. The start body carries optional
`ix_patterns: string[][]` — applied **corpus-wide** for that run (not per
fingerprint). Missing patterns with flow axes ⇒ `400`. **Promote** copies the
run's patterns into the created fingerprint's
`metric_config.m_flow_ix.ix_patterns` as fee-wildcard entries — a run configures
label sequences only, and a budget pin is a fingerprint edit (`find_or_create` ignores
`metric_config` for identity, then patches). Discovery (lab
`/strategies/flow-discovery`) is a separate job that scores structures and writes
the same key — mutual `409` with sweeps. See
[`plans/strategies/metrics-reference.md`](../plans/strategies/metrics-reference.md).

## Workstation resource fences (lab-only)

Grouped sweep runs hard **inside** a reserved slice of the analysis box so the desktop stays usable. No mid-run throttle. **No env overrides** — values are auto-detected / hardcoded:

| Policy | Value |
| --- | --- |
| Rayon threads | `max(1, cores − 2)` (e.g. 14 on 16 logical CPUs) |
| RAM reserve | keep host RAM free for OS/UI. Mid-run usable RAM = `max(free − reserve, total − reserve − corpus_baseline)` (`registry::usable_from`): the structural term prices the run's own **transient** buffers as reusable so the sweep doesn't starve on its own RSS, while the corpus (measured once at `corpus_loaded`) stays priced as consumed — bounded so `baseline + usable ≤ total − reserve` (desktop reserve always free). No half-of-free degradation. **Per-run knob** — the sweep form's *RAM reserve* radio (4G/2G/1G/512M/256M) sends `ram_reserve_mb`; default **1 GB** (`DEFAULT_SWEEP_RAM_RESERVE_MB`), clamped server-side to 256 MB…32 GB. A *preference*, not a cliff: a tight reserve costs wall-clock, not the run (see below). Held in a process-global (`registry::set_ram_reserve_mb`, safe because the handler single-flights sweeps) so every sizing/shard/fold helper reads it without threading a param through the rayon paths. Run-local: not persisted on the run row (a property of the box at run time, not of the analysis) |
| **Sizing under RAM pressure** | **Degrade, don't refuse.** `registry::plan_sweep_sizing` walks a ladder — threads `N→1`, then fold budget `cap→floor` — and runs at the largest plan that fits. The plan's fold budget is installed as a *ceiling* (`FOLD_BUDGET_CEILING`), so the per-call live sizing still shrinks further if free RAM drops mid-run. Every degradation is reported (`SweepObserver::notice` → `sweep_notice` SSE → info toast) so a slow run is explained, not mysterious. The only refusal left is a **true-floor overflow**: 1 thread + 1 token's series + the minimum fold batch + the minimum shard still don't fit. See [../plans/sweep/ram-sizing.md](../plans/sweep/ram-sizing.md) |
| Series admission | `min(12 GB cap, usable)`; the flat 12 GB is the fallback when host RAM is unreadable (non-Windows/Linux) — that case is now reported as a notice rather than silently disabling the guard |
| Fold batch budget | `usable / 4` clamped to 32..=512 MB; hard max **65 536** combos/batch (8192 when under reserve) |
| Driver (large groups) | **wave-outer** when shard fits (series once/token); else **pass-outer** with disk **spill** of finalized metrics |
| Driver (small groups) | `sweep_group_serial`: **token-outer** (series built once/token, combos folded in batches over it) when the full `n_combos × ComboAgg` set fits across all workers; else **batch-outer** fallback (bounded `batch × ComboAgg`, series rebuilt once/batch). Single-batch groups are token-outer either way. The fit test reads `usable_host_bytes()`, which prices the run's **permanent** resident set (the corpus) as consumed but its own **transient** fold buffers as reusable headroom — without that, the sweep's own RSS drives `usable → 0` mid-run and token-outer never fires at all (measured 0/405 groups before the fix vs 152/405 normal and 601/601 under tight reserve after, with a 7.9× faster coarse pass on the tight run). Before/after: [../plans/sweep/ram-sizing.md](../plans/sweep/ram-sizing.md#measured-performance-2026-07-19) |
| Exit-scan path | **Bind-time req classification, then a per-class search.** `BoundCombo::new` classifies every exit req once per combo (`ExitClass`): a monotone `m_position.pnl` bound → prefix-extrema hull, **O(log n)**; a `>=` bound on `m_position.held` → binary search on `series.at`, **O(log n)**; `m_position.retrace` → running-peak scan, **O(n)** (vectorized — *not* O(log n); a running peak is not a static prefix query); `m_position.bounce` → running-trough scan, **O(n)**; anything else (token-scoped column, multi-arm DNF, `=`/`!=`) → `General`. One `General` req drops the whole rule to the scalar walk. `bound.fast_exit` = no `General`, and it — not `has_exit_metrics()` — is what gates building the index (`wants_exit_index`). The earliest row across the classified reqs wins, ties broken by `exit_reqs` order (so desugared SL > TP > authored); `Dead` outranks all. Optional **AVX-512** toggle (`resolve_exit_simd`, 8×`f64`) for A/B on the pure-`pnl`-bound shape, comparing **in pnl space** per lane (IEEE ops are exactly rounded ⇒ bit-identical to scalar, so no threshold inversion is needed); other shapes delegate to the index path. **Byte-identical** to scalar (guards `index_exit_scan_matches_scalar_*` + `simd_exit_scan_matches_scalar_across_paths`), plus a **reachability** guard (`tp_sl_rules_actually_reach_the_exit_index`) — the Phase-2 desugaring made `has_exit_metrics()` true for every TP/SL rule, which silently disabled both fast paths for a whole phase without breaking a single equality test. Money math stays the one `kernel` copy. Lab-only. **The AVX-512 toggle buys nothing against the index and stays off.** Its 2.2× (0.63 s → 0.29 s) is measured against the *linear* scalar scan, which the O(log n) index replaced as the default — head-to-head on the current default path a 4541-token × 1600-combo pure-TP/SL run is **3.2 s toggle-off vs 4.3 s toggle-on**. It survives for A/B only; in debug it is ~2.3× *slower* still. The cost that matters is the `General` fallback: the same corpus with `m_flow_ix` / `m_price_lifetime` exits takes **62 s** against 4 s, so a metric-exit rule is ~15× a TP/SL one and the scalar walk — not the vector kernel — is the optimization target. |
| Pricing (fill + cost model) | **Part of a run's identity, chosen per run.** `Pricing { buy_amount_sol, fill_model, cost }` threads from the request through `run_grouped` → `GenericSweepStrategy` → every scan fn. `fill_model` picks which trade in the window prices each leg (the same `FillModel` `ReplayConfig` threads — fill *eligibility* is identical across models, so the taken set never moves, only the price); `cost_model` picks `pumpfun_impact` (the default) vs `pumpfun_fee_only`. Both persist on the run row (migration `0010`) and the drill-in re-simulates under the run's own pair, for the same reason it re-uses the run's `as_of` (parity plan B7). **Fixed per-leg tip/priority** inside either cost model comes from process-wide `FeeTuning` (`JITO_MIN_TIP_SOL` + `CU_PRICE_MICRO_LAMPORTS` — same knobs live applies to the trader; lab installs at boot). **`NULL` ⇒ `pumpfun_impact`; an unrecognized value is a decode error, not a fallback.** No cost model charges a flat per-leg slippage: the fill model already prices execution slippage, so a flat term counts it twice, and because `fixed_cost_sol_per_leg` is per-leg that haircut scales with how often a combo fires — i.e. it is *not* rank-preserving across combos. Runs priced under the deleted flat-slippage model are **deleted, not migrated**, so no row names it; a row that somehow does fails loudly rather than reporting a model it was never computed under. Guard: `assert_parity` runs scan ≡ `run_replay` under **every** `FillModel`. |
| Sharding | large `N` split into RAM-sized combo ranges; up to 4 shards in parallel (RAM-capped); spill+merge |
| Smarter search | full `grid` with ≥200k combos and no refine → auto `lhs:50000` + refine (override with explicit `refine:` / `random:`) |
| Combo materialisation | index-only `GenericCombo { idx }`; `CompiledRule` bound per batch; combo JSON for **retained survivors only** |
| Combo-side sizing | peak priced as **one shard**, not full N; the planner prices it at the *shardable floor* (`MIN_SHARD_COMBOS` = 8192) since `plan_shards` can always cut down to that |
| Failure persistence | a failed run row is **never deleted** — `partial` when groups had already folded (they stay queryable), `failed` when none had. Groups stream to the DB as they fold, so a stop at group 380/400 keeps 380 groups |
| Horizon clamps | sparse-grid ceilings ~7d; gap tick hard-cap; `combo_count` checked mul |

Start log includes cores, threads, wave, planned/shard-peak combos, RSS, host total/available MB.

### Idle reclamation — the caches are given back when nothing is using them

The fences above bound a run's **peak**. They say nothing about what the lab holds
*after* a run, and two caches outlive their job:

| Cache | Held | Reclaimed by |
| --- | --- | --- |
| `LocalState::sweep_corpus_cache` | one whole loaded corpus (trades + fingerprints) written at the end of every sweep / flow-discovery, so drilling into a combo skips the Parquet read and `attach_fingerprints` | idle reaper, after 10 min with no read **or** under host pressure |
| `LocalState::analysis_cache` | fingerprint-scoped candidate scans + lake histories, 60 min TTL | idle reaper, every pass (`AnalysisCache::gc`) |

`hunter/lab/src/state/idle_reaper.rs` runs a 60 s pass and is gated on `LocalState::is_idle()` — no
heavy Duck job **and** no in-flight backtest. `is_idle` is the SSOT for that question:
`heavy_job_block` alone covers only the five single-flight jobs, while simulations are
gated by `backtest_sem`, so a reaper checking the heavy flags would reap out from under
a "Simulate All" batch.

**Neither reclamation can slow analysis.** `gc` sweeps only entries past `CACHE_TTL`,
which `get_candidates`/`get_histories` already refuse — dead bytes, never a hit
(locked by `gc_never_evicts_a_live_entry`). The corpus is released only after
`CORPUS_IDLE_TTL` (10 min) with no read, and any read `touch()`es it, so an actively
drilled corpus never expires. The pressure override (host available < 2 GB) exists
because a corpus that far under pressure is already resident in the pagefile: re-reading
it from Parquet is not slower than faulting it back in, and holding the slot costs the
rest of the box.

**Why it matters on a 16 GB workstation.** Without the reaper the corpus is held until
the *next* load happens to find a different hash — a lab that finished its last job kept
~8 GB of commit indefinitely, measured at 86% paged out, which the OS then pays disk I/O
to evict and fault back.

**Last-resort corpus/combo shrink knobs** (manual UI / rare opt-in; change *what* is computed) — use only when admission still fails after thread reduction:

| Knob | Where | Effect | Fidelity cost |
| --- | --- | --- | --- |
| `ram_reserve_mb` | UI (RAM reserve radio) | Raises the admission ceiling by giving the sweep more of host free RAM | None to the sim — but the box gets less headroom |
| `token_cap` | UI / `Selection` | Newest-N corpus trim (`ORDER BY created_at DESC LIMIT`); clamped to `MAX_TOKEN_CAP` = 100k, persisted post-clamp | Smaller / newer-biased sample (simulate has no cap) |
| `created_after` / `created_before` | UI / `Selection` | Smaller time slice | Misses other days |
| `min_tokens` | UI | Drops tiny groups | Fewer groups ranked |
| `max_combos` / narrower axes / `random:N` / `refine:N:K` | UI | Fewer combos | Less param coverage |
| `curve_only` | UI | Drops AMM legs | No post-migration path |
| `SWEEP_PER_MINT_CAP` | optional env (not in `.env.example`) | Caps trades/token | Breaks simulate parity |

## Persistence + API

Tables per strategy: `<strategy>_grouped_sweep_{runs,groups,combos,results}` — **lab-only**, defined in `lab/migrations/` and applied by `lab::storage::lab_migrations` (never on EC2/live; see [@arch/database.md](@arch/database.md)). Generic `grouped_sweep_repo.rs` (table-name-driven). Incremental writes: run header up front (`status='running'`), groups appended one at a time by a single DB-writer task fed over an unbounded channel, finalized on completion via `finalize_run`. Crash-recovery: `reconcile_orphaned_runs` at boot. Retention filter applied write-time so only ~660 rows/group are ever inserted.

**Corpus freshness is stamped on the run.** `grouped_sweep_runs.corpus_last_trade_at`
(lab migration `0011`) is the corpus-wide `max(block_time)` captured at corpus load —
the same instant the frozen-tail resolve anchors on (one definition:
`Corpus::last_trade_at`). The sweep is LakeSource-only while `simulate` splices the
fresh PG tail, so a stale export is otherwise invisible: the sweep freezes positions as
`Open (est)` at old prices that a simulate of the same rule watches die. The run panel
renders it as a **Data through** row next to Pricing, warning when the lake was ≥1 h
behind the run's start. `NULL` on rows written before the column existed ⇒ "unknown".

**Terminal status is honest about partial persistence.** The writer task tracks the persisted-group tally + the first write error; `finalize_run` stamps `completed` only when every folded group actually committed, else `partial` (an engine-complete run whose DB writes fell short) — distinct from `cancelled` (user abort). The reason rides the `SweepFinished` SSE frame's `error` field so the client toasts it.

**Live per-group visibility (no wait for the whole run).** Each committed group emits a `SweepGroupDone` SSE frame `{run_id, group_index, groups_done, group_count}` (plus one announce frame with `group_index: null` when the surviving counts are first known). The frontend (`BackgroundJobsContext`) throttles a per-run `GroupedSweepGroups` cache invalidation off these frames, so the groups table streams in mid-run (largest group first) and a "groups saved N/M" counter renders. Persistence is a concurrent drain, **not** a progress phase — the progress phases are strictly sequential `corpus → coarse → sweep` (`SweepProgress` observer; `corpus` = the DuckDB lake load, indeterminate `total: 0`; `coarse` = refine runs only). The old `saving` phase interleaved with `sweep` frames and made the phase bars flip-flop.

API: `POST /api/strategies/sweeps` (start, detached → 202 with `run_id`), `POST .../cancel`, `DELETE .../sweeps/{run_id}`, `PATCH .../sweeps/{run_id}` (rename), `DELETE .../sweeps?before=` (prune), `GET` for runs/groups/results.

**Prune cutoff is a UTC instant, but the UI speaks local time.** `before` is required server-side (`PruneQuery`) so the route can't wipe the whole history by accident, and `delete_runs_before` compares it to `created_at` in UTC. The "Clear runs before" picker in `GenericSweepView` therefore converts its `YYYY-MM-DD` value through **local** midnight (`localMidnightIso`), not the browser's default date-only parse — that parse yields *UTC* midnight, which would prune on a boundary offset from the local-time stamps the run picker renders. The picker is also capped at today (`max` + a re-check in `onPrune`): a future cutoff matches every run, including one still sweeping whose writer task is mid-commit against the run row. The `{deleted}` count is surfaced in the UI, since a cutoff that matched nothing otherwise leaves the page unchanged and reads as a broken button.

## Parquet lake + DuckDB corpus (Phase 4 — `lab/src/lake/`)

The 3-hop analysis pipeline that feeds the sweep on the workstation:
`EC2-PG → local-PG → Parquet lake → DuckDB`.

| Hop | What | Where |
| --- | --- | --- |
| 1 · sync | Sealed daily Timescale chunks (yesterday-and-older) pulled EC2→local PG over postgres_fdw/SSH; `wallet_dict` id-preserving, no partition loop (Timescale auto-chunks) | `scripts/db-incremental-sync.ps1` |
| 2 · lake export | Each newly-sealed local day → **immutable** `trades/dt=YYYY-MM-DD/data.parquet` (write-once, temp+rename); `tokens/tokens.parquet` dimension (fingerprint cols) rewritten each run. Streamed + row-group-flushed. Units mirror `trade_repo`'s `TradeDbRow` exactly (lamports→SOL ÷1e9; raw token f64; vsol raw→f64; `real_*` dropped). Column **names** are single-sourced in `lake/schema.rs` (writer schema references the consts; a guard test pins the writer's field order to `TRADE_WRITE_COLS`/`TOKEN_WRITE_COLS` and a by-name round-trip catches a same-typed builder swap in `finish()`) | `lab/src/lake/export.rs` (`export_lake`), `lab/src/lake/schema.rs` (column names), `lab/src/lake/mod.rs` (layout) |
| 3 · DuckDB corpus | `LakeSource: CorpusSource` (the **sole** corpus source) reads the lake via an in-memory DuckDB: candidate select + per-mint cap over the trades glob (`hive_partitioning=true`), fingerprints from the dimension → `TokenTrades`/`SweepTrade`. The trade query has **two shapes** (see *Corpus-load cost model* below): a `ROW_NUMBER` CTE when `per_mint_cap` clips, a plain scan when it doesn't. Per-mint order is `(slot, tx_index, leg_index, block_time)` — `block_time` is the final tiebreaker because the RPC backfill path leaves `tx_index`=0 (only the live LaserStream feed sets it) and `leg_index`=0 for single-leg txs, so the first three are non-unique; the 4-tuple is unique per mint, giving a deterministic total order. Uses DuckDB's **row API** only (not `query_arrow`) so its bundled arrow never clashes with lab's `arrow 53`. The export carries no `real_*_reserves`, so the loader **reconstructs** `real_sol_reserves` per row from the priced reserve pair + `venue` (`approx_real_sol_reserves`: AMM→`reserve_sol`, curve→`reserve_sol−30` clamped ≥0) so the sim's real-reserve gates (`min_liquidity_sol`, dead-token) resolve — an approximation of the live program-emitted value, not lamport-identical | `lab/src/lake/duck.rs` |

CLI: `cargo run -p hunter-lab -- lake-export` (batch job; reads `SWEEP_LAKE_DIR`, default OS-temp `pumpfun-lake`; a relative value anchors to the loaded `.env`'s dir, not the CWD — `lake_root()` via [`config::env_paths`](../../core/src/config/env_paths.rs) — so the launch folder can't point the export at a second, empty lake). Add `--include-today` to also export today's still-open UTC day (force-overwritten, non-immutable) — the only way to sweep current-day data, since the default export is sealed-days-only. Each sealed day also writes `_meta.json` (`row_count`); a later export re-seals if the sidecar is missing or mismatches PG `COUNT(*)`. One-shot current-day refresh: `./scripts/db-incremental-sync.ps1 -IncludeToday -ExportLake`. `duckdb = { features=["bundled"] }` is a lab-only dep (lab never ships to EC2).

**Cutover (DONE):** the lake is the **sole** grouped-sweep corpus source. The grouped-sweep handler always calls `LakeSource::new(lake_root()).load(sel)`, and `list_token_results` reloads from the lake when its warm in-memory cache (Option A) misses. The PG `DbSource`, `load_grouped_corpus`/`load_or_build`, the Parquet corpus-cache, and the separate `attach_fingerprints` pass are gone — `LakeSource` embeds fingerprints (`has_fingerprints`). Validated by a byte-identical lake-vs-PG metric diff (the divergence was a non-unique trade-order key; fixed with the `block_time` tiebreaker above). `SWEEP_CORPUS_SOURCE` is retired.

### Corpus-load cost model

The trades glob is the expensive object in the lab (GBs across `dt=` partitions), so
every consumer of `LakeSource::load` is bound by *how much of it the query has to
touch*. Four rules keep that bounded — the first is the one that matters:

1. **Scope before you load, never after.** A caller scoped to a saved fingerprint
   resolves it to an explicit `Selection::mints` via `LakeSource::matching_mints`,
   which reads the **tokens dimension only** (one small Parquet file) and applies the
   engine `fingerprint::matches` SSOT. Filtering *after* `load` — the old flow- and
   metric-discovery shape — reads the full trade history of every token in the window
   and then discards all but the matching handful, which was essentially the entire
   cost of a scoped run. Consequence to know: under this shape `token_cap` bounds
   **matched** tokens, not candidates, so a scoped run covers the same set the
   matched-token count chip reports.
2. **Prune partitions with the `dt` floor.** `created_after` implies a lower bound on
   the day partitions a selected token's trades can occupy (`trade_partition_floor`,
   one day of slack), so the glob skips whole files. There is **no upper bound**:
   `created_before` bounds token *creation*, and tokens keep trading for days — an
   upper `dt` predicate would silently truncate live histories. Pinned by
   `parity_tests::partition_floor_drops_no_trades`.
3. **No window operator when nothing is clipped.** `SWEEP_DEFAULT_PER_MINT_CAP =
   i64::MAX` (the analysis default) makes `rn <= ?` a no-op, and the `ROW_NUMBER`
   window sorts on the same key the outer `ORDER BY` needs — so keeping it sorted the
   corpus twice for a rank nobody reads. The uncapped path emits a plain scan instead;
   `parity_tests::uncapped_plain_scan_matches_the_windowed_scan` pins the two shapes
   row-identical (order included — the fold's f64 summation is order-sensitive).
4. **Reuse the corpus across runs.** `sweep_corpus_cache` is keyed on
   `LakeSource::selection_hash` = *(selection, lake version)*. Grouped sweep, flow
   discovery, metric discovery, and rule search all read and write it, so re-running any of them
   over one selection — the normal tune-and-re-run loop — costs one Parquet read
   rather than one each, and a fresh `lake-export` moves the lake version so a stale
   corpus can never be served. Always cache the **unfiltered** selection corpus: the
   key doesn't include the in-memory `field_filters`, so a post-filter subset would
   serve a later run a corpus missing the tokens the earlier filter dropped. (A
   fingerprint scope is *not* such a filter any more — per rule 1 it is part of the
   selection, hence correctly part of the key.)

**Single-rule simulate shares the lake too — ONE row type.** The `.../simulate` backtests read the **same** lake through the **same** `LakeSource::load`/`SweepTrade`; there is no separate `SimTrade`. The only difference is `Selection::with_signatures`: the sweep loads it `false` (rows stay slim — the trigger is resolved by index, not signature), simulate loads it `true` so `SweepTrade::tx_signature` (an `Option<Box<str>>`, `None` on the sweep) is populated for the result tables' Solscan links. Shared entry point `strategies::sim_fetch::fetch_sim_histories` (uncapped per-mint, `curve_only: false`, stale-lake warn). The trades Parquet schema carries `tx_signature` (~88 B/row, only read when `with_signatures`); DuckDB reads use `union_by_name=true` so pre-migration day files null-fill until a full re-export. Because one loader + one type serve both, sim↔sweep pricing is parity by construction; `lake::duck::parity_tests::signature_flag_changes_only_the_signature` (auto-runs when `$SWEEP_LAKE_DIR` points at a populated lake, self-skips otherwise — not `--ignored`) pins that the flag touches nothing but `tx_signature`, and `duck::tests::reader_columns_are_canonical` ties the reader's column names to `lake/schema.rs`.

**Full audit (all migrated except one narrow, accepted exception).** Every bulk trade-history read path under `lab/src/` is lake-sourced: grouped sweep, simulate, and the backtests — all through `sim_fetch::fetch_sim_history_one`/`fetch_sim_histories` (uncapped, full-history, `curve_only` applied at load, since the projected `CorpusTrade` has no `venue`). A batch path resolves its **entire** mint list in one `fetch_sim_histories` call (one DuckDB scan, mints staged into a temp table) rather than a per-mint PG fan-out. The only remaining PG touch on the `trades` table is `grouped_sweep.rs`'s `resolve_fill_signatures` (called from `list_token_results`): a bounded, indexed `(mint, slot, side)` lookup against `TradeRepo` that back-fills `entry_tx`/`exit_tx` Solscan links for a combo's fills, since the sweep loads `Selection::with_signatures = false` (see above) and its slim `CorpusTrade` never carries a signature. This is a deliberate keep — it's a handful of indexed point-lookups, not a bulk scan, and the alternative (threading `tx_signature` through every sweep row) costs ~88 B/row for a field only the drill-in view needs. Everything else PG still serves in `lab` (sweep run/group/combo/result metadata, `strategy_rules`/`strategy_runs`/`strategy_positions`, the `tokens`/`tokens_info` dimension + candidate scan, the token-list boot seed) is dimension/job state, not trade history, and was never a lake-migration candidate.

## Metric-combo discovery pipeline (`lab/src/discovery/`)

Lab-only, built entirely on top of the generic sweep engine above (no new engine)
— an automated baseline-select → screen → family-grid → joint-interacting →
out-of-sample-validate pipeline whose **primary deliverable is a grouped-sweep seed**
(`SweepSeed` / `AxisSpec[]`): which metrics deserve axes, which narrowed value menus (incl. `off`)
are worth gridding, and which families must be gridded jointly. Promote into the
shared rule editor remains a secondary exit on OOS survivors. Nothing ships to
EC2; live/paper are untouched. Registry-driven throughout: a metric added to
`REGISTRY` needs no pipeline edit (family tag + unit/scope/monotonic flags are
all it reads).

| File | Role |
| --- | --- |
| `discovery/objective.rs` | `DiscoveryWeights` (tunable constants below) + `discovery_score(ComboStats) → Ranked \| BelowMinClosed \| NoFire` — a pure re-rank over persisted `ComboMetrics`, not a `checklist_score`/kernel edit (that stays the live/paper/sweep SSOT). The min-N gate is **cohort-aware**: `effective_min_closed = clamp(min_closed_frac × cohort, min_closed_floor, min_closed)` relaxes it on a regime-scoped cohort (never tightens it), and `confidence = n_closed / min_closed` discounts what the relaxed gate lets through, so a thin combo ranks low instead of vanishing |
| `discovery/baseline.rs` | **Layer 0**: `BaselineGrid` → one single-combo segment per `(tp, sl)` in ONE additive pass → `BaselineSelection{chosen, candidates, all_unprofitable}`. Layers 1–3 screen against the winner. Runs only when the grid holds 2+ brackets; a one-bracket grid is the caller naming a baseline. Fits on the **train slice only** — a bracket chosen with the held-out slice in view leaks into every Layer-1 number |
| `discovery/candidates.rs` | `screen_plan` (registry → screenable metrics + `SkipReason`) → `collect_percentiles` (measured `[p05..p99]` per metric, via the engine's own `MetricSeries` — deliberately **not** DuckDB SQL, else percentile semantics could drift from `hunter_engine`) → `build_menus` (`p10/p25/p50/p75/p90` + `off`, rounded by unit) → feeds `AxesModel` directly; the hand-derived table in [axis-value-candidates.md](../plans/sweep/axis-value-candidates.md) is now generated, not authored |
| `discovery/screen.rs` | Layer 1: `ScreenStrategy`, an additive scan mode (`GenericSweepStrategy::share_precompute`) that sweeps every candidate metric alone against the run's TP/SL baseline over **one** shared per-token precompute (~6N combos, not 6^N) → `Verdict{Keep\|DropNoEdge\|DropNegative\|DropSpike\|DropThin\|DropNoBaseline}` per metric → ranked shortlist. Every `ResponsePoint` carries win rate / median pnl% / **SOL** beside the unitless score, and the bare bracket's own row is hoisted to `ScreenReport::baseline_stats` — the reference line every `lift` is a delta against |
| `discovery/family.rs` | Layer 2: `plan_families` groups the Layer-1 shortlist by the registry's `MetricFamily` tag (`price`/`flow`/`flow_ix`/`liquidity-age`), grids within each family, then runs an O(families²) pairwise interaction check (pin A's best, sweep B) → `Independent \| Interacting \| Inconclusive`. **L2b** builds connected components of undirected `Interacting` pairs and product-grids them under `FamilyLimits` (enforced, not advisory) → `JointResult` winners. **L1b** is the *synergy rescue*: the strongest winner is pinned and up to `rescue_cap` Layer-1 rejects re-screened under it through the same `classify`, so a metric with no standalone lift can still earn an axis — flagged `rescued`, because that lift is conditional on the pin |
| `discovery/validate.rs` | Layer 3: `split_tokens` (age-based train/validate split) + `validate_candidates` re-scores each Layer-2 family **and** joint winner on the held-out slice via `simulate_one_combo` under the run's own `Pricing`/`as_of` → `ValidationVerdict{Holds\|Degraded\|Failed\|ThinValidate\|NoFireValidate\|UnrankableTrain}` (the two "can't tell" outcomes are never silently a pass). The slice carries its **own** cohort-scaled gate (`effective_min_closed`), reported so `ThinValidate` reads as a statement about the slice's size rather than about the candidate |
| `discovery/seed.rs` | `build_sweep_seed` — Keep axes (`off` + narrowed) + TP/SL menus expanded ±1 rung on the canonical ladders + near-miss `optional_axes` + cluster notes. Near-miss is `DropNoEdge` **or `DropSpike`** with a positive-scoring pick, **and every `DropNegative` at any sign**; the ladder re-prices exactly what each of them failed on — a losing baseline for the negative lead, an unsupported peak for the spike. The seed note counts the three classes separately and flags the spikes unstable, since a spike converts far less often than the other two. Pure projection onto the same `AxisSpec` wire the generic sweep consumes |
| `discovery/pipeline.rs` | `run_pipeline` — splits the cohort first, selects the baseline (L0) and fits Layers 1–2 (+ L1b/L2b) on train, validates on the held-out slice (a degenerate split fits the whole cohort and reports `no_validation` rather than a vacuous pass). `diagnose` emits the run-level findings a reader would otherwise have to derive across sections: which gate ran, whether the reference line was profitable, how much of the field died for want of data, how much power the validate slice had |
| `discovery/dto.rs` + `api/handlers/strategies/metric_discovery.rs` | `PipelineDto` (incl. `sweep_seed`, `diagnostics`, `baseline_selection`, `cohort_capped`) + `POST /api/strategies/metric-discovery` (+`/cancel`/`/last`/`/{run_id}`, SSE progress, single-flight mutually exclusive with sweep / flow-discovery / rule-search, cohort scoping by fingerprint/`ix_labels`/field filters, `take_profit_menu`/`stop_loss_menu` for L0). The handler is the only layer that knows `token_cap`, so it is the only one that can set `cohort_capped` — and it does so **on the result**, not only as an SSE notice a reader has long since missed |
| `frontend/src/lab/pages/strategies/MetricDiscoveryPage.tsx` | diagnostics + reference line (with the measured bracket table) → shortlist with money columns → drops ordered most-actionable-first → rescues → family winners + joint grids + interaction map → validation; primary **Open as sweep** writes a sessionStorage handoff that `GenericSweepConfigForm` applies once; **Promote…** secondary on winners |

**Objective (Layer 1's ranking core):** `robust_profit × fire_rate × win_component ×
min_n_gate × confidence`, where `robust_profit` is the combo's **capital-weighted
return** — the SSOT `weighted_return_pct(Σ pnl, Σ capital)`, which under the sweep's
fixed per-trade notional reduces exactly to `mean_pnl_pct` (pinned by a no-DB guard test;
**percent-of-vsol sizing inside a sweep breaks that identity and needs a real capital
sum**) — with an open-position mark discounted by `OPEN_HAIRCUT`,
`win_component` blends `win_rate` with a capped `profit_factor`, `min_n_gate` hard-zeroes
any combo under the cohort's **effective** gate (the anti-overfit backbone — no profit%
lets a 4-trade "edge" rank), and `confidence` discounts the band between that gate and
`MIN_CLOSED` so a relaxed gate buys a lower rank, not equal trust. The score is
unitless: it ranks, and only the money columns beside it can be checked against a trade. **Open (unpinned) constant:** `OPEN_HAIRCUT` / `profit_factor` cap /
`MIN_CLOSED` / plateau-penalty weight are seeded from the `axis-value-candidates.md`
anchors but never validated-and-pinned as a permanent tuning — revisit once a discovery
run's picks are checked against live/paper outcomes.

**The profit centre is sign-locked to money, never a median.** `Keep` requires a positive
score, and the score's sign is the centre's sign — so a median centre demands a positive
*median trade*, i.e. a **win rate above 50%**, and rejects every asymmetric-payoff combo,
which is the shape this cohort trades. Whale resistance lives in `win_component`
(`win_rate ×` capped `profit_factor`), which out-ranks a whale-carried combo ~3× on its
own; the centre is not a second copy of that job.

**The rank is only meaningful above zero.** The score is multiplicative over a *signed*
profit term, so below zero a higher `fire_rate` scores **worse** and a bare `max_by`
returns whichever option trades least. Every argmax — `select_baseline`'s bracket,
`classify`'s `best_value` (which `narrow` then builds Layer 2's range around) — ranks over
the positive picks only, and falls back to realised ◎ when none is positive.

**Reading a run** — four things decide whether a shortlist means anything, and each is
stated on the result rather than left to be inferred:

- **The reference line** (`screen.baseline_stats`): the chosen bracket's own bare
  result. A `lift` is a delta against it, so a shortlist read without it cannot
  separate "makes money" from "loses less than doing nothing". When it is negative the
  page says so and every `Keep` is a rescue, not an improvement.
- **The gate that ran** (`screen.effective_min_closed`): a regime-scoped cohort cannot
  afford the corpus-wide 20-closed gate, so it is relaxed toward the floor and the run
  reports both numbers. `DropThin` is a statement about cohort size, never evidence
  that a metric has no edge.
- **The cohort's reach against that gate**: the scan opens at most one position per
  token, so `n_closed <= fit_tokens` and a gate must fire on `gate / fit_tokens` of the
  slice merely to be scored. Past a quarter, the p75/p90 rungs — the ones most likely to
  carry an edge — cannot clear the gate whatever they screen, and `diagnose` states the
  arithmetic rather than letting a field of `DropThin` read as "no edge here".
- **`cohort_capped`**: a cap hit means the run scored the newest N *matched* tokens,
  not the range that was asked for.

**Honest L1 limit:** Layer 1 is univariate, so a metric with no standalone lift never
reaches a family grid on its own. L1b's synergy rescue is the bounded repair — the
strongest winner pinned, up to `rescue_cap` rejects re-screened under it — and it is
deliberately *conditional*: a rescued axis is valid alongside that pin, not by itself,
and carries the `rescued` flag everywhere it appears. Rejects the rescue does not
reclaim can still be seeded as `optional_axes`, or added by hand in the sweep form.

**Perf shape is scan/precompute-bound, not fold-bound** (few combos; cost is the corpus
load + per-token `MetricSeries` build + the exit scan). A discovery run should therefore
pick a **tighter RAM reserve** (bigger resident series wave, fewer precompute rebuilds)
rather than inherit the interactive sweep's defaults — see `discovery/screen.rs`'s knob
table. **Leave the AVX-512 toggle off**: it does not beat the index path (§ exit-scan
path). The dominant lever regardless is precompute reuse: one corpus load + one
series-union precompute shared across every metric screen, not N re-loads — and, because
a metric-exit rule costs ~15× a TP/SL one, batching many axes into one run rather than
splitting them across runs.

**Data reality:** the fingerprint dimension (`tokens`/`tokens_info`) covers only ~7% of
the tradable universe (a backfill gap, not a design choice) — this throttles Layer-2/3
*grouping/scoping* only; Layer-1 metric-axis screening runs over the full trade corpus
unaffected. Default to a tight single-regime cohort (one fingerprint scope or
`ix_labels`-only): the cohort-aware gate is what makes that affordable, and the run
reports the relaxed gate so the trade-off stays visible. Widen when the `DropThin` tally
dominates the drop table.

## Rule search (`lab/src/rule_search/`)

Lab-only job that finds one champion `RuleParams` for a **single fingerprint** and
a datetime range. Sibling of grouped sweep / flow discovery / metric discovery — not
a sweep mode. The form does not expose metrics, windows, or thresholds; those come
from this range's cuts and the registry. An incumbent rule is compare-only (never a
seed). Governing workflow:
[market-model-and-workflow.md](../plans/strategies/market-model-and-workflow.md).

| File | Role |
| --- | --- |
| `rule_search/roles.rs` | Registry flags → entry roles / exit bags / compete keys. New registry rows join by flags. |
| `rule_search/cuts.rs` | Cohort windows + phase samples → threshold menus (peak contrast primary; run-lead / launch / fill-moment extras; dump-lead / giveback-lead / after-dump / outcome held on exit). Declared `m_position` exits stay on the menu. |
| `rule_search/generator.rs` | Entry fillings × exit bags → complete `RuleParams`. Empty entry and empty exit are combos. One extra phase per metric beside peak. Extra OR on the top 5 after scoring; same-phase retune on the top 3. |
| `rule_search/scorer.rs` | `CompiledRule` series walk (shared entry across bags) then copycat (+ caps) time-order merge. Horizon is Simulate's (`as_of` / corpus last trade), not sweep's per-token tail cap. |
| `rule_search/report.rs` | Report columns are `run_replay` for the board (champion, empty-entry, incumbent, archive). Paying replays rank by authority SOL, then tighter fill spread. Extra archive slice when the top slice has no paying replay. Verdict refuse / ungated / candidate. Optimistic fill is `FirstInWindow`. |
| `api/handlers/strategies/rule_search.rs` | `POST /api/strategies/rule-search` (+`/cancel`/`/last`/`/{run_id}`). `202` after fingerprint/incumbent admission; corpus load and search run detached. SSE progress, persist last result under `$SWEEP_LAKE_DIR/rule-search/last.json`. Fingerprint mint scan **before** the lake load. `as_of` freezes at session open. Single-flight vs sweep / flow-discovery / metric-discovery. |
| `frontend/src/lab/pages/strategies/RuleSearchPage.tsx` | Form (required fingerprint, range, buy, fill, cost, copycat default ON, optional incumbent) → board (verdict, three columns, champion params, archive, Promote / Simulate) |

Fill/cost defaults: worst fill + `pumpfun_impact`. Buy and caps come from the
incumbent when one is set, else the form. Copycat is ON unless the request sets it
off — empty-entry vs champion needs the guard.

## Family search (`lab/src/family_search/`)

Lab-only job that grades **one fingerprint's sibling family**: siblings share
`ix_labels` and differ on exactly one axis, resolved mechanically off the
`fingerprints` table. Only an **exact** predicate has a position to order a family
by, so a range-valued axis makes two rows non-siblings rather than collapsing to a
bound. Rank comes from a pooled fit across the
family, level from the held-out target cohort alone. Rule search is not modified;
every change to shared sweep code is additive.

| File | Role |
| --- | --- |
| `family_search/family.rs` | Sibling resolve off the `fingerprints` table. Same shape, identical on every axis but one; a dropped axis is a different population, not a sibling. A family of one degrades to single-cohort. Unpinned ties land on the first axis in `AXES`. |
| `family_search/generator.rs` | Signature-earned candidates (rule search's cut table, read-only), **composed to the working shape**: entry ANDs of 0–4 *quantities* densest-first (a floor+ceiling band is ONE quantity, so a 3-idea entry writes 5 clauses), exit ORs of 2–5 alarms drawing at most one clause per end-event family (flow · organic · stall-clock · liquidity-ceiling · price-trail). The quota buckets on the family **set** — with multi-family bags a first-alarm bucket holds nearly everything — and `by_family` reports coverage. Price trail stays in the library, flagged. **Standing terms** (D10) ride at the end of every bag and the control, searched by nothing. `ungated_control` is the exit-less diagnostic, kept apart. |
| `family_search/score.rs` | Pooled fit `Σpnl_sol / Σentry_sol` and pooled win rate `Σwins / Σcloses` (never a mean of per-cohort rates), Spearman ρ as the procedure's self-test, the **two-sided selection** (first ranked candidate clearing both the ungated control's win rate and a positive return, read narrow), the narrow re-check — which grades an entry term by win rate and an exit term by return, because grading both on money deletes every entry condition — and `wilson_low_pct`, the 95% lower bound that says when a win-rate clearance is inside the sample's own noise. |
| `family_search/enrich.rs` | The only stage that can make a rule **denser**. Offers each earned idea the fitted skeleton lacks, judges it in its own side's currency, and confirms every acceptance against the rule as it grows so two forms of one idea cannot both get in. Bounded at 12 trials + 3 accepts, all on the resident target cohort. |
| `family_search/oracle.rs` | Capture ratio against the oracle exit — the best price printed after the fill, priced through the same cost and fill as the realized exit. `n_no_upside` is its own line and grades the **entry**. Also the cohort's net-move distribution and `execution_band_pct`, which the cost gate reads, and the two counterfactuals regret is graded against: `best_after_pnl_sol` (the best exit still ahead of a close) and `terminal_pnl_sol` (holding to the last print). |
| `family_search/diagnose.rs` | Reliability diagnostics on the finalist (D13), all on the resident target: **threshold ladders** (`x0.5..x1.5`, plateau vs a spike), **alarm regret** (each alarm's closes against both counterfactuals — only when the alarm both leaves real upside AND loses to holding on is it cutting winners), **entry redundancy** (solo score + veto-set overlap, which drop-one ablation cannot see because a sibling covers for the clause), and **per-clause fill sensitivity** (drop-one contribution under both pricings; a flip or a collapse means the contribution is the fill model). Grades only — nothing here reaches selection, or the held-out cohort is leaked. |
| `family_search/attribution.rs` | Per authored exit slot: n, **wins**, Σpnl_sol, Σentry_sol, a **standing** flag, plus the **authored threshold against the mean realized gross return** — offered only where the two are one quantity (`m_position.pnl`), so a stop that gaps past its level is visible without blaming gapping for execution cost. Bucketing mirrors `ComboAgg::record`, pinned equal by a no-DB test. |
| `family_search/gates.rs` | Four gates: freshness refuse (D7), **cost-clearance refuse** (D8), the axis-duplication refuse (an entry clause whose admit rate tracks the varied axis at \|ρ\| ≥ 0.8), and the lagging-entry-clause diagnostic. |
| `family_search/report.rs` | Board payload + the portrait prose. Every candidate row carries the rank-only `fit_ret_pct` beside the reportable `target_ret_pct`. |
| `api/handlers/strategies/family_search.rs` | `POST /api/strategies/family-search` (+`/cancel`/`/last`/`/{run_id}`). Scope resolves for every member up front (dimension-only), then the **target cohort stays resident** while fit siblings load one at a time. Persists the last result under `$SWEEP_LAKE_DIR/family-search/last.json`. Single-flight against every other heavy job. |

The board is `/strategies/family-search` in the lab app — see
[frontend.md](frontend.md) "Lab **Family search**".

Two tiers: the fit stage stops at `score_combos`' archive fold (it needs a ranking,
and candidates are near-free against the token walk), and `run_replay` is the
authority pass on the **target cohort and the finalist only**.

Buy size, caps, fill, cost and the copycat setting come from the **request** only. An
incumbent rule is a display column and supplies none of them — cost is U-shaped under
`pumpfun_impact`, so an incumbent's buy size silently moves the economics, and its
caps change which tokens are entered at all.

`Selection::with_oracle` is the one additive corpus field the job adds: it builds
`CorpusToken::peak_after` (`projection::suffix_peak`) at load, 4 B/row, opt-in, and
folded into `lake_hash` so an oracle load and a plain load cannot share a cache entry.

**Execution honesty (D8).** Before the generator runs, the ungated control's authority
pass supplies a rule-free oracle distribution, and the **median net move over every
priceable entry** (losers included — a winners-only median is positive by construction)
is compared against `execution_band_pct`, one round trip priced on a flat trade at the
run's buy and the cohort's median pool depth. Under `margin × band` the search is
refused before a candidate exists; between there and one band the run is badged
`thin`, because a rule takes only a fraction of the best available exit. A refusal
**boards a report** with an empty library rather than erroring — the measurement is the
finding. The finalist then carries a **dual-pricing spread**: a second replay of that
one rule at `FirstInWindow` + `pumpfun_fee_only` (the zero-impact bound, so the spread
isolates fill luck from sizing cost), intersected with the authority pass
on mint so both returns cover one taken set, with any drift counted rather than
averaged over. An edge no larger than its own spread is priced on fill luck.

A corpus load cannot be cancelled mid-flight, so `check_cancelled` runs after the scope
resolve, before every sibling load, and before the authority pass — those checkpoints
are the whole cancellation story.

## Adding a strategy

`strategies/<x>.rs` (`Strategy`+`ParamSpace`+`AxesSpec`) + `registry.rs` arm (table triple + dispatch) + `<x>_grouped_sweep_*` tables in a `lab/migrations/` SQL file + frontend param-key list + axes defs. Engine, grouping, repo, handler, and page are reused unchanged.
