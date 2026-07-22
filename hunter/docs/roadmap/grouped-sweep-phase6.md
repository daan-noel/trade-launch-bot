# Grouped sweep — Phase 6 completion (fill fidelity → TP/SL migration → re-entry)

Follow-on to [flow-scalper-implementation-plan.md](flow-scalper-implementation-plan.md)
Phase 6. Step 1 (position-scoped exit eval + price-window entry) is **done**; this file
covers the three work items that must land before the Phase 6 **grid** (step 2) produces
numbers worth acting on, and before the sweep can score the strategy the engine actually
runs.

Read [flow-reversion-scalper.md](flow-reversion-scalper.md) for the WHY and the target
numbers. This file is the HOW: exact seams, files, and acceptance gates, self-contained
enough to execute in a fresh session.

Order is deliberate: **A → B → C**. A is small and invalidates the grid until it lands.
B must land as one unit (migration alone regresses perf; classification alone leaves the
SSOT violation). C is its own phase — the scan is cheap, the outcome model is not.

## Anatomy (so you don't re-explore)

| File | Role in this plan |
| --- | --- |
| `lab/src/sweep/generic/strategy.rs` | `GenericSweepStrategy` (holds `cost`, `as_of`, `columns`, `grid`); `resolve_entry` / `resolve_exit` / `resolve_exit_simd` / `resolve_exit_indexed`; `BoundCombo` (bind-time column indices) |
| `lab/src/sweep/generic/exit_index.rs` | `ExitIndex` — prefix-extrema hulls; `first_tp_row(price)` / `first_sl_row(price)` / `dead_row()` / `last_finite_row()`, O(log n) |
| `lab/src/sweep/generic/axes.rs` | `AxisSpec` → `ResolvedAxis`; `entry_key`, `combo_params`; entry axes sorted to high-order digits |
| `lab/src/sweep/generic/guard.rs` | scan ≡ `run_replay` parity + index/SIMD ≡ scalar. Every item below extends it |
| `lab/src/sweep/engine.rs` | `fill_outcomes_with_state` (entry cache + `out.push` per combo); `aggs[combo_id].record(o)` at :527; `combo_batch_size` memory model |
| `lab/src/sweep/aggregate.rs` | `ComboAgg` → core `RunAgg` (streaming DDSketch, O(1)/combo) → `ComboMetrics` |
| `lab/src/sweep/strategy.rs` | `TokenOutcome` (`Copy`), `Strategy` / `ParamSpace` traits |
| `core/src/strategies/paper_fill.rs` | `FillModel {WorstCase, FirstPrint, Signal}`; `find_paper_entry_at(.., model)` / `find_paper_exit_at(.., model)`; the `find_worst_case_*` wrappers |
| `core/src/strategies/kernel.rs` | `CostModel::pumpfun_default()` (with slippage) vs `pumpfun_fee_only()` (:134) |
| `engine/src/arm.rs` | `MetricReq { position_scoped, origin: ReqOrigin }`; `CompiledRule::{has_exit_metrics, exit_fired}` |

Commands: `cargo check -p hunter-lab`, `cargo test -p hunter-lab --lib sweep::generic`,
`--target-dir "C:/Users/User/Documents/Bot/target-check"` when a bin is running. Build the
test target with `-j 2` — a full-parallelism link OOMs this box (pagefile error 1455).

---

## Item A — fill-model + cost-model fidelity (BLOCKER for the grid)

**The sweep is hardcoded to the one fill regime where this strategy loses money.**

`strategy.rs` calls `find_worst_case_paper_entry_at` / `find_worst_case_paper_exit_at`,
which are thin `find_paper_entry_at(.., FillModel::WorstCase)` wrappers — the model
parameter already exists on the real fns, the sweep just never threads it. `paper_fill.rs`
even documents the assumption: *"what live paper and the sweep book."*

That collides with the Phase 4 gate. Same taken set, repriced:
`realFee` = **+0.61** under `first`, **+0.51** under `signal`, **−0.50** under `worst`.
So a grid ranked today ranks combos inside the losing regime. It is **not** a harmless
constant pessimism — worst-in-slot penalises short holds disproportionately, so it biases
the grid toward wide retrace / long holds, the exact direction the deep-dip sweep already
rejected.

