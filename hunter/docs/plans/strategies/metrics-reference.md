# Metrics reference — flow groups

Deep-dive for aggregate flow (`m_flow_lifetime` / `m_flow_window`) and the
volume/organic split (`m_flow_split` / `m_flow_split_window`). High-level map:
[`arch/strategies.md`](../../arch/strategies.md). The split's origin roadmap
(`roadmap/volume-flow-split-plan.md`) is deleted — fully shipped and superseded by
this file.

## Aggregate flow (`m_flow_lifetime` / `m_flow_window`)

Classifier-free SOL totals on the token. Same four JSON metric names; distinct
registry `MetricId`s so lifetime can be monotonic while the window is not.

| group | kind | strict params | state |
| --- | --- | --- | --- |
| `m_flow_lifetime` | static | none | two running counters on `TokenTrack` |
| `m_flow_window` | dynamic | `window_size_sec` | ring buffer deduped by window size |

| metric | meaning | unit | eq-tol | monotonic (lifetime only) |
| --- | --- | --- | --- | --- |
| `buy` | buy SOL | SOL | 0.1 | ✓ |
| `sell` | sell SOL | SOL | 0.1 | ✓ |
| `net_flow` | `buy − sell` | SOL | 0.1 | ✗ |
| `gross_flow` | `buy + sell` | SOL | 0.1 | ✓ |

Non-finite / negative SOL is ignored. Windowed variants are never monotonic.
Lifetime is the maturity / critical-mass gate; window is the hot-right-now filter.
No fingerprint config — unlike the split groups below.

## Classifier (per trade × fingerprint)

A trade is **volume-side** iff any of:

1. its ordered `ix_labels` hash ∈ the fingerprint's configured `volume_ix_patterns`
   (exact ordered sequence — same semantics as fingerprint `ix_labels`);
2. its wallet was previously tagged volume-side on **this token** (wallet contagion);
3. it is the **creator wallet** (unconditionally volume-side).

Otherwise **organic**. Contagion is per-token only (cross-token is a future toggle).

Config lives on the fingerprint (not the rule):

```json
{
  "m_flow_split": {
    "volume_ix_patterns": [
      ["Pump.Fun: Create", "Pump.Fun: Buy"],
      ["Pump.Fun: Buy", "Token Program: CloseAccount"]
    ]
  }
}
```

`m_flow_split_window` reads the **same** `m_flow_split` key (one classifier, two views).
Unconfigured fingerprint (no `m_flow_split` key) ⇒ every flow metric is **NaN**
(satisfies nothing). `ix_hash = None` (pre-0002 / missing lake labels) ⇒ organic
unless wallet-tagged/creator.

Flow state is **fingerprint-scoped** on `TokenTrack` (`BTreeMap<FingerprintId, FlowState>`),
not token-scoped — two fingerprints with different pattern sets diverge.

### The flow context is patterns **AND** creator — every consumer seeds both

`ensure_flow` alone is not a complete flow context. The creator seed is rule 3 of the
classifier *and* the contagion set's origin, so a fold that skips `seed_creator` books
the dev buy + dev dump — a token's two largest single flows — as **organic**, and its
`vol_*`/`nonvol_*` are a different classification from the one the engine decides on.
Both calls, on every path that folds flow:

| consumer | seeds at |
| --- | --- |
| live engine | `reduce.rs` — `TokenCreated { creator_wallet_hash }` → `track.seed_creator` |
| simulate | `engine_sim.rs` — `ReplayToken.creator_wallet_hash` (hashed from `tokens.creator_wallet`) |
| metric-series (`/metric-series`) | `metric_series.rs` — `resolve_flow_ctx` loads the creator; `build_series` seeds after `ensure_flow` |
| chart overlay (browser preview) | `classifyFlow.ts` — `FlowClassifyOptions.creatorWallet`, passed by `TokenTradeChart` |

`/metric-series` shipped without the seed and drew a `nonvol_net` that disagreed with
both the chart overlay and the live engine; locked by
`the_creator_wallet_is_volume_side_even_without_a_pattern_match`. Order is free —
`ensure_flow` copies an already-set creator, `seed_creator` back-fills existing states —
but one of the two alone is a silent misclassification, never an error.

**The browser overlay is a preview, not the metric.** `classifyFlow.ts` mirrors the Rust
classifier but folds a *different corpus* (PG-only `/api/tokens/:mint/trades`, vs the
sealed lake + PG tail the endpoint reads) and renders in the chart's display unit and
flow basis. Compare it to `m_flow_split.nonvol_net` only in SOL on the `cost_sol` basis,
and expect drift wherever the two corpora differ (PG retention has dropped a token's
early trades; pre-V0 lake days null-fill `ix_labels`/`wallet`).

