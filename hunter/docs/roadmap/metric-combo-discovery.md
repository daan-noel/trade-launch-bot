# Metric-combo discovery pipeline

**Goal.** A repeatable, automated way to find the metric/param combos that actually
make money for a chosen token cohort — ranked by *profit × frequency × stability*,
guarded against overfitting, and **registry-driven** so a metric added later flows
through with no pipeline edit.

**Owner surface.** Lab only (`hunter-lab`), built on the existing grouped-sweep
engine + lake corpus. Nothing ships to EC2. Live/paper are untouched — this is an
analysis aid that *outputs* combos to promote, exactly like the sweep does today.

**Status.** **COMPLETE.** All six steps shipped (see §8): objective re-ranker,
candidate generation, Layer-1 screen, Layer-2 family grid + interaction check, Layer-3
out-of-sample validation, and the lab pipeline endpoint + page. The pipeline runs
screen → family → validate over one corpus load and surfaces shortlist → combos →
verdicts with a one-click promote into the shared rule editor.

Related code (grounding — read before touching):
- Metric SSOT: [`engine/src/metrics/mod.rs`](../../engine/src/metrics/mod.rs) (`REGISTRY`)
- Axes → combos: [`lab/src/sweep/generic/axes.rs`](../../lab/src/sweep/generic/axes.rs)
- Sweep driver + RAM ladder: [`lab/src/sweep/registry.rs`](../../lab/src/sweep/registry.rs)
- Scoring SSOT: [`core/src/strategies/kernel.rs`](../../core/src/strategies/kernel.rs) (`checklist_score`, `RunMetrics`)
- Persisted combo row: [`core/src/models/grouped_sweep.rs`](../../core/src/models/grouped_sweep.rs) (`ComboMetrics` / `GroupedSweepResult`)
- Candidate anchors (hand-derived today): [`docs/plans/sweep/axis-value-candidates.md`](../plans/sweep/axis-value-candidates.md)
- Prior art for a scoring lab job: [`lab/src/strategies/flow_discovery.rs`](../../lab/src/strategies/flow_discovery.rs)

---

## 0. What already exists (so we don't rebuild it)

The redesign already gives us most of the machinery. The pipeline is **orchestration
+ a robust objective + automated candidate generation** on top of it — not a new engine.

| Need | Already there |
| --- | --- |
| "Which metrics exist" | `REGISTRY` — one const, every layer derives vocabulary from it. Adding a metric surfaces it in axes + FE with zero change. |
| Run a param grid over a cohort | `run_grouped` / `GenericSweepStrategy` — precompute `MetricSeries` columns once/token, scan combos, fold to one `ComboMetrics` row/combo. |
| The `off` sentinel (with-vs-without a gate) | `axes.rs` — a `null` axis value omits that condition. Marginal value of a gate is read straight off the ranked table. |
| Profit / frequency / win-rate / open-position accounting | `ComboMetrics` persists **all** ingredients: `n_fired`, `n_open`, `n_closed`, `win_rate`, `total_pnl_sol`, `open_pnl_sol`, `mean/median/p90/best/worst_pnl_pct`, `std_pnl_pct`, `profit_factor`, `mtm_pnl_pct`, `expectancy_sol`, holding. |
| A blended objective | `checklist_score = mtm_pnl_pct × fire_rate × (1 − 0.5·open_drag) × win_rate` — profit × frequency × win-rate already. |
| Re-simulate one combo on a token slice | `simulate_one_combo` — the Layer-3 validation primitive. |
| Don't-OOM-the-box | RAM degradation ladder (`plan_sweep_sizing`), bounded rayon (`cores − 2`). |

**Two real gaps** the pipeline must close:

1. **Candidate values are hand-derived.** Today someone runs throwaway DuckDB
   percentile queries and records anchors in `axis-value-candidates.md`. There is
   **no** percentile query in `duck.rs`. A new metric has no menu until a human
   derives one → the single biggest extensibility hole.
2. **The objective's profit term is a mean.** `mtm_pnl_pct` is the *mean* per-trade
   pnl% over all fired positions. Two whale winners inflate it. That is exactly the
   "one or two big profits" the objective is supposed to *not* reward.

