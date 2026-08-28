# Metrics reference — flow groups

Deep-dive for aggregate flow (`m_flow_lifetime` / `m_flow_window`), the crowd counts
(`m_crowd_window`) and the instruction-structure split (`m_flow_ix` /
`m_flow_ix_window`) — the wallet-keyed groups.
High-level map: [`arch/strategies.md`](../../arch/strategies.md). The split's origin roadmap
(`roadmap/volume-flow-split-plan.md`) is deleted — fully shipped and superseded by
this file.

## A window is a span: size, lag, and the unit both are counted in

Every dynamic group's window is a `WindowSpec { size, lag, unit }`. **There are no
parallel per-basis metrics** - the unit lives on the window, so `m_flow_window`,
`m_crowd_window`, `m_price_window` and `m_flow_ix_window` read every basis for free.
Internally each buffer entry carries a `pos` already in its own unit (milliseconds for
`sec`, the slot number for `slot`, the token's print ordinal for `print`), so the fold,
the eviction and the read are ONE implementation over an `i64` cursor. `WindowUnit::ALL`
is the single place the bases are enumerated; a `WindowAxis` names one size param per
unit, and every resolve, validate and label site goes through it rather than branching
per pair.

| param | meaning |
| --- | --- |
| `window_size_sec` | size in seconds (a closed interval, continuous) |
| `window_size_slots` | size in slots (exactly `size` discrete slots) |
| `window_size_prints` | size in PRINTS of this token's tape (exactly `size` trades) |
| `window_lag` | how many units back from now the window ENDS; default `0` |

**Exactly one size param per group instance** - two is two spans claiming one axis,
none leaves the window undefined. `validate_group` enforces it, because "one of these"
is a cross-param rule a `StrictParamSpec` cannot spell. The two-window metrics
(`m_flow_window.trade_share` / `.sol_share`) take a nested `slice_size_sec` — with
`slice_size_slots` / `slice_size_prints` as its twins — and both axes must use the
same unit: a slice in slots over a reference in seconds is a ratio across two
different clocks.

```json
{ "m_flow_window": [
    { "window_size_sec":    60, "gross_flow": [{"operator": ">=", "value": 45}] },
    { "window_size_slots":  30, "window_lag": 1, "buy": [{"operator": "<=", "value": 3}] },
    { "window_size_prints":  1, "gross_flow": [{"operator": ">=", "value": 10}] }
] }
```

**Why slots, not seconds.** A slot is what the chain batches in, so a bundle is a slot
fact. At ~400 ms a one-second window straddles two or three slots: it merges bursts
that landed separately, and one transaction sitting in a NEIGHBOURING slot poisons a
composition read that was clean in the slot being judged.

**Why prints, not either.** A print is what the TAPE batches in, and it is the only
basis in which a quantity is a statement about a trade. `gross_flow >= 10` over one
second is ten one-SOL prints or one ten-SOL print, and neither a wall clock nor a slot
can separate them; `window_size_prints: 1, window_lag: 0` is the current transaction
alone, so the same gate on it means exactly "this tx moved 10 SOL". Pair it with a
lagged print window - `prints: 20, lag: 1` - and the rule reads "a 10-SOL print into a
tape whose previous twenty moved almost nothing", with no arithmetic between the two
and no way for the trigger to leak into its own reference.

A print window is also the one span silence does not move: `prints: 20` is twenty
trades whether they landed in one slot or across an hour, so a print gate reads the
same on a busy token and a dead one. That is the property to reach for when a
threshold has to mean the same thing at both ends of a token's life.

`m_flow_window.trade_share` is the exception that proves it: on the print basis it is a
count over a count, so it is the constant `100 * slice / window` on every tape and
carries no information. Its SOL twin `sol_share` is the reading that survives there —
same two spans, but the numerator is money, which still varies when the counts cannot.

**`window_lag` is what makes a window causal in its own terms.** A gate on "the state
entering this slot" must not be able to see the slot it fires in. The slice is
`slots: 1, lag: 0` and the quiet tape before it is `slots: 30, lag: 1` - same group,
same metric, no arithmetic between windows and no way for one to leak into the other.
`lag: 0` is a real value (end at now) and the only behaviour that existed before the
param, so it is the default and a stored rule round-trips byte-identically.

**A slot or print window is trade-driven; a time window is tick-driven.** A tick is a
wall clock and carries no slot, so slot windows HOLD their last cursor across ticks
rather than estimating one from elapsed time - slot durations vary, and a guessed
cursor is a silently wrong reading rather than a stale one. A print cursor holds for a
stronger reason: a tick is not a print, so no amount of silence evicts anything from a
print window. Entry decisions are taken at a trade, where both cursors are exact.

That is also why a print span contributes `0.0` to the grid horizon
(`ClockHorizons::absorb_req`): nothing a tick does can change what a print window
reads, and a trade emits its own row. `0.0` there is the exact horizon, not an
under-estimate.

**The loader is obliged, same as for the wallet-keyed metrics.** A lake read without
the slot column leaves `TradeLite::slot = 0` and every slot window frozen, which looks
like a strategy result rather than a load error. The print cursor has no such
dependency - the engine counts prints itself as it folds - but it inherits the fold
ORDER: canonical order is slot -> tx_index -> leg, and a loader that feeds trades in a
different order gives print windows a different span from live. With 95% of the money
in same-slot pairs ~0.5 ms apart, ordering by timestamp will not reproduce it.

**One span, one spelling.** `WindowSpec::label` / `WindowSpec::parse` are the single
grammar for naming a window: `30s`, `30sl@1`, `20p`. A persisted exit reason, a live
chip, a chart legend, a `?windows=` query and a sweep axis all carry that string, so a
span that round-trips means the same window everywhere. A **bare number is seconds**,
which is why every pre-basis spelling still parses to exactly what it meant.

Every basis is reachable end to end:

| surface | how a span is spelled |
| --- | --- |
| rule editor / stored rule | `window_size_sec` \| `_slots` \| `_prints` + `window_lag` |
| exit reason | `metric(30sl@1)` |
| `/metric-series?windows=` | `10,30sl@1,20p` - the chart folds the span it is given |
| sweep axis (`AxisSpec.window`) | a number (seconds) or a span string |
| metric discovery (`entry_window_sec`) | same |

A sweep axis assembles the size param its own unit spells, so a slot axis sweeps
`window_size_slots`; two axes that differ only in basis open two group instances,
because merging them on size alone would drop one of the two swept conditions.

## Aggregate flow (`m_flow_lifetime` / `m_flow_window`)

Classifier-free SOL totals on the token. Same four JSON metric names; distinct
registry `MetricId`s so lifetime can be monotonic while the window is not.

| group | kind | strict params | state |
| --- | --- | --- | --- |
| `m_flow_lifetime` | static | none | two running counters on `TokenTrack` |
| `m_flow_window` | dynamic | one of `window_size_sec` / `_slots` / `_prints`, plus a nested `slice_size_*` for the two-window metrics | ring buffer deduped by the whole span |

| metric | meaning | unit | eq-tol | monotonic (lifetime only) |
| --- | --- | --- | --- | --- |
| `buy` | buy SOL | SOL | 0.1 | ✓ |
| `sell` | sell SOL | SOL | 0.1 | ✓ |
| `net_flow` | `buy − sell` | SOL | 0.1 | ✗ |
| `gross_flow` | `buy + sell` | SOL | 0.1 | ✓ |
| `trade_count` | trades landed | count | 0.5 | ✓ |
| `buy_count` | number of BUYS in the window | count | 0.5 | ✗ |
| `sell_count` | number of SELLS in the window | count | 0.5 | ✗ |
| `buy_share` | `buy / (buy + sell)`, **percent 0-100** (window only) | percent | 0.5 | ✗ |
| `trade_share` | trades in the nested slice, percent of the window's (window only) | percent | 0.5 | ✗ |
| `sol_share` | gross SOL in the nested slice, percent of the window's (window only) | percent | 0.5 | ✗ |

`buy_count` is not `trade_count`: sells inflate the latter, and on a one-slot window
only `buy_count` answers "how many people bought into this burst". `sell_count` is its
twin, registered rather than left to arithmetic because a condition cannot subtract —
`trade_count - buy_count` has no spelling, so without it "at most two sells" cannot be
authored at all. The three always add up, on every window, which
`buys_and_sells_add_up_to_the_trade_count` pins.

Non-finite / negative SOL is ignored. Windowed variants are never monotonic.
Lifetime is the maturity / critical-mass gate; window is the hot-right-now filter.
No fingerprint config — unlike the ix-split groups below.

## Crowd (`m_crowd_window`)

| group | kind | strict params | state |
| --- | --- | --- | --- |
| `m_crowd_window` | dynamic | one of `window_size_sec` / `_slots` / `_prints` | the same ring buffer, plus a per-wallet occurrence map |

| metric | meaning | unit | eq-tol |
| --- | --- | --- | --- |
| `unique_wallets` | distinct trading wallets in the window | count | 0.5 |
| `trades_per_wallet` | `m_flow_window.trade_count / unique_wallets` | count | 0.05 |

**Its own group because its subject is WHO traded, not how much** — and that
difference is a load obligation rather than a taste. These two are the only metrics
`MetricId::needs_wallet_identity` returns true for, and an offline read that did not
request the wallet column folds every trade as one anonymous wallet: `unique_wallets`
reads `1` forever and the gate looks strict instead of broken. One group, one
obligation, so a loader answers the question by group instead of by metric list.

A rule wanting both flow and crowd gates over one window authors two instances at the
same `window_size_sec`. They are ANDed like any two groups, and they share the buffer —
`TokenTrack` dedupes by the whole span, so the second instance costs nothing.

`unique_wallets` counts **people, not SOL**: one wallet churning and a crowd arriving are
identical in `gross_flow` and different here. It keeps a per-wallet occurrence map beside
the SOL deque, so a wallet leaves the count only when its **last** entry leaves the window
— eviction that `remove()`s on the first drops a wallet that is still trading. Its `=`
tolerance is half a wallet: a tally has no sub-unit, and anything wider would make `== 5`
also match 6.

**A windowed `gross_flow` floor subsumes the lifetime one.** The window is a sub-interval
of the token's life and both metrics are the same `buy + sell` SOL, so
`m_flow_window(W).gross_flow >= X` implies `m_flow_lifetime.gross_flow >= X` for every `W`.
Stacking a lower lifetime floor under a windowed one is a **no-op clause** — every seeded
rule family already carries `m_flow_window(60).gross_flow >= 45…70`, so none of them needs
one.

**Where the lifetime floor earns its place is as the *replacement* for a windowed hot gate,
not an addition to it.** A liveness floor is worth ~12.5 pp of mean PnL by ablation on a
broad universe (it is what holds the `Dead` exit rate down — see
[wallet-8dtx-logic.md](wallet-8dtx-logic.md)), but a *windowed* one risks selecting
post-move moments created by the very move it gates on, which is what the entry-timing
diagnostic (`family_search::gates`) exists to catch and what dropping `gross_flow(60) >= 55`
from the scalp family confirmed. The lifetime floor cannot have that defect: it reads
cumulative maturity and does not bind the entry instant. So when the diagnostic flags a
windowed hot gate, swap it for `m_flow_lifetime.gross_flow >= 30` rather than leaving the
rule with no liveness gate at all. The same applies to any entry whose window gate points
*downward* (a quiet-tape gate) — there the lifetime floor is load-bearing from the start.

**`buy_share` is window-only**; `trade_count` exists in **both** groups. The lifetime one is a
monotonic accumulator like `buy`/`sell`/`gross_flow`, so an entry UPPER bound on it (`<= 140`)
is a **one-way door**: once a token crosses it the requirement can never come back and the arm
is disarmed as unsatisfiable rather than re-checked for the rest of the token's life. That is
what makes it a maturity gate — "still early in its trading life" — rather than a tape reading.
`trade_count` is how BUSY the tape is, against `unique_wallets`'
how many people are on it: one wallet re-entering ten times reads 10 and 1. It is the only
one of the three that needs **no wallet column**, so it survives an offline load that did not
request wallet identity — prefer it whenever the count, not the crowd, is what the rule means.
`buy_share` is the tape's DIRECTION independent of its size (`net_flow` conflates the two:
+5 SOL net means something different on 6 SOL of turnover than on 200) and is `NaN` on an
empty window, which satisfies no condition.

