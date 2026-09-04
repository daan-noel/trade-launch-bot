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

| Kind | Stored as | Overlay match | Fees | Narrowing unit |
| --- | --- | --- | --- | --- |
| `exact` | `patterns`: `[{ group, ix_labels, cu_limit?, cu_price?, tip_lamports? }]` | `'labels'` (tagged) | yes, catch-all vs pin | `group` |
| `templates` | `working_templates`: grain-id strings | `'grain'` (working) | no | the grain itself |

`group` labels a subset (a launch client / aggregator name) so an exact lens
narrows to one of them without re-pasting. A templates set has no such label and
needs none: a grain IS the launch client, so each grain is its own narrowing
unit. The classifier never sees either: exact identity is labels **plus** pins
(`patternRowKey`); template identity is the grain id (`templateGrain`). Both feed
`classifyOptsForTape`, which reads the narrowed key set (`keysForSet`).

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

The badge reports the NARROWED key set, so a click has to flip that or it reads
as broken. Either vocabulary is off for one of two reasons and they take opposite
writes: absent from the set (the click adds it) or present but narrowed out (the
click brings that unit back). One write for both deletes a row the reader cannot
even see and leaves the badge unchanged.

- **Templates** — un-mutes the grain. An add also clears any leftover mute for
  that id, which would otherwise swallow it.
- **Exact** — un-mutes the whole GROUP the stored pattern sits in, since that is
  the unit there. `mutedPatternForClick` decides: nothing ENABLED may carry the
  shape (otherwise the badge is on and the click means remove), and the muted row
  must provably accept this click — unpinned, or its pins equal the pins the click
  carries. Unprovable ⇒ the ordinary write, never a guess.

The badge says which click it is: `storedPatternIds` (grains) and
`storedPatternRows` (exact) travel beside the narrowed keys, so a row that is off
only because its unit is muted reads *"on the set but MUTED by the lens chips"*
instead of *"click to save"*.

## Page surface

`FlowLensBar` (Trader Analysis, above the analytics deck): set picker (shows
kind), create-time Templates / Exact toggle (default Templates), the JSON box,
narrowing chips (group names on exact, one per grain on templates), the two
classifier switches, rename / delete / copy-JSON, and copy-to-fingerprint.

**Show JSON** opens the box on the stored set rather than empty, so the whole set
is readable on the page — a clipboard-only Copy left no way to see what a lens
holds. One serializer (`setJson`) feeds both the box and `Copy JSON`, and the text
round-trips through the parser, so viewing and editing are the same control: read
it, change a line, **Replace set**. What it shows is the stored set, never the
narrowing.

Exact paste accepts `{ "patterns": [...] }`, a `[{ tool, ix_labels, cu_limit? }]`
list, bare label arrays, or one JSON array per line — the `A > B` display form
is rejected because those shortened action names match no trade. Templates paste
accepts a JSON string array of grain ids or one id per line, and rejects
ix_labels payloads.

Narrowing is view state (per set, in `localStorage`); the set itself is the only
thing persisted server-side, so a chip click never edits it — on a templates set
the chip's separate `×` is what removes a grain. Kind is insert-only.

The two kinds store the narrowing with opposite polarity, and the reason is which
one survives an edit. A group exists only while some pattern carries its name, so
the pref holds the groups that are **on** (`groupsBySet`) and a retired name drops
out. A grain is an entry the user adds by hand, so the pref holds the grains that
are **muted** (`mutedBySet`) — a freshly pasted or badge-clicked grain then
classifies at once instead of landing outside the filter and reading as a failed
write. Muting every grain narrows to nothing and stands: the chips read off and
the bar's `0/N classifying` says so.

## Pre-entry trigger probe

The overlay reads one token at a time, so it can confirm a story and never test
one. The probe asks the same set a question across every token in the window:
**did a structure from this set land on the tape before the trader entered, and
how often does that happen anyway?**

### Anchor and window

