# Family search — charter

Why a second search workflow exists, what it is allowed to depend on, and what the
measurements say. The buildable form is [family-search-plan.md](family-search-plan.md).

The existing [rule search](../plans/strategies/rule-search.md) stays as it is. This
is a sibling job, not a rewrite of it.

---

## 1. The requirement

Find, for one launch style, the metric combination that works in practice on **both
sides** — entry and exit — and express it so an operator can use it as-is or adjust
it. Partial exits follow later and inherit the same machinery.

Three constraints the operator sets, all load-bearing:

| Constraint | Consequence |
| --- | --- |
| The unit of analysis is the **fingerprint**, never `creator_wallet` | A dev rotates wallets constantly, so a wallet carries no habit. A fingerprint is what a dev cannot change cheaply, so it is the durable proxy for one. A fingerprint holds several creators running several logics, and is still the sharper instrument. |
| The result depends on **no existing rule** | A search anchored to a promoted rule can only rediscover it. The output must be identical with an empty `rules` table. |
| Speed matters, correctness more | Never buy throughput by changing what the decision kernel computes. |

---

## 2. What a fingerprint cohort is

Scope goes through `hunter_engine::fingerprint::matches`
([fingerprint.rs](../../engine/src/fingerprint.rs)) — the SSOT simulate uses — never a
hand-rolled predicate. `matches` is `matches_phase(.., MatchPhase::Full)`: every
configured axis, first-slot axes included.

A fingerprint matches on **every** non-null axis, not just `ix_labels`: `cu_limit`,
`cu_price`, `init_buy_lamports`, `max_cost_lamports`, `spendable_lamports_in`, both
`first_slot_*` axes, and `bucket_size_amount` — where a NULL bucket is an **exact**
compare, not an unset one. Unconfigured axes always pass.

An ix-labels-only approximation of `3ix:BuyExactSolIn · spend=5 · bkt=exact` takes
3,440 tokens where the engine takes 264 — that label set spreads over 12 distinct
`spendable_lamports_in` values and the fingerprint owns 7.7% of it. Two rules
**invert** in rank between the two populations. Cheapest guard: check `n_matched`
against a hand count on every run.

The shipped scope paths are already correct —
[`LakeSource::matching_mints`](../../lab/src/lake/duck.rs) and simulate's
`scan_matched_candidates` both call the engine. Only an offline probe can drift.

### The sibling family

Siblings share `ix_labels` and `bucket_size_amount` and are identical on every axis
but one. Purely mechanical off the `fingerprints` table, no heuristic. The reference
family, all `3ix:BuyExactSolIn · bkt=exact`, varying `spendable_lamports_in`:

| Axis value | `fingerprints.id` |
| --- | --- |
| spend=1 | `e6299eac-6ebe-4a62-a2ac-e9e616dc68bd` |
| spend=1.5 | `c9ac419e-abc0-4fde-b67b-93a109c75d04` |
| spend=2 | `9027c886-0289-4bf3-92b3-7e6f9726420d` |
| spend=3 | `cf404966-b439-4d1c-b31a-2d2fd7dde99c` |
| spend=4 | `1a040cb7-c1ed-4eeb-90fd-9c5fdb61e0b8` |
| spend=5 | `219e0772-bce4-4dff-9e7e-b9335ce496af` |

---

## 3. What the measurements say

Conditions for every number below: buy 0.01 SOL, entry `m_snapshot.liquidity > 20`,
`pumpfun_impact`, `WorstCase` fill, copycat guard on, range 08-01 → lake end.
<!-- pt-ok: the range is a data cutoff to re-check against, not a timeline -->

