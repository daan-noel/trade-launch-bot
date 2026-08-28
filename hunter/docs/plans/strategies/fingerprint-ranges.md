# Fingerprint axes: an explicit registry over integer ranges

A fingerprint is a token-creation shape. It names **axes**, each carrying one
**predicate**. There is no bucket width, no match mode, and no float in identity.

## The vocabulary

```rust
enum AxisPredicate {
    /// Inclusive `[min, max]`. Either bound open. `min == max` IS exact match.
    Range { min: Option<u128>, max: Option<u128> },
    /// Two or more disjoint, ascending, non-touching spans — what `!=` and `|`
    /// produce. Canonical by construction (see below).
    Spans { spans: Vec<Span> },
    /// Exact ordered instruction-label sequence.
    Sequence { labels: Vec<String> },
}
```

* An axis absent from the map is **not part of identity**.
* Exact is the degenerate range, so there is no second spelling of the same
  intent and no mode flag two readers can disagree about.
* A numeric predicate is a **set of spans**. Over the integers a union or a
  complement of windows is just more windows, so `!=` and `|` need no new
  matching rule — only the second variant.
* Every numeric axis is a **non-negative integer** — lamports, compute units,
  tallies. `u128` in Rust, a **decimal string** on the wire (a JSON number is
  unsafe past 2^53 and `max_sol_cost = u64::MAX` is real data). SOL is a display
  unit, converted at the UI edge only.
* Integer lamports make `[lo, hi)` and `[lo, hi-1]` the same set, so an
  inclusive range is exactly as expressive as the half-open bucket it replaces —
  losslessly, with no boundary epsilon anywhere.

## One set, one stored spelling

`<=2 | >=4` and `!=3` select the same tokens, so they must be the same
fingerprint row — `criteria` IS identity, and two spellings of one set would key
as two rows for one gate. Every builder therefore routes through `SpanSet`, which
sorts, merges what overlaps or touches, and collapses a one-span set back to
`Range`. `Spans` with fewer than two spans, or with spans out of order or
touching, is **refused** at the write edge rather than normalised on read:
normalising quietly admits the second spelling that the canonical form exists to
prevent.

Adjacency counts as touching. The domain is integer, so `[0,2]` and `[3,5]` cover
exactly `[0,5]`.

One ambiguity survives and predates this: a bottom edge of zero can be spelled
open (`{max: 2}`) or closed (`{min: 0, max: 2}`), and both name the same set on a
non-negative axis. Neither is rewritten, because rewriting would change the
identity of rows that already carry one — an exact `0 … 0` gate included. `<=`,
`<` and every complement emit the OPEN form, which is what stored rows already
use, so the operator spellings agree with each other; only a hand-typed bare `0`
lower edge differs.

## The condition grammar

One text ⇄ predicate translation
([`fingerprint::grammar`](../../../engine/src/fingerprint/grammar.rs), mirrored by
`frontend/src/shared/lib/strategy/fingerprintGrammar.ts`), shared by the axis
form, the dashboard filter boxes and every chip pasted back into either.

```text
expr    := arm ( '|' arm )*          OR   — union of the arms
arm     := atom ( ',' atom )*        AND  — intersection of the atoms
atom    := op? operand
op      := '>=' | '<=' | '>' | '<' | '=' | '==' | '!='
operand := n | n '..' n | n '-' n | n '–' n
```

* **`..` is inclusive, `-` is half-open.** `1..2` is `[1, 2]`; `1-2` is `[1, 2)`,
  which is what a group chip spans, so a chip's own text pasted into a filter box
  selects exactly that chip's tokens. The parse is always echoed back inclusive,
  so which was typed is never hidden.
* **`>` and `<` are exact.** The domain is integer, so `>1.5◎` is
  `>= 1500000001` lamports — the same set, named in the storage vocabulary.
* **Amounts parse as decimal text, never a float.** `max_sol_cost = u64::MAX` is
  real launch data above 2^53.
* Strict: any malformed fragment fails the whole parse. A dropped fragment would
  read as "no constraint", which *widens* a match instead of failing the write.
* An expression that constrains nothing (`>=0`, `<=2 | >=3`) or nothing at all
  (`<=2, >=7`) is refused rather than stored — one reads as narrowed while
  matching every token, the other is a gate that can never fire.

The form is **one field per axis**, not a min/max pair: exact, band, open end,
gap and alternatives are all the same question, and a pair of boxes can only ask
two of the five.

## The registry is the extension point

[`hunter_engine::fingerprint::axis`](../../../engine/src/fingerprint/axis.rs) holds one <!-- ref-ok: this doc proposes the module; the path is the target, not a citation -->
`AxisDef` per axis: wire key, display label, kind, unit, match phase, the one-line
definition the UI renders, and the reader that pulls the observed value off a
`TokenFingerprint`.

