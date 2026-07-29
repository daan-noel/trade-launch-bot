# Sweep ↔ simulate parity — divergences, decisions, open items

Reference for *why* the grouped sweep and `simulate` can report different numbers for
the same rule + token, which of those gaps are deliberate, and what is still open.
Audit date **2026-07-19**, extended 2026-07-26 (D0 + D1). High-level summary lives in
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
- **D4 · Single-position-per-token exclusivity ignored in sweep** (`RuleParams.exclusive`
   / `priority`). `reduce` lets an `exclusive` rule stand down while ANY other arm on the
   token holds a position, with `priority` deciding who claims it when two contest the
   same event. The sweep enforces neither: `AxesModel::assemble` never sets the fields,
   and each combo/token is scanned independently in a rayon fan-out with **no shared
   cross-combo state** — exclusivity needs cross-*rule* state at the same instant, which
   is D2's problem one level harder (it would require a globally time-ordered fold over
   the whole corpus, exactly what the sparse-grid design avoids). So a sweep over
   exclusive rules reports the same un-deconflicted upper bounds D2 already describes:
   **re-run a promoted exclusive combo through simulate before trusting its PnL.**
   Guard: `sweep_ignores_exclusivity_but_the_engine_enforces_it` — both exclusive combos
   fire in `scan`, only the priority/id winner enters through `reduce` (with a
   flag-dropped non-vacuity leg). It drives `reduce` directly rather than `run_replay`
   because the lab replay driver keys in-flight fills by *mint* and so cannot carry two
   concurrent positions on one token whatever the engine decides.
- **D5 · Scale-out in-flight-sell blindness absent in sweep (2026-07-29).** The live /
   replay fold stays `ExitPending` until a partial fill confirms (and may defer that
   confirm to a later adverse print in the fill window) — no new decision while the
   sell is in flight. The sweep's staged resolver books each partial instantly and
   resumes from the next series row, so a global exit that becomes true on the trade
   *after* a stage fire is taken immediately. Same-batch confirms (fill window =
   fire trade) match byte-for-byte; deferred fills can diverge. Guard:
   `scan_matches_replay_scale_out_two_stage` + `…_global_sl_mid_ladder` (trades spaced
   past `MAX_FILL_WAIT_SLOTS` so every fill collapses to the fire print).
- **D6 · Scale-out frozen-tail (D1) not applied (2026-07-29).** A rule with
   `scale_out` that leaves the in-series scan `Open` does **not** get the analytic
   quiet-tail clock resolve — a stage / remainder `held` that would only fire past
   the per-token cut stays `Open` in the sweep. Legacy (no scale-out) keeps full D1.
   Re-measure staged ladders through simulate when the close lives in the quiet tail.
- **D3 · Sketched quantiles.** Persisted sweep quantiles come from a 64-bucket DDSketch
   (~15% rel. error); `simulate` and the sweep drill-in compute exact ones. **Ranking is
   unaffected** — `score` is exact. O(1) memory per combo is the point.
4b. **`pnl_percent` is not notional-invariant.** `fixed_cost_sol_per_leg` does not scale
   with trade size, so PnL% is only comparable across runs at the *same* `buy_amount_sol`.
   The notional *chain* itself is consistent (see below).

### Fixed