## Hash SSOT

`hunter_engine::metrics::flow_split::{ix_hash, wallet_hash, ix_hash_opt}` are the
**only** hashers. Every adapter (live producer, lake replay, event-log) calls them;
patterns compile to a hash set at `RulesReloaded`. No interner ⇒ replay parity by
construction. See hunter/CLAUDE.md Gotchas.

## Metric groups

| group | kind | strict params | fingerprint config |
| --- | --- | --- | --- |
| `m_flow_split` | static (fingerprint-scoped) | none | `volume_ix_patterns: string[][]` (required when key present) |
| `m_flow_split_window` | dynamic | `window_size_sec` | none (reads `m_flow_split`) |

**Multi-window per group** (any dynamic group — `m_flow_window`, `m_price_window`,
`m_flow_split_window`): a group appears under a side as a single object (one window — the
legacy shape) OR a JSON **array** of objects, each with its own `window_size_sec`, to
gate the same group at several window sizes at once (e.g. a 30s `gross_flow` hot gate AND
a 2s `net_flow` exhaustion gate on entry). Each window is an independent clause (entry-AND
/ exit-OR); windows must be distinct; static groups take a single object. The single-object
form round-trips byte-identically (no DB migration). SSOT for the shape + validation:
`hunter_engine::rule_params` module docs.

Both flow groups expose the same nine JSON metric names; registry `MetricId`s are distinct so
lifetime monotonic flags can differ. All SOL values use absolute trade notional;
buy = +, sell = − for `*_net`.

| metric | meaning | unit | eq-tol | monotonic (lifetime only) |
| --- | --- | --- | --- | --- |
| `vol_buy` | volume-side buy SOL | SOL | 0.1 | ✓ |
| `vol_sell` | volume-side sell SOL | SOL | 0.1 | ✓ |
| `vol_net` | `vol_buy − vol_sell` | SOL | 0.1 | ✗ |
| `vol_gross` | `vol_buy + vol_sell` | SOL | 0.1 | ✓ |
| `nonvol_buy` | organic buy SOL | SOL | 0.1 | ✓ |
| `nonvol_sell` | organic sell SOL | SOL | 0.1 | ✓ |
| `nonvol_net` | `nonvol_buy − nonvol_sell` | SOL | 0.1 | ✗ |
| `nonvol_gross` | `nonvol_buy + nonvol_sell` | SOL | 0.1 | ✓ |
| `vol_share` | `vol_gross / (vol_gross + nonvol_gross)` ×100; NaN when total 0 | % | 1.0 | ✗ |

Windowed variants are never monotonic. Lifetime monotonic ✓ metrics participate in
derived-unsatisfiability disarm (`arm.rs` reads the registry flag).

## NaN rules

| situation | flow metrics |
| --- | --- |
| Fingerprint has no `m_flow_split` key | all NaN |
| Pre-first-trade (no classifier state yet) | NaN (existing convention) |
| Trade `ix_hash = None`, wallet not tagged, not creator | counts as organic |
| Token row missing / no `creator_wallet` | creator unseeded (logged `warn`); creator trades classify by pattern/contagion only |
| Pre-V0 sealed lake days (NULL `ix_labels`) | organic in runtime; **excluded** from discovery score denominators |

Rule save **warns** (does not reject) when params reference flow groups but the
fingerprint is unconfigured.

## Discovery scoring (lab authoring aid)

`lab/src/strategies/flow_discovery.rs` + `POST /api/strategies/flow-discovery`.
Partitions the `with_flow` corpus by sweep `GroupKey`, scores each distinct trade
ix-structure:

| signal | formula (summary) |
| --- | --- |
| `volume_share` | structure gross / group gross ×100 |
| `wash_symmetry` | mean `|net|/gross` over tokens (→0 = wash) |
| `cross_token_recurrence` | % of group tokens with gross ≥ 0.05 SOL |
| `group_lift` | share(S\|G) / share(S\|window) — **only meaningful when `lift_defined`** |
| `slot_burst` | % of trades in ±1-slot same-structure clusters |
| `wallet_reuse` | `1 − distinct_wallets/trades` |
| `wallet_overlap` | mean pairwise Jaccard of per-token wallet sets — one crew across launches |
| `first_slot_gross_sol` | structure gross landing in the token's **creation slot** (+ `first_slot_trades`) |

