# `u64` creation-instruction args — the `max_cost_lamports` ceiling

Reference for why `max_cost_lamports` can read `18446744073709551615` lamports
(≈1.84e10 SOL), what that value means, and the shape every layer must carry it in.
Written 2026-08-04 after the value surfaced as a group key rendered
`18446744073.7096`.

## It is not an overflow — it is the on-chain argument

pump.fun's `buy` / `buy_v2` are **tokens-out** instructions: the client names the
token amount it wants and `max_sol_cost` is the **slippage ceiling**, not the spend.
A dev (or their bot/SDK) who wants "fill at any price, no cap" passes `u64::MAX`.
Some SDKs also compute `amount * (1 + slippage)` in `u64` and wrap to the same
place. Either way it is a **sentinel meaning "no limit"**, never an amount — nobody
bid 18.4 billion SOL. ~11,250 tokens carried it in a 30-day window on 2026-08-04.

`spendable_lamports_in` shares the `u64` domain (SOL-in encodings) and gets the same
treatment for uniformity, though only the ceiling arg is observed at the top of the
range in practice.

## Storage is, and stays, the JSON number

`ingest::consumer::buy_ix_to_json` writes the decoded `u64` straight into
`tokens.initial_buy_instruction`. Postgres stores `jsonb` numbers as
arbitrary-precision `numeric`, so **nothing is lost at rest** and `->>` yields the
same text whether the value was written as a number or a string. There is no
migration and no backfill: every corruption was a lossy *read*, not a lossy write.

Both persisted shapes are nevertheless accepted on read (number and numeric string),
because the *wire* encoding is a string (below) and a value can round-trip back in.

## The three readers that used to disagree

One sentinel with three readings is the bug this file exists to prevent. Before
2026-08-04 the same row was:

| Reader | Saw | Why |
| --- | --- | --- |
| `grouping::extract_lamports` → engine matcher, sweep group key, Parquet lake | `-1` | `v.as_u64().map(\|u\| u as i64)` wrapped |
| `creation_stats_repo` group key SQL | `18446744073.7096` | `::float8 / 1e9` → 15 significant digits |
| `parse_lo_lamports` (group key → promoted fingerprint) | `i64::MAX` | `f64 as i64` saturated |

## The shape each layer carries

- **Decode — one seam.** `hunter_engine::grouping::extract_lamports` returns
  `Option<u64>`, the on-chain domain, and never narrows. Anything needing an `i64`
  goes through `bucketable_lamports`, which returns `None` rather than lying.
  `MAX_BUCKETABLE_LAMPORTS` (= `i64::MAX`) is the one threshold; the SQL mirror
  interpolates that same constant.
- **`TokenFingerprint`** carries `Option<u64>` for both args. The stored
  `Fingerprint` axis stays `BIGINT`/`i64` — a saved axis can only ever name a value
  that fits, which is exactly why the matcher (`fingerprint::sol_axis_u64`) makes an
  out-of-`i64` token **fail** a configured axis. Fails closed: it can arm on
  nothing, never on everything.
- **Group key.** A value past `i64` is not binned. It renders its exact SOL digits
  (`grouping::exact_sol_label_u64`, integer arithmetic) in *both* precision modes —
  bucketing a ceiling into a 0.1-SOL bin is noise, and the exact digits keep
  distinct ceilings distinct. It stays separate from the `∅` missing key: "the dev
  set no slippage cap" and "the field is absent" are different facts, and folding
  them together loses a real behavioural signal.
- **SQL mirror** (`creation_stats_repo`). Split at the same threshold. Exact labels
  and exact comparisons use `numeric`; **bucket** arithmetic stays `float8` on
  purpose, because the engine bins in `f64` and the mirror has to reproduce that
  rounding rather than a more exact one.
  - The exact label multiplies by `0.000000001`, never divides by `1e9`. Postgres
    picks a quotient's scale from `select_div_scale` — 16 significant digits, i.e.
    only 8 decimals on an 11-digit result — so even `::numeric / 1e9` drops the low
    lamport digits. Numeric multiplication takes the operands' scale (`0 + 9 = 9`),
    so it is exact by construction.
- **Lake.** `fp_max_sol_cost` / `fp_spendable_sol_in` are Parquet `UInt64`. The
  tokens dimension is a single file rewritten wholesale each export, so this needed
  no schema migration — but **an existing `tokens.parquet` keeps the old `-1`s
  until the next `cargo run -p hunter-lab -- lake-export`.**
- **Wire.** Raw `u64` args serialize as JSON **strings**
  (`trading_core::serde_wire::u64_as_string`), applied to the whole family
  (`initial_supply_token`, `token_amount`, `max_cost_lamports`,
  `spendable_lamports_in`, `min_tokens_out`) — a half-applied encoding rule is worse
  than either shape. `cu_limit`/`cu_price` stay numbers (bounded far below 2^53).
  The frontend reads them through `lib/u64Wire` and shows a ceiling as `∞` with the
  exact amount on hover.

## Known limitation (would need a schema change)

A saved fingerprint **cannot express** "no slippage cap": the axis column is
`BIGINT`. Promoting the ceiling group therefore yields a rule that arms on nothing
(`parse_lo_lamports` clamps to `i64::MAX` deliberately — returning `None` would drop
the axis, and a dropped axis matches *every* token). Likewise the value-filter box
takes human-SOL `i64` amounts, so it cannot select the ceiling cohort. Expressing it
properly needs the axis to carry the state — e.g. a `max_cost_no_limit BOOLEAN`
companion column, or widening the axis to `NUMERIC` — plus the matcher, the SQL
mirror, and a parity guard. Do that only if the ceiling cohort turns out to trade
differently.

## Guards

- `grouping::u64_max_instruction_arg_survives_both_persisted_shapes`
- `grouping::exact_sol_label_is_lossless_across_the_whole_u64_domain`
- `grouping::exact_sol_label_matches_the_float_form_everywhere_the_float_form_was_correct`
  — pins that the integer rewrite moved **no** existing group key
- `grouping::the_no_limit_ceiling_groups_separately_from_missing`
- `fingerprint::a_no_limit_ceiling_never_satisfies_a_configured_axis`
- `creation_stats_repo::the_u64_axes_split_at_the_engine_threshold_and_stay_exact`
- `creation_stats_repo::the_scope_mirror_guards_the_same_range_as_the_matcher`
- `serde_wire::u64_max_serializes_as_exact_digits`