**Second half of the same bug: the cost double-count.** `CostModel::pumpfun_fee_only()`
exists (kernel.rs:134, documented for exactly this case) and is used by **nothing** — the
sweep, `replay.rs`, and `strategy_repo.rs` all use `pumpfun_default()`, which charges
`slippage_bps` on top of a fill model that already prices slippage. This is `realA`. It is
**not rank-preserving across combos**: `fixed_cost_sol_per_leg` is per-leg, so a combo that
fires 200 times eats twice the haircut of one firing 100 — it distorts precisely the
comparison a sweep exists to make.

### Steps

1. `GenericSweepStrategy` gains `fill_model: FillModel` beside the existing `cost` field.
   Replace both `find_worst_case_*` call sites with `find_paper_{entry,exit}_at(.., self.fill_model)`.
   (`worst_case_exit_fill` keeps `market_fill_on_empty_window: true`.)
2. Sweep start request gains `fill_model` + a cost-model selector, mirroring
   `EngineSimRequest.fill_model` (`#[serde(default)]` ⇒ `WorstCase` + `pumpfun_default`, so
   every stored/replayed run keeps its current meaning). Thread through `registry::run_grouped`.
3. Persist both on the run row and surface them on the run header — a sweep whose fill model
   isn't visible next to its PnL is a trap. **Two runs under different fill models are not
   comparable**; say so in the UI copy.
4. FE: dropdown next to the existing simulate one (`FILL_MODELS` in `types.ts` is already there).
5. Consider defaulting *new* runs to `first` + `pumpfun_fee_only` — that is the pair the
   Phase 4 gate was measured under. Keep the deserialization default as-is for old rows.

### Acceptance

- Guard: same corpus + rule under each `FillModel` still matches `run_replay` configured
  with that same model (extend `assert_parity` to take a `FillModel`). This is the real
  lock — replay already threads `ReplayConfig.fill_model`.
- A `first`-fill sweep over the eval cohort reproduces the `flow_scalper_fill_sensitivity`
  sign (+, not −) for the anchor combo. If it doesn't, stop: the sweep and the harness
  disagree about something else.
- Cost selector: `pumpfun_fee_only` + a fill model must equal the harness's `realFee`.

---

## Item B — TP/SL → `m_position.pnl` migration + bind-time req classification

Two goals that must be solved together.

**Goal 1 (SSOT).** Phase 2 collapsed TP/SL into position `pnl` reqs in the engine, but the
sweep still fires them from the surviving `CompiledRule::{take_profit, stop_loss}` sugar via
its own `entry_price · (1 ∓ pct/100)` branch. Two representations of one fact — the exact
thing the repo's SSOT rule forbids. It was kept for a minimal Phase-6 diff; that was the
wrong call.

**Goal 2 (perf, and a regression to repair).** `has_exit_metrics()` is
`!exit_reqs.is_empty()`, and desugaring means **it is true for every rule carrying
`take_profit` or `stop_loss`** (verified: a pure TP+SL rule compiles to `exit_reqs = 2`).
Both `resolve_exit_indexed` and `resolve_exit_simd` early-return to scalar on that flag, and
`build_exit_ctx` only builds the `ExitIndex` when it is false. **So the O(log n) exit index
and the AVX-512 scan have been dead for all TP/SL rules since Phase 2 — silently.**
Correctness was never affected (scalar is the SSOT reference the guards compare against),
which is why nothing caught it.

Migrating naively makes this worse. Classifying fixes both.

### Design: classify exit reqs at bind time

Replace the blunt `has_exit_metrics()` branch with a per-req classification computed once in
`BoundCombo::new` (alongside the existing column indices — same place, same cost model):