**`trades_per_wallet` is the one ratio the other two cannot express.** Six people trading once
and one wallet trading six times are identical in `trade_count` AND in `gross_flow`, and read
1 vs 6 here — so `<= 2` is "a crowd is arriving" and a large value is "one wallet is working
the tape". It is a **count ratio, never an identity**, which is what makes it survive the
wallet rotation that renders identity useless. Like `buy_share` it is `NaN` on an empty
window, and for a sharper reason: `0.0` would let `trades_per_wallet <= 2` pass on a DEAD
tape, which is the exact reading the gate exists to exclude.

## Launch size is an AXIS, not a metric

Total buy SOL in the token's creation slot is `first_slot_buy_lamports`, a **fingerprint
axis**. It is not in `m_state` and there is no metric spelling of it.

The test a fact has to pass to be a metric is WHEN it can change. `time` moves every
tick; `liquidity` moves on every trade. The creation-slot total is fixed by the creation
slot: it selects WHICH tokens a rule arms on, never when it fires, which is what a
fingerprint is for — the same reading that puts `ix_count` and `prior_launches` on the axes.

It was briefly both, on one argument: a fingerprint pinned a bucket `floor(v/width)`, so
a threshold like `>= 6.41` had no axis spelling. Ranges retired the bucket. An
[`AxisPredicate`](../../../engine/src/fingerprint/axis.rs) is an inclusive `[min, max]`
with either bound open, plus `Spans` for `!=` and `|`, so `>= 6.41 SOL` is
`{"first_slot_buy_lamports": {"kind": "range", "min": "6410000000"}}` — and the axis
expresses strictly more than a condition list could.

