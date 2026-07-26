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

- **D1 · Bounded-tail asymmetry — CLOSED (2026-07-26).** The sweep still caps each
   token's series at its OWN `last_trade + DEAD_QUIET + TAIL_MARGIN` (that keeps every
   series short — extending the *tick grid* to the corpus horizon is the RAM cost the
   sparse-grid design exists to avoid). Instead, when the in-series scan leaves a
   position `Open`, `resolve_frozen_tail` (`sweep/generic/strategy.rs`) resolves the
   quiet tail **analytically in O(1)**: at a frozen price only the rate-1 clocks move
   (`time` since creation, `stall` since the last high, `held` since entry), so the
   earliest deterministic crossing up to the corpus-wide horizon — `min(as_of,
   corpus_last_trade + DEAD_QUIET + TAIL_MARGIN)`, the same cap `run_replay` uses — is
   computed and booked at the last trade's market fill (byte-identical to the exit
   `run_replay`'s `queue_exit_fill` books). `Dead`, SL/TP and price-movement exits can
   never newly fire on a flat price, and a windowed exit metric (which *does* keep
   changing in the tail) conservatively keeps the legacy `Open` — the one remaining
   residual, documented at the fn. The horizon is opt-in per run
   (`GenericSweepStrategy::set_corpus`); the drill-in threads the same horizon
   (`frozen_tail_horizon` over its token set) so a row's exit matches the aggregate the
   user clicked. Locked by `guard::scan_matches_replay_multi_token_frozen_tail` (the
   first **multi-token** guard — fails if the resolve is reverted) and
   `held_frozen_tail_matches_across_exit_paths` (scalar ≡ index ≡ SIMD in the tail).
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
6. **`best_combo` ranking includes open positions in score.** Checklist `score` uses
   MTM% (opens included) × fire-rate × open-drag × win-rate; `marked_pnl_sol` remains
   the tie-break. Pinned by `pnl_tiebreak_counts_open_positions` and
   `score_still_outranks_marked_pnl`.

### Open — not user-approved work, listed so it is not lost

- **No `RunSummary` / `mtm` band in `sweep/`.** The "all 3 surfaces unified" claim is
  half true: the realized band is genuinely shared and test-locked; `mtm` exists only on
  simulate.
- **Guard-test gaps.** Most fixtures use single-token streams, so the concurrency caps
  and global ordering are untestable *by construction*. No mono-kill disarm, no
  `PendingFirstSlot`, no retry/give-up terminals. **Partly closed (2026-07-26):**
  `scan_matches_replay_multi_token_frozen_tail` is the first multi-token guard — it
  compares the sweep aggregate against `run_replay` over a ≥3-token corpus and so covers
  the corpus-wide tail case (the D1 fix); caps stay `∞` on both sides there, so
  concurrency ordering is still out of scope.
- **Fingerprint matching + first-slot gating are absent from sweep** (it compiles
  `Uuid::nil()`), so entry-gate behavior is not swept.
- **Sweep has no row at the `TokenCreated` instant** — `build_series` starts at
  `created_at + TICK`.
- **A third `MetricSeries` constructor** drives the charts
  (`api/handlers/tokens/metric_series.rs`), with no ticks and `created_at` anchored at
  `trades[0]`. Its doc claims parity with sweep/live: true of the values, false of the
  sampling grid and clock origin.

## Performance backlog (grouped sweep) — measured, closed

The P1–P3 perf backlog was gated by two smoke runs on 2026-07-19 and **closed** (P1/P2
skip, P3 keep-as-designed); the low-RAM run passed the degradation ladder, and the runs
surfaced a larger lever — the sweep's own RSS drove `usable_host_bytes()` to 0 mid-run,
now **fixed**. Full numbers, verdicts, and the abort-safety argument live in
[ram-sizing.md → Measured performance](ram-sizing.md#measured-performance-2026-07-19).
