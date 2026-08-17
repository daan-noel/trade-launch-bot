# Family search — implementation plan

Buildable form of [family-search.md](family-search.md). Read that first: the
decisions D1–D7 are the contract this plan implements.

**Scope rule.** A new job under `lab/src/family_search/`. The existing
[rule search](../plans/strategies/rule-search.md) is not modified — not its module,
not its handler, not its report. Where family search needs something from shared
sweep code, the change is **additive only**: a new opt-in field, a new function, a
new enum variant. No existing signature, default, or behaviour moves.

---

## 0. Module map

```
  lab/src/family_search/
    mod.rs           run_family_search()  — the orchestrator, no decision logic
    family.rs        sibling resolve off the `fingerprints` table
    oracle.rs        suffix-peak + capture ratio
    attribution.rs   per-alarm rollup (n + pnl per authored exit slot)
    generator.rs     signature-earned candidates + per-family diversity quota
    gates.rs         axis-duplication refuse · freshness · fill-timing ladder
    score.rs         pooled fit (Σpnl/Σentry) · Spearman rho
    dto.rs           request / report wire types
    report.rs        the board payload

  lab/src/api/handlers/strategies/family_search.rs
    POST   /api/strategies/family-search          → 202 { run_id }
    POST   /api/strategies/family-search/cancel
    GET    /api/strategies/family-search/{run_id}
    GET    /api/strategies/family-search/last

  additive elsewhere (nothing removed, nothing re-signed):
    lab/src/sweep/corpus.rs        Selection { with_oracle: bool }
                                   CorpusToken { peak_after: Option<Arc<Vec<f32>>> }
    lab/src/sweep/projection.rs    pub fn suffix_peak(&[CorpusTrade]) -> Vec<f32>
    lab/src/state/local_state.rs   HeavyJob::FamilySearch + result cache
    core/src/models/ingest.rs      SseEvent::FamilySearch{Progress,Notice,Finished}
```

Reused read-only, never forked: `hunter_engine::reduce` (the ONE kernel),
`hunter_engine::fingerprint::matches`, `LakeSource`, `Corpus`, `Pricing`,
`CostModel`, `FillModel`, `weighted_return_pct`, the dupe guard.

---

## 1. Slice 1 — measurement

Nothing here changes a decision. It makes the two questions answerable that the
current report cannot answer at all.

### 1a. Oracle / capture ratio

The oracle is a property of `(token, entry moment)` — **not of the exit rule**. So it
is computed once per corpus and reused across every candidate and every run.

```rust
// lab/src/sweep/projection.rs  (additive)
/// `peak_after[i]` = max `chart_spot_price` over `trades[i..]`. One backward pass,
/// O(n) at load, O(1) at any entry index.
pub fn suffix_peak(trades: &[CorpusTrade]) -> Vec<f32>;
```

- Gate it behind `Selection::with_oracle`, the same opt-in idiom as the existing
  `with_signatures` / `with_flow_text`. Every other sweep pays nothing.
- Cost when on: 4 B/row against `CorpusTrade`'s ~100 B — under 5%, one pass, inside
  the projection that already runs at load. Never a second walk.
- Price the oracle exit through the **same** `pnl_with_costs` and the same
  one-print-later fill discipline as the realized exit. A denominator that pays no
  round trip is not a comparison — see
  [execution-costs.md](../plans/strategies/execution-costs.md).

```rust
// lab/src/family_search/oracle.rs
pub struct Capture {
    pub capture_pct: f64,      // 100 * Σ realized_pnl / Σ oracle_pnl, over oracle_pnl > 0
    pub n_with_upside: u64,
    pub n_no_upside: u64,      // no profitable exit ever existed after entry
    pub oracle_pnl_sol: f64,
    pub realized_pnl_sol: f64,
}
```

`n_no_upside` is its own line and never folds into the ratio (D3). It is the entry
score, and it is readable with no exit rule at all.

### 1b. Per-alarm attribution

Everything needed exists; nothing is aggregated. `ExitReason::Metrics` carries
`{ metric, operator, value, window }`, `exit_metric_labels()` in
[strategy.rs](../../lab/src/sweep/generic/strategy.rs) resolves an authored slot per
exit req at bind time, and `TokenOutcome` already has `exit_metric*` /
`exit_metric_slot`.

- Family search's scorer **populates** those fields from the `ExitReason` the engine
  returns. Rule search's three hardcoded `None`s stay as they are — out of scope.
- `attribution.rs` rolls up `n`, `Σpnl_sol`, and `Σentry_sol` per slot, so the
  percentage is money-over-capital. Count alone is misleading: a term that fires 200×
  for −0.4◎ and one that fires 20× for +1.1◎ read the same today.