- **D0 · Entry-cache poisoning across exit variants — FIXED (2026-07-26).** This one was
   a **bug, not a divergence**: the fold's single-slot entry cache
   (`sweep/engine.rs::fill_outcomes_with_state`) is keyed by `AxesModel::entry_key` =
   the *entry-axis picks only*, but `resolve_entry` mirrors the engine's `can_enter`
   veto — never buy while the exit conditions already hold — so the resolved entry is
   **exit-dependent**. With `order_for_entry_cache` making same-entry combos contiguous,
   the first combo of every entry class resolved entries under *its own* exit veto and
   every sibling silently inherited that entered set: wrong `n_fired`, wrong entry rows,
   wrong prices, wrong crown.

   *Proof (run `593844a2`, fp-scoped, 248 tokens):* combos 655360..79 share entry params
   and all stored `n_fired = 55` — 655360's honest number (its `buy(30s)<3 | buy(60s)<1`
   exits veto exactly the quiet-market rows its own entry needs). A fresh drill-in of the
   promoted 655362 (exits `trail(5s)>30 | liq>85`, veto ~never) under the same
   corpus/`as_of`/pricing gives **101 entered**, agreeing with an independent engine
   simulate (100/250). First-in-class combos reproduce their stored rows exactly
   (589824: 93↔93; 655360: 55↔55) — aggregate-vs-own-drill-in disagreement is the
   signature. True numbers for the promoted combo: 101 fired / 19 closed / 82 open,
   realized +0.219 vs stored 55 / 14 / 41, +0.246.

   *Fix:* the fold caches only what is provably exit-independent. `Strategy` grew a
   two-stage entry — `entry_candidates` (Stage A: the dead / mono-kill / entry-cond
   walk, opened once per class per token and **resumed** on demand so the first-row
   short-circuit survives) and `resolve_entry_from` (Stage B: per combo, walk the shared
   candidates applying that combo's veto, then a per-class fill memo). `ExitCtx` is
   rebuilt on the resolved `fill_row` (`Strategy::exit_ctx_key`) rather than on entry-key
   staleness, which per-combo entries made unsound as well as wasteful. Pure TP/SL
   sweeps are bit-for-bit untaxed: position-scoped exit reqs read `NaN` before entry and
   can never veto (`BoundCombo::entry_veto_possible`), so Stage B is a candidate lookup
   plus a memo hit. `resolve_entry` survives as the fused SSOT reference, and Stage B is
   asserted equal to it on **every** resolution under `cfg(test)`.

   *Why no guard caught it:* the whole suite drove one combo at a time, so the fold's
   cache was invisible by construction. The new locks are
   `guard::fold_gives_each_exit_variant_its_own_entry` (a real 3-combo `AxesModel`
   through the real fold, both combo orders, each combo ≡ its own `scan` ≡ its own
   `run_replay`, plus a non-vacuity assert that the three exit variants resolve three
   different entries) and `engine::tests::fold_reresolves_entry_per_exit_variant_within_one_class`
   (the same property for the strategy-agnostic fold). Both were confirmed to **fail**
   with the old caching restored.

   *Blast radius:* every stored grouped run with exit-side metric axes is suspect for
   every combo that was not first in its entry class. The code fix does not repair them
   — re-run, and expect rankings to move (that is the fix working).
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
  concurrency ordering is still out of scope. `fold_gives_each_exit_variant_its_own_entry`
  is the first **multi-combo** guard (it drives the real fold, which every single-combo
  fixture bypasses — that blind spot is what let D0 ship).

### Comparison-context traps (workflow, not code)

Two ways a sweep and a simulate of the same rule disagree without either being wrong.
Both were mistaken for the D0 bug during its investigation, so they are recorded here:

- **Notional × fixed per-leg cost.** Sweep defaults to `buy_amount_sol = 1.0`
  (`registry::SWEEP_DEFAULT_BUY_AMOUNT_SOL`); a promoted rule may trade 0.01. Every cost
  model except `frictionless` charges `fixed_cost_sol_per_leg` = `JITO_MIN_TIP_SOL` +
  avg CU priority fee ≈ 0.001025 SOL/leg ≈ 0.00205 per round trip — **0.2 % of notional
  at 1.0 SOL, 20.5 % at 0.01**. Breakeven gross move goes from ~+2.2 % to ~+22.6 %, which
  alone flips win% and PnL sign. `pumpfun_fee_only` drops `slippage_bps`, never the fixed
  cost. Sweep at the notional you intend to trade (this is residual 4b above).
- **Different corpus / different pricing.** The sweep is LakeSource-only; `simulate`
  splices the fresh PG tail (`sim_fetch::pg_tail_beyond_lake`), so a stale export leaves
  the sweep holding `Open (est)` rows at hours-old prices that the sim watches die.
  Re-export the lake before sweeping. `SimulatePage` also resets to
  `worst_case` + `pumpfun_default` on every page load, so a rule simulated right after
  promote can silently run under different fill/cost than the run that crowned it — check
  the chips. **Surfaced (2026-07-26):** a run now stores its corpus-wide max
  `block_time` (`corpus_last_trade_at`, lab migration `0011`, from the SSOT
  `Corpus::last_trade_at` the frozen-tail horizon also reads) and the run panel shows a
  **Data through** row — warning when the lake was ≥1 h behind the run's start — with the
  `est` badge's tooltip pointing at it. Still open: have a simulate launched from a
  promoted rule inherit the source run's fill/cost pair and warn when the notionals
  differ (`SimulatePage` resets both on every page load).

### Verifying a suspected sweep↔sim gap (the D0 method)

1. Stored run config — pricing, corpus window, fingerprint scope — is on
   `grouped_sweep_runs`; per-combo aggregates + params are `grouped_sweep_results` joined
   to `grouped_sweep_combos`.
2. Honest re-sim of one combo (no fold, no entry cache — threads the run's own pricing,
   `as_of` and corpus):
   `GET /api/strategies/sweeps/{run}/groups/{gid}/token-results?strategy_id=generic&combo_id=N`.
3. Ground truth for a rule: `$SWEEP_LAKE_DIR/sim-results/{rule_id}.{meta,rows}.json`.
4. Lake freshness: `$SWEEP_LAKE_DIR/trades/dt=*/_meta.json` `exported_at_utc` vs PG
   `max(trades.block_time)`.
5. **The tell:** a stored row that disagrees with its *own* drill-in is a fold bug; rows
   that reproduce exactly but disagree with a simulate are a context/divergence issue
   from the lists above.
- **Fingerprint matching + first-slot gating are absent from sweep** (it compiles
  `Uuid::nil()`), so entry-gate behavior is not swept.
- **Sweep has no row at the `TokenCreated` instant** — `build_series` starts at
  `created_at + TICK`.
- ~~**A third `MetricSeries` constructor** drives the charts
  (`api/handlers/tokens/metric_series.rs`), with no ticks and `created_at` anchored at
  `trades[0]`. Its doc claims parity with sweep/live: true of the values, false of the
  sampling grid and clock origin.~~ **CLOSED 2026-07-27.** The no-ticks half was a live
  bug, not just a caveat: with rows only at trade instants, every time-decaying metric is
  sampled exactly where a fresh trade has just been folded back in, so a between-trades
  crossing is invisible and the chart's condition-fire marker lands *late*. Measured on
  `8HJNtq7k…hpump` under rule `promoted g0 c92432` (`m_flow_window@60 buy < 5`): the chart
  drew the exit at 19:54:22, simulate booked it at 19:53:12 — **70 s** apart, because the
  window dipped under 5 during a 1.3 s gap between two trades.
  Fixed by extracting the sweep's sparse tick grid to `hunter_engine::metrics::grid`
  (`SparseGrid` + `fold_sparse` + `estimate_sparse_rows`) and driving **both** the sweep
  precompute and the chart endpoint through it — one loop, so a trade-only fold cannot be
  reintroduced in one caller. The endpoint takes the rule's `time`/`stall` condition
  ceilings as query params to size the grid (windows are implied by `windows`), and bounds
  the response at `MAX_SERIES_ROWS`, reporting `truncated` / `covered_until` rather than
  silently returning a short series. The clock origin stays at `trades[0]` (the dev-buy
  slot) — same as the replay driver. Locked by
  `grid::tests::ticks_expose_a_between_trades_window_dip_that_a_trade_only_fold_hides`;
  every `scan_matches_replay_*` guard still passes, so the sweep is unchanged.

## Performance backlog (grouped sweep) — measured, closed

The P1–P3 perf backlog was gated by two smoke runs on 2026-07-19 and **closed** (P1/P2
skip, P3 keep-as-designed); the low-RAM run passed the degradation ladder, and the runs
surfaced a larger lever — the sweep's own RSS drove `usable_host_bytes()` to 0 mid-run,
now **fixed**. Full numbers, verdicts, and the abort-safety argument live in
[ram-sizing.md → Measured performance](ram-sizing.md#measured-performance-2026-07-19).