---

## 1. The objective function (effectiveness core)

Everything ranks on one score. It must encode the three things you asked for —
**profit (incl. still-open), frequency, and stability (many small wins ≫ one whale)** —
and it must be an honest number, not gameable by leaving losers open.

### 1.1 Compute it as a re-rank, not a kernel edit

`checklist_score` is SSOT — live, paper, and sweep all fold through it. Mutating it
would silently move live rankings. Instead, the pipeline's objective is a **pure
function of the already-persisted `ComboMetrics` columns**, applied as a post-hoc
re-rank. No kernel change, no migration, and it works on any stored run.

```
DiscoveryScore(row, matched) =
      robust_profit(row)                       # stability-weighted profit, incl. open marks
    × fire_rate(row, matched)                  # frequency
    × win_component(row)                        # consistency
    × min_n_gate(row)                           # 0/1 — kills thin cells
```

### 1.2 Each term, explicit

- **`robust_profit`** — replace the outlier-prone mean with a **median-anchored,
  open-aware** figure. Use `median_pnl_pct` as the center (already persisted; open
  marks are excluded from it, so pair it with an open-inclusive adjustment):

  ```
  robust_profit = median_pnl_pct · closed_share
                +  mtm_open_pct   · open_share · OPEN_HAIRCUT
  ```
  where `closed_share = n_closed/n_fired`, `open_share = n_open/n_fired`, and
  `mtm_open_pct` is the mean mark over opens (derivable: `mtm_pnl_pct` and
  `mean_pnl_pct` + counts back this out, or persist it). `OPEN_HAIRCUT ∈ [0,1]`
  discounts unrealized marks so a combo can't look good on paper gains it never
  took. **Why median:** "small profit on many tokens" scores as high as it should;
  a combo that is +5% on 40 tokens beats one that is +400% on 2 and −20% on 38.
- **`fire_rate = min(1, n_fired / matched)`** — frequency. Identical to the kernel's
  term; `matched` is the group's token count, so a combo that fires on 3 of 500
  tokens is penalized 166× vs one that fires on 500. This is what makes "fires
  often" a first-class goal, not an afterthought.
- **`win_component`** — a *blend* of `win_rate` and downside control, not raw
  win-rate. `win_rate · clamp(profit_factor)`. Win-rate alone rewards 99 tiny wins
  + 1 ruinous loss; multiplying by a capped `profit_factor` (gross win ÷ gross loss)
  demands the wins actually outweigh the losses. Floor both so an all-open book
  isn't zeroed outright.
- **`min_n_gate`** — hard 0/1. A combo with `n_closed < MIN_CLOSED` (e.g. 20) scores
  **0**, full stop. This is the anti-overfit backbone: an "edge" on 4 closed trades
  is noise, and no amount of profit% should let it rank. Report how many combos the
  gate killed (never silently drop — see the no-silent-caps rule).

### 1.3 Stability, measured two ways

Median already buys within-combo stability. Add **across-neighbor stability** in
Layers 1–2: a combo whose score collapses when you nudge one threshold to the
adjacent candidate is a spike (overfit); one that holds is a plateau (robust). The
response-curve analysis (§2.2) computes this; it feeds a small penalty/annotation,
not a hard cut.

> **Decision needed (D1):** exact weights (`OPEN_HAIRCUT`, `profit_factor` cap,
> `MIN_CLOSED`, floors). These are tunable constants; I'll seed defaults from the
> `axis-value-candidates.md` anchors and let a run override them. Pin the final set
> in `docs/plans/sweep/` once validated.

---

## 2. Layer 1 — Univariate screen (build first)

**Purpose.** Before any grid, learn *which metrics have an edge at all*, at what
threshold, in what direction — and throw out the dead ones. This shrinks every
downstream grid and is where the overfit guards live.

### 2.1 Automated candidate values (closes gap #1) — **BUILT**

`lab/src/discovery/candidates.rs`: `screen_plan` (registry → screenable metrics +
skip reasons) → `collect_percentiles` (measured `[p05..p99]` ladder per metric) →
`build_menus` (percentile-spaced `p10, p25, p50, p75, p90` + the `off` sentinel,
rounded by `unit`) → `MetricCandidates::axis_spec` hands a menu straight to
`AxesModel`. The exact recipe `axis-value-candidates.md` documents by hand, now
generated.