The axis is **deferred**: it is summed from the creation slot's trades, so it does not
exist at `TokenCreated`. A fingerprint configuring it holds the arm at
`PendingFirstSlot` until `FirstSlotSettled`, and an unknown value FAILS a configured
axis, so an unscreened token never arms.

## The nested slice (`m_flow_window.trade_share` / `.sol_share`)

Two ratios across a **nested pair** of trailing windows — the reads whose basis is a
window PAIR, and the reason `MetricReq` carries a `Windows` carrier instead of a bare
`Option<f64>`.

| metric | meaning | unit | eq-tol | monotonic |
| --- | --- | --- | --- | --- |
| `trade_share` | `trade_count(slice) / trade_count(window)`, **percent 0-100** | percent | 0.5 | ✗ |
| `sol_share` | `gross_flow(slice) / gross_flow(window)`, **percent 0-100** | percent | 0.5 | ✗ |

**How CONCENTRATED the tape is, independent of how busy it is.** Ten trades arriving in
the last three seconds and ten spread evenly over a minute are the same `trade_count`
and the same `gross_flow`, and 50 vs 10 here. It is the scale-free way to ask "is this
accelerating against its own pace" — two absolute bounds are not a substitute,
because they silently re-read size.

**The two are not restatements of each other.** Ten prints carrying a tenth of a SOL
each and one print carrying ten are the same `trade_share` and far apart in `sol_share`.
On a PRINT window only `sol_share` survives at all: a fixed count of transactions inside
a fixed count of transactions makes `trade_share` the constant `slice / window`.