- **Duplication guard** (required by the SSOT rule in
  [../../CLAUDE.md](../../CLAUDE.md)): the rollup's per-slot counts must equal the
  sweep's `n_exit_metrics_by_slot` on a shared fixture. A no-DB test asserts it.

### 1c. Freshness gate

`Corpus::last_trade_at` onto the report. Refuse — or badge loudly — when the
requested `until` outruns it by more than a slack the request sets. This is the check
that catches a window silently ending two days early.

**Acceptance:** on one cohort, one rule, the report prints capture, the no-upside
count, and a per-alarm table whose slot counts match the sweep's, and a run whose
range outruns the lake is refused.

---

## 2. Slice 2 — the family

### 2a. Sibling resolve

```rust
// lab/src/family_search/family.rs
pub enum Axis { CuLimit, CuPrice, InitBuy, MaxCost, SpendableIn,
                FirstSlotBuy, FirstSlotSell }

pub struct Sibling { pub fp_id: Uuid, pub name: String, pub value: f64 }
pub struct Family  { pub target: Uuid, pub varied: Axis, pub members: Vec<Sibling> }

/// Same `ix_labels`, same `bucket_size_amount`, identical on every axis but one.
/// Mechanical off the `fingerprints` table — no heuristic, no fuzzy match.
pub async fn resolve(repo: &FingerprintRepo, target: Uuid) -> Result<Family>;
```

A family of one is a valid outcome. It means fit-broad does not apply, the run
degrades to single-cohort, and the report says so rather than inventing siblings.

### 2b. Fit broad, validate narrow

```rust
// lab/src/family_search/score.rs
pub struct BroadFit {
    pub rank_fit: Vec<usize>,    // pooled over fit siblings, Σpnl_sol / Σentry_sol
    pub ret_validate: Vec<f64>,  // held-out target cohort only
    pub rho: f64,                // Spearman(rank_fit, rank_validate)
}
```

- Pool by `Σpnl_sol / Σentry_sol`, never a mean of per-cohort percents.
- Report `rho` as the procedure's self-test (D2). Below a floor, the board states
  that fit-broad does not hold on this family instead of ranking anyway.
- The fit stage never quotes a level. Every candidate can be negative on the fit set
  while the winner pays +31% on the target; that is the expected shape, not a bug.

### 2c. Axis-duplication refuse gate

Costs **zero extra runs** — `enter_pct` per cohort already falls out of the scoring
the family loop performs.

> For each entry clause, take its admit rate on every family member. If
> `|Spearman(admit_rate, varied_axis_value)| >= 0.8`, the clause is a proxy for the
> fingerprint axis. Refuse it, or demote it to a diagnostic.

### 2d. Narrow re-check

After the broad fit picks a finalist, re-score it on the target cohort with each term
dropped in turn. A term worth nothing broad can be worth 10 points narrow, and only
this stage sees it.

**Acceptance:** on the reference family, the pooled fit reproduces ρ ≈ 0.83 against
the held-out cohort, and `liquidity > 20` is flagged as axis-duplicating.

---

## 3. Slice 3 — the generator

- Candidates are **earned** from cohort signatures (the §3 test in
  [rule-search-habit.md](rule-search-habit.md)), not nudged from an existing rule.
- **Per-family diversity quota**, applied at generation *and* at every expansion
  stage: bucket by end-event family (flow · organic · stall-clock ·
  liquidity-ceiling · price-trail) and let no family exceed ~40% of slots. Three
  thresholds of one metric is one thesis, not three.
- Price-trail stays in the library, flagged (§3 of the charter).
- **Expansion bases are picked under the quota, not by raw rank** — D5, fourth line.

**Acceptance:** the generated set spans ≥ 3 end-event families, and the run's output
is byte-identical with the `rules` table emptied.

---

## 4. Performance

The budget: a family of 6 × ~40 candidates. The plan targets **6 corpus loads and 6
folds**, not 240 runs.

