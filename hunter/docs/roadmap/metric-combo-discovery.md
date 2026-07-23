# Metric-combo discovery pipeline

**Goal.** A repeatable, automated way to find the metric/param combos that actually
make money for a chosen token cohort — ranked by *profit × frequency × stability*,
guarded against overfitting, and **registry-driven** so a metric added later flows
through with no pipeline edit.

**Owner surface.** Lab only (`hunter-lab`), built on the existing grouped-sweep
engine + lake corpus. Nothing ships to EC2. Live/paper are untouched — this is an
analysis aid that *outputs* combos to promote, exactly like the sweep does today.

**Status.** In progress. **Step 1 (objective re-ranker) — DONE** (see §8). Steps 2–6
not started; continue there. Build Layer 1 first (highest leverage, least code);
Layers 2–3 compose on top.

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

### 2.1 Automated candidate values (closes gap #1)

New: a **lake percentile service** in `lab/src/lake/duck.rs`
(`metric_percentiles(metric, subset) -> [p05..p99]`). For each numeric metric it
runs one `approx_quantile` (DuckDB native, cheap) over the loaded corpus, subset to
the cohort. The candidate menu is then percentile-spaced (`p10, p25, p50, p75, p90`)
plus the `off` sentinel — the exact recipe `axis-value-candidates.md` documents by
hand, now generated.

Registry-driven specifics (all read off `MetricSpec`/`GroupSpec`, so a new metric is
handled automatically):
- **unit** picks sensible rounding (`seconds`/`sol`/`percent`).
- **operator direction** inferred from `monotonic` + unit, overridable.
- **dynamic** groups (`window_size_sec`) → screen at a default window (30s entry /
  10s exit per the doc); windows are cross-run, not swept in one run.
- **position-scoped** (`m_position`) → screened on the **exit** side only (registry
  `scope == Position`; entry axis is rejected by `axes.rs`).
- **flow-split** (`m_flow_split*`) → needs `volume_ix_patterns`; skip in the generic
  screen unless a pattern set is supplied for the run (mirrors the existing 400).

### 2.2 The screen itself (additive, not multiplicative)

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

## 3. Layer 2 — Family grid + interaction check

**Purpose.** Combine only the survivors, and spend the combo budget on metrics that
*interact*, not on a blind full cross-product.

### 3.1 Families

Metrics interact **within a family**, largely independently **across families**. The
registry's groups already are the natural families; we add one lightweight
`family` tag so the mapping is data, not code:

| family | groups |
| --- | --- |
| `price` (dip/scalper) | `m_price_lifetime`, `m_price_window`, `m_position` |
| `flow` | `m_flow_lifetime`, `m_flow_window` |
| `flow_split` | `m_flow_split`, `m_flow_split_window` |
| `liquidity/age` | `m_snapshot` |

(These mirror the existing color-family grouping in the registry's hue guards — same
intuition, promoted to a real field. Default for any new group = `standalone`.)

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
families had to be gridded jointly.

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
2. **Lake percentile service** (§2.1) — `metric_percentiles` in `duck.rs` + candidate
   generator keyed off `REGISTRY`. Closes the hand-derivation gap.
3. **Layer 1 screen** (§2.2) — orchestrate the additive per-metric scan + response-
   curve analysis → ranked metric shortlist. *This is the deliverable you feel first.*
4. **Layer 2 family grid + interaction check** (§3).
5. **Layer 3 validation** (§4).
6. Surface: a lab page/endpoint (mirrors `flow_discovery` job shape) that runs the
   pipeline for a cohort and shows shortlist → combos → validation verdicts, with a
   one-click promote (reuse the sweep's promote path).

---

## 9. Open decisions

- **D1** — objective constants (`OPEN_HAIRCUT`, `profit_factor` cap, `MIN_CLOSED`,
  floors, plateau-penalty weight). Seed from anchors; validate; pin in `docs/plans/`.
- **D2** — Layer-1 screening: new additive scan mode that **precomputes the series
  once and reuses it across all metric screens** (saves N−1 precompute passes — the
  dominant cost, see §6.1) vs. N one-axis `run_grouped` calls on the warm corpus
  (zero engine change, but re-precomputes per metric). Recommend starting with the
  fallback to prove the pipeline, then promoting to the scan mode — the reuse win is
  large enough that it's likely worth building, not just a fallback-if-slow.
- **D3** — `family` field on `GroupSpec` (needs the registry + `registry_json` guard
  tests updated) vs. a lab-side family map (no engine touch, but a second place to
  keep in sync). Recommend the registry field — it's the SSOT and the color-family
  intuition already lives there.
- **D4** — validation split policy: time-split now, or wait-for-new-data cycle.
  Recommend supporting both; time-split is available immediately.
