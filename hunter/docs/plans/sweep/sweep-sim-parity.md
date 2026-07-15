# Grouped-sweep ↔ single-rule simulate parity

**Goal:** a grouped-sweep group/combo row and a single-rule simulate of *that same
combo + fingerprint* should produce **identical** numbers. Today they don't, because
the two paths are **parallel reimplementations** that drifted — same decision kernel
(`find_*_entry` / `find_trade_driven_exit`, death-close inside the exit fn), but
different token universe, pricing, open-handling, and rollup.

Scope: all three strategies — `tpsl_sniper_1`, `tpsl_sniper_2`, `swing_1`.

Paths:
- SWEEP: `lab/src/sweep/strategies/{tpsl1,tpsl2,swing1}.rs` → `sweep/grouped_engine.rs`
  → `sweep/aggregate.rs` + `core/strategies/kernel.rs` (`RunAgg`/DDSketch); corpus from
  the **frozen lake** (`LakeSource::load`).
- SIM: `lab/src/strategies/{tpsl_sniper_1,tpsl_sniper_2,swing_1}/backtest.rs`; candidates
  from **live Postgres** (`collect_matching_tokens` → `TokenRepo`), histories from the lake.

The entry/exit *decisions* are already byte-identical for a token both paths run. Every
divergence below is **around** those calls.

---

## Divergences (what must be unified)

### Class A — Pricing (moves every PnL number)

| # | Divergence | SWEEP | SIM | Applies to |
| --- | --- | --- | --- | --- |
| A1 | **Cost model** | `round_trip_with_costs(..., CostModel::pumpfun_default())` (fees 100bps/leg + slippage 100bps/leg + Jito+priority) | **tpsl2:** same (costed). **tpsl1 & swing1:** **frictionless** `pct=(exit-entry)/entry`, no cost model | tpsl1, swing1 **(bug)** |
| A2 | **Notional** | `rule.buy_amount_sol` from the **sweep request** (default 1.0) | `rule.buy_amount_sol` from the **DB rule** | all 3 |
| A3 | **f32 quantization** | per-token PnL cast to `f32` in `TokenOutcome` before the f64 kernel sums | keeps `f64` end-to-end | all 3 |

A1 is the dominant one. tpsl1 (`backtest.rs:114-116`) and swing1 (`backtest.rs:103-104`)
have **no** `CostModel`; their sweeps do. Fix understates costs today regardless of parity.

### Class B — Population (moves fired count + everything downstream)

| # | Divergence | SWEEP | SIM |
| --- | --- | --- | --- |
| B1 | **Candidate source** | frozen lake token dimension | live PG scan (`token_criteria_satisfied`/`token_matches_buy_rule`) + lake histories |
| B2 | **Group ≠ matched universe** | partitions corpus into fingerprint groups; one group = one exact bucket | filters whole universe by the rule's `p_token_*` gates |
| B3 | **Portfolio caps** | none — folds every firing token | applies `select_simulated_tokens` (`max_concurrent`/`max_total`); **3 copies** of this fn |
| B4 | **Mayhem** | lake corpus (excluded at `duck.rs:268`) | `!is_mayhem_mode` — same net effect, different store |
| B5 | **token_cap** | default 10 000 | uncapped |
| B6 | **`curve_only`** | per request | hardcoded `false` (`candidate_cache.rs:56`) |
| B7 | **`min_tokens`** | groups `< 10` dropped | n/a |
| B8 | **bucket width** | grouping hardcoded `SOL_BUCKET_WIDTH=0.1` | matcher uses `rule.bucket_width_sol` |
| B9 | **ix_labels** | group key = lossy `" | "` join; copy round-trips `split/trim` | exact-ordered array compare on raw `instruction_labels` |
| B10 | **is_cashback_enabled** | selectable group dimension | **no rule criterion exists** — cannot be reproduced |
| B11 | **retention** | persists only metric-extreme + best combo | returns all |
| B12 | **lake freshness** | frozen at last export | live PG (newer tokens) |

### Class C — Close determinism & opens

| # | Divergence | SWEEP | SIM |
| --- | --- | --- | --- |
| C1 | **Death verdict `Utc::now()`** | `find_death_point(..., Utc::now())` inside the exit fn | same fn, **different wall-clock** → a token near `DEAD_QUIET_SECS` flips Dead↔Open between runs |
| C2 | **Open-position PnL** | mark-to-last-price, `fired:true`, **folded into** total/win_rate/mean/median/best/worst | `pnl=None`, excluded from summary sums |

### Class D — Aggregation

| # | Divergence | SWEEP | SIM |
| --- | --- | --- | --- |
| D1 | **Quantiles** | `median_pnl_pct`/`p90_pnl_pct`/`median_holding_secs` from a 64-bucket **DDSketch (~15% error)** | exact over rows |

Exact on both already: `n_fired/open/closed`, `win_rate`, `total_pnl_sol`, `mean`,
`best/worst`, `std`, `score`, `expectancy`, `profit_factor`, exit counts — *given* A/B/C fixed.

---

## Fix plan

### Phase 1 — Pricing (unblocks headline equality; also fixes a real bug)
1. **A1:** give tpsl1 & swing1 simulate the same `round_trip_with_costs(entry, exit,
   rule.buy_amount_sol, &CostModel::pumpfun_default())` tpsl2 sim + all sweeps use. Make
   `CostModel` the single shared source. (Decide deliberately: costed is the live-accurate
   choice; frictionless would need to change all three sweeps instead.)