**Where the percentiles come from (design change vs. the original sketch).** This was
planned as `metric_percentiles` in `lake/duck.rs` — one DuckDB `approx_quantile`. It
isn't, deliberately: only `time`/`liquidity` are raw lake columns; `trail`, `stall`,
the rolling-window flows and the price-window extrema are *engine* quantities. Writing
them as DuckDB window functions would be a **second implementation of metric
semantics** that can silently drift from `hunter_engine` (SSOT rule), and the published
anchors would then describe values the screen never actually gates on. The ladder is
instead measured through the engine's own `MetricSeries` compute — the exact numbers
the Layer-1 scan reads — over the same per-token precompute the screen needs anyway
(§6.1). Cost keeps its shape: one corpus load, one series pass. Nearest-rank quantile
is the kernel's `exact_quantile_f64` (now `pub`), so anchors and reported medians are
one statistic.

Sampling: values are taken at **trade moments** (trades folded, no synthetic ticks) —
what the hand-derived table measured. Weighting by wall-clock silence would drag every
menu toward dead-token values. RAM is bounded per metric by a deterministic decimating
reservoir (`sample_cap`, default 200k ⇒ ~1.6 MB/column): no RNG, so the same corpus
yields the same menu on every run.

Registry-driven specifics (all read off `MetricSpec`/`GroupSpec`, so a new metric is
handled automatically) — as built:
- **unit** picks the rounding step, widening with magnitude (`round_for_unit`), so a
  menu reads like the hand-authored ones (`5/8/15/25` %, `30/60/120/300` s).
- **operator direction is measured, not inferred.** `DirectionPolicy::Both` (default)
  screens `>=` and `<` per metric — the plan already treats the operator as an *output*
  of the screen (§2.2 "keep, with the suggested operator"), and the anchor table shows
  the same metric earning a gate in either direction by side (`liquidity >` floor vs
  `liquidity <` pre-migration cap). It stays additive (2 × 5 values), never a product.
- **dynamic** groups (`window_size_sec`) → screened at the run's per-side window
  (30 s entry / 10 s exit by default); windows are cross-run, not swept in one run.
- **position-scoped** (`m_position`) → **exit** side only, and their values are
  *declared* (`POSITION_MENUS`): a position metric anchors on your entry fill, so no
  token-independent distribution exists to measure. This is the "one explicit
  annotation" §5 predicts. `m_position.pnl` is skipped entirely — it **is** the
  baseline TP/SL (they desugar into it), already carried by every screening combo.
- **flow-split** (`m_flow_split*`) → needs `volume_ix_patterns`; skipped unless a
  pattern set is supplied for the run.
- **Nothing is dropped silently.** Every registry metric × side is either screened or
  carries a `SkipReason`; a measured metric with no usable menu is reported as a
  `MenuGap` (`NoSamples` / `Degenerate`). A guard test asserts screened-xor-reported
  over the whole registry, so a metric added later can't go quietly unscreened.

### 2.2 The screen itself (additive, not multiplicative) — **BUILT**

`lab/src/discovery/screen.rs`: `run_screen` = `screen_plan` → `collect_percentiles` →
`build_menus` → one additive sweep → `DiscoveryScore` per pick → verdict. The scan mode
is `ScreenStrategy`: N per-metric sub-models presented as **one flat combo space over
one shared precompute** (`GenericSweepStrategy::share_precompute` widens every segment
onto the column union + widest grid), so the engine builds each token's `MetricSeries`
once for the whole screen and reuses its RAM ladder / wave driver / cancellation
verbatim — D2 resolved in favour of the scan mode, and it is a scan mode, not a second
engine.