| Finding | Number | What it forces |
| --- | --- | --- |
| **Cohort dominates rule** | one rule spans −13.8% to +40.8% over six siblings | A rule is never reportable without the cohort it belongs to. |
| **Cohort quality is separable** | a trivial control and a tuned rule rank the six cohorts almost identically | Rank the cohort and rank the rule as two questions. |
| **Exit logic is portable** | the same exit improves **6 of 6** cohorts, losers included | Fit the exit broad. |
| **Entry logic is not** | see the axis-duplication row | Select entry narrow. |
| **Rank transfers, level does not** | pooled fit rank → held-out rank, Spearman **ρ = 0.833**, while every candidate is negative on the fit set (best −1.24%) and the winner pays +31% on the holdout | Take the ordering from the broad fit, take the number from the target cohort. Never quote a fit level. |
| **An entry clause can re-read a fingerprint axis** | `liquidity > 20` admits 84% / 66% of spend=4 / spend=5 but 36–44% of spend=1 / 1.5 / 2 / 3 — a larger initial buy mechanically creates the liquidity | Refuse an entry clause whose admit rate tracks the varied axis. |
| **A broad fit is blind to a cohort-specific term** | dropping `nonvol_buy >= 1.6 @2s` leaves spend=1.5 and spend=4 byte-identical and costs 10 points on spend=5 | Re-check the finalist narrow, after the broad fit. |
| **The exit is an OR, and the OR is the edge** | OR +30.97% / +31.66%; `stall >= 30` alone +14.21 / +21.02; `gross_flow < 15 @10s` alone +8.69 / +27.63; `nonvol_buy >= 1.6 @2s` alone −1.87 / +10.22; `retrace >= 36` alone −39.50 / −44.20 (15d / 7d) | Exit is the primary search axis. An unarmed `retrace` is a hard stop from entry — see the landmine table in [hunter/CLAUDE.md](../../CLAUDE.md). |
| **A price trail destroys a working exit** | adding `trail >= 15 @10s`: spend=5 +30.7 → −15.3, spend=4 +40.8 → −14.1, spend=1.5 −13.8 → −14.2 | Trail stays in the library, flagged. A library that cannot express a refuted term cannot re-refute it on the next family. |
| **Pool by money, not by mean** | pooled = `Σpnl_sol / Σentry_sol`, same rule as [PnL %](../plans/strategies/pnl-percent-definition.md) | A mean of per-cohort percents lets a 99-token cohort outvote a 565-token one. |
| **Execution can be the entire loss** | on a dump-scalp family, the same taken set repriced worst-fill + impact vs first-fill + fee-only differs by **6.93 pp/trade** (n = 5,872); the signal is near-breakeven (PF 0.95 optimistic) and the round trip eats it — a rule targeting 3–12% moves cannot clear a ~6 pp cost, and no threshold changes a ratio ([2026-08-16-dump-scalp-execution-gap.md](../history/2026-08-16-dump-scalp-execution-gap.md)) | Refuse a cohort whose available moves live inside the execution band **before** generating anything (D8), and print the dual-pricing spread beside every finalist. |
| **A stop does not stop on sparse prints** | authored `pnl <= -8` realizes a **−19.4%** mean (worst −102%) — prints are sparse and price gaps straight past the level | Attribution carries mean **realized** level beside the **authored** threshold, per alarm slot. |
| **A gate the move itself creates is a lagging gate** | `gross_flow(60) >= 55` selected post-move moments and was the AND-binding clause deciding entry timing; dropping it improved quality *and* volume (5,872 vs 4,747) | Flag an entry clause that binds timing **and** correlates with low capture — the event-proxy sibling of the axis-duplication gate. |

Per-cohort counts for the reference family, useful as a totals sanity check: a
four-term exit closes 1,086 = 253 + 697 + 136 over spend=1 / 2 / 3. A low `n` is not
evidence of a dropped cohort — `gross_flow < 25` closes ~76% as many positions as
`< 15` in every cohort and totals 856 = 192 + 565 + 99. Verify a suspected gap by
re-running it.

---

## 4. Decisions

**D1 — the family is the unit of a run, the fingerprint is the unit of a result.**
Fit ranks on the pooled siblings, level comes from the target cohort alone, and both
print. `run_search` stays single-cohort; the family logic sits above it and adds no
decision path.

**D2 — ρ is a self-test, not a result.** The Spearman between fit rank and holdout
rank grades the *procedure* on this family. Where it collapses, fit-broad does not
apply there and the board says so instead of ranking anyway.

**D3 — the denominator is an oracle, not an incumbent.** Capture ratio is
`realized pnl ÷ the best exit available after entry`, priced through the same cost
and fill models as the realized exit. Without it, a rule that takes 31 of 40
available points and one that takes 31 of 300 score identically.

Report the two halves apart:

> *of the tokens entered, N had a winning exit available and the rule took X% of it;
> M never had one at all.*

The first number grades the exit. The second grades the **entry**, and it needs no
exit rule to exist — which is how the entry side becomes measurable at all.