Ambiguity chip when top structure's `group_lift < 1.25` **and** `lift_defined`.
Apply writes `metric_config` via fingerprint `PUT` or promote-style bind.
Auto-promote stays future work (gated on hand-label kit).

### `lift_defined` — lift needs something to be measured against

`group_lift`'s denominator is the structure's share of the **whole scored
corpus**. When the group *is* that corpus, the ratio is the group's own share over
itself and every structure scores exactly `1.0`. That happens on the page's main
workflow: a fingerprint-scoped run loads only matched tokens and groups by
nothing, so it is one `ALL` group over everything. A corpus with zero scored
volume is degenerate the same way (ratio ≡ 0).

`DiscoveryGroup.lift_defined` reports this, and **readers must skip the lift gate
when it is false, never fail it** — failing a `lift >= 1.25` gate against a
constant `1.0` rejects every row of the run. That is exactly what silenced the
UI's `Auto` verdict on every scoped run (each row rendered `—`, the bulk-select
sat disabled) and made the "split may be noisy" chip fire unconditionally. The UI
also renders the Lift column itself as `—` there: printing `1.00` reads as a
verdict when it means *not measured*. `#[serde(default)] = true` for pre-field
cached results; those stay gated until re-run. Locked by
`whole_corpus_group_reports_lift_undefined`.

### The `Auto` composite (client-side, `flowDiscoverySuggest.ts`)

Not a backend fact — a client composite over the columns above, so the human
doesn't eyeball every row. Four properties it deliberately holds, each fixing a
way the first version misled:

- **The number IS the decision.** `score >= SUGGEST_SCORE` (0.5) is the whole
  badge rule. It used to show a mean while badging on a count of strong signals,
  so a 49% row could sit un-badged beside a badged 33% one.
- **Correlated columns count once.** Score is a mean over *families* — `Recur`,
  `Burst`, `Wallets` (= max of `wallet_reuse`/`wallet_overlap`), `Wash`
  (both-sided rows only; n/a is dropped from the mean, not scored 0). One launch
  bundle trips same-slot bursts *and* few-wallets off the same fact, and under the
  old "≥2 strong signals" vote that alone was a pass. Averaging families also
  encodes "needs ~two kinds of evidence" in the single number: one family at 1.0
  with the rest cold lands at 0.25–0.33.
- **The verdict never moves as you click.** Contagion% is **not** an input: it is
  defined against the current draft, so feeding it in made a row score differently
  depending on click order and made the bulk-select non-idempotent (pressing twice
  took more rows than once). It stays a read-only column.
- **Small samples don't vote.** `wallet_reuse` is `1 − distinct/trades`, so 2
  trades from 1 wallet reads 0.5 — "strong" off a coin flip. Below
  `SUGGEST_MIN_REUSE_TRADES` (4) it is dropped as unavailable.

Gates, all reported by name in the cell tooltip: dust floor, `SUGGEST_MIN_TOKENS`
(≥ 2 tokens carry the shape meaningfully — a pattern is written onto the whole
fingerprint, so a one-token curiosity is out of scope), and the lift gate *when
`lift_defined`*. `suggestExplain` renders every family with pass/fail, including
the ones that fell short, so a near-miss explains itself; hovering a bulk-select
outlines the rows it acts on (*Auto-select suggested* outlines the rows it would
check, *Select launch shapes* its full set — see below).

### First-slot (launch) presence — the second auto-select

Two different reads of the same pair of fields, deliberately not the same test:

- **`first_slot_gross_sol / gross_sol` = the `Launch%` column** — purity, i.e. how
  much of the shape landed at launch. Sort/filter/inspect only.
- **`first_slot_trades > 0` = the *Select launch shapes* predicate**
  (`isFirstSlotPresent`, plus the `SUGGEST_MIN_GROSS` dust floor) — *presence*.
  The launch bundle is the set of shapes that appear in the creation slot, and a
  bundler shape that also trades later is still bundler tooling, so the button
  takes a shape at any Launch% above 0.

The creation instruction needs no clause of its own: a shape carrying it is in the
creation slot by construction, so the presence test already takes it. Both reads
answer a different question from the `Auto` composite — *when* the shape trades,
not how bot-like it scores — so the two buttons are independent and neither gates
the other. No lift gate on this one: creation-slot presence is an identity claim,
while lift measures group-vs-window concentration and would drop a bundler shape
that happens to be ambient across the whole window.