| Class | Recognised from | Resolution |
| --- | --- | --- |
| **Static price threshold** | position `pnl` req with a single `>=` / `<=` bound (incl. the desugared TP/SL) | derive `entry · (1 ± pct/100)` → existing `ExitIndex::first_{tp,sl}_row`, **O(log n)** |
| **Static time threshold** | position `held` req with a `>=` bound | `entry_at + X` → binary search on `series.at`, **O(log n)** — new capability, `held` is unsupported by any fast path today |
| **Trailing** | position `retrace` req with a `>=` bound | running max + compare; **O(n)**, vectorizable (below) |
| **General** | token-scoped column reqs, multi-arm DNF, tolerance-sensitive, anything unrecognised | scalar walk — unchanged, and stays the SSOT reference |

Then the exit row is `min` across whichever classes are present, with the existing
`Dead > SL > TP > authored` tie-break. `ReqOrigin` still supplies the `StopLoss` /
`TakeProfit` label, so exit codes and analytics are unchanged.

Delete the `c.stop_loss` / `c.take_profit` branches from `resolve_exit` once every rule's
TP/SL arrives as a classified `pnl` req. **Keep the `CompiledRule` sugar fields** — they are
still the authoring/DB/FE surface; only the sweep's *evaluation* of them goes away.

### AVX-512 for the trailing stop

The trailing stop's first crossing is `first j where price[j] <= k · max(price[fill..j])`,
`k = 1 − R/100`. Same shape as the existing `first_exit_row_avx512`, plus a prefix-max:

1. 8-lane inclusive prefix-max over `price` (Hillis-Steele: 3 shift-and-max steps via
   `_mm512_permutexvar_pd`), seeded with the carried block max.
2. Compare `price <= prefix_max · k` (ordered compare so NaN never matches), `AND` with the
   finite mask, `OR` the dead mask.
3. `tzcnt` the first hit; carry `max` into the next block; scalar `< 8` remainder.

**Do not claim O(log n) for the trailing stop** — it is not a static prefix query and there
is no cheap index for it. Vectorized O(n) is the honest target. If it is still the hot spot
afterwards, the upgrade is one structure per (token, entry-key) answering *all* retrace
values at once, not a per-combo index.

### Acceptance

- Every new fast path proves scalar-equivalence exactly like the existing SIMD/index guards:
  crafted arrays exercising the 8-lane block boundary, the remainder tail, NaN/±inf, and a
  cross planted at every index.
- **Regression lock:** a pure TP/SL rule must now take the index path. Assert
  `index.is_ready()` for a TP-only rule — that is the assertion whose absence let this rot.
- Existing `guard.rs` parity tests unchanged and green (outcomes must be byte-identical —
  this is a refactor, not a behaviour change).
- Benchmark `resolve_exit` before/after on a real grid; it is the measured hot spot.

---

## Item C — re-entry in the grouped sweep

The engine has re-entry (Phase 4: `RuleParams.reentry`, `ArmState::Cooldown`, per-token
episode counter). The sweep does not — a swept combo scores one episode per token, so any
re-entry rule is mis-scored.

### The scan is the easy part

Episodes are **sequential and non-overlapping**, so the cursor moves monotonically:
`entry → exit → cooldown_until → re-arm → resume search`. A multi-episode scan over a token
is still **one forward pass, O(n) total** — not O(episodes × n). Re-entry costs nothing
asymptotically. Mirror `reduce.rs`: re-arm on **normal exits only** (TP/SL/Metrics — never
Dead/Manual/Migrated), and honour `cooldown_sec`.

**Cap semantics.** The sweep deliberately strips `max_concurrent_tokens` / `max_total` (they
would serialize the token fan-out — documented in `compile_combo`, do not "fix"). But
`max_episodes_per_token` is **not** a concurrency cap; it is part of the strategy's identity
and bounds a per-token quantity the sweep already evaluates per token. **The sweep must
honour it.** It is also what makes the memory model below computable.

### The hard part 1: it breaks the entry cache

Today `resolve_entry` is resolved once per (token, distinct entry-key) and reused across
every exit-variant combo (`engine.rs:48-59`; entry axes are the high-order combo digits so
same-entry combos are contiguous). That is a large constant-factor win — with 100 entry ×
20 exit combinations it is 20× fewer entry scans.