### Why these are metrics of `m_flow_window` and not a group

The slice is one more span over the same tape, so a group of its own would be a second
name for one subject — and `m_flow_window`'s basis would still be a single window
while its sibling's was a pair.

What keeps the axis honest is that it is required **per metric**, not per group:

* `m_flow_window` declares `slice_size_*` for every instance, all three units, none
  `required` on its own.
* `metrics::is_two_window` names the metrics that read it. `validate_group` requires a
  slice exactly when one of them is present, and REJECTS one when neither is — a
  slice nothing reads changes no value and no requirement identity, so it would sit in
  the params looking like a gate. Same principle as an `arm_above_pct` with no trailing
  metric.
* `arm::build_reqs` attaches `Windows::secondary` to those metrics alone. Attaching it
  to the instance would give a `gross_flow(30s)` requirement a different IDENTITY
  depending on whether a sibling clause happened to read a slice — and two rules on
  the same window would stop sharing one buffer.

It owns **no state**: both readings come off `m_flow_window`'s own `trade_count` and
`gross_flow` on buffers the track already keeps, so a rule that also gates on those two
windows pays nothing extra. That reuse is what makes `m_flow_window{60,3}.trade_share`
and `m_flow_window(3).trade_count / m_flow_window(60).trade_count` the same number by
construction rather than by agreement.

Three properties to author against:

* **`NaN` on an empty reference window** — no trades, no share, and a `0.0` would let
  `trade_share <= X` pass on a dead tape.
* **Both windows are clipped by the token's age**, so on a token younger than
  `slice_size_sec` every trade is inside both and both read `100`. That is a true reading
  of a short life, not a sentinel: a rule that means it as a *maturity* signal must bound
  `m_state.time` itself. The same clipping applies to the SQL a rule is fitted in, so
  backtest and engine agree.
* **Entry side only — these two metrics, not the group.** The persisted exit-reason
  label carries one window qualifier, so two clauses differing only in the slice would
  record the same reason. The save gate rejects the metric rather than write an
  ambiguous label; its single-window siblings stay perfectly good exits —
  [roadmap/two-window-exit-labels.md](../../roadmap/two-window-exit-labels.md).



> **Measured and refuted as an entry gate** (2026-08-10, OOS 07-29..08-09 on `fs3-00`):
> tightening it *anti-selects*, monotonically — `>= 20` replacing `gross_flow >= 45` scores
> −0.75 %/ep against −1.22 at 40, −1.97 at 60 and −2.04 at 80, and stacking it on top of the
> volume gate is either inert (it does not bind below ~30) or worse (−2.47 at 60, −3.94 at
> 100). At matched fire count the crowd gate beats the volume gate by 0.43 pp, well inside
> the ±1.07 pp standard error. See [flow-scalper-findings.md](flow-scalper-findings.md).
> The metric stays because it is a real, cheap capability — but do not re-propose it as a
> selection gate on this family without new evidence.

**Wallet-keyed metrics oblige the loader.** Offline, the lake omits the `wallet` /
`ix_labels` columns unless the run asks for them, and a fold over rows without them sees
every trade as one anonymous wallet — so `unique_wallets` reads `1` forever and a gate on it
never fires, which looks like a strategy result rather than a load error. The answer lives
on the metric (`MetricId::needs_wallet_identity`) rather than as a group list copied into
each loader, because a wallet-keyed metric in an otherwise SOL-only group is exactly what a
group list misses. **A new wallet-keyed metric must be added there.**

### A trailing-window read is O(1) — keep it that way

`flow_window` / `flow_ix` maintain running sums over a **time-sorted** deque and correct
only the two out-of-window ends on read. A flow-split rule pays that read once per metric
per rule per event, so a full-buffer rescan — or re-deriving the window width per element —
is a hot-path regression. Never reintroduce one inside a `value()`, and never assume the
caller already evicted at `now`: `TokenCreated` / `FirstSlotSettled` do not, and a skipped
tick leaves entries un-evicted by design.

## Classifier (per trade × fingerprint)

A trade is **tagged** iff any of:

1. the configured marker mask says so — `tagged_ix_markers` when the trade's markers
   **intersect** it, `untagged_ix_markers` when they **miss** it entirely;
2. its ordered `ix_labels` hash ∈ the configured `ix_patterns`
   (exact ordered sequence — same semantics as fingerprint `ix_labels`);
3. `wallet_contagion` is on AND its wallet was previously tagged on
   **this token**;
4. `creator_is_tagged` is on AND it is the creator wallet.

Otherwise **untagged**. Contagion is per-token only (cross-token is a future toggle).

Tagged usually reads as creator tooling and untagged as organic retail, and the
sections below argue in those terms. The metric names do not: they say which side of
the classifier a trade fell on, which is the only thing the engine knows.

### Markers: the mechanism, not a snapshot of it

A marker is one bit set by the **producer** (the only layer holding the label strings)
and compared by the engine. The vocabulary is fixed and small on purpose - a marker set
that grows per rule is a pattern list again. Two kinds, both mechanisms:

