# One group key, three readers, three units (2026-08-04)

Two defects found together in the grouped-sweep group-key path. Both are the SSOT trap
named in the product `CLAUDE.md`: one wire field, several readers, each silently reading
it in its own unit.

## 1. The SOL field filter was unsatisfiable on both backends

The sweep compared the raw **lamports integer**. The dashboard compared the rendered
**bucket label** — so it was evaluating `'1.5–1.6' = '1.515'`, a string compare that could
never be true. The frontend sent **human SOL** to both, and `parseNumbers` rejected the
range syntax that could at least have matched the dashboard. Three readers, no two in
agreement, and no error anywhere: the filter simply returned nothing.

**Fixed by** one shared parser, `hunter_engine::grouping::SolFilter`, accepting human SOL
in two forms — an exact amount (`"1.515"`) or a half-open bucket range (`"1.5–1.6"`, the
text a group chip renders, so a chip pastes straight into the box). Consumers that must
agree are each locked by a test: `grouped_sweep::matches_field_filter`,
`creation_stats_repo::field_filter_pred`, and the frontend's single `buildFieldFilters`.

## 2. An 8-position `to_char` mask silently produced wrong groups

`to_char` renders `########` on overflow — and that string is a **valid TEXT group key**,
so the overflow became a *wrong group* rather than an error. The 8-position mask did that
to every token carrying pump.fun's `max_cost_lamports = u64::MAX` "no slippage limit"
sentinel (≈1.84e10 SOL): **11,250 tokens in a 30-day window**, all collapsed into one
`########.#` group that disagreed with `bucket_sol_label`.

**Fixed by** `SQL_MASK_INT_DIGITS = 18` on every group-key mask.

## 3. The selection was re-derived per consumer, always wider

Each consumer rebuilt a group's selection from `group_key` alone, and every derivation
lost clauses in the **widening** direction: `field_filters` dropped with a `warn!`; a
scoped run that also grouped by extra fields promoted the *scope* (wider than the group);
a `∅` key dropped the axis, so the rule matched tokens that HAVE a value; and
`token_program_id` / `is_cashback_enabled` had no axis to land in.

Each of those shipped a **rule that armed on a superset of the tokens the promoted numbers
came from** — the worst possible failure for a promote, because the rule looks validated.

**Fixed by** `lab/src/sweep/selection.rs` (`GroupSelection`) resolving it ONCE, with every
consumer reading that, plus **promote fails closed**: a selection with no fingerprint
expression returns 400 naming the blocking clauses rather than dropping one to produce a
wider gate.

## The rules these produced

- A rendered label and the value it renders are different types. Never compare across
  them, and never let two layers each pick a unit.
- A formatter that degrades to a *valid-looking* output on overflow (`to_char` →
  `########`) is worse than one that errors — size the mask for the domain's real maximum,
  which for an on-chain `u64` arg is `u64::MAX`.
- A derivation that can only lose clauses must not exist per consumer; resolve once, and
  fail closed when the target representation cannot hold the answer.

Current contract: [`@arch/sweep.md`](../arch/sweep.md).
