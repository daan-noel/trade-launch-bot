# Grouped-sweep entry cache poisoned every sibling combo (2026-07-26)

**Symptom.** Grouped sweeps with **exit-side metric axes** reported wrong `n_fired` and
wrong entry rows/prices for every combo that was not first in its entry class — so the
ranking, and therefore the promoted "winner", could be decided by cache order rather than
by the rule.

**Cause.** `can_enter` is **exit-dependent**: an exit metric that holds at a candidate row
vetoes the entry, and `resolve_entry` mirrors that. So the resolved entry is a function of
the *whole* rule, not just its entry axes — two combos sharing an `entry_key` but
differing on the exit side can legitimately enter on different rows.

The fold's single-slot cache was keyed on `entry_key` alone. Caching the *resolved entry*
there made the first combo of each class donate its entered set to every sibling.

**Fix — two-stage entry.**

- **Stage A** `entry_candidates` — the exit-independent walk (dead check, mono-kills,
  entry-condition eval), opened once per `entry_key` per token and **resumed** as combos
  ask for deeper candidates, so the short-circuit at the first admissible row survives.
- **Stage B** `resolve_entry_from` — per combo: walk the shared candidates applying that
  combo's veto, then price the first admissible row through a per-class fill memo.

Pure TP/SL sweeps (the 1M-combo shape) are untaxed: their exit reqs are position-scoped,
read `NaN` before entry, and so can never veto (`BoundCombo::entry_veto_possible`), making
Stage B a candidate lookup plus a memo hit. `ExitCtx` (the prefix-extrema hulls) rebuilds
on `exit_ctx_key` — the resolved `fill_row` — not on entry-key staleness.

Locked by `guard::fold_gives_each_exit_variant_its_own_entry` (fold ≡ per-combo `scan` ≡
`run_replay`, both combo orders) and
`engine::tests::fold_reresolves_entry_per_exit_variant_within_one_class`.

**The rule this produced.** Only cache a value against a key that determines it. `entry_key`
does not determine the resolved entry, because the exit side can veto — and a cache keyed
too coarsely produces *plausible* wrong answers, which is the expensive kind.

**Still live as an operational caveat:** stored runs from before this fix carry poisoned
aggregates and must be re-run. Kept in
[`@arch/sweep.md`](../arch/sweep.md).
