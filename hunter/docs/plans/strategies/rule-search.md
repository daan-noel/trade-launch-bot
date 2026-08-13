# Rule search — lab job

Lab page that runs [rule-search-method.md](rule-search-method.md) for one fingerprint
and one datetime range. It is a sibling of Grouped sweep, Flow discovery, and Metric
discovery, not a sweep mode.

**Input (the form):** fingerprint, datetime range, buy size, fill model, cost model,
copycat guard. Optional incumbent rule (compare only).
**Output (the board):** champion `RuleParams`, empty-entry, incumbent, verdict,
top archive rows. Promote uses the existing sweep modal.

The form does not expose metrics, windows, or thresholds. Those come from the
cohort and the registry. Another rule's params are not an input.

```
  LAB  Strategies → Rule search

  form                                      hunter-lab job
  ────                                      ──────────────
  fingerprint                               load matched + lake once
  datetime range                            cut table from this range
  buy / fill / cost / copycat               role product → full RuleParams
  [optional incumbent]                      fast archive (series + shared entry)
                                            copycat (+ caps) time-order merge
                                            run_replay: champion, empty-entry,
                                                        incumbent, top few
              │
              ▼
  board:  champion vs empty-entry vs incumbent
          useful / ungated / refuse
          params · top archive
          Promote → inactive paper · Simulate
```

## Why not Grouped sweep

| | Grouped sweep | Rule search |
| --- | --- | --- |
| You set | fingerprint and metric axes | fingerprint and datetime range |
| Machine | ranks your grid | fills roles from the registry and this range's cuts |
| Inner loop | `GenericSweepStrategy::scan` over groups | series walk on **one** fingerprint, then `run_replay` on the report set |
| Output | best combo per group among a large grid | one champion and a short archive |

Sweep `scan` exists to rank tens of thousands of combos across a huge corpus and
deliberately drops copycat (D7) and caps (D2). This job's corpus is one habit
window. It does not call sweep `scan` as the PnL authority and does not reuse
`GenericAxisBuilder`.

## Form

Reuse sweep/discovery controls: fingerprint picker, `DateTimeRangePicker`, buy,
fill, cost, copycat override (default ON, same as Simulate unless the request
sets it), optional incumbent picker.

| Field | Role |
| --- | --- |
| Fingerprint | match set (engine `match_all`) |
| Since / until | the habit; search this range only |
| Buy size | incumbent's size when one is set, else the form value |
| Fill / cost | same pair Simulate uses; default worst fill + `pumpfun_impact` |
| Copycat | `skip_duplicate_identity`; ON for the empty-entry vs champion verdict |
| Incumbent | compare-only; never a seed, never a template |

No axis rows, no harvest templates, no metric checklist.

## Job

`hunter-lab` owns the search. Lab-only route, lab-only API. Same job shell as
sweep/discovery: `POST` → `202`, SSE progress, cancel, persist the run, single-flight
against sweep / flow-discovery / metric-discovery (one heavy analysis job).

`as_of` freezes at session open so every combo shares one deadness "now".

Load histories with flow columns when the fingerprint has split config; otherwise
every `m_flow_split` clause is silent.

Generator, scorer, and report live in `hunter-lab`. The page starts the job and
renders the archive.

## Scorer

One condition evaluator: `CompiledRule` on a precomputed `MetricSeries` (the same
leaf live, Simulate, and sweep already use). There is no third engine.

```
  load matched tokens + series          ONCE

  rayon over tokens
    for each unique ENTRY filling:      walk the series once
      candidate rows (exit-independent)
      for each EXIT bag:
        first candidate this exit does not veto → fill
        first exit OR → sell

  merge outcomes in global time order
    copycat guard
    caps, when the run has them

  archive = those summaries

  run_replay (shared event queue) ONLY
    champion, empty-entry, incumbent, top few archive rows
```

The generator is entry × exit, so the entry walk is shared across bags. Re-walking
entry per pair is not an inner loop.

Copycat and caps are a merge after per-token scoring, not a reason to fold one
global event stream per combo. Sweep dropped that merge because a 100k-combo
token fan-out cannot keep cross-token state. This corpus is one fingerprint, so
the merge is cheap and required: empty-entry eats copycats; a selector often does
not. Ranking with the guard off inflates ungated PnL and can flip the verdict.

Horizon is Simulate's (`as_of` / corpus last trade), not sweep's per-token tail
cap. Quiet tokens close the same way as Simulate.

**Report columns are `run_replay`.** Same fill, cost, guard, buy, `as_of` as
`POST /api/strategies/simulate`. If the fast archive winner and the replay winner
disagree, the board shows both and the champion is the replay one.

Guard test: `run_replay` of one draft equals HTTP Simulate summary on the same
fingerprint, range, fill, cost, guard, and buy.

A small corpus (hundreds of tokens × hundreds of combos) may run combo-parallel
`run_replay` on a shared queue instead of the series loop. The report columns stay
`run_replay` either way. Swap only the inner archive.

Do not persist every archive row as a Simulate result. Slim summaries during
search; one Simulate (or promote) for the champion the operator wants to inspect.

## Board

Not a 100k-row sweep table.

| Block | Content |
| --- | --- |
| Verdict | refuse / juice ungated / candidate — [method report](rule-search-method.md#4-report) |
| Three columns | champion, empty-entry (same exit bag), incumbent; n, SOL, PF, enter%; fill spread next to every SOL number |
| Champion params | readable entry AND / exit OR clauses |
| Archive | top few full rules, including stacked different-metric exits when they won |
| Actions | Promote (existing `PromoteRuleModal`, inactive paper) · open in Simulate |

Refuse is a finished run, not an empty page.

## Locked

- No metric is privileged. New registry rows join by flags.
- Incumbent is compare-only.
- No kitchen-sink entry, no greedy add/drop on entry.
- Exit bags are 0–2 different metrics; giveback and clock still compete.
- Entry can-fail is 0–2 independent clauses (selector and extra pooled); trigger
  stays 0–1.
- Cut table: contrast + winner floor on entry; dump-lead + runner after-dump +
  dumper p90 on exit. Mixed-everyone quantiles only when the split is thin.
- Do not rank an exit bag on empty-entry before it meets an entry.
- Staged scale-out is the same generator on a third bag; this job scores entry +
  the global exit until that bag exists.

An algorithm change is graded by [method §5](rule-search-method.md#5-prove-an-update).
The board's three columns (champion / empty-entry / incumbent) are the operator
ablation. Constructed-corpus tests own the cut sources; the page owns whether
the champion pays.