The cost of presence-over-purity is real and is what `Launch%` is *for*: **live
classifies volume by `ix_hash` alone — there is no slot predicate**
(`flow_split.rs`). A checked shape that carries launch *and* organic flow tags the
organic tail too, and wallet contagion then sweeps those wallets' other trades in
as well. So the button is a bulk *proposal* — read `Launch%` on the rows it
checked and uncheck the mixed ones before Apply.

`first_slot_trades == null` is **unknown**, not 0: a pre-field cached run selects
nothing rather than guessing (re-run discovery to fill it).

**The button gates on the corpus, not on the draft.** `disabled` reads
`firstSlotAll.length === 0` — "this group has no launch shapes at all" — while the
click adds only `firstSlotUnchecked`, the ones not already staged. They must not be
the same test: the draft is re-seeded from the target fingerprint's *saved*
`volume_ix_patterns` on every run (`seedFromFingerprint`, keyed on `result.run_id`),
so once a launch set has been applied, re-running over a new time window re-stages it
and the diff is empty even though the new corpus is full of launch shapes. Gating on
the diff collapsed three distinct facts — *no launch shapes here*, *already saved*,
and *presence unscored* — into one dead button, and a `disabled` button fires no
mouse events, so it also killed the hover outline that could have told them apart.
Clicking with an empty diff is a deliberate no-op; the hover preview passes the FULL
launch set so the outline still answers "which rows do you mean?". The badge reports
`N at launch · all staged` vs `· M new`, and an all-`null` group is badged
`launch presence unscored` rather than silently reading as "no launch bundle".

The creation slot is the offline stand-in
`lab::sweep::projection::creation_slot` — the slot of the token's first trade,
**one fn** shared with replay's `FirstSlotSettled` derivation, because the lake
`tokens` dimension carries only the derived `fp_first_slot_*` sums, not
`tokens.creation_slot` itself. Known divergence: a token whose creation slot saw
no trade at all reports its first *later* slot instead.

Both fields are `Option` on the wire (`#[serde(default)]`): a result cached before
they existed must read back `—` ("unknown"), never an authoritative `0%` the
button would then rank on. Same contract as the identity fields below.

### The result carries its own corpus identity

`DiscoveryResult` echoes `bucket_width_sol` (`null` = `SolPrecision::Exact`),
`ix_labels_filter` and `fingerprint_id` alongside `groups`, and both GET endpoints
serialize them. **The page must rebuild fingerprint identity from these, never from
its own form state**, for two reasons:

- The result is disk-cached and rehydrated on mount, so it is routinely a run from
  an earlier session while the form holds something else entirely. Reading the
  form attributes a card to the wrong fingerprint, and binds one that arms on a
  window the card never showed.
- `bind_flow_discovery` builds the fingerprint from the **posted `group_key`
  alone** — unlike the sweep's `promote_group` it has no run row to recover the
  label filter from. So the client must re-attach it (`withIxLabelsFilter`) before
  posting, or the bound fingerprint silently drops its `ix_labels` axis and fires
  on every token shape. Same failure `promote_group`'s filter copy exists to
  prevent.

Precision is part of identity: an exact fingerprint and a bucketed one with equal
axes are different rules that arm on different token sets, so `withIxLabelsFilter`
and the width both feed `findFingerprintForGroupKey`, and an exact-mode auto-name
ends in `bexact` rather than a width.

Discovery has no `GroupSelection` resolver — that seam is grouped-sweep-only
(`lab/src/sweep/selection.rs`), because a discovery run has no persisted run row to
resolve against. The echoed fields are its equivalent.

## Future toggles (not built)

- **Cross-token contagion**: wallets tagged on token A pre-tagged on token B of the
  same fingerprint. Needs a bounded shared set inside `EngineState` keyed by
  fingerprint (size-capped, log-replayable). Powerful; risky (one false tag poisons a
  whole group) — build only after v1 data shows rotation defeats per-token contagion.
- **Baselines / since-entry variants**: anchor metrics to lifecycle moments (creator
  first sell, entry fill). New metrics inside `flow_split.rs`, no structural change.
- **Transfer ingestion**: direct wallet-linking via SOL/token transfers — a separate,
  expensive ingest feature; only if the proxy demonstrably fails.
- **Discovery auto-promote**: above a score threshold (likely `group_lift` +
  `cross_token_recurrence` gates), write `volume_ix_patterns` without a toggle pass.
  **Blocked on V4.4 hand-label kit.** Even then, default remains review-then-apply;
  auto-promote is an opt-in mode on the discovery page, never a silent background job.