| kind | markers | what it identifies |
| --- | --- | --- |
| machinery | `AdvanceNonceAccount` · `CreateAccountWithSeed` · `System Program: Transfer` · `Pump.Fun: Create` · `Memo Program` | what the transaction DOES |
| router | `Axiom Trade` · `Photon` · `Bloom Router` · `Trojan Trade` · `Terminal` | the retail front-end a person clicked through |

Matching is substring containment over each label, because a label carries its program
prefix. An unknown marker name is an **error**, never an empty mask: a typo that
silently matched nothing would let a cleanliness gate pass on bot traffic.

A router is a property of the **build**, not of who sent it, which is why it lives here
and not in a wallet list - and it is the reason the vocabulary can hold it at all
without becoming per-rule: the set grows when a new front-end starts carrying retail
order flow, and at no other time.

**Why a marker beats an exact-sequence list here.** `CreateAccountWithSeed` means the
transaction creates a throwaway account inline - nobody is coming back to it, so it is
a disposable machine rather than a person with a wallet. That stays true of every
future build. A list cannot promise it: on the 08-01..08-21 tape **531 distinct label
sequences** carry the seed marker and new variants ship continuously, so a list books
the unlisted ones as human demand.

### A mask names ONE side, and which side is the rule

`tagged_ix_markers` and `untagged_ix_markers` are not two spellings of one thing, and
configuring both is an error (so is `untagged_ix_markers` alongside `ix_patterns`,
which is itself a tagging statement). They differ on the case that decides most
gates - **a build carrying no configured marker at all**:

| mask | a marked build | an unmarked build |
| --- | --- | --- |
| `tagged_ix_markers` | tagged | **untagged** - identifies machines, leaves the rest unjudged |
| `untagged_ix_markers` | untagged | **tagged** - identifies people, judges the rest machine |

Say the one the rule means. On the 8dtx tape the same fires, same thresholds, same
exit read **+0.99 % per trade** under `tagged_ix_markers: [CreateAccountWithSeed]` and
**+6.86 %** under `untagged_ix_markers: [<routers>]`, because the 8,566 fires the first
admits and the second rejects average **-0.68 %**.

An `untagged_ix_markers` mask also fails **closed**: a loader that leaves `ix_labels`
empty marks every trade tagged, so the gate fires nothing rather than firing on
everything.

### The two wallet rules are switchable, and a structural gate wants them OFF

`wallet_contagion` and `creator_is_tagged` both default **true**, so every fingerprint
stored before markers existed classifies exactly as it did.

A structural gate turns them off. "Did this transaction come through a named router" is
a property of the transaction; contagion makes it a property of the sender's history on
that token, and the creator rule adds an identity term. Leaving them on does not merely
*tighten* such a gate - it measures a different thing, and the fire set stops matching
the one the rule was derived on. Wallet-keyed rules are also the axis a
[wallet-free](wallet-8dtx-derived-rule.md) derivation is not allowed to use.

Both are checkboxes on the fingerprint form, under the pattern rows. The form writes
them **explicitly** on every save rather than leaving them to the backend default: a row
that omits them says nothing about which classifier it meant, and the whole `m_flow_ix`
object round-trips through `metricConfigWithIxPatterns`, so a save that touches only the
name still preserves the marker masks and both flags. Write the key from any other
caller the same way - the PUT replaces the row, so a partial write lands as a full one.

```json
"m_flow_ix": {
  "untagged_ix_markers": ["Axiom Trade", "Photon", "Bloom Router", "Trojan Trade", "Terminal"],
  "wallet_contagion": false,
  "creator_is_tagged": false
}
```

Config lives on the fingerprint (not the rule):

```json
{
  "m_flow_ix": {
    "ix_patterns": [
      ["Pump.Fun: Create", "Pump.Fun: Buy"],
      ["Pump.Fun: Buy", "Token Program: CloseAccount"]
    ]
  }
}
```

### The counts are the tagged set tallied, and only the tagged set

`tagged_buy_count` / `tagged_sell_count` are `tagged_buy` / `tagged_sell` counted instead
of summed. They exist because a SOL sum cannot state *how many*: one 2 SOL sell and two
1 SOL sells are the same `tagged_sell`, and "**two** dump-shaped sells landed at once" is
a rule about the second. On a one-slot window the count is exactly that reading.

Only the tagged side is tallied. A pattern list names the volume side, so "how many of
them landed" is a statement about the tagged set; the untagged remainder is everyone the
classifier declined to judge, and counting it counts strangers rather than a machine.

The two sides do not mix, and that is what lets one list do both jobs: matching is on the
transaction's own ordered labels, and the side split happens after. A **buy** pattern can
never match a sell, so a list holding the volume-making buy shapes *and* the dump sell
shape leaves `tagged_sell_count` counting the dump shapes alone — provided
`wallet_contagion` is **off**, which is the one rule that would let a wallet's tagged buy
make its later sells count.

`m_flow_ix_window` reads the **same** `m_flow_ix` key (one classifier, two views).
Unconfigured fingerprint (no `m_flow_ix` key) ⇒ every flow metric is **NaN**
(satisfies nothing). `ix_hash = None` (pre-0002 / missing lake labels) ⇒ organic
unless wallet-tagged/creator.

