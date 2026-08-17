# Family search

The one reference for this workflow: what it is for, what it decides and why, what is
built, how to run it, and what is still open. The existing
[rule search](../plans/strategies/rule-search.md) is a separate job and stays
untouched — this is a sibling, not a rewrite.

Code lives in `lab/src/family_search/`; the module map is in
[../arch/sweep.md](../arch/sweep.md).

---

## 1. What the operator asked for

> Find the creator's habit and the metric combination that works in practice on
> **both sides** — entry and exit — and express it so I can use it as-is or adjust it.
> Partial exits come later.

Four constraints, all stated by the operator and all load-bearing:

| Constraint | Why it binds |
| --- | --- |
| The unit is the **fingerprint**, never `creator_wallet` | A dev rotates wallets constantly, so a wallet holds no habit. A fingerprint is what a dev cannot change cheaply. One fingerprint holds several creators running several logics and is *still* the sharper instrument. |
| The result depends on **no existing rule** | A search anchored to a promoted rule can only rediscover it. Emptying the `rules` table must not change the output. |
| **More meaningful conditions is more reliable** | Each condition is one more thing considered. **Entry decides safety (win rate); exit decides profit (PnL).** A rule with no entry and one alarm is not safer than one with several of each. |
| Speed matters, correctness more | Never buy throughput by changing what the decision kernel computes. |

### The shape that works

The operator's own promoted rule, read correctly:

```
  ENTRY  5 clauses = 3 QUANTITIES     (two are bands: floor + ceiling on one metric)
         gross_flow(60s) · time · liquidity
  EXIT   6 clauses = 5 ALARMS + 1 mechanic
         the mechanic is `liquidity >= 85` — "sell at migration", added by hand
```

Two consequences the design turns on. A **band is one idea written as two clauses**,
so counting clauses overstates entry density and "prefer fewer clauses" deletes bands
first. And a hand-added mechanic is **not a discovered edge** — it must be present in
every simulation so the numbers describe a runnable rule, and credited to nothing.

---

## 2. What a cohort is

Scope goes through `hunter_engine::fingerprint::matches`
([fingerprint.rs](../../engine/src/fingerprint.rs)) — the SSOT simulate uses, never a
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
against a hand count on every run. The shipped paths are correct
([`LakeSource::matching_mints`](../../lab/src/lake/duck.rs) and simulate's
`scan_matched_candidates` both call the engine); only an offline probe can drift.

**The sibling family.** Siblings share `ix_labels` and `bucket_size_amount` and are
identical on every axis but one — mechanical off the `fingerprints` table, no
heuristic. The reference family, all `3ix:BuyExactSolIn · bkt=exact`, varying
`spendable_lamports_in`:

| Axis | `fingerprints.id` |
| --- | --- |
| spend=1 | `e6299eac-6ebe-4a62-a2ac-e9e616dc68bd` |
| spend=1.5 | `c9ac419e-abc0-4fde-b67b-93a109c75d04` |
| spend=2 | `9027c886-0289-4bf3-92b3-7e6f9726420d` |
| spend=3 | `cf404966-b439-4d1c-b31a-2d2fd7dde99c` |
| spend=4 | `1a040cb7-c1ed-4eeb-90fd-9c5fdb61e0b8` |
| spend=5 | `219e0772-bce4-4dff-9e7e-b9335ce496af` |

---

## 3. The pipeline

