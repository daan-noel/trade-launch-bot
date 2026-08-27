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
   omitted at the default

Tokens match the axis chips (`max`, `fs_buy`, `bkt`), not a second abbreviation
set. Empty → `ALL`. Example: `3ix:Buy · max=1 · bkt=1`.

A `wildcard` row short-circuits the whole generator to `ALL`: it carries no axis
(the `fingerprints_wildcard_excludes_axes` CHECK) and never reads its bucket
width, so `bkt=exact` must not leak into the name of the one row that matches
everything. `ALL` therefore names two drafts — the wildcard, and the criterion-less
one the write edge rejects — and only the first is a usable name. The form's
`autoNameIsReal` is the one reader that tells them apart.

No provenance prefix (`c` / `f` / `s`) and no sweep run-id. Source is not part
of the matcher.

## Who writes it

| Path | How |
| --- | --- |
| Sweep promote | `materialize` then `ensure_auto_name` then `find_or_create` |
| Creation-stats create / flow-discovery bind | `fingerprintNameFromGroupKey` (group-key → identity → auto-name) |
| Fingerprint form | glued to the auto-name while the field is blank, still the previous auto-name, or a retired generator shape; **Reset** restores it; a typed nickname sticks |
| Insert / update | blank or retired shape is replaced at the repo boundary |

`find_or_create` keeps an existing nickname. `list` / `find` rewrite retired
shapes in place (`sweep {8-hex} · group N`, `c · …` / `f · …` / `s · …`,
`flow-discovery bind`, blank) so leftover rows pick up the new label on first
read. A nickname is never overwritten.

## Form / picker

The form suggests the auto-name as axes change and does not clobber a nickname.
Pickers (`FingerprintPicker`, `FingerprintScopeControl`) search `fingerprintParamsSearchText`
(axes, not just the name) and render the chip row in the dropdown.