Flow state is **fingerprint-scoped** on `TokenTrack` (`BTreeMap<FingerprintId, FlowState>`),
not token-scoped — two fingerprints with different pattern sets diverge.

**A pattern list carries VARIANTS or it carries nothing.** Matching hashes the whole
ordered label list, so one instruction of difference is a complete miss: the same
launch bot appears with and without a trailing `System Program: Transfer` (the tip)
and with `Associated Token: Create` vs `CreateIdempotent`, which is four sequences for
one behaviour. A list holding some of them books the rest as **organic demand**, and
an organic-flow gate then fires on bot traffic — an exit that is arithmetically
correct and impossible to explain from the trades. Audit a list by variant, never by
example.

A rules reload **adopts** an edited set on tokens already being tracked
(`TokenTrack::ensure_flow`). Trades already folded keep the classification they were
folded under — the totals are running sums and no trades are retained to redo — so an
edit moves a live token's future, never its past.

### The flow context is patterns **AND** creator — every consumer seeds both

`ensure_flow` alone is not a complete flow context. The creator seed is rule 3 of the
classifier *and* the contagion set's origin, so a fold that skips `seed_creator` books
the dev buy + dev dump — a token's two largest single flows — as **organic**, and its
`tagged_*`/`untagged_*` are a different classification from the one the engine decides on.
Both calls, on every path that folds flow:

| consumer | seeds at |
| --- | --- |
| live engine | `reduce.rs` — `TokenCreated { creator_wallet_hash }` → `track.seed_creator` |
| simulate | `engine_sim.rs` — `ReplayToken.creator_wallet_hash` (hashed from `tokens.creator_wallet`) |
| metric-series (`/metric-series`) | `metric_series.rs` — `resolve_flow_ctx` loads the creator; `build_series` seeds after `ensure_flow` |
| chart overlay (browser preview) | `classifyFlow.ts` — `FlowClassifyOptions.creatorWallet`, passed by `TokenTradeChart` |

`/metric-series` shipped without the seed and drew a `untagged_net` that disagreed with
both the chart overlay and the live engine; locked by
`the_creator_wallet_is_volume_side_even_without_a_pattern_match`. Order is free —
`ensure_flow` copies an already-set creator, `seed_creator` back-fills existing states —
but one of the two alone is a silent misclassification, never an error.

**The browser overlay is a preview, not the metric.** `classifyFlow.ts` mirrors the Rust
classifier but folds a *different corpus* (PG-only `/api/tokens/:mint/trades`, vs the
sealed lake + PG tail the endpoint reads) and renders in the chart's display unit and
flow basis. Compare it to `m_flow_ix.untagged_net` only in SOL on the `cost_sol` basis,
and expect drift wherever the two corpora differ (PG retention has dropped a token's
early trades; pre-V0 lake days null-fill `ix_labels`/`wallet`). With **no** configured
patterns the overlay still draws, but the structural test never fires and the two lines
are creator-plus-contagion vs the rest — a cohort split, not the metric. The chart
toolbar names which of the two is on screen.

## Hash SSOT

`hunter_engine::metrics::flow_ix::{ix_hash, wallet_hash, ix_hash_opt}` are the
**only** hashers. Every adapter (live producer, lake replay, event-log) calls them;
patterns compile to a hash set at `RulesReloaded`. No interner ⇒ replay parity by
construction. See hunter/CLAUDE.md Gotchas.

## Metric groups

| group | kind | strict params | fingerprint config |
| --- | --- | --- | --- |
| `m_flow_ix` | static (fingerprint-scoped) | none | `ix_patterns: string[][]` (required when key present) |
| `m_flow_ix_window` | dynamic | `window_size_sec` | none (reads `m_flow_ix`) |

**Multi-window per group** (any dynamic group — `m_flow_window`, `m_price_window`,
`m_flow_ix_window`): a group appears under a side as a single object (one window — the
legacy shape) OR a JSON **array** of objects, each with its own `window_size_sec`, to
gate the same group at several window sizes at once (e.g. a 30s `gross_flow` hot gate AND
a 2s `net_flow` exhaustion gate on entry). Each window is an independent clause (entry-AND
/ exit-OR); windows must be distinct; static groups take a single object. The single-object
form round-trips byte-identically (no DB migration). SSOT for the shape + validation:
`hunter_engine::rule_params` module docs.

Both flow groups expose the same eleven JSON metric names; registry `MetricId`s are distinct
so lifetime monotonic flags can differ. All SOL values use absolute trade notional;
buy = +, sell = − for `*_net`.