With re-entry, only **episode 1** is a pure function of the entry key; episode 2's entry
begins after episode 1's exit, which depends on exit params. The cache silently stops
applying.

**Fix: cache an entry-eligible row bitmap, not a resolved entry.** For a given (token,
entry-key), precompute a bitset over series rows where the entry conditions hold. That *is*
a pure function of the entry key regardless of episode count, so all the expensive shared
work (evaluating entry reqs across every row) survives. Each combo's episode loop becomes
"find next set bit ≥ cursor" — a word scan + `tzcnt`.

The per-combo half of `can_enter` (*exit metrics must not already hold*) depends on exit
params, so it stays per-combo — but it is now only evaluated at candidate rows, not every
row. Net: this should be **faster than today's cache**, not merely re-entry-compatible.
Memory: `n_rows / 8` bytes per entry-key, rebuilt on the same cadence as today's cache.

### The hard part 2: the outcome transport

`TokenOutcome` is `Copy`, one per combo, consumed positionally
(`aggs[combo_id].record(&outs[combo_id])`, `engine.rs:527`) across a producer→folder channel,
and `combo_batch_size` budgets `batch × sizeof(TokenOutcome)`. Variable episodes per combo
break all three.

**Do not aggregate episodes inside the scan.** `RunAgg` is a streaming fixed-size DDSketch
(O(1) per combo), so folding **each episode as its own `record` call** yields per-episode win
rate, median and p90 for free — which is exactly what the analysis reasons in (median gap
~31 s, up to 31 episodes/token, per-episode edge). Collapsing to a token-level sum would
silently redefine `win_rate` as "token was net positive" and corrupt every ranked column.

Recommended shape:

1. Stamp `combo_id` on `TokenOutcome` (or ship a parallel `episode_counts` vector) so the
   channel is self-describing and the folder stops relying on position.
2. Emit N outcomes per (combo, token); folder loop becomes `aggs[o.combo_id].record(o)`.
3. Update `combo_batch_size`: the per-combo term becomes
   `sizeof(ComboAgg) + inflight × max_episodes × sizeof(TokenOutcome)`. Bounded **because**
   `max_episodes_per_token` is honoured — absent re-entry it is 1 and the model is unchanged.
4. Drill-in + chart markers assume one entry/exit pair per token — they need to render N.

### Acceptance

- Guard: a re-entry rule's scan ≡ `run_replay` **episode for episode** (same count, same
  per-episode entry/exit price and reason), on a fixture with ≥3 episodes and one that hits
  the episode cap.
- One-shot rules (`reentry: None`) produce byte-identical results to today — the existing
  `guard.rs` suite is the non-regression.
- Cooldown boundary: an exit and a re-entry signal inside the same cooldown window must not
  re-enter; the first eligible row at/after `until` must.
- Dead/Manual/Migrated must **not** re-arm.
- Bitmap cache: a combo scanned with a shared bitmap must equal the same combo scanned
  standalone (the `shared_bind_matches_per_token_bind` pattern — detach the cache from the
  token, which is the only way this class of bug surfaces).

---

## Gotchas

- **The sweep is a parallel impl of the fold.** Replay/simulate inherit engine changes free;
  this scan does not. Never claim "backtested" from a sweep until `guard.rs` covers the new
  path.
- **Scalar is the SSOT.** Every fast path (index, SIMD, bitmap) must be provably equal to the
  scalar walk, and the scalar walk must never be deleted. Item B's regression exists because
  a fast path could silently stop being *taken* — assert reachability, not just equality.
- **Fill model and cost model are part of a run's identity.** Persist and display them; two
  runs under different models are not comparable, and neither is a sweep vs a simulate.
- **`resolve_exit` is the measured hot spot**, not `prepare_token`. Spend effort there.
- **Test-build OOM:** build test targets with `-j 2`; full parallelism hits pagefile error
  1455 on this box.
- **Don't lower the fast-path bar to make a guard pass.** If a class can't be recognised
  safely (tolerance-sensitive `=`, multi-arm DNF, `!=`), it belongs in **General** — a
  correct scalar walk always beats a clever wrong index.