Both numeric shapes are read through `AxisPredicate::spans`, so the matcher, the
SQL mirror, the auto-name and the group key each write ONE loop and gained
`!=`/`|` without an edit. The matcher routes by `AxisKind`, never by variant — a
shape the loop had not heard of would otherwise fall through to "matches nothing"
while the row still read as a numeric gate.

Everything derives from that table — the matcher loop, `has_any_criterion`,
`has_first_slot_criteria`, `auto_name` and its grammar, JSON parsing, validation,
the dashboard's SQL mirror, the sweep partition, and the form controls. **Adding an
axis is one `AxisDef` plus one reader arm.** Adding a predicate shape is one enum
variant.

Axes: `cu_limit`, `cu_price`, `init_buy_lamports`, `max_cost_lamports`,
`spendable_lamports_in`, `first_slot_buy_lamports`, `first_slot_sell_lamports`,
`ix_labels`, `ix_count`, `prior_launches`.

`ix_count` is derived (`ix_labels.len()`), so it needs no token-side field.
`prior_launches` is the engine's own creator tally, stamped onto the observed axes
in `reduce` at `TokenCreated` before the match runs — a stateful engine value, not
a `tokens` column.

## Storage

`fingerprints.criteria JSONB` — `{axis_key: predicate}` — plus the `wildcard`
flag. One column instead of two per axis, so a new axis needs no migration.

Identity is `criteria = $1::jsonb`; Postgres normalises `jsonb` key order, so the
comparison is canonical without a canonicalisation pass.

### Row identity is wider than match identity

Two rows are the same **row** when `criteria`, `wildcard` AND `metric_config`
agree. `metric_config` selects no token, so it is not *match* identity — but it
compiles into that row's live `m_flow_ix` patterns at reload, keyed by
`fingerprint_id`. Two rows selecting the same tokens with different patterns
classify flow differently, so they are different fingerprints and both must
exist: eleven `8dtx · <router>` carriers share `{}` + `wildcard` and differ only
here.

Leave it out and `find_or_create` returns an **arbitrary** one of the eleven
(`LIMIT 1`, no ordering) — promoting a wildcard group could bind the rule to the
`GMGN Bot` carrier and then overwrite that carrier's patterns with the sweep's,
silently reclassifying flow for every rule already bound to it.

A `UNIQUE` index on the same three makes duplicates impossible at the storage
layer rather than by convention. It indexes the two `jsonb` columns as `md5(…)`
digests: a btree row is capped at ~2704 bytes and the carriers' pattern sets
alone exceed it. Equal `jsonb` always yields equal `md5`, so the constraint is
exactly as strict, and a digest collision could only reject a write, never admit
a duplicate. Reads keep comparing the values themselves.

`fingerprint_repo`'s test module reads the migration and asserts the predicate
and the index name the same columns — the one no-DB guard on a fact stored twice,
because the two drifting apart fails either in a migration against live data or,
silently, as that arbitrary row.

## Grouping partitions by the same predicate

A sweep group's key carries `AxisPredicate`s, not rendered `"lo–hi"` labels, so
promoting a group to a fingerprint is a copy. `PartitionSpec::Distinct` gives one
group per value; `PartitionSpec::Ranges { edges }` bins by an explicit ascending
list — edge `i` opens `[edges[i], edges[i+1] - 1]`, open-ended below the first and
above the last, so the edges tile the whole domain and no token is dropped.
`PartitionSpec::quantiles` derives edges from a corpus by equal count.

No implicit lattice, no width to reconcile across axes, no byte-identical-label
lockstep with SQL.

## Match phases

`first_slot_*` are trade-derived and settle only after the creation slot closes,
so matching stays two-phase: `MatchPhase::Instant` judges the axes whose
`AxisDef::phase` is `Instant`, `MatchPhase::Full` judges every configured axis.
The phase lives on the axis definition — nothing else knows which axes defer.

## Validation

* At least one criterion (`wildcard` counts; an empty map matches **nothing**).
* A wildcard row carries no axis.
* Every predicate is satisfiable: `min <= max`, a non-empty span list, a
  non-empty label sequence.
* A `Spans` list is canonical: two or more spans, ascending, disjoint and not
  touching.
* `ix_count` and `ix_labels` must agree — a count range excluding
  `labels.len()` is an unsatisfiable row that would silently arm on nothing.

## What this removed

`SolPrecision`, `same_bucket`, `bucket_index`, `BUCKET_EPS`, `decimals_for`,
`bucket_sol_label`, `exact_sol_label`, `SolFilter`, `MAX_BUCKETABLE_LAMPORTS`,
`bucketable_lamports`, `fingerprints.bucket_size_amount`,
`grouped_sweep_runs.bucket_width_sol`, the `sol_bucket_sql` / `sol_exact_sql` /
`SQL_MASK_INT_DIGITS` mask machinery, the `exact_sol` wire flag, and the promote
blockers for a ceiling, an arbitrary range, and two axes wanting different widths.

Three promote blockers remain, and each is a real fact about the group rather than
a limit of the representation: an **absent** axis, a **multi-value** filter, and the
two **grouping-only** fields the matcher has no axis for.
