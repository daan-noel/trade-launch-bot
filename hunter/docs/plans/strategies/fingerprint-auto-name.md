# Fingerprint auto-name

`fingerprints.name` is a **label**, not identity. Match identity lives on the axes
(`FingerprintRepo::find_or_create` ignores `name`). The auto-name is the compact
handle every create path writes so a picker, hover, or log can tell two rows
apart without opening chips.

## Generator

One function, two languages — `Fingerprint::auto_name` (Rust SSOT) and
`fingerprintAutoName` (TS mirror). Golden strings in both test files stay
byte-equal.

Order, unset axes skipped:

1. `Nix:Tail` from `ix_labels` (`ixLabelsCountTail` / the Rust twin) — **first**,
   because the trailing action is the discriminator a name has no color ribbon for
2. `cu_limit` / `cu_price` (chip `formatCompact`, e.g. `80K`)
3. `init` / `max` / `spend` / `fs_buy` / `fs_sell` (SOL, 4 dp trimmed, no ◎)
4. `bkt=exact` when width is NULL; `bkt={width}` when width ≠ default `0.1`;
   omitted at the default — and omitted entirely when **no SOL axis** is configured
   (see [Inert width](#the-width-is-an-axis-only-where-a-sol-axis-spends-it)). The
   width renders at `decimals_for(width)`, never a fixed count: legal widths reach
   down to `MIN_BUCKET_WIDTH_SOL` (1e-6), and a fixed 4 rendered `1e-5` as `bkt=0`,
   the one width `validate` rejects.

Tokens match the axis chips (`max`, `fs_buy`, `bkt`), not a second abbreviation
set. Empty → `ALL`. Example: `3ix:Buy · max=1 · bkt=1`.

### A numeric chip names the whole set the axis accepts

Both numeric predicate shapes render through `AxisPredicate::spans`, so one span
list is one chip body:

| Predicate | Chip body |
| --- | --- |
| `1.5 … 1.5` | `1.5` |
| `1.5 … 2` | `1.5~2` |
| open above / below | `1.5~` / `~2` |
| the complement of ONE window | `!3`, `!3~5` |
| anything else multi-span | `1~2\|7~8` |

A gap set is named for the hole it excludes rather than the two half-lines around
it, because `ix_count=!3` says what the operator asked for and `ix_count=~2|4~`
does not. It is still a pure function of the span list, so one token set has
exactly one name.

`is_auto_name_chip` strips a leading `!` and splits the body on `|`; each part is
a span the single-window grammar already describes. So the recogniser gained the
two new shapes as one more split, not a second grammar — and a name carrying one
still heals when the axes drift.

A `wildcard` row short-circuits the whole generator to `ALL`: it carries no axis
(the `fingerprints_wildcard_excludes_axes` CHECK) and never reads its bucket
width, so `bkt=exact` must not leak into the name of the one row that matches
everything. `ALL` therefore names two drafts — the wildcard, and the criterion-less
one the write edge rejects — and only the first is a usable name. The form's
`autoNameIsReal` is the one reader that tells them apart.

No provenance prefix (`c` / `f` / `s`) and no sweep run-id. Source is not part
of the matcher.

## The width is an axis only where a SOL axis spends it

`bucket_size_amount` reaches a match through exactly one road: the five
bucket-matched SOL axes. With none configured every `sol_axis` call short-circuits
on the `None` fingerprint value, so the width changes nothing.

`Fingerprint::effective_bucket_size_amount` is the one reader of that fact — the
width, or `None` when no SOL axis exists. **Every write edge stores it**
(`from_json` at the HTTP boundary, `FingerprintRepo` insert / update /
`find_or_create` for non-HTTP writers), `auto_name` names it, and the
`fingerprints_bucket_width_needs_a_sol_axis` CHECK (`0006`) is the backstop.

Left uncanonicalised the inert width forked the same fingerprint two ways at once:
`IDENTITY_WHERE` keys on the column, so `find_or_create` minted a fresh row rather
than reusing one the engine matches identically; and `auto_name` printed it, so one
match carried several names. This is the same collapse `configured_labels` makes
for `Some([])` and the wildcard arm makes for its own inert width — one spelling
per state, or two readers disagree.

`0006` canonicalised the stored rows and merged what they had duplicated.

## A stored name has to be able to change

`auto_name` is a pure function of the axes, but its output is **stored**, so every
edit to it strands the copies already written — and two rows with identical axes
then read as two fingerprints, the exact confusion the name exists to prevent.

`is_generated_auto_name` decides "generated, never typed" by **grammar**: every
` · `-separated part must be a chip `auto_name` emits. `has_stale_auto_name` is
then `is_legacy_auto_name || (generated && ≠ auto_name())`, and `list` / `find`
persist the re-derivation. A change to `auto_name` heals itself on the next read;
no new retired-prefix entry is needed.

Deliberately strict — an unrecognised part makes the whole name a nickname. The two
mistakes do not cost the same: re-deriving a name it declined to touch is free,
while rewriting a nickname destroys the only record of why that fingerprint was
created (`probe group mc0.0108 (held +17.13pc 9of9)` cannot be recovered from the
axes). A chip added to `auto_name` is added to `is_auto_name_chip` in the same edit.

## Who writes it

| Path | How |
| --- | --- |
| Sweep promote | `materialize` then `ensure_auto_name` then `find_or_create` |
| Creation-stats create / flow-discovery bind | `fingerprintNameFromGroupKey` (group-key → identity → auto-name) |
| Fingerprint form | glued to the auto-name while the field is blank, still the previous auto-name, or any stale auto-label; **Reset** restores it; a typed nickname sticks |
| Insert / update | a blank or stale auto-label is replaced at the repo boundary |

`find_or_create` keeps an existing nickname. `list` / `find` rewrite stale
auto-labels in place — the retired shapes (`sweep {8-hex} · group N`, `c · …` /
`f · …` / `s · …`, `flow-discovery bind`, blank) and any current-grammar name
that has drifted from the axes — so leftover rows pick up the new label on first
read. A nickname is never overwritten.

## Form / picker

The form suggests the auto-name as axes change and does not clobber a nickname.
Pickers (`FingerprintPicker`, `FingerprintScopeControl`) search `fingerprintParamsSearchText`
(axes, not just the name) and render the chip row in the dropdown.

Where the stored name is **not** the auto-name — i.e. it is a nickname — the
fingerprints table and the picker dropdown show the auto-name beside it in dim
mono. A nickname says why a fingerprint exists and the auto-name says what it
matches; without both on the row, several nicknames over one match look like
several fingerprints. The `bkt=` chip follows the name: it is dropped from the chip
row, the search text, and `fingerprintIdentityKey` when no SOL axis spends it, so
two rows that match identically also sort together.