| Lever | Why it holds |
| --- | --- |
| **Fan out over candidates inside one pass per token** | `score_combos` already folds every combo while walking a token's trades once — that is what `set_total(total_tokens, combos_per_token)` counts. Candidates are near-free against the token walk. One HTTP simulate per candidate is the shape to avoid: it re-loads the corpus every time (240 loads for the same work). |
| **One cohort resident at a time** | The load phase is the RAM spike. Iterate siblings sequentially and keep rayon *inside* a cohort (`available_parallelism − 2`, as the existing jobs do). Six concurrent corpora is how a run OOMs. |
| **Oracle once per corpus** | Rule-independent (D3) — computed at load, reused by every candidate in every stage. Recomputing per candidate is the single biggest available mistake. |
| **Fit stage stops at the archive fold** | `score_combos` produces the ranking; `build_report`'s `run_replay` is the authority pass. Fit needs only rank, so run the full replay on the **target cohort and the finalists only**. This is the existing two-tier design, not a new approximation. |
| **Warm corpus cache** | Keyed by `Corpus.hash`. A re-run over the same family and range re-loads nothing. |
| **Scope is dimension-only** | `matching_mints` reads `tokens.parquet` and never scans trades. Six scope resolves are cheap; do them all up front so an empty cohort fails before any load. |
| **Settled-tick skip** | Already ~180× on multi-day ranges. Anything new that moves a tick must declare a `ClockHorizons` field — see the landmine table in [../../CLAUDE.md](../../CLAUDE.md). |
| **Attribution is free** | The slot is resolved at bind time and the outcome field already exists; the rollup is one pass over outcomes already in hand. |
| **The axis-duplication gate is free** | Reads `enter_pct` the family loop already computed. |
| **DuckDB spill** | Per-connection spill dir, as the existing lake sessions use. A shared `temp_directory` is what produces `Unknown exception in Finalize!` under concurrency. |

What is explicitly **not** a lever: shrinking `token_cap` on fit cohorts, sampling
tokens, or lowering fill fidelity on the fit stage. Each changes the ranking that
Slice 2 exists to transfer.

Progress reporting: the family loop emits per-cohort phase labels through the same
`SweepObserver` shape rule search uses, so a long run is legible while it runs.

---

## 5. Land order

```
  SLICE 1  measurement — no new decisions                                   DONE
    [x] Selection::with_oracle + suffix_peak (additive, opt-in)
    [x] oracle.rs      Capture, priced through the same cost + fill
    [x] attribution.rs per-alarm n / pnl / entry by slot + parity test
    [x] gates.rs       freshness refuse (D7)
        UNLOCKS: exit terms gradeable in ONE run;
                 entry gradeable WITHOUT an exit rule

  SLICE 2  the family                                                      DONE
    [x] family.rs  sibling resolve
    [x] score.rs   pooled fit + rho self-test
    [x] gates.rs   axis-duplication refuse
    [x] score.rs   narrow re-check of the finalist
        UNLOCKS: cohort quality separated from rule quality

  SLICE 3  the generator                                                   DONE
    [x] signature-earned candidate menu
    [x] per-family diversity quota at generate AND expansion

  SLICE 4  board + wiring                                                   DONE
    [x] handler, routes, SSE, HeavyJob::FamilySearch, result cache
    [x] portrait prose + draft columns (ungated control · oracle · optional incumbent)
    [x] a lab page over the report payload
```

All four slices are code-complete and the module is documented in
[../arch/sweep.md](../arch/sweep.md) and [../arch/frontend.md](../arch/frontend.md).
**Nothing below has run against the lake yet** —
until it does, the acceptance numbers in §1–3 and the last line of §6 stay open, and
this file stays. First run needs
`scripts/db-incremental-sync.ps1 -IncludeToday -ExportLake`.

Slice 1 does not reorder. Every finding in the charter is rank-only: which exit is
better, never how much of the available money any of them leaves behind — and one
re-run burned per term to learn which alarm did the work. Both are measurement gaps,
both are fixed with mechanisms already in the repo, and neither adds a decision path.

---

## 6. Constructed tests

Pass without the lake, same bar as
[rule-search-method.md](../plans/strategies/rule-search-method.md) §5.

- [x] `suffix_peak` on a known series equals the naive backward max, and the oracle
      exit is charged the same round trip as the realized one.
- [x] A token whose price only falls after entry lands in `n_no_upside` and is absent
      from the capture denominator.
- [x] Per-alarm counts equal the sweep's `n_exit_metrics_by_slot` on a shared fixture.
- [x] Two exit terms with the same metric name but different windows occupy distinct
      slots (a dynamic group and its lifetime twin share `metric.name()`).
- [x] Pooled fit equals `Σpnl / Σentry`; swapping two cohorts' order does not move it,
      and a mean-of-percents implementation fails the case.
- [x] A family of one degrades to single-cohort and reports no `rho`.
- [x] An entry clause whose admit rate tracks the varied axis is refused.
- [x] Emptying the `rules` table changes nothing in the output (D5).
- [x] A request whose `until` outruns `Corpus::last_trade_at` is refused.
- [x] The generated candidate set spans ≥ 3 end-event families.
- [ ] Family search's replay of a draft equals HTTP Simulate on the same fingerprint,
      range, fill, cost, guard, and buy. (Needs the lake — the one test here that
      cannot be constructed.)

---

## 7. Fold-in

When Slices 1–3 run, fold the charter's decisions into
[rule-search-method.md](../plans/strategies/rule-search-method.md) and the job wiring
into [rule-search.md](../plans/strategies/rule-search.md), move the module map into
[../arch/sweep.md](../arch/sweep.md), and delete both roadmap files.
