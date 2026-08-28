# Trader flow lens — analysis-owned `ix_labels` pattern sets

## The gap

The chart stack splits trade flow into **vol / non-vol** from a fingerprint's
`metric_config.m_flow_ix.ix_patterns`: an exact ordered `ix_labels`
sequence is "volume", everything else is organic. That is the engine's own
classification, so the set lives on the row rules are bound to.

Trader Analysis studies a wallet, and the tokens it traded belong to no cohort.
With no fingerprint there are no patterns, so the overlay never draws and the
per-candle trades table has no Vol column — precisely on the page where the
question is *which structures surround this trader's entries and exits*.

## The shape

One classifier, **two owners** of the pattern set.

- **Fingerprint** — what the engine trades on. Unchanged.
- **`ix_pattern_sets`** (lab-only table, migration `lab/migrations/0002`) — a
  named, reusable set with no rule behind it. Rows:
  `{ id, name, wallet_address?, patterns, notes }`, patterns being
  `[{ group, ix_labels }]`.

`group` labels a subset (a launch client / aggregator name) so a lens narrows to
one of them without re-pasting. The classifier never sees it: a pattern's
identity is its ordered `ix_labels` array, `JSON.stringify`d — the same key
`lib/flow/classifyFlow.ts` and `hunter_engine::metrics::flow_ix` match on.

Crossing from study to engine is one explicit copy (**Copy to fingerprint**),
never a side effect of editing a lens.

## Classifier options a lens needs

`FlowClassifyOptions` gains three knobs; all default to the engine's behavior, so
every other surface is unchanged.

- **`contagion`** (lens default **off**). The engine tags a wallet forward: one
  structural match, and every later trade of that wallet counts as volume, with
  the creator seeding the set. That answers "who is in the volume crew". A lens
  asks "which STRUCTURES are around this moment", and on a busy token contagion
  turns that into one wallet set within seconds. Off, each trade is judged by its
  own labels alone and the creator carries no special rule.
- **`excludeWallets`** (lens default: the studied wallet). A trader must not
  classify itself, or the lines describe the subject instead of its surroundings.
- **`side`** (lens default **both**). A pattern is an ordered `ix_labels`
  sequence and those labels carry no direction — an aggregator's structure is
  byte-identical on the buy and on the sell that unwinds it — so one key matches
  both legs and an unnarrowed line sums two opposite events. The readings are
  different theses: a matched structure BUYING before a trade is a crowd impulse
  joined, the same structure SELLING is exit liquidity absorbed, and mixed they
  partially cancel. Narrowing filters TRADES, not patterns: no set edit, and it
  composes with the group chips, so Axiom-buy vs Axiom-sell falls out of the two
  together. An off-side trade books non-volume and never seeds contagion; a trade
  with no side is off-side under any narrowing.

The lines are already **net** (`buy − sell`) per basis — see
`lib/flow/flowChartData.ts`. Only the per-trade `volSol`/`nonVolSol` fields are
magnitudes.

## Wiring

Keys travel the existing prop path (`TokenTable` → `TokenChartsGrid` →
`TokenTradeChart` → `TokenPriceChart` / `BarTradesPanel`). The rest of the lens —
classifier options and the Vol-badge write target — travels through
`context/FlowLensContext`, provided once by the page: threading two more props
through five layers to serve one page is the worse trade. Absent, every chart
behaves exactly as before.

A Vol badge under a lens writes to `ix_pattern_sets`, never to a fingerprint.
Patterns are filed under the lens' active group, which is the single enabled
group when narrowed to one, else ungrouped.

## Page surface

`FlowLensBar` (Trader Analysis, above the analytics deck): set picker, paste box
(accepts `{ "patterns": [...] }`, a `[{ tool, ix_labels }]` list, bare label
arrays, or one JSON array per line — the `A > B` display form is rejected because
those shortened action names match no trade), group chips, the two classifier
switches, rename / delete / copy-JSON, and copy-to-fingerprint.

Group narrowing is view state (per set, in `localStorage`); the set itself is the
only thing persisted server-side.

## Open

- **Per-group lines.** The overlay draws one vol series and one non-vol series,
  so groups are compared by toggling chips rather than side by side. N series on
  the left scale is the next step if the comparison earns it.
- **Entry-aligned aggregate.** Per-token charts confirm a story; they cannot test
  one. Aligning every entry at `t = 0` and plotting mean net target-structure flow
  over `t−10s … t+30s` against a liquidity/age-matched control window is the panel
  that answers what influences the trader's entries. It waits on the lens being
  read on real tokens first.
