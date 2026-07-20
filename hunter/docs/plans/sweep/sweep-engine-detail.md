# Sweep Engine — Implementation Detail

Deep-dive on the grouped sweep internals: combo column families, coarse→refine algorithm, full invariants, and grouping mechanics. See [@arch/sweep.md](@arch/sweep.md) for the module map and entry points.

## Combo Column Families

Each combo in `<strategy>_grouped_sweep_combos` stores its params as a JSONB dictionary keyed by param name. The `ComboMetrics` struct serialized to `<strategy>_grouped_sweep_results` has these column families:

**Identity / ranking:**
- `combo_id UUID` — FK into `combos` table
- `group_id UUID` — FK into `groups` table
- `score f64` — composite score (primary ranking key; formula + intuition in [Metrics reference](#metrics-reference--what-each-column-means) below)

**Win-rate / count:**
- `win_rate f64` — fraction of tokens with positive PnL
- `entry_count i64` — total entries across all tokens in the group
- `exit_count i64` — total exits (may be < entry_count if some positions are still open at end of data)

**PnL metrics:**

> **Realized vs unrealized.** Every stat below except `open_pnl_sol` is measured over
> **closed** positions only. A position still `Open` at the end of the corpus window
> counts toward `n_fired`/`n_open` but its mark-to-last-price PnL is excluded from
> `total_pnl_sol`, `win_rate`, `expectancy_sol`, and `score` (parity plan C2 — folding
> an unrealized mark in made headline numbers depend on when the window happened to
> end). Read `total_pnl_sol` together with `open_pnl_sol`: a combo that simply never
> closed its losers shows a clean realized total while sitting on open losses.
> Note `score` (and therefore best-combo selection) still ranks on realized figures
> alone, so open exposure does not penalise a combo's ranking.

- `total_pnl_sol f64` — sum of PnL across **closed** positions in the group (realized)
- `open_pnl_sol f64` — sum of still-`Open` positions' mark-to-last-price PnL
  (unrealized; never included in `total_pnl_sol`). `total_pnl_sol + open_pnl_sol` is
  the mark-to-market total the UI shows as "PnL (MTM)"

The combo-results summary panel renders this as two bands over one cohort-agnostic
metric engine (`pnlBlock` in `GenericSweepView.tsx`): **Realized** (closed rows) and
**Incl. open (MTM)** (all fired rows, open bags carrying their marks). Divergence
between the bands is the signal — it measures how much of the headline is unsettled.
The group/combo tables carry the matching `n_open` / open-share columns.
- `avg_pnl_sol f64` — mean PnL per position
- `median_pnl_sol f64` — p50 of per-position PnL distribution (QuantileSketch)
- `p90_pnl_sol f64` — p90 of per-position PnL distribution (QuantileSketch, ~15% relative error)
- `expectancy_sol f64` — `avg_pnl_sol` weighted by win probability (primary ranking for best-combo selection per group)

**Exit reason mix** (fraction of exits by type, sums to 1.0):
- `exit_tp f64` — take-profit exits
- `exit_sl f64` — stop-loss exits
- `exit_trailing f64` — trailing-stop exits
- `exit_time f64` — time-stop exits
- `exit_stall f64` — stall exits
- `exit_liquidity f64` — liquidity-drop exits

## QuantileSketch — `aggregate.rs`

`ComboAgg` uses a DDSketch (deterministic relative-error sketch) for median/p90 approximation. Why not exact sort: at 10k combos × 1k tokens per group, storing all PnL values per combo would be 10M f64 values = 80 MB per sweep run. The sketch uses ~0.6 KB per combo with ~15% relative error, giving 6 MB total — fits in L3 cache.

```rust
pub struct ComboAgg {
    sketch: DDSketch,          // ~0.6 KB, relative error ~0.015
    count: u64,
    sum: f64,
    wins: u64,
    exits_by_reason: [u32; 6],
}
```

`merge_outcome(outcome: &TokenOutcome)` is O(log(1/relative_error)) per call. `finalize() -> ComboMetrics` converts the sketch to percentile queries in O(log n).

The 15% relative error is acceptable for sweep ranking because combos are ranked by score (which uses `expectancy_sol`, computed exactly), not by percentile. Percentiles are displayed in the UI for human interpretation — a displayed p90 of 0.85 SOL vs actual 0.87 SOL is not a trading decision.

## Coarse→Refine Algorithm — `grouped_engine.rs`

Optional two-phase sweep to handle large parameter spaces efficiently:

**Phase 1 (coarse):** `SweepMethod::Grid` with wide steps across the full param space. Ranks all combos. Takes the top-K combos (default K = 20% of total, min 50).

**Phase 2 (refine):** `SweepMethod::Refine` around each top-K combo's param values. For each param, generates a fine-grained range centered on the combo's value (±1 step, ×3 density). Re-sweeps only these refined combos.

```
Full space: 10 000 combos (coarse, ~10min)
Top 20%: 2 000 combos shortlisted
Refine: ~6 000 combos around those (fine, ~6min)
Total: ~16 000 combos in ~16min vs 100 000 combos exhaustive (~100min)
```

The refine phase reuses the same corpus (already projected and cached) so it's pure CPU — no additional DB reads.

Entry point: `run_grouped_with_refine(strategy, config, corpus, observer, sink)`. Config flag: `sweep_method: "CoarseRefine"`. The API accepts this as a JSON enum variant.

## Grouping — `grouping.rs`

`TokenFingerprint` is a struct of grouping dimensions. Nine are `tokens`-creation
facts; `first_slot_{buy,sell}_sol` are trade-derived (from `tokens_info`). Creator
wallet is **deliberately absent** — pump.fun creators rotate wallets, so it only ever
yields singleton groups.

```rust
pub struct TokenFingerprint {
    pub token_program_id: Option<String>,
    pub initial_buy_sol: Option<f64>,        // continuous SOL amount → binned
    pub cu_limit: Option<i64>,               // discrete → exact
    pub cu_price: Option<i64>,               // discrete → exact
    pub is_cashback_enabled: bool,
    pub max_cost_lamports: Option<i64>,      // continuous SOL amount → binned
    pub spendable_lamports_in: Option<i64>,  // continuous SOL amount → binned
    pub first_slot_buy_sol: Option<f64>,     // trade-derived (tokens_info) → binned
    pub first_slot_sell_sol: Option<f64>,    // trade-derived (tokens_info) → binned
    pub ix_labels: Vec<String>,
}
```

`GroupKey` is a `Vec<(GroupField, String)>` in **selection order** (not sorted) — the
chosen subset of fields, each rendered to its group-key string by `render_field`.
Two tokens with the same `GroupKey` are in the same group.

**Exact-value vs binned rendering (`render_field`).** Discrete fields (program id,
CU limit/price, cashback, ix-labels) render their exact value. The continuous
SOL-amount fields (`initial_buy_sol`, `max_cost_lamports`, `spendable_lamports_in`,
`first_slot_{buy,sell}_sol`) are **bucketed** into fixed `SOL_BUCKET_WIDTH`-wide ranges via
`bucket_sol_label`, rendered `"lo–hi"` (e.g. `"1.0–1.1"`) — exact-value grouping there
would make every token its own group. **Tuning constant:** `SOL_BUCKET_WIDTH = 0.1` SOL,
`SOL_BUCKET_DECIMALS = 1`. Lamports-native fields (`max_cost`/`spendable`) are ÷1e9 to
SOL first so the label reads in SOL. `0.1` is not f64-exact, so `bucket_sol_label` adds a
`+1e-9` epsilon on the ratio before `floor` — an on-edge value (e.g. `0.3`) lands in
the upper bucket, and the nudge (0.1 lamport in ratio units) can never promote a
genuinely sub-edge value. The dashboard mirror `creation_stats_repo::sol_bucket_sql`
applies the identical epsilon + 1-decimal `to_char` rounding so sweep and dashboard
produce byte-identical labels. Making the width runtime-configurable is a separate,
not-yet-merged per-run knob.

Fingerprints are **embedded by the lake loader** (`LakeSource`/`duck.rs::load_fingerprints`
reads the `fp_*` columns of the tokens dimension file), not a separate PG pass — the
old `attach_fingerprints` PG pass was deleted at the lake cutover.

Groups with fewer than `MIN_GROUP_SIZE` (default 30) tokens are merged into an `_other` bucket and swept separately.

## GroupSink — incremental persistence

`GroupSink` in `grouped_engine.rs` decouples the sweep computation from DB writes. It implements `FnMut(GroupResult) -> Result<()>`:

1. Each completed group calls `sink(group_result)` inline (within the rayon thread pool)
2. `GroupSink::submit()` sends to a tokio channel (bounded 4)
3. A dedicated tokio task drains the channel and calls `grouped_sweep_repo` in order

This means: the sweep continues computing while previous groups persist. DB write latency does not add to sweep wall-clock time. The channel cap (4) provides light backpressure — if the DB is slow, the rayon workers pause after 4 pending groups rather than accumulating unboundedly.

## Full Sweep Invariants

1. **Corpus loaded once per run** — `DbSource::load()` pulls all tokens + their trade slices into `BacktestTradeCache` at the start. No DB queries during the simulate loop.

2. **Entry cache reuse per token** — `Strategy::entry_key(params) -> EntryKey` maps params to a cache key. Tokens with the same entry params (e.g., same scalp gates) reuse the same entry result. For TPSL2: 14 params but entry varies on only 8 → 2^8 = 256 entry cache slots max per token, not 2^14.

3. **Buffer recycling in `engine.rs`** — `Vec<SweepTrade>` per-combo buffers are taken from a pool and returned after use. No allocation in the hot loop except for the `ComboAgg` accumulators (allocated once per combo at the start of each token's sweep).

4. **Cancel signal** — `SweepObserver::should_cancel()` is checked between tokens. A cancelled run returns `Err(SweepCancelled)`. `GroupedEngine` catches this and calls `sink.mark_cancelled()` which writes `status = 'cancelled'` to the DB.

5. **Crash recovery** — `reconcile_orphaned_runs` runs at startup. Finds `status = 'running'` rows older than `ORPHANED_RUN_THRESHOLD` (1h), marks them `cancelled`. This prevents the UI from showing stale "running" indicators after a backend restart.

6. **Retention filter is idempotent** — `retained_combo_ids` is deterministic given the same `ComboMetrics` input. Applied write-time before results are persisted (only the ~660 retained combos are ever inserted), so re-running a group selects the identical set — no separate compaction pass.

7. **No sweep work on server** — sweeps use the `batch` pool and are CPU-bound (rayon). The deployed EC2 box (2vCPU/4GB) cannot sustain sweep workload without crowding out the ingest pipeline. Sweeps **must** run on local only. The server's batch pool has `max_connections = 1` to make this impossible to violate accidentally.

## Fold hot-path rules (engine.rs) — do not regress

Three properties the wave-outer fold depends on. Each replaced a measured waste.

1. **Bind once per shard, never per wave.** `bound_all` is built *outside* the
   `corpus.tokens.chunks(wave)` loop in `run_sweep_unsharded`. Params don't vary with
   the token wave, so binding inside it re-compiled every combo `n_waves` times
   (`n_waves = group_tokens / threads`) — 62.5M `CompiledRule::compile` calls for 100k
   distinct params on a 10k-token group at 16 threads, each allocating a `RuleParams`
   with nested condition maps. The pass-outer branch always hoisted correctly; only
   wave-outer had the loops nested the wrong way. Slice it as
   `bound_all[offset..offset + chunk.len()]` — the chunk offsets index `params` and
   `bound_all` identically.

2. **`fold_wave_into` borrows the accumulators, never copies them.** It takes
   `&mut [ComboAgg]` into a scoped thread and merges in place. It used to `to_vec()` in
   and `clone_from_slice` back out: `ComboAgg` is ~640 POD bytes, so at a 65536 batch
   that was ~42 MB in + ~42 MB out *per wave, per pass* — tens of GB of memcpy on a
   large group, moving the same accumulators back and forth for nothing.

3. **`order_for_entry_cache` uses `sort_by_cached_key`.** `entry_key` allocates a
   `vec![0; n_axes]` per call, and `sort_by_key` calls the key fn once per
   *comparison* (~n log n) rather than once per element. Both sorts are stable, which
   the same-entry contiguity depends on.

4. **Series-column indices resolve at bind time, not per (token, combo).** Every
   token's series is built from `self.columns.clone()`, so the column layout is fixed
   for the whole run and a combo's indices are the same on every token. `BoundParams`
   is therefore `BoundCombo` (`CompiledRule` + `entry_cols`/`mono_cols`/`exit_cols`),
   built once in `bind_param`. Previously `resolve_exit` — uncached, once per combo
   per token — called `resolve_cols`, making it the single most-executed heap
   allocation in a sweep.

   The invariant is load-bearing: if the column set ever varied per token, cached
   indices would silently read the wrong metric. Two guards — a `debug_assert` in each
   scan that re-derives the indices from the series, and
   `shared_bind_matches_per_token_bind`, which binds once and scans many tokens. The
   `scan_matches_replay_*` tests cannot catch this (each binds against the very series
   it scans, so a stale index agrees with itself).

5. **The refine driver frees `coarse_groups` before the final pass.** The coarse pass
   uses `NoopSink`, so `free_persisted_metrics` never trims it; without the explicit
   `drop` it holds `n_groups × n_combos` `ComboMetrics` resident straight through the
   memory-heaviest sweep.

**Memory model.** `full_combo_aggs_fit` (and `plan_shards` through it) takes
`bound_bytes_per_combo` — pass `size_of::<S::BoundParams>()`. Both wave-outer paths
(the shard-wide `bound_all`, and the grouped driver's group-wide `bound` in the
token-outer fold) hold `BoundParams` for the *whole* combo set, so it is priced next to
the accumulators instead of being left to the alloc slack. That is the inline size
only; `SmallVec` heap spill is still absorbed by the 256 MB slack.

Equivalence is pinned by `multi_wave_fold_matches_single_batch`, which drives enough
tokens to span many waves — `batched_fold_matches_single_batch` cannot catch a
regression in any of the above, because its 3-token corpus is a single wave.

## Observability — two instruments, different shapes (`obs.rs`)

A run used to be a black box between the `corpus_loaded` and `done` milestones: four
timing sites existed, all whole-run, so no stage's *cost* was recoverable. Worst case,
a refine run does two complete sweeps of the corpus and nothing said which half was
slow.

- **Milestones** (`log_milestone`) are *points* on one run-long `SweepClock`:
  `admitted`, `corpus_loaded`, `done`. They need the clock threaded from admission, so
  only the handler emits them. (An older doc listed a `partitioned` milestone that was
  never emitted — partitioning is a stage; its duration is the useful number.)
- **Stages** (`Stage::start`, drop-based) are *durations*: `corpus_load`, `partition`,
  `refine_coarse_pass`, `refine_final_pass`, `writer_drain`. Self-contained, so engine
  internals use these. Bind the guard (`let _s = …`) — an unbound temporary drops
  immediately and logs a zero-length stage. Drop-based means a cancelled or failed
  stage still reports how long it ran, which is when the number matters most.

`writer_drain` exists because the write channel is unbounded *on purpose* (a rayon
worker must never stall on the DB). A writer that fell behind during the fold therefore
drains serially **after** the engine logs "all groups folded" — previously unlabeled
dead time at the end of every slow run.

Per-group cost is reported as `slowest_group_{secs,index,tokens}` on the run's summary
line, not one line per group: at ~1k groups, per-group logging buries the single number
worth acting on (a group taking a large share of the run = a skewed partition, and the
index says which one to reproduce). Timing spans fold **plus retention**, since
retention runs in the same worker.

Throughput (`evals_per_sec`) is logged at `debug` from the throttled progress tick
(~100/run, never the fold loop). The user-facing **ETA is the client's** — one
`estimateEtaMs` shared by sweeps, simulations and swings. Do not add a server-side ETA;
it would be the same fact computed twice.

## Retention — what is and isn't per-metric (`retention.rs`)

`retained_combo_ids` runs per group *inside the fold's rayon workers*, so it competes
with the fold for cores. Two of its three costs are **not** metric-dependent and are
hoisted out of the 11-metric loop:

- **eligibility** (`n_closed >= min_closed`) — one filter, not 11.
- **the cap tie-break order** (`score` desc, `combo_id` asc) — one sort, not 11
  byte-identical ones. This is a total order, which is what makes the result
  order-independent (pinned by `retention_is_order_independent`); if ties fell back to
  input position, two runs over the same data would persist different rows.

Only the **value sort** is genuinely per-metric. Net: 22 sorts → 11, and 10 of the 11
`n_combos`-sized allocations removed via buffers reused across metrics.

## TPSL1 `Strategy` Impl — `sweep/strategies/tpsl1.rs`

Sweeps 6 exit-ladder knobs: `take_profit_pct`, `stop_loss_pct`, `trailing_stop_pct`, `time_stop_secs`, `stall_secs`, `liquidity_drop_pct`.

Entry is param-free: `entry_key` always returns the same key → `prepare_token` is called once per token, resolves entry by scanning trades for the first trade that satisfies the base rule's entry criteria, caches it. All combos share that entry point.

`simulate(token, params)` calls `find_trade_driven_exit` + `find_clock_driven_exit` (same fns as live), accumulates `TokenOutcome { pnl_sol, exit_reason, held_secs }`.

## TPSL2 `Strategy` Impl — `sweep/strategies/tpsl2.rs`

Sweeps all 14 knobs (8 scalp entry + 6 exit ladder). Entry varies on 8 scalp-gate params → `entry_key` hashes those 8 params → `prepare_token` is a no-op (`TokenState = ()`) and the entry resolves fresh for each unique `(token, entry_key)` combination.

The 8-param hash typically produces far fewer than 2^8 distinct values in practice (most params have 3–5 sweep values → ~3^5 = 243 distinct entries per token in a typical run).

## Metrics reference — what each column means

Human-facing reference for the **Grouped Sweep** result-table and combo-table columns.
The storage layout for these is [Combo Column Families](#combo-column-families) above; the
approximation mechanism is [QuantileSketch](#quantilesketch--aggregaters).

### Score — `aggregate.rs` → `robust_score()`

**Formula:** `mean − 1.64 × (std / √n)` — a **lower confidence bound** on the combo's true
average return. It answers *"what's the worst I can credibly believe this combo's real mean
is, given the data?"*

| Variable | Meaning |
| --- | --- |
| `mean` | Average % return across closed trades |
| `std` | Sample standard deviation of closed returns |
| `n` | Number of closed trades |
| `1.64` | ~95% one-sided confidence multiplier (Z-score) |

**Example:** mean = +50%, std = 80%, n = 10 → `50 − 1.64 × (80 / √10) = +8.5`.

Why it matters: high mean with few trades → low score (no repeatable edge proven); high mean
with volatile returns → low score (punishes inconsistency); high mean + many trades + tight
returns → high score (reliable edge). Score is `None` (shown `—`) below 2 closed trades.

### Combo-table columns

- **Profit Factor** — `gross_wins_sol / gross_loss_sol`: SOL won per 1 SOL lost (3.48 = 3.48
  gained per 1 lost). Higher is better; `∞` when there are zero losing trades.
- **Mean %** — `Σ(pnl%) / n_fired`: simple average % return over **all fired tokens**,
  including open positions marked to current price.
- **Median %** — p50 of all trade returns, via the log-bucketed quantile sketch (~15% rel.
  error). Robust to outliers.
- **P90 %** — p90 of all trade returns (same sketch): the upside-tail "good day", a ceiling
  not a floor.
- **Std %** — sample std of **closed** returns, `√[(Σx² − n·μ²) / (n−1)]`. Feeds the Score
  formula to penalise inconsistency; `0` below 2 closed trades.

Best % / Worst % are exact running min/max — no approximation.

### Closed vs. all trades (which scope each metric uses)

| Metric | Scope |
| --- | --- |
| Mean %, Median %, P90 % | All fired (open positions mark-to-market included) |
| Std %, Score | Closed trades only |
| Holding time (avg/median) | Closed trades only |
| Win rate, Total PnL, Expectancy | All fired |

See the [Realized vs unrealized](#pnl-metrics) note under Combo Column Families for why
`score`/best-combo selection ranks on realized figures alone.
