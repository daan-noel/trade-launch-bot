# Sweep Engine — Implementation Detail

Deep-dive on the grouped sweep internals: combo column families, coarse→refine algorithm, full invariants, and grouping mechanics. See [@arch/sweep.md](@arch/sweep.md) for the module map and entry points.

## Combo Column Families

Each combo in `<strategy>_grouped_sweep_combos` stores its params as a JSONB dictionary keyed by param name. The `ComboMetrics` struct serialized to `<strategy>_grouped_sweep_results` has these column families:

**Identity / ranking:**
- `combo_id UUID` — FK into `combos` table
- `group_id UUID` — FK into `groups` table
- `score f64` — composite score (primary ranking key; see `@plans/sweep/sweep-metrics-explained.md`)

**Win-rate / count:**
- `win_rate f64` — fraction of tokens with positive PnL
- `entry_count i64` — total entries across all tokens in the group
- `exit_count i64` — total exits (may be < entry_count if some positions are still open at end of data)

**PnL metrics:**
- `total_pnl_sol f64` — sum of PnL across all positions in the group
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

`TokenFingerprint` is a struct of optional fields, each representing one dimension for grouping:

```rust
pub struct TokenFingerprint {
    pub initial_buy_sol_bucket: Option<SolBucket>,  // <0.1, 0.1–0.5, 0.5–1, 1–5, >5
    pub launch_hour_utc: Option<u8>,                // 0–23
    pub venue: Option<Venue>,                       // Curve, Amm
    // ... up to 8 fields
}
```

`GroupKey` is a sorted `Vec<(GroupField, GroupValue)>` — the selected subset of fingerprint fields for this sweep run. Two tokens with the same `GroupKey` are in the same group.

`attach_fingerprints(tokens, group_fields)` computes fingerprints for all corpus tokens in one pass:

1. Look up `initial_buy_sol`, `creator_wallet`, `created_at` from the projected corpus (already in memory)
2. Bucket each field into its `GroupField` variant
3. Build `GroupKey` by selecting only the `group_fields` subset

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

6. **Retention filter is idempotent** — `retained_combo_ids` is deterministic given the same `ComboMetrics` input. If `vacuum_full_results` runs twice on the same group (e.g., after a reconcile), the second run is a no-op (all retained ids are already retained).

7. **No sweep work on server** — sweeps use the `batch` pool and are CPU-bound (rayon). The deployed EC2 box (2vCPU/4GB) cannot sustain sweep workload without crowding out the ingest pipeline. Sweeps **must** run on local only. The server's batch pool has `max_connections = 1` to make this impossible to violate accidentally.

## TPSL1 `Strategy` Impl — `sweep/strategies/tpsl1.rs`

Sweeps 6 exit-ladder knobs: `take_profit_pct`, `stop_loss_pct`, `trailing_stop_pct`, `time_stop_secs`, `stall_secs`, `liquidity_drop_pct`.

Entry is param-free: `entry_key` always returns the same key → `prepare_token` is called once per token, resolves entry by scanning trades for the first trade that satisfies the base rule's entry criteria, caches it. All combos share that entry point.

`simulate(token, params)` calls `find_trade_driven_exit` + `find_clock_driven_exit` (same fns as live), accumulates `TokenOutcome { pnl_sol, exit_reason, held_secs }`.

## TPSL2 `Strategy` Impl — `sweep/strategies/tpsl2.rs`

Sweeps all 14 knobs (8 scalp entry + 6 exit ladder). Entry varies on 8 scalp-gate params → `entry_key` hashes those 8 params → `prepare_token` is a no-op (`TokenState = ()`) and the entry resolves fresh for each unique `(token, entry_key)` combination.

The 8-param hash typically produces far fewer than 2^8 distinct values in practice (most params have 3–5 sweep values → ~3^5 = 243 distinct entries per token in a typical run).