The anchor is the wallet's first BUY in the window — `(wallet_entry_slot,
wallet_entry_tx_index)`, already on the row. A candidate print is a trade on the
same mint with

    slot in [entry_slot - W, entry_slot]   AND   (slot, tx_index) < (entry_slot, entry_tx_index)

so a print in the trader's own slot counts only when it is strictly earlier in
that block. `block_time` is second-precision and ties across a whole slot, so
`(slot, tx_index)` is the only key that can order two prints — the same reason
`CoTrader.entry_lag_slots` is a slot key.

`W` is in SLOTS, and the row carries the nearest match's lag: slots, plus the
`tx_index` delta when the lag is 0. That readout is load-bearing, not decoration.
A set whose matches are all at lag 0 describes co-arrival, not a trigger, and a
trigger inside our own reaction time is not reachable at p50 +1 slot — the
distinction the structural-precision-without-money results turn on.

### Match and threshold

Matching runs on the engine's own classifiers, never a second spelling:
`template_grain::grain` (and the program name) against a templates set,
`flow_ix::ix_hash` + `BuildPatterns::contains` with the set's fee pins against an
exact one. The probe reads the NARROWED key set — what the charts classify with —
so the chips move the filter and the overlay together.

`side` comes from the lens. The studied wallet is always excluded: his own entry
tx carries his own structure, and a set built from his tool matches himself on
every token.

A token matches on `hits >= min_hits` (default 1) and `sol >= min_sol` (default
0) over the matching prints in the window. Presence is the default, and the two
knobs are what separate a dust print from the event — the decision node is a gap
followed by N buys from one tool, which is a count and a size, not a boolean.

### The control window

`[entry_slot - 2W, entry_slot - W)` — the same shape, same thresholds, one window
earlier. Reported per row and summed in the bar:

    matched 41/120 · control 38/120 · median lag 3 slots

Presence before an entry is a conditional with no denominator: a structure a
crowd shares is on the tape before everything, and a filter alone can only ever
confirm. The control count is what makes a null result visible on the screen that
produced the claim.

### Three states, never two

`matched` / `no-match` / `unknown`. A row is `unknown`, not a no-match, when

- the window caught no buy leg (exit-only row) — there is no anchor;
- the mint has no tape in `trades` (rolling ~30d retention against a look-back
  that reaches 90d);
- an exact set pins fees over tape written before the fee columns exist
  (2026-08-30 17:48 UTC). A NULL fee is unknown, never a wildcard hit.

The count sentence says how many rows are unknown. Folding them into `no-match`
would let retention quietly shape the hit rate.

### Wiring

`POST /api/wallets/{wallet}/pre-entry-ix` — body: one anchor per row on screen
(`mint` + the entry's `(slot, tx_index, block_time)`), `W`, the two thresholds,
`side`, and the narrowed set (kind + patterns / grains). Response per mint:
state, hits, matched SOL, nearest lag (slots + tx), the matched unit, and the
control hits — plus `skipped` (anchors past the 4,000 ceiling) and the
`tape_floor` a truncated row ran into.

The anchors come from the page rather than a second rollup query, so a window or
threshold change re-probes without re-reading them. A probe failure leaves the
plain table standing, the way a co-trade failure already does.

`TradeRepo::prints_in_slot_windows` is the read: one `UNNEST`ed
`(mint, lo_slot, hi_slot)` triple per row, joined to `trades` — a nested loop of
one index range each on `idx_trades_mint_order`. The `block_time` bounds are the
SPAN of the chunk's windows, not per mint: slot already filters precisely, and a
constant range is what lets the planner exclude chunks before the loop starts.
300 mints a query, one query at a time — an unbounded concurrent scan is what
OOMs the 6 GB VM.

The tape floor is the oldest CHUNK's start (`timescaledb_information.chunks`),
not `MIN(block_time)`: milliseconds against seconds, and exact for this question
because retention drops whole chunks.

Legs collapse onto `(slot, tx_index)` — the transaction identity within a mint —
so a two-leg buy counts once, and `leg_index = 0` never enters it.

### Page surface

A `pre_entry` column group on the trader token table: Pre-entry (Before / Absent
/ Unknown), Lag, Hits, Hit SOL, Matched, Control.

**Show** narrows the table to one verdict — All / Before / Absent / Unknown. It
is a row filter over answers already in hand, so every click is instant and it
re-probes nothing, and it runs on the table's INPUT set, so the charts grid and
the counts below follow it. `All` is the default: turning the probe on adds
columns, never removes rows from a table someone is already reading.

The summary sentence is computed over every PROBED row, never over the visible
ones, so `matched 591/1748 · control 540/1748` keeps saying what the filter is
hiding. That is the whole reason narrowing is a view over the answer rather than
a narrower question: a filter whose own counts move with it can only ever show
confirmations.

The probe strip sits under the lens (`PreEntryProbeControls`): the toggle, `W`
(default 25 slots ≈ 10s), Min hits, Min SOL, and the summary sentence. Off by
default, so a study that never opens it costs exactly what it did before. The
set, its narrowing, its side and the excluded wallet all come from the lens —
this strip only carries the question asked with it.

## Open

- **Per-group lines.** The overlay draws one vol series and one non-vol series,
  so groups are compared by toggling chips rather than side by side. N series on
  the left scale is the next step if the comparison earns it.
- **Entry-aligned aggregate.** The probe answers presence per token; the panel
  answers shape. Aligning every entry at `t = 0` and plotting mean net
  target-structure flow over `t−10s … t+30s` is a fold over the same per-token
  lags the probe already returns, against the same control window.