```
  INPUT   one fingerprint (the TARGET) + a datetime range + buy/caps/fill/cost
          + optional standing exit terms
                                  │
  ┌───────────────────────────────▼───────────────────────────────────────┐
  │ SCOPE     fingerprint::matches on the tokens dimension, per sibling.   │
  │           Dimension-only, no trade scan — an empty cohort fails first. │
  └───────────────────────────────┬───────────────────────────────────────┘
  ┌───────────────────────────────▼───────────────────────────────────────┐
  │ GATE 1  freshness (D7)   `until` outruns the lake  ⇒ REFUSE, fatal     │
  │ GATE 2  cost clearance (D8)  the cohort's typical BEST available exit  │
  │         sits inside one round trip ⇒ REFUSE before generating anything │
  └───────────────────────────────┬───────────────────────────────────────┘
  ┌───────────────────────────────▼───────────────────────────────────────┐
  │ EARN      signatures from the TARGET cohort's own paths → cut table    │
  │ COMPOSE   entry = ANDs of 0-4 QUANTITIES, densest first (band = 1)     │
  │           exit  = ORs of 2-5 ALARMS, max ONE per end-event family      │
  │           quota: no exit SHAPE takes >40% of slots                     │
  └───────────────────────────────┬───────────────────────────────────────┘
  ┌───────────────────────────────▼───────────────────────────────────────┐
  │ FIT       score the fixed menu on every sibling, one corpus at a time  │
  │           rank = pooled Σpnl/Σentry over the FIT siblings only         │
  │           rho  = Spearman(fit rank, held-out rank) — the SELF-TEST     │
  └───────────────────────────────┬───────────────────────────────────────┘
  ┌───────────────────────────────▼───────────────────────────────────────┐
  │ SELECT    walk the ranking; take the first candidate clearing BOTH,    │
  │           on the HELD-OUT target:                                      │
  │             win rate > the ungated control's own win rate  (SAFETY)    │
  │             return   > 0                                    (PROFIT)   │
  └───────────────────────────────┬───────────────────────────────────────┘
  ┌───────────────────────────────▼───────────────────────────────────────┐
  │ ENRICH    offer every earned idea the skeleton lacks; keep what pays   │
  │           in ITS OWN currency (entry→win rate, exit→return), each      │
  │           acceptance re-confirmed against the growing rule             │
  └───────────────────────────────┬───────────────────────────────────────┘
  ┌───────────────────────────────▼───────────────────────────────────────┐
  │ GRADE     authority replay on the target: capture vs the oracle,       │
  │           per-alarm attribution, ablation both sides, fill spread,     │
  │           axis-duplication refuse, lagging-clause diagnostic           │
  └───────────────────────────────┬───────────────────────────────────────┘
             portrait (prose) + draft + ungated control + oracle
```

---

## 4. Decisions

**D1 — the family is the unit of a run, the fingerprint the unit of a result.** Fit
ranks on the pooled siblings; the level comes from the held-out target alone. Both
print, and a fit level is never quoted as a result.

**D2 — ρ is a self-test, not a result.** Spearman between fit rank and held-out rank
grades the *procedure* on this family. Below `RHO_FLOOR` (0.5) the board says
fit-broad does not apply here instead of ranking anyway.

**D3 — the denominator is an oracle, not an incumbent.** Capture = realized ÷ the best
exit available after the fill, priced through the same cost and fill models. Reported
in two halves: how much of the available money the exit took, and how many entries
never had any. The second grades the **entry** and needs no exit rule to exist.

**D4 — attribution comes from the engine, not from ablation.** `ExitReason::Metrics`
already carries `{metric, operator, value, window}` and the sweep already numbers
authored slots at bind time; the rollup adds n, wins, Σpnl, Σentry per slot. One run
replaces one re-run per term — and it is the prerequisite for partial exits, since a
ladder cannot fire on a signal it cannot name.

**D5 — no anchoring to an existing rule, of any kind.**

```
  MAY enter:   fingerprint · range · buy size · caps · fill · cost · copycat
               · standing exit terms          — all from the REQUEST
  MAY NOT:     any RuleParams from the rules table; any threshold, metric,
               window or structure off one; any buy size or cap read off one;
               any expansion base picked without a quota
  The test:    empty the `rules` table and the output is identical.
```

Carried by the type system: nothing in `family_search` imports a rule repo, and
`StartFamilySearchBody` is the only source of buy/caps/fill/cost/copycat. (Rule
search's own handler *does* override buy size and caps from its incumbent, and since
`expectancy_floor_sol` derives from buy size that silently moves its pass/fail bar —
which is why this job takes all three from the request only.)

**D6 — an incumbent is an artifact, not a baseline.** Two comparisons are legitimate
because both are properties of the cohort and exist before any rule: the **ungated
control** and the **oracle**. An incumbent is neither — optional, off by default,
display-only, touching nothing.

**D7 — freshness is a gate, not a footnote.** A run whose `until` outruns
`Corpus::last_trade_at` is silently shorter than requested and nothing downstream can
detect it. Fatal refusal, with the sync command in the message.

**D8 — a cohort must clear execution before a search runs.** Compare the cohort's
median net oracle move (losers included — a median over winners is positive by
construction and could never refuse) against one round trip at this buy size. Inside
the band, no rule can exist: the loss is a ratio and thresholds do not change ratios.
Refuse before the generator spends anything. The band is derived from the run's own
cost model and is **U-shaped in buy size**, so it can never be a constant. Corollary
per finalist: report the worst-fill vs first-fill spread on the same taken set; an
edge smaller than its own spread is priced on fill luck.

**D9 — the generator composes the working shape, it does not hope for it.** Count
entry **quantities** (a band is one) and prefer more of them; build exit ORs of 2–5
alarms drawing **at most one clause per end-event family**, because an OR pays by
firing at the earliest of several *independent* alarms and two thresholds of one
quantity are not two alarms.

