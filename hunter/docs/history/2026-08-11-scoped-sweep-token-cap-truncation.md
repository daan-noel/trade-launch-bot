# 2026-08-11 — fingerprint-scoped grouped sweeps were truncated by `token_cap`

## What happened

`grouped_sweep.rs` applied the saved-fingerprint scope as a `corpus.tokens.retain(...)`
*after* `LakeSource::load`, so `token_cap` was a `LIMIT` on a `created_at DESC` scan over
**all** candidate tokens rather than over matched ones. Flow discovery and metric discovery
had already been moved to `matching_mints` (scope before the trade scan, `docs/arch/sweep.md`
rule 1); the grouped sweep never was, and the doc's rule read as if it already covered it.

The lake holds ~25k token creations per day, so even the `MAX_TOKEN_CAP` of 100k covered only
about the newest four days of creations. Any fingerprint whose tokens spanned a longer window
silently lost every match older than that slice.

Measured on fingerprint `sweep 0f53d622 · group 13` (`897353e1`), whose 106 matching tokens
span 17 days: a scoped run at `token_cap = 100000` loaded **28** tokens. The 28th-newest match
sat at candidate rank 110,418. At the default cap of 10,000 it would have loaded **0**.

Nothing surfaced the loss. `candidates_capped` fires the truncation `SweepNotice` only when
`sel.mints.is_none()`, which was true, so the notice did appear — but it counts candidates, and
the run still reported a healthy `token_count`, a full combo grid and plausible per-combo stats.

## Live consequence

**Every `grouped_sweep_runs` row with a non-NULL `fingerprint_id` created before this date was
computed on a newest-N slice of its fingerprint, not the whole thing.** Their `token_count`,
per-combo `n_fired`, and `score` are all understated against a fraction of the intended corpus,
and `best_combo_id` was crowned on that fraction. Re-run any such sweep before trusting it —
including any rule promoted from one. Unscoped runs and `simulate` are unaffected (simulate has
no cap).

## Fix

`grouped_sweep.rs` resolves the scope before the load: look the fingerprint up, call
`src.matching_mints(&sel, fp_to_engine(&fp))`, set `sel.mints`, and drop the post-load `retain`.
The clip bit now comes from `matching_mints`, so the truncation notice reports *matching* tokens.
Because `sel.mints` folds into `selection_hash`, a scoped and an unscoped run over the same
window key differently — which is correct, and how flow discovery already behaved.

Same-fingerprint re-run after the fix: 28 tokens → **106**.
