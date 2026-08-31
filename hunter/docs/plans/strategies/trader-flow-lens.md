# Trader flow lens — analysis-owned pattern sets

## The gap

The chart stack splits trade flow into **vol / non-vol** from a fingerprint's
lists (`m_flow_ix.ix_patterns` exact sequences, or `m_burst_slot.working_templates`
grain ids). That is the engine's own classification, so the set lives on the row
rules are bound to.

Trader Analysis studies a wallet, and the tokens it traded belong to no cohort.
With no fingerprint there are no patterns, so the overlay never draws and the
per-candle trades table has no Tagged / Working column — precisely on the page
where the question is *which structures surround this trader's entries and exits*.

## The shape

One classifier, **two owners** of the pattern set.

- **Fingerprint** — what the engine trades on. Unchanged.
- **`ix_pattern_sets`** (lab-only table, migrations `0002` + `0005`) — a named,
  reusable set with no rule behind it. A set is **one vocabulary**, chosen at
  create (`kind`, insert-only). The set picker is the switch.

| Kind | Stored as | Overlay match | Fees | Groups |
| --- | --- | --- | --- | --- |
| `exact` | `patterns`: `[{ group, ix_labels, cu_limit?, cu_price?, tip_lamports? }]` | `'labels'` (tagged) | yes, catch-all vs pin | yes |
| `templates` | `working_templates`: grain-id strings | `'grain'` (working) | no | no |

`group` labels a subset (a launch client / aggregator name) so an exact lens
narrows to one of them without re-pasting. The classifier never sees it: exact
identity is labels **plus** pins (`patternRowKey`); template identity is the
grain id (`templateGrain`). Both feed `classifyOptsForTape`.

A set with no kind stored is exact. An unpinned exact row is `{ group, ix_labels }`.

Crossing from study to engine is one explicit copy (**Copy to fingerprint**),
never a side effect of editing a lens. Exact copies into `m_flow_ix.ix_patterns`
(fees kept). Templates copy into `m_burst_slot.working_templates`.

## Classifier options a lens needs

`FlowClassifyOptions` gains three knobs; all default to the engine's behavior, so
every other surface is unchanged.

- **`contagion`** (lens default **off**). The engine tags a wallet forward: one
  structural match, and every later trade of that wallet counts as volume, with
  the creator seeding the set. That answers "who is in the volume crew". A lens
  asks "which STRUCTURES are around this moment", and on a busy token contagion
  turns that into one wallet set within seconds. Off, each trade is judged by its
  own labels or grain alone and the creator carries no special rule.
- **`excludeWallets`** (lens default: the studied wallet). A trader must not
  classify itself, or the lines describe the subject instead of its surroundings.
- **`side`** (lens default **both**). A pattern is an ordered `ix_labels`
  sequence (or a grain) and those identities carry no direction — an aggregator's
  structure is byte-identical on the buy and on the sell that unwinds it — so one
  key matches both legs and an unnarrowed line sums two opposite events. The
  readings are different theses: a matched structure BUYING before a trade is a
  crowd impulse joined, the same structure SELLING is exit liquidity absorbed,
  and mixed they partially cancel. Narrowing filters TRADES, not patterns: no set
  edit, and it composes with the group chips, so Axiom-buy vs Axiom-sell falls
  out of the two together. An off-side trade books non-volume and never seeds
  contagion; a trade with no side is off-side under any narrowing.

The lines are already **net** (`buy − sell`) per basis — see
`lib/flow/flowChartData.ts`. Only the per-trade `volSol`/`nonVolSol` fields are
magnitudes.

## Wiring

Keys travel the existing prop path (`TokenTable` → `TokenChartsGrid` →
`TokenTradeChart` → `TokenPriceChart` / `BarTradesPanel`). The rest of the lens —
classifier options, fee pins, kind, and the badge write target — travels through
`context/FlowLensContext`, provided once by the page: threading two more props
through five layers to serve one page is the worse trade. Absent, every chart
behaves exactly as before.

A badge under a lens writes to `ix_pattern_sets`, never to a fingerprint.
Exact clicks file under the lens' active group (the single enabled group when
narrowed to one, else ungrouped) and copy the strip's fee-pin mask off the tx.
Template clicks toggle `templateGrain(ix_labels)` and skip launch grains.

## Page surface

`FlowLensBar` (Trader Analysis, above the analytics deck): set picker (shows
kind), create-time Templates / Exact toggle (default Templates), paste box,
group chips (exact only), grain chips (templates), the two classifier
switches, rename / delete / copy-JSON, and copy-to-fingerprint.

Exact paste accepts `{ "patterns": [...] }`, a `[{ tool, ix_labels, cu_limit? }]`
list, bare label arrays, or one JSON array per line — the `A > B` display form
is rejected because those shortened action names match no trade. Templates paste
accepts a JSON string array of grain ids or one id per line, and rejects
ix_labels payloads.

Group narrowing is view state (per set, in `localStorage`); the set itself is the
only thing persisted server-side. Kind is insert-only.

## Open

- **Per-group lines.** The overlay draws one vol series and one non-vol series,
  so groups are compared by toggling chips rather than side by side. N series on
  the left scale is the next step if the comparison earns it.
- **Entry-aligned aggregate.** Per-token charts confirm a story; they cannot test
  one. Aligning every entry at `t = 0` and plotting mean net target-structure flow
  over `t−10s … t+30s` against a liquidity/age-matched control window is the panel
  that answers what influences the trader's entries. It waits on the lens being
  read on real tokens first.