| metric | meaning | unit | eq-tol | monotonic (lifetime only) |
| --- | --- | --- | --- | --- |
| `tagged_buy` | buy SOL from tagged wallets | SOL | 0.1 | ✓ |
| `tagged_sell` | sell SOL from tagged wallets | SOL | 0.1 | ✓ |
| `tagged_net` | `tagged_buy − tagged_sell` | SOL | 0.1 | ✗ |
| `tagged_gross` | `tagged_buy + tagged_sell` | SOL | 0.1 | ✓ |
| `untagged_buy` | buy SOL from untagged wallets | SOL | 0.1 | ✓ |
| `untagged_sell` | sell SOL from untagged wallets | SOL | 0.1 | ✓ |
| `untagged_net` | `untagged_buy − untagged_sell` | SOL | 0.1 | ✗ |
| `untagged_gross` | `untagged_buy + untagged_sell` | SOL | 0.1 | ✓ |
| `tagged_share` | `tagged_gross / (tagged_gross + untagged_gross)` ×100; NaN when total 0 | % | 1.0 | ✗ |
| `tagged_buy_count` | tagged BUY transactions | count | 0.5 | ✓ |
| `tagged_sell_count` | tagged SELL transactions | count | 0.5 | ✓ |

Windowed variants are never monotonic. Lifetime monotonic ✓ metrics participate in
derived-unsatisfiability disarm (`arm.rs` reads the registry flag).

## NaN rules

| situation | flow metrics |
| --- | --- |
| Fingerprint has no `m_flow_ix` key | all NaN |
| Pre-first-trade (no classifier state yet) | NaN (existing convention) |
| Trade `ix_hash = None`, wallet not tagged, not creator | counts as organic |
| Token row missing / no `creator_wallet` | creator unseeded (logged `warn`); creator trades classify by pattern/contagion only |
| Pre-V0 sealed lake days (NULL `ix_labels`) | organic in runtime; **excluded** from discovery score denominators |

Rule save **warns** (does not reject) when params reference flow groups but the
fingerprint is unconfigured.

## Creator history (the `prior_launches` fingerprint axis)

How many tokens the token's creator launched **before** it, counted over a trailing
`PRIOR_LAUNCH_WINDOW_DAYS` (30) window. A creator-history filter: `0` is a first-time
launcher, a large value a factory. Static from `TokenCreated`, so a gate on it is a token
filter that can never re-trigger.

The tally lives in `EngineState.creator_launches`, keyed by `creator_wallet_hash`, and is
read strictly before its own increment — so a creator's first token reads `0`. It is ONE
tally shared by every path that folds events, which is what keeps live and `simulate` from
disagreeing.

| fact | why |
| --- | --- |
| **`0` is a real value; unknown is `NaN`** | A creation event with no `creator_wallet_hash` leaves the metric unseeded. Seeding `0` there would widen `= 0` to every token whose creator the feed failed to resolve — the one direction that silently inflates the rule. |
| **The tally must be PRIMED** | A fresh process starts empty and reads every creator as new. `EngineState::prime_creator_launches` loads real history first: live from `TokenRepository::creator_launch_counts` at boot, `simulate` from the same query bounded to `[corpus_start - 30d, corpus_start)`. |
| **The window is part of the rule** | Every threshold is denominated in `PRIOR_LAUNCH_WINDOW_DAYS`. Widening it re-scales every `prior_launches` condition already authored. |
| **Unavailable on lake-corpus paths** | The lake's tokens dimension carries no creator column, so the grouped sweep, rule search and family search cannot seed it. `MetricId::needs_creator_history` flags this and the sweep's axis resolver REJECTS the axis rather than scoring every cell on zero trades. Use `simulate`, which reads the creator off the PG `tokens` row. |

Same class of load-time hazard as `needs_wallet_identity`: the value depends on data the
loader may not have asked for, and the failure looks like a strict gate that never fires.

## Semantics that read as one thing and mean another

Seven facts that produce silently wrong rules rather than errors. None is derivable from the
registry, and each has cost a search run.

| fact | what goes wrong without it |
| --- | --- |
| **`m_flow_ix*` is all `NaN` without `ix_patterns`** — on the request *and* in the fingerprint's `metric_config` | `NaN` satisfies nothing, so the conditions read as present and never fire. Rule save warns; the sweep does not. |
| **`m_state.liquidity` is the REAL SOL reserve** — `TradeLite::reserve_sol` from `real_reserve_sol`, which is `vsol - 30` on the curve. Floors at **0** (empty curve), tops near **85** (migration). | A gate written against the virtual 30/115 scale sits ~30 too high. `liquidity >= 85` fires only on tokens that actually migrate. |
| **`m_price_lifetime.stall` is seconds since the last ALL-TIME HIGH**, not since the last trade | An exit below ~60 fires on ordinary chop. It caps every hold, so it doubles as an entry filter. `m_position.held` is the time stop. |
| **`m_position.retrace` without `arm_above_pct` is a hard stop from entry** — the peak seeds at entry | Reads as a trailing stop, behaves as a fixed stop. |
| **`m_position` is exit-only** | It reads `NaN` before a fill, so it could never fire on entry. The sweep rejects it there. |
| **`m_flow_window.buy_share` is PERCENT 0-100, not a 0-1 ratio** | An analysis carrying it as a ratio and authoring `>= 0.8` writes a gate every token passes, which reads as a working rule that took every trade in the universe. |
| **`take_profit` / `stop_loss` axes reject `null`** | To test "no take-profit", omit the axis or pass an unreachable value (`1000` TP, `100` SL). |