**D4 — attribution comes from the engine, not from ablation.** `ExitReason::Metrics`
already carries `{ metric, operator, value, window }`
([event.rs](../../engine/src/event.rs)), the sweep already resolves an authored slot
per exit req at bind time ([strategy.rs](../../lab/src/sweep/generic/strategy.rs)),
and `TokenOutcome` already has the fields
([kernel.rs](../../core/src/strategies/kernel.rs)). Deletion-ablation costs one
re-run per term and gets replaced by one run. Attribution is also the hard
prerequisite for partial exits: a ladder cannot fire on a signal it cannot name.

**D5 — no anchoring to an existing rule, of any kind.** Three separate leaks:

```
  MAY enter a search:      fingerprint · datetime range · buy size · caps
                           · fill model · cost model · copycat setting
                           — all from the REQUEST; physics or operator choice

  MAY NOT enter a search:  any RuleParams that exists in the rules table
                           any threshold, metric, window, or structure from one
                           any buy size or cap read off one
                           any expansion base chosen without a family quota

  The test:  delete every row in `rules` and the result is identical.
```

The third line is not hypothetical. Rule search's handler overrides `buy_amount_sol`,
`max_concurrent_tokens`, and `max_total_tokens` from the incumbent
([rule_search.rs](../../lab/src/api/handlers/strategies/rule_search.rs)). Because
cost is U-shaped under `pumpfun_impact` and `expectancy_floor_sol` is derived from
buy size, an incumbent silently moves both the economics and the Refuse/Candidate
bar; the caps additionally change which tokens are entered at all. Family search
takes all three from the request only.

The fourth line covers a leak with no incumbent in sight: greedy expansion around a
run's own top-3/top-5 amplifies whichever family the initial library favours. Pick
expansion bases under a per-family quota, not by raw rank.

**D6 — an incumbent is an artifact, not a baseline.** Two comparisons are legitimate
because both are properties of the cohort and exist before any rule does: the
**ungated control** (what the fingerprint pays with no gate) and the **oracle**
(what money was available). An incumbent is neither. Keep it as an optional,
off-by-default display column and let it touch nothing.

**D7 — freshness is a gate, not a footnote.** `Corpus::last_trade_at`
([corpus.rs](../../lab/src/sweep/corpus.rs)) exists precisely so a run can state how
fresh its data is. A run whose `until` outruns it is silently shorter than requested.
Refuse, or badge loudly.

**D8 — a cohort must clear execution before a search runs.** Compare the cohort's
oracle upside distribution (net — the oracle is already charged the same round trip
as a realized exit, D3) against the execution band. Where the typical available move
lives inside that band, **no rule can exist there**: the loss is a ratio, and
thresholds do not change ratios. Refuse the cohort before the generator spends
anything on it. The gate is free once the oracle exists, and it is the cheapest
refusal in the whole pipeline — it saves the entire search, not a candidate.
Corollary, per finalist: report the worst-fill vs first-fill spread on the **same
taken set**; a finalist whose edge is smaller than its own spread is priced on fill
luck, not signal.

---

## 5. What is void

Recorded so it is not re-derived, and not re-trusted.

- **Every offline number computed on a labels-only cohort.** That population is ~13×
  the real one. This voids an offline "validation" of a promoted rule at +2.8% where
  simulate reads +31.0%, and it voids a *refutation* of launch tells — void means
  untested, not refuted.
- **"The generator loses to the incumbent."** The candidate library was the incumbent
  plus seven perturbations of its own structure. A search over the neighbourhood of X
  can only conclude X. The comparison says nothing about a from-scratch generator,
  because none ran.
- **`creator_wallet` as a unit of analysis.** 262 distinct wallets over 264 tokens is
  the signature of rotation, not 262 independent creators. The lake's
  `tokens.parquet` carries no such column, which is consistent with it having no
  analytic use here.

---

## 6. Open

- Entry at true scope is unexplored — every entry finding to date came from the
  diluted cohort.
- A from-scratch generator has never been run against a promoted rule.
- The reference family varies one axis (`spendable_lamports_in`). Whether fit-broad
  holds on a family varying `cu_price` or `ix_labels` is unmeasured; D2 is the
  instrument that answers it.
- Any forward test needs `scripts/db-incremental-sync.ps1 -IncludeToday -ExportLake`
  first.
