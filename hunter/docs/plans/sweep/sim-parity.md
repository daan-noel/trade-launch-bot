# Sweep ↔ simulate parity — divergences, decisions, open items

Reference for *why* the grouped sweep and `simulate` can report different numbers for
the same rule + token, which of those gaps are deliberate, and what is still open.
Audit date **2026-07-19**. High-level summary lives in
[../../arch/sweep.md](../../arch/sweep.md); engine internals in
[sweep-engine-detail.md](sweep-engine-detail.md).

## The governing decision

> The sweep is an **approximate ranking tool**. `simulate` is the authority on any
> single combo's PnL.

Converging the two onto one engine was considered and **declined** — it costs too much
sweep speed. Do not re-open this without the user; it is a settled design decision, not
an unfixed bug.

## Two generations, only one of which shares code

| Generation | Sweep path | Replay path | Shares one fn? |
| --- | --- | --- | --- |
| **Generic (current)** | `GenericSweepStrategy::scan` over a precomputed `MetricSeries` | `hunter_engine::reduce` event fold | **No** — parallel hand-written impls, held together by the 8-fixture guard test (`sweep/generic/guard.rs`) |
| **Legacy tpsl1/tpsl2/swing1** | `entry::find_entry_fill_in_trades` / `exit::find_trade_driven_exit_as_of` | same fns | Yes |

Ironically the *old* generation satisfies the "one original function" requirement and the
new one does not. That is the trade the decision above accepts.

**Genuinely shared by both — do NOT "fix" these into separate copies:**
`CostModel` / `round_trip_with_costs` (`core/src/strategies/kernel.rs`), `is_dead_verdict`,
`TokenTrack` metric compute, `RunAgg` / `exact_run_metrics` / `robust_score`, the leaf
condition `eval`, `CompiledRule::compile`.

## Divergences (ranked by risk of different numbers)

### Deliberate — accepted, keep

- **D1 · Bounded-tail asymmetry.** Sweep caps a token's series at
   `last_trade + DEAD_QUIET + TAIL_MARGIN`; replay caps at the corpus-wide last trade.
   A quiet-but-liquid token reads `Open` in sweep and closed `Metrics` in simulate.
   Per-token tails keep every series short; a corpus-wide horizon would extend each
   token to the newest trade in the run.
- **D2 · Concurrency / lifetime caps stripped in sweep** (`max_concurrent_tokens: u32::MAX`).
   `n_fired` / `total_pnl_sol` are therefore **upper bounds** vs. a live rule under its
   own caps. Caps make token outcomes order-dependent, which would serialize the rayon
   token fan-out.
- **D3 · Sketched quantiles.** Persisted sweep quantiles come from a 64-bucket DDSketch
   (~15% rel. error); `simulate` and the sweep drill-in compute exact ones. **Ranking is
   unaffected** — `score` is exact. O(1) memory per combo is the point.
4b. **`pnl_percent` is not notional-invariant.** `fixed_cost_sol_per_leg` does not scale
   with trade size, so PnL% is only comparable across runs at the *same* `buy_amount_sol`.
   The notional *chain* itself is consistent (see below).

### Fixed

3. **`as_of` was three different instants** (sweep run start / `run.created_at` /
   request time), so deadness flipped `Open` ↔ `Dead` with wall-clock and a run
   disagreed with its own drill-in as it aged. `simulate_one_combo` already took `as_of`
   but only the `generic` arm used it — tpsl1/tpsl2/swing1 re-derived `Utc::now()`.
   All three now thread `as_of`.
4. **Default notional** — no code change needed; the chain was already consistent:
   request `buy_amount_sol` → stored on the run → drill-in
   `run.buy_amount_sol.unwrap_or(DEFAULT)` → promote drafts the rule at the same
   notional. The 1.0 default only fires when the caller omits it, and a sweep has no
   rule to source a notional from (it explores many combos). Residual = 4b above.
6. **`best_combo` ranking counts open positions.** `marked_pnl_sol =
   total_pnl_sol + open_pnl_sol` drives the `rank_combo` tie-break and the sub-floor
   fallback. **`score` stays realized-only** by design — it is `μ − Z·σ/√n` over CLOSED
   trades, and an open mark is a one-point valuation with no trade distribution; folding
   it in would inflate `n` with a non-observation. This moved the tie-break, not the
   primary key. Pinned by `pnl_tiebreak_counts_open_positions` and
   `score_still_outranks_marked_pnl`.

### Open — not user-approved work, listed so it is not lost

- **No `RunSummary` / `mtm` band in `sweep/`.** The "all 3 surfaces unified" claim is
  half true: the realized band is genuinely shared and test-locked; `mtm` exists only on
  simulate.
- **Guard-test gaps.** The 8 fixtures use single-token streams only, so the concurrency
  caps and global ordering are untestable *by construction*. No mono-kill disarm, no
  `PendingFirstSlot`, no retry/give-up terminals.
- **Fingerprint matching + first-slot gating are absent from sweep** (it compiles
  `Uuid::nil()`), so entry-gate behavior is not swept.
- **Sweep has no row at the `TokenCreated` instant** — `build_series` starts at
  `created_at + TICK`.
- **A third `MetricSeries` constructor** drives the charts
  (`api/handlers/tokens/metric_series.rs`), with no ticks and `created_at` anchored at
  `trades[0]`. Its doc claims parity with sweep/live: true of the values, false of the
  sampling grid and clock origin.

## Performance backlog (grouped sweep) — measured, closed

Both gating smoke runs were executed on 2026-07-19; full numbers in
[perf-measurements.md](perf-measurements.md).

| Item | Verdict |
| --- | --- |
| **P1** `fold_wave_into` per-wave concurrency churn | **Skip** — the fold is the constraint (92.5%) but the cost is not the per-wave setup; hypothesis refuted by measurement |
| **P2** `insert_combos_indexed` redundant sends | **Skip** — `writer_drain` is 11.7 µs; the channel never backs up |
| **P3** Refine sweeps the corpus twice | **Keep as designed** — the coarse pass is what cuts 944,784 combos to 28,389 |

The low-RAM run also **passed**: the degradation ladder shrank the fold budget,
emitted its notice, and still completed 405/405 groups. No refusal, no abort.

What the runs *did* surface is a separate, larger lever — the sweep's own RSS drives
`usable_host_bytes()` to 0 mid-run, so every group takes the slow series-rebuild
path and the documented token-outer "primary path" never executes. Written up with
evidence and options in [perf-measurements.md](perf-measurements.md); it needs a
design decision because it touches RAM-admission safety.
