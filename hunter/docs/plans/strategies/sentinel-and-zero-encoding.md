# Sentinels and zero encoding

Why `0` means "off" in a few places, why it must never spread, and the failure shape that
appears whenever one sentinel gets two readers.

## `0` may mean "off / unbounded" only where 0 is not a valid value

Two governance caps carry that encoding, and nothing else does:

| Field | Encoding | Reader |
| --- | --- | --- |
| `max_total_tokens` | `0 ⇒ unlimited` | `Cap::zero_unlimited` |
| `max_concurrent_tokens` | `0 ⇒ the default of 1` (the API rejects `< 1`) | `Cap::zero_defaults_to` |

Both decode through the ONE reader `hunter_engine::Cap` — never a re-derived `!= 0` at a
call site. `UNLIMITED = u32::MAX`, so `allows()` stays a single `<` on the hot path.

**Slippage bps is NOT such a field.** A typed value is honored literally and `0` is a 400
(`validate_slippage_bps`), because *blank* — not `0` — carries the per-side policy (buy ⇒
default, sell ⇒ no floor). See
[`../trade-execution/slippage-logic-buy-sell.md`](../trade-execution/slippage-logic-buy-sell.md).

## Anything measured uses `Option`/NULL for "not set"

A SOL amount, a count, a bucket edge, a width: `0` is a real observation there. Never fold
the two encodings. A fingerprint axis of `0` lamports is the bucket `[0, width)`, and only
`None` drops the axis from the fingerprint's identity — `bucket_axis`,
`IS NOT DISTINCT FROM`, and the `∅` grouping sentinel all depend on that split.

**An empty collection is the same sentinel.** `ix_labels: Some([])` means "not set", so it
collapses to `None` through the ONE decider `hunter_engine::fingerprint::configured_labels`,
and `from_json` folds `[]` → `None` at the wire boundary so the ambiguous state never
reaches storage.

## One sentinel, one reader — the failure shape

A second reader of the same sentinel is the bug, and on the fingerprint path the two
readers fail in **opposite directions**: the engine matcher turns "no criteria" into
*matches nothing* (rules go silently dead), while `fingerprint_scope_clauses` turns it into
*matches every token in the window*.

Three shapes to recognize:

- **Two readers, different defaults** — `bucket_size_amount` decoded as `0 ⇒ default 0.1`
  in the SQL mirror but as a literal `0` in the matcher, where it saturates every positive
  amount into one bucket and arms on any non-zero value.
- **Two readers, different emptiness tests** — `ix_labels` checked with `is_some()` on one
  side and empty-filtered on the other.
- **A writer that clamps the sentinel away before any reader sees it** — the slippage
  path, where `0` inverts from "accept any fill" into the tightest possible floor on the
  bot's own exits.

Locked by `has_any_criterion_agrees_with_engine`,
`fingerprint_scope_sql_buckets_every_sol_axis_at_the_engine_width`, and
`a_typed_percent_reaches_the_trader_unchanged`.

## Surfacing a sentinel in the UI

Where a sentinel stays, the UI marks it with the `Input` `blankZero` prop — never a
truthiness check. Today that is the rule editor's **Max total** and Trader Analysis **Max
tokens** (blank / `∞`).