**Why the percentile pass is still its own pass** (revising step 2's finding (a)): the
menus are an *input* to the sparse grid's `time`/`stall` horizons, so one fused pass
would have to size the grid from percentiles it hasn't measured yet. The percentile
pass folds trades only (no synthetic ticks) — the cheap half — and the corpus is still
loaded once for both. The reuse that actually mattered (N−1 precompute passes across
the metric screens) is fully realised.

The description below is what it does.


For a chosen cohort, sweep **each metric alone** across its candidate menu with a
fixed baseline TP/SL, and record the `DiscoveryScore` response curve per metric.

Crucially this is a **sum over metrics, not a product**: N metrics × ~6 values =
~6N combos total, *not* 6^N. Two ways to get there, cheapest first:
- **Preferred:** one screening pass that precomputes the **union** of all metric
  columns once per token (the engine already unions columns for a run) and scans
  each metric's candidates independently against a shared baseline — additive work,
  one corpus load, one precompute. This is a small new scan mode in
  `generic/strategy.rs`, not a new engine.
- **Fallback (no engine change):** N one-axis `run_grouped` calls reusing the warm
  in-memory corpus (Option A cache) so the lake is read once.

**Output per metric:** kept/dropped verdict + why, with the response curve:
- *Smooth + directional + plateau* → **keep**, with the suggested operator and a
  narrowed 2–3 value range for Layer 2.
- *Flat* (score ≈ the `off` pick everywhere) → **drop**, no edge.
- *Single spike* surrounded by noise → **drop**, overfit.

The `off` pick is the baseline: a metric only survives if its best candidate beats
its own `off` by a margin, on ≥ `MIN_CLOSED` trades.

**Deliverable:** a ranked metric shortlist. That alone answers "which of the many
metrics are worth combining."

---

## 3. Layer 2 — Family grid + interaction check — **BUILT**

**Purpose.** Combine only the survivors, and spend the combo budget on metrics that
*interact*, not on a blind full cross-product.

`lab/src/discovery/family.rs`: `plan_families` (Layer-1 shortlist → per-family members,
bounded by `FamilyLimits`, every capped member reported) → **one** additive pass of
family grids → **one** additive pass of pairwise interaction checks → `FamilyReport`.
Two passes, and they must be two: the interaction models are built *from* phase 1's
winners. Both ride the same `discovery::additive` scan mode as Layer 1, so each phase
is one sweep over one shared precompute regardless of family count.

### 3.1 Families

Metrics interact **within a family**, largely independently **across families**. The
registry's groups already are the natural families; the `family` tag makes the mapping
data, not code:

| family | groups |
| --- | --- |
| `price` (dip/scalper) | `m_price_lifetime`, `m_price_window`, `m_position` |
| `flow` | `m_flow_lifetime`, `m_flow_window` |
| `flow_split` | `m_flow_split`, `m_flow_split_window` |
| `liquidity/age` | `m_snapshot` |

(These mirror the existing color-family grouping in the registry's hue guards — same
intuition, promoted to a real field. Default for any new group = `standalone`.)

**D3 settled: the registry field.** `hunter_engine::metrics::MetricFamily` on
`GroupSpec`, mirrored into `registry_json` and the frontend `GroupSpec` type, guarded by
`families_group_the_registry_the_way_hues_do` (families must stay aligned with the hue
families they were promoted from, and nothing may sit unclassified in `standalone`).
Layer 2 never names a group — it reads `group_spec(..).family`, so a group added later
lands in a family with no edit.

### 3.2 The grid

- **Grid within a family** over the Layer-1-narrowed ranges (small — a few metrics ×
  2–3 values each). This is where the full grouped-sweep grid earns its cost.
- **Keep families independent by default**, then run the cheap **pairwise
  interaction check**: fix family A at its best combo, sweep family B on top.
  - B's best value **unchanged** regardless of A → independent → keep separate
    (their scores compose; no joint grid needed).
  - B's best value **moves** with A → interacting → merge into one joint grid.

  This is O(families²) small sweeps, not one exponential grid. It answers your exact
  uncertainty ("flow-split maybe doesn't need to interact with flow-scalper") with a
  measurement instead of a guess.

**Output:** the combined multi-metric combo(s), plus an interaction map showing which
families had to be gridded jointly. As built: `FamilyResult{members, dropped, combos,
best: BestCombo{picks, score, params_json}, n_gated}` per family, plus one
`Interaction{pinned, swept, alone, given, verdict}` per **ordered** pair —
`Independent` when B's best picks are unchanged, `Interacting` when they move,
`Inconclusive` when nothing under A was rankable (never silently read as independent).
`BestCombo::params_json` is the canonical `RuleParams`, so a winner promotes / re-sims
directly — that is Layer 3's input.

---

## 4. Layer 3 — Out-of-sample validation (the overfit verdict)

**Purpose.** You're extending the DB. That's a free natural experiment: a combo is
only "useful" if its edge survives on tokens it was never tuned on.

- **Split** the cohort by time — earlier tokens = train, later = validate (or, as new
  data lands, validate today's winners on it next cycle).
- **Re-score** each Layer-2 winner on the held-out slice via `simulate_one_combo`
  (already re-simulates a stored combo per token, under the run's own pricing).
- **Verdict:** keep only combos whose `DiscoveryScore` holds within a tolerance on
  validation. A combo that looked great in-sample and dies out-of-sample was noise —
  and you'll now *see* that, per combo.

Report the train→validate delta next to each combo. Big positive-to-nothing drops
are the overfit signature.

---

## 5. Extensibility contract (the thing you specifically asked about)

**To add a metric later: register it in `REGISTRY` and tag its family. Done.** The
pipeline picks it up next run with no edits. Concretely, each layer only ever
iterates the registry and reads `MetricSpec`/`GroupSpec` flags:

| New-metric property | Who consumes it, automatically |
| --- | --- |
| exists in `REGISTRY` | Layer-1 screen iterates all metrics → included |
| `unit`, `monotonic` | candidate generator picks spacing + default operator |
| `kind = dynamic` (+ `window_size_sec`) | screened at the default window |
| `scope = Position` | screened exit-side only |
| `fingerprint_config` (flow patterns) | skipped unless patterns supplied |
| `family` tag (new field) | Layer-2 places it in the right grid / interaction check |

The **only** non-free step is a genuinely novel *kind* of metric: the interaction
check still runs the pairwise test to decide if it interacts with existing families —
handled automatically, just at some compute cost. Numeric metrics with a lake column
get a candidate menu for free; a metric with no lake-derivable distribution (rare)
must declare a fixed menu, the one explicit annotation.

This is the same contract the redesign already honors for the rule-authoring UI
(`registry_json` → FE), extended to the discovery pipeline.

---

## 6. Performance design

Non-negotiable (EC2/RAM-constrained box, hot sweep pool):

- **One corpus load per pipeline run.** The lake read dominates; Layers 1–3 reuse the
  warm in-memory corpus (Option A cache), never reload per metric.
- **Additive screening, not multiplicative.** Layer 1 is ~6N combos, not 6^N — one
  precompute of the column union, independent per-metric scans. This is the single
  biggest perf lever; a naive "all metrics as off-axes in one grid" would explode to
  the product and defeat the point.
- **Small family grids.** Layer 2 grids are bounded per family (a few metrics × 2–3
  narrowed values); the interaction checks are O(families²) tiny sweeps. Nowhere near
  the 100k `MAX_COMBOS` default.
- **Reuse the RAM ladder.** All sweeps go through `plan_sweep_sizing` / bounded rayon
  — degrade (threads → fold budget), never OOM, stream results per group. No new
  resource path.
- **Percentiles are one `approx_quantile` pass** over the already-loaded DuckDB
  corpus — cheap, no extra scan of the lake.

Rough budget: a full 3-layer run on one cohort ≈ *one* corpus load + a screening pass
(~6N combos) + a handful of small family grids + a validation re-sim on the winners.
Orders of magnitude under a single 187K/777K-combo grouped sweep.

### 6.1 Run-time speed knobs (set them; don't just inherit defaults)

Discovery is **scan/precompute-bound, not fold-bound** (few combos; the cost is the
corpus load + per-token `MetricSeries` precompute + the exit scan — `resolve_exit` is
the measured hot path, not `prepare_token`). That **flips** the usual grouped-sweep
tuning, so the pipeline should actively pick these per run rather than take the
interactive defaults:

| Knob | Interactive default | Discovery-run choice | Why (given the scan-bound shape) |
| --- | --- | --- | --- |
| **RAM reserve** (`ram_reserve_mb`) | 1 GB | **512 MB** | Combos are few, so the reserve's real effect here is the **series wave** (resident token series), not fold-batch size. Tighter reserve → bigger wave → fewer precompute rebuild passes. It's a dedicated analysis session, so desktop headroom matters less; the ladder still degrades-not-OOMs. |
| **AVX-512** (`use_avx512`) | off | **on iff `avx512_available()` + release build** | 2.2× on the exit scan for the pure-`pnl`-bound (TP/SL) shape — which the screening baseline **always** carries. Partial: entry-only gates don't hit the exit scan, and non-pnl exit shapes (`retrace` running-peak, token-scoped columns, multi-arm) fall to the scalar/index path. **Release-only** (2.3× *slower* in debug). Not persisted, bit-identical to scalar by guard ⇒ zero correctness risk, free to toggle per run. |
| **Threads** | `cores − 2` | unchanged | Already leaves the desktop 2 cores; discovery is a foreground job like any sweep. |
| **Fold budget** | ladder | unchanged | Not the constraint here (few combos). Let the ladder size it. |

**The dominant lever is none of the above — it's precompute reuse.** Cold `sim_load` +
`sim_scan` dominate wall-clock, so the single biggest win is **one corpus load + one
series-union precompute shared across every metric screen** (the additive-scan mode,
D2). N separate `run_grouped` calls would re-precompute the series N times; the scan
mode precomputes once and scans each metric's candidates against it — saving N−1 of the
expensive passes. This is the real reason to prefer the scan mode, and it reframes D2:
it's about **reusing the precompute**, not merely bounding combo count. AVX + a tight
reserve are second-order on top of it.

**Operational (not design):** run discovery in `--release`; use
`--target-dir "C:/Users/User/Documents/Bot/target-check"` if a bin `.exe` is running;
sccache already caches rustc output across builds.

---

## 7. Data reality (must be stated, not hidden)

- **Grouping runs on ~7% of the tradable universe today** — the `tokens`/`tokens_info`
  fingerprint dimension only covers recently-captured launches (2,322 of 32,365 busy
  tokens). This is a backfill gap, not a field choice. The **metric-axis screening
  operates on the full trade corpus and is unaffected**; only the fingerprint
  *grouping/scoping* is throttled. Since you're extending the DB, backfilling the
  dimension widens Layer-2/3 cohorts directly — call it out as the highest-value data
  extension for this pipeline.
- **Cohort scoping.** Default to a **tight, single-regime** cohort (one fingerprint
  scope, or `ix_labels`-only grouping per the doc's recommendation) — a combo tuned
  across mixed archetypes averages to mush. Widen only when a regime lacks enough
  tokens to clear `MIN_CLOSED` (statistical power fallback, not a quality choice).

---

## 8. Build order

Each layer is independently useful; ship in order.

1. **Objective re-ranker** (§1) — **DONE.** `lab/src/discovery/objective.rs`:
   `DiscoveryWeights` (D1 defaults), `ComboStats` (+ `from_combo_metrics`),
   `discovery_score → ScoreOutcome{Ranked|BelowMinClosed|NoFire}`. Pure fn over the
   in-memory `ComboMetrics`; 6 unit tests (median-beats-whales, min-N gate, fire-rate
   scaling, open haircut, profit-factor cap); no engine/DB change.
   **Finding for step 2+:** the *persisted* DB row `GroupedSweepResult`
   (`core/src/models/grouped_sweep.rs`) does **not** carry `mtm_pnl_pct` — only the
   in-memory `ComboMetrics` (`lab/src/sweep/aggregate.rs`) does. So re-ranking a
   *stored* run (a `from_result` constructor) needs a migration to persist
   `mtm_pnl_pct` on `grouped_sweep_results` (+ the repo read/write). Deferred — the
   forward pipeline builds `ComboStats` from freshly-computed `ComboMetrics`, so this
   only blocks re-ranking historical runs.
2. **Candidate generation** (§2.1) — **DONE.** `lab/src/discovery/candidates.rs`:
   `ScreenConfig` / `screen_plan` (+ `SkipReason`), `collect_percentiles` →
   `PercentileTable` (deterministic bounded reservoir, nearest-rank via the kernel's
   now-`pub` `exact_quantile_f64`), `build_menus` → `MetricCandidates` (+ `MenuGap`),
   `round_for_unit`, and `axis_spec()` into `AxesModel`. 13 unit tests (registry
   screened-xor-reported, flow-split needs patterns, position exit-only + declared
   menus, per-side windows + column dedup, reservoir exactness/boundedness, rounding,
   off-first menus, degenerate/no-sample reporting, axis round-trip, end-to-end over a
   synthetic corpus). **NOT** in `duck.rs` and **not** DuckDB SQL — see §2.1 for why
   (SSOT: the ladder is measured through the engine's own `MetricSeries`).
   **Findings for step 3:** (a) the percentile pass and the Layer-1 screen want the
   *same* per-token series — build step 3's scan so `collect_percentiles` and the screen
   share one precompute rather than folding the corpus twice (that is D2's real payoff);
   (b) `collect_percentiles` is sequential — parallelising it needs a mergeable
   reservoir (buffers with different strides can't be concatenated without bias), so
   fold it into the screen's rayon pass instead of parallelising it standalone.
3. **Layer 1 screen** (§2.2) — **DONE.** `lab/src/discovery/screen.rs`:
   `ScreenBaseline` (fixed TP/SL, no `Default` — it is part of a screen's identity, like
   `Pricing`), `ScreenThresholds` (lift ratio / abs floor / plateau / narrow-to),
   `ScreenSegment` + `ScreenStrategy` (the additive scan mode over one shared
   precompute), `run_screen` / `screen_with_menus`, and the response-curve classifier
   `Verdict{Keep|DropNoEdge|DropSpike|DropThin|DropNoBaseline}` → `ScreenReport`
   (ranked `shortlist`, every skipped/gapped/errored metric with its reason, the
   min-N gate's kill count, the percentile audit trail). Enabled by one new seam in the
   engine: `GenericSweepStrategy::share_precompute`.
   **Findings for step 4:** (a) `ScreenStrategy` deliberately does **not** override
   `order_for_entry_cache` — flat `(segment, pick)` order is already entry-contiguous,
   which is what keeps `combo_id == flat index` so rows map back without a lookup table;
   preserve that property in any additive model Layer 2 adds. (b) `Verdict::Keep`
   carries `narrowed` (2–3 values) — that is Layer 2's grid input, so the family grid
   should consume the shortlist rather than re-deriving menus. (c) The `off` sentinel is
   what makes a *marginal* comparison possible; keep it as pick 0 in the family grid so
   an interaction check can read "with-vs-without family A" the same way.
4. **Layer 2 family grid + interaction check** (§3) — **DONE.**
   `lab/src/discovery/family.rs`: `FamilyLimits` (axis + combo caps, drops reported
   with a `DropReason`), `FamilyMember` / `plan_families` (shortlist → registry
   families, lift-ordered), `run_family_layer` (phase 1 = one additive pass of family
   grids, phase 2 = one additive pass of pinned-A/swept-B interaction checks) →
   `FamilyReport{families, interactions, combos_scanned}` with
   `InteractionVerdict{Independent|Interacting|Inconclusive}`. Enabling changes:
   `MetricFamily` on the engine's `GroupSpec` (**D3**, + `registry_json` + the FE
   `GroupSpec` type + a guard test), `AxesModel::combo_picks` /
   `ResolvedAxis::{value_count,value_at}` (the mixed-radix decode was duplicated
   between `combo_params` and `entry_key`; now one decode, three readers), and the
   step-3 scan mode generalised into `discovery::additive::AdditiveStrategy` so every
   layer's fan-out shares one precompute.
   **Findings for step 5:** (a) `BestCombo::params_json` is already the canonical
   `RuleParams`, so Layer 3 validates through `simulate_one_combo` with no adapter —
   but that fn needs the run's own `Pricing` + `as_of`, so a validation must carry the
   same pair the grid ran under or the train→validate delta is meaningless (parity
   plan B7). (b) Interaction checks are **ordered** pairs and `Inconclusive` is a real
   third outcome — Layer 3 must not read a missing verdict as independence. (c) The
   time-split (D4) has to split the *corpus*, not the report: `plan_families` is pure,
   so re-running `run_family_layer` on a held-out sub-corpus with the same shortlist is
   the cheapest honest validation of the grid itself.
5. **Layer 3 validation** (§4) — **DONE.** `lab/src/discovery/validate.rs`:
   `SplitPolicy{AgeFraction|Boundary}` + `split_tokens` (by `(created_at, mint)`, so a
   re-run splits identically), `Candidate` (+ `from_family` / `candidates_from_family_report`
   — the Layer-2 winner drops in with no adapter), `ValidationThresholds`,
   `validate_candidates` (re-scores **both** slices with `simulate_one_combo` under the
   run's own `Pricing`+`as_of`, exact-quantile folded), and the verdict
   `ValidationVerdict{Holds|Degraded|Failed|ThinValidate|NoFireValidate|UnrankableTrain}`.
   The two "can't tell" outcomes are first-class — never silently a pass. 6 unit tests
   (age-split earliest-first, tie-determinism, boundary split, holds/degraded/failed on
   retention, inconclusive-not-a-pass, end-to-end on both slices).
   **D4 settled:** time-split now (`AgeFraction`/`Boundary`); the wait-for-new-data cycle
   is the *same call* with two arbitrary token slices, so no extra machinery.
6. **Surface** (§8) — **DONE.** `lab/src/discovery/pipeline.rs`
   (`run_pipeline` — splits first, fits Layers 1–2 on the train slice, validates winners
   on the held-out slice; a degenerate split fits the whole cohort and reports
   `no_validation` rather than a vacuous pass), `lab/src/discovery/dto.rs`
   (`PipelineDto` — the whole report flattened to stable tag strings + numbers, the
   redesign wire vocabulary), and the endpoint
   `lab/src/api/handlers/strategies/metric_discovery.rs`
   (`POST /api/strategies/metric-discovery` + `/cancel` + `/last` + `/{run_id}`,
   single-flight gate mutually exclusive with sweep/flow-discovery, `MetricDiscovery*`
   SSE progress, cohort scoping by fingerprint / ix_labels / field filters — same SSOT
   as flow-discovery). Frontend: `frontend/src/lab/pages/strategies/MetricDiscoveryPage.tsx`
   + `lib/metricDiscoveryTypes.ts` + labEndpoints, showing shortlist → family winners +
   interaction map → validation verdicts, with **Promote…** on any winner/candidate
   (builds a `PromotedRuleDraft` client-side from the winner's `params` + the scoped
   fingerprint and opens the shared `PromoteRuleModal` → rule editor). No new backend
   promote endpoint — the winner's `params_json` is already the canonical `RuleParams`.

---

## 9. Open decisions

- **D1** — objective constants (`OPEN_HAIRCUT`, `profit_factor` cap, `MIN_CLOSED`,
  floors, plateau-penalty weight). Seed from anchors; validate; pin in `docs/plans/`.
- ~~**D2**~~ — **SETTLED (step 3): the scan mode.** `ScreenStrategy` presents the N
  per-metric sub-models as one flat combo space over one shared precompute
  (`share_precompute`), so the series is built once per token for the whole screen. The
  N-`run_grouped` fallback was skipped entirely: the reuse win is the dominant cost and
  the scan mode reuses the existing engine wholesale (~1 new method).
- ~~**D3**~~ — **SETTLED (step 4): the registry field.** `MetricFamily` on `GroupSpec`,
  mirrored into `registry_json` + the FE `GroupSpec` type, guarded by
  `families_group_the_registry_the_way_hues_do`. Layer 2 never names a group.
- ~~**D4**~~ — **SETTLED (step 5): both, via one call.** `SplitPolicy{AgeFraction|Boundary}`
  splits the loaded cohort by token age for the time-split-now path;
  `validate_candidates` takes two arbitrary token slices, so the wait-for-new-data cycle
  is the same function with last cycle's cohort as train and a fresh newer one as
  validate — no extra machinery.