**D10 — a mechanic is not a finding.** Standing terms (`liquidity >= 85`) ride into
every candidate **and** the ungated control, so the numbers describe a runnable rule.
None is searched, ablated, credited, or counted toward the quota. Written as the
attribution table prints them and parsed through the one label SSOT, so a term
round-trips out of a result and back into the next run.

**D11 — entry is safety, exit is profit, each graded in its own currency.** The
ranking alone cannot pick a draft: a candidate must clear a **win-rate** bar and a
**return** bar, both read on the held-out target. The win-rate bar is the ungated
control's own win rate — a gate that does not enter more safely than buying everything
is not filtering anything — raised by any absolute floor the request sets. Both bars
are narrow, never on the fit set, because every candidate can be negative on the
pooled fit while the winner pays +31% on the target. The same split governs the
ablation: an entry term earns its place by lifting the win rate, an exit alarm by
lifting the return. **Grading both on return is the recorded reason a search deletes
every entry condition.**

**D12 — density must be earnable, not merely survivable.** Every other stage only
*removes*. Combined with a broad fit being blind to a term only one cohort needs, the
pipeline converges on the sparse portable core by construction. So the enrich stage
offers each earned idea the skeleton lacks, keeps what pays in its own currency,
re-confirms each acceptance against the growing rule (two ideas that each pay alone
can be the same idea twice), and reports everything tried — accepted or refused.

---

## 5. What the measurements say

Conditions for every number: buy 0.01 SOL, entry `m_snapshot.liquidity > 20`,
`pumpfun_impact`, `WorstCase` fill, copycat on, range 08-01 → lake end.
<!-- pt-ok: the range is a data cutoff to re-check against, not a timeline -->

| Finding | Number | What it forces |
| --- | --- | --- |
| **Cohort dominates rule** | one rule spans −13.8% to +40.8% over six siblings | Never report a rule without its cohort. |
| **Cohort quality is separable** | a trivial control and a tuned rule rank the six cohorts almost identically | Rank cohort and rule as two questions. |
| **Exit logic is portable** | the same exit improves **6 of 6** cohorts, losers included | Fit the exit broad. |
| **Rank transfers, level does not** | pooled fit rank → held-out rank at **ρ = 0.833**, while every candidate is negative on the fit set (best −1.24%) and the winner pays +31% on the holdout | Ordering from the fit, number from the target. |
| **An entry clause can re-read a fingerprint axis** | `liquidity > 20` admits 84% / 66% of spend=4 / 5 but 36–44% of spend=1 / 1.5 / 2 / 3 — a larger initial buy mechanically creates the liquidity | Refuse an entry clause whose admit rate tracks the varied axis. |
| **A broad fit is blind to a cohort-specific term** | dropping `nonvol_buy >= 1.6 @2s` leaves spend=1.5 and spend=4 byte-identical and costs 10 points on spend=5 | Re-check narrow (D11) and enrich narrow (D12). |
| **The exit OR is the edge** | OR +30.97 / +31.66; `stall>=30` alone +14.21 / +21.02; `gross_flow<15@10s` alone +8.69 / +27.63; `nonvol_buy>=1.6@2s` alone −1.87 / +10.22; `retrace>=36` alone −39.50 / −44.20 (15d / 7d) | Exit is the primary search axis. An unarmed `retrace` is a hard stop from entry. |
| **A price trail destroys a working exit** | adding `trail>=15@10s`: spend=5 +30.7 → −15.3, spend=4 +40.8 → −14.1, spend=1.5 −13.8 → −14.2 | Kept in the library, flagged — a library that cannot express a refuted term cannot re-refute it. |
| **Execution can be the entire loss** | a dump-scalp family: the same taken set repriced worst-fill+impact vs first-fill+fee-only differs by **6.93 pp/trade** (n=5,872) while the signal is near-breakeven (PF 0.95 optimistic) — [history](../history/2026-08-16-dump-scalp-execution-gap.md) | D8, and the spread column. |
| **A stop does not stop on sparse prints** | authored `pnl <= -8` realizes a **−19.4%** mean (worst −102%) — price gaps straight past the level | Attribution prints realized level beside authored. |
| **A gate the move itself creates is a lagging gate** | `gross_flow(60) >= 55` selected post-move moments; dropping it improved quality *and* volume (5,872 vs 4,747) | The lagging-clause diagnostic. |
| **Pool by money, not by mean** | `Σpnl/Σentry` and `Σwins/Σcloses` | A mean lets a 99-token cohort outvote a 565-token one. |

Sanity anchors for the reference family: a four-term exit closes 1,086 = 253+697+136
over spend=1/2/3. A low `n` is **not** evidence of a dropped cohort — `gross_flow<25`
closes ~76% as many positions as `<15` in every cohort, totalling 856 = 192+565+99.
Verify a suspected gap by re-running it.