Combination semantics: **entry conditions AND together, exit conditions OR together.** Adding
an exit condition can only make exits fire earlier or as early, never later.

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
  badge rule. Showing a mean while badging on a count of strong signals lets a 49%
  row sit un-badged beside a badged 33% one.
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
- **`first_slot_trades > 0` = the *Launch shapes · group* predicate**
  (`isFirstSlotPresent`) — *presence*. The launch bundle is the set of shapes that
  appear in the creation slot, and a bundler shape that also trades later is still
  bundler tooling, so the button takes a shape at any Launch% above 0. **No dust
  floor** (it applied here until 2026-08-05): presence is an identity claim about
  the launch, so size gets no vote — and `SUGGEST_MIN_GROSS` was read against
  *group-wide* gross anyway, so it dropped exactly the rare small bundler tail the
  button exists to find. The floor still gates the `suggested` composite.

The creation instruction needs no clause of its own: a shape carrying it is in the
creation slot by construction, so the presence test already takes it. Both reads
answer a different question from the `Auto` composite — *when* the shape trades,
not how bot-like it scores — so the two buttons are independent and neither gates
the other. No lift gate on this one: creation-slot presence is an identity claim,
while lift measures group-vs-window concentration and would drop a bundler shape
that happens to be ambient across the whole window.

The cost of presence-over-purity is real and is what `Launch%` is *for*: **live
classifies volume by `ix_hash` alone — there is no slot predicate**
(`flow_ix.rs`). A checked shape that carries launch *and* organic flow tags the
organic tail too, and wallet contagion then sweeps those wallets' other trades in
as well. So the button is a bulk *proposal* — read `Launch%` on the rows it
checked and uncheck the mixed ones before Apply.

`first_slot_trades == null` is **unknown**, not 0: a pre-field cached run selects
nothing rather than guessing (re-run discovery to fill it).

**The button gates on the corpus, not on the draft.** `disabled` reads
`firstSlotAll.length === 0` — "this group has no launch shapes at all" — while the
click adds only `firstSlotUnchecked`, the ones not already staged. They must not be
the same test: the draft is re-seeded from the target fingerprint's *saved*
`ix_patterns` on every run (`seedFromFingerprint`, keyed on `result.run_id`),
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

### Per-token launch set — the *Launch shapes · this token* button

`StructureScore.first_slot_trades` cannot answer "what was in THIS token's launch
bundle", and reading it as if it could is the bug that motivated this section. It
is lossy three separate ways:

1. **Aggregated over the group.** The count sums every member token's creation
   slot, so the group button proposes shapes that launched a *different* token.
2. **Rank-truncated server-side.** `structures` is sorted by lift → volume_share →
   wash_symmetry and cut at `max_structures_per_group` (64). A shape past the cut
   never reaches the browser, so no client-side predicate can recover it — and a
   rare, small bundler shape loses that ranking by construction.
3. **Trade-only.** A launch-bundle instruction that produced no buy/sell trade row,
   or whose `ix_labels` failed to parse, is not a structure at all
   (`parse_trade_ix_labels`). Nothing downstream can add what was never scored.

So `TokenGross` carries its own answer: `first_slot` (the creation slot) and
`first_slot_ix_labels`, **every distinct shape that traded in that token's slot**,
ranked by first-slot gross desc (ties broken on the labels, so the order is stable
across runs). Uncapped and unfloored — a slot holds a handful of shapes, and the
whole point is that neither size nor rank may veto membership. Accumulated in the
same pass as the group aggregate (`token_first_slot_gross`), so it costs one extra
map, not a second scan.

`Option<Vec<_>>` on the wire: a pre-field cached run reads *unknown* (badge/tooltip
say re-run discovery), `Some([])` is the real "no ix_labels in that slot". The
button appears only while a token is picked in the preview panel; its hover outline
can only mark shapes that also have a row in the ranked table, which is precisely
the set the group button was limited to — the ones it adds beyond that are the
point.

### The result carries its own corpus identity

`DiscoveryResult` echoes the `plan` it partitioned by, `ix_labels_filter` and
`fingerprint_id` alongside `groups`, and both GET endpoints
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
ends in `bkt=exact` rather than a width.

Discovery has no `GroupSelection` resolver — that seam is grouped-sweep-only
(`lab/src/sweep/selection.rs`), because a discovery run has no persisted run row to
resolve against. The echoed fields are its equivalent.

## Future toggles (not built)

- **Cross-token contagion**: wallets tagged on token A pre-tagged on token B of the
  same fingerprint. Needs a bounded shared set inside `EngineState` keyed by
  fingerprint (size-capped, log-replayable). Powerful; risky (one false tag poisons a
  whole group) — build only after v1 data shows rotation defeats per-token contagion.
- **Baselines / since-entry variants**: anchor metrics to lifecycle moments (creator
  first sell, entry fill). New metrics inside `flow_ix.rs`, no structural change.
- **Transfer ingestion**: direct wallet-linking via SOL/token transfers — a separate,
  expensive ingest feature; only if the proxy demonstrably fails.
- **Discovery auto-promote**: above a score threshold (likely `group_lift` +
  `cross_token_recurrence` gates), write `ix_patterns` without a toggle pass.
  **Blocked on V4.4 hand-label kit.** Even then, default remains review-then-apply;
  auto-promote is an opt-in mode on the discovery page, never a silent background job.