2. **A3:** store `f64` in `TokenOutcome` (or round sim to f32) so both quantize identically.
3. **A2:** assert the sweep run's `buy_amount_sol` == the rule's, or source both from one field.

### Phase 2 — Close determinism & opens
4. **C1:** thread a deterministic as-of `now` (corpus max `block_time` or the run's
   `created_before`) into `find_death_point` on both paths — remove `Utc::now()` from analysis.
5. **C2:** pick ONE open convention and apply on both. Recommended: exclude open (unrealized)
   PnL from the realized aggregates on both — most tokens already close via death-close, and
   "realized" win_rate/PnL shouldn't include marks. (Alternative: mark-to-last on both.)

### Phase 3 — Population (the structural root)
6. **B1/B2/B12 — NOT DONE, deliberately deferred.** Make simulate **replay the sweep
   group's exact persisted mint list** rather than re-deriving via a live PG scan — the
   sweep already stores per-group mints. This is the clean SSOT fix (one candidate set,
   no PG-vs-lake / freshness / group-vs-universe drift) but it's a real feature, not a
   bugfix: single-rule simulate's API surface (`run_backtest` + the 3 REST endpoints) has
   no concept of "replay this sweep run/group" today, so this needs a new request shape,
   corpus-source variant, and (likely) frontend wiring to invoke it — same shape as the
   "Architectural end-state" below, just for one field instead of the whole engine.
   Everything B1/B2/B12 would fix (B4/B5/B6/B7/B11 below) is a symptom of the two paths
   deriving their candidate set independently; landing this item removes them as a side
   effect instead of patching each one.
7. **B3 — DONE.** Collapsed the **3 byte-identical copies** of `select_simulated_tokens`
   (tpsl1/tpsl2/swing1 `backtest.rs`) into one generic fn,
   `lab::strategies::admission::select_simulated_tokens`, with its own unit tests. Unifying
   the *caps themselves* between a sweep run and a rule (null them for the comparison, or
   add the admission pass to the sweep) is still a per-comparison hygiene concern, same
   status as `buy_amount_sol` (A2) — no rule-to-run linkage exists to assert against yet.
8. **B8/B9/B10 — NOT DONE.** Bucket width: grouping's `render_field` hardcodes
   `SOL_BUCKET_WIDTH = 0.1` with no width parameter at all (not "defaults to 0.1" — there
   is no override path), while the live/analysis *matcher* already reads a per-rule
   `bucket_width_sol`. Threading a width through `group_key`/`render_field`/
   `bucket_sol_label` is mechanical but ripples into the dashboard SQL bin
   (`creation_stats_repo`, which must stay in lockstep) and the grouped-sweep request/
   frontend. ix_labels: the lossless-encoding sub-fix below is scoped but also touches the
   frontend (`groupedTypes.ts`) and is unfixed. Cashback: still no rule criterion exists —
   grouping by `is_cashback_enabled` remains unreproducible from a rule.
9. **B4/B5/B6/B7/B11 — NOT DONE**, and mostly subsumed by item 6 above once it lands
   (mayhem policy already nets out equal; `token_cap`/`curve_only`/`min_tokens` are sweep
   **request** knobs simulate has no equivalent field for, same non-issue as A2's
   `buy_amount_sol` until there's a rule↔run linkage to source them from; retention is
   inherent to the sweep's bounded-storage design for the *wide* multi-combo case, not
   something to "fix" for a single-combo drill-in).

### Phase 4 — Aggregation
10. **D1:** for the single-combo drill-in, compute **exact** quantiles over the retained rows
    (the group is bounded), keeping the DDSketch only for the wide multi-combo sweep. Or accept
    ~15% error on those three metrics and document it.

### Architectural end-state (the real "same logic")
Make single-rule simulate a **special case of the sweep engine**: one combo, one group =
the rule's fingerprint, same lake corpus, same cost model, same open-handling, same rollup.
That removes the parallel reimplementation instead of patching it in ten places.

---

## ix_labels lossless copy (sub-fix, already scoped)
- Copy the group's **original label array** into the rule, not a `split(" | ")` of the joined
  chip (`groupedTypes.ts:110-111`).
- Make the group key a lossless encoding (JSON array / non-collidable delimiter) in
  `grouping.rs render_field`.
- Guard test: label-set → group-key → copy → rule must round-trip identical (no-DB test).
- Verify the lake `fp_ix_labels` order/dedup matches the matcher's exact-ordered compare
  (the `TokenFingerprint.ix_labels` doc says "sorted + deduped" but `normalize_label_vec` is
  identity — reconcile).

## Comparison hygiene (until Phase 3 lands — what the user controls)
Re-export the lake first; same `buy_amount_sol` on run and rule; `bucket_width_sol=0.1`;
`group_by` == the rule's exact fingerprint axes; one group whose key == the rule's bucket;
`min_tokens=1`; `token_cap` ≥ population; `curve_only=false`; no caps on the rule;
`SWEEP_PER_MINT_CAP` unset. Even with all of these, A1/C1/C2/D1 keep the numbers apart until
the code fixes land.