---

## 6. What is void

Recorded so it is neither re-derived nor re-trusted.

- **Every offline number computed on a labels-only cohort** — a ~13× diluted
  population. This voids an offline "validation" reading +2.8% where simulate reads
  +31.0%, and voids a *refutation* of launch tells (void = untested, not refuted).
- **"The generator loses to the incumbent."** The library was the incumbent plus seven
  perturbations of its own structure. A search over the neighbourhood of X can only
  conclude X; no from-scratch generator ran.
- **`creator_wallet` as a unit of analysis.** 262 distinct wallets over 264 tokens is
  the signature of rotation, not 262 creators. The lake's `tokens.parquet` carries no
  such column.

---

## 7. Performance rules

The budget for a family of 6 × ~40 candidates is **6 corpus loads and 6 folds**, not
240 runs.

| Rule | Why |
| --- | --- |
| Fan out over candidates **inside one pass per token** | `score_combos` folds every combo while walking a token's trades once. One HTTP simulate per candidate re-loads the corpus every time. |
| **Two corpora resident, never six** | The target stays loaded (fit, level, capture, attribution, ablation, enrich all read it); fit siblings iterate one at a time. Six concurrent corpora is how a run OOMs. |
| **Sims strictly sequential** | Measured, not theoretical: concurrent DuckDB runs starve each other (`usable_mb=0`), then fail. |
| Oracle computed **once per corpus** | Rule-independent, so recomputing per candidate is the single biggest available mistake. Opt-in via `Selection::with_oracle` (~4 B/row), so no other sweep pays. |
| Fit stops at the **archive fold** | It only needs a ranking; the full `run_replay` runs on the target and finalists only. |
| Scope is **dimension-only** and resolved up front | `matching_mints` never scans trades, so an empty cohort fails before any load. |
| Attribution, the axis gate and cost clearance are **free** | All read numbers the run already produced. |
| Cancel is checked **between** sibling loads | A corpus load cannot be cancelled mid-flight; this is the difference between a 30 s abort and killing the process. |

**Not levers:** shrinking `token_cap` on fit cohorts, sampling tokens, or lowering fill
fidelity on the fit stage. Each changes the ranking the whole design exists to
transfer.

---

## 8. Running it

```powershell
scripts/db-incremental-sync.ps1 -IncludeToday -ExportLake   # freshness is a FATAL gate
cargo run -p hunter-lab                                     # :8140
cd hunter/frontend; npm run dev                             # lab app :5174
```

Then `/strategies/family-search`. Pick a target fingerprint; siblings resolve
themselves. Form fields worth setting:

| Field | Guidance |
| --- | --- |
| **Standing exit** | One per line, exactly as the attribution table prints them — `liquidity >= 85`. Put mechanical alarms here so they stop competing as discovered ones. A term that does not parse fails the run. |
| **Min win %** | Leave at 0 first: the ungated control already sets the bar. Raise only if the draft's win rate sits too near it. |
| **Min closes** | 8. Three wins in four trades is not a 75% win rate. |
| **Cost bar (x)** | 0 refuses only the unarguable case. 1 is the stricter bar the dump-scalp result argues for. |
| **Slots** | 40. Every candidate is now g4-shaped, so the budget buys depth rather than filtering out junk. |

Board order is the argument: verdict and its gates → portrait → execution → grade →
the draft → family → which alarm made the money → what each condition is worth →
conditions offered → entry timing → entry gates → archive.

The one presentation rule the payload imposes: **`fit_ret_pct` ranks, `target_ret_pct`
reports.** Every fit number is dimmed and labelled `rank only`.

Verification: `cargo test -p hunter-lab --lib family_search` (73 no-DB tests covering
the pooling rules, both selection bars, the composer's shape guarantees, the standing
split, the enrich acceptance rules, and the attribution/sweep parity guard).

---

## 9. Open

- **Never run against the lake.** Every acceptance number is still open: ρ ≈ 0.83 on
  the reference family, `liquidity > 20` flagged as axis-duplicating, the composer's
  top candidate carrying ≥ 2 entry ideas and ≥ 3 alarm kinds on real signatures.
- The one test that cannot be constructed: family search's replay of a draft equals
  HTTP Simulate on the same fingerprint, range, fill, cost, guard and buy.
- Whether fit-broad holds on a family varying `cu_price` or `ix_labels` rather than
  `spendable_lamports_in`. D2's ρ is the instrument that answers it.
- Partial exits, which inherit the attribution machinery (D4).
- When the acceptance numbers land, fold this file into
  [rule-search-method.md](../plans/strategies/rule-search-method.md) and
  [../arch/sweep.md](../arch/sweep.md), and delete it.
