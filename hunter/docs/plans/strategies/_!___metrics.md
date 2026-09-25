# Metrics: how the system is built and extended

How the engine's metric system is put together, the invariants that keep it honest, and
what it costs to extend. The ideas that use the metrics are
[_!___inventory.md](_!___inventory.md); every word is [_!___terms.md](_!___terms.md); the
numbers they produced are [_!___evidence.md](_!___evidence.md); the basis and the laws are
[_!___strategy.md](_!___strategy.md). The engine map is
[arch/strategies.md](../../arch/strategies.md).

**This file defines no metric.** Every family, metric, span kind, tag matcher and rule part
carries its one definition (summary, example, note, unit, accepted tags and spans) in
[`registry.rs`](../../../engine/src/metrics/registry.rs), with the span kinds in
[`span.rs`](../../../engine/src/metrics/span.rs) and the tag vocabulary in
[`tags/config.rs`](../../../engine/src/metrics/tags/config.rs). `registry_json()` serves
all of it at `GET /api/meta/strategy-registry`; every picker, tooltip, condition sentence
and the Guide page ([`StrategyGuide.tsx`](../../../frontend/src/shared/components/strategy/StrategyGuide.tsx),
`/strategies/guide`) render that text as-is. The frontend tests read the same document from
`engine/fixtures/registry.json`, pinned by `registry_fixture_is_current`. What a metric
means is answered there; this file carries what the definitions cannot: structure,
landmines, measured costs, and why a rule exists.

## 1. One read: `m_family.metric @tag [span]`

A condition reads one [`MetricRef`](../../../engine/src/metrics/metric_ref.rs): which
metric, whose trades, over what stretch.

```json
{ "metric": "m_flow.buy_sol", "tag": "!volume", "span": "10s", "is": [{"operator": ">=", "value": 2}] }
```

- **The family** is one subject; **the metric** is one number in it, its unit the last word
  of its name (`every_name_ends_in_its_unit`): `_sol`, `_pct`, `_sec`, `_lamports`,
  `_count` (prints: every leg), `_tx_count` (transactions: leg 0), `_slots`, `unique_*`;
  no suffix is a 0/1 flag.
- **The tag** picks whose trades (section 3). **The span** picks the stretch (section 2).
- **Each metric declares what it accepts**: `TagUse` (none / optional / required),
  `TagLevel` (trade / template / wallet class) and `SpanUse` (life / window / since age /
  slice). `MetricRef::check` refuses anything else with a reason naming the fix. There are
  no parallel per-basis or per-tag metrics: `buy_sol [10s]`, `buy_sol [20sl]` and
  `buy_sol @!volume [5p]` are one metric read three ways.
- **A reference is a condition's identity.** Two conditions with equal references read the
  same number from the same buffer and the same series column; that is how windows dedupe,
  and how a readout, an entry blocker, a sweep axis and an exit label name what they read.
- **One spelling.** `MetricRef::label` (`m_flow.buy_sol @!volume [10s]`) is the text in exit
  labels, the live condition strip, `strategy_arms.end_detail`, sweep columns and errors. A
  sell line without a label is labelled from its first condition.

| family | subject | compute module(s) |
| --- | --- | --- |
| `m_state` | the pool now | `state` |
| `m_price` | the chart | `price_lifetime`, `price_window` |
| `m_flow` | money moving, all trades or one tag's | `flow_lifetime`, `flow_window`, `flow_slice`, `tags::state` |
| `m_holdings` | what a tag or wallet class holds | `tags::state` (`profit_sol`), `holder_book` (`bag_share_pct`) |
| `m_crowd` | who shows up | `crowd_window`, `build_window`, `crowd_after_age`, `burst_slot` |
| `m_print` | the print being read | `print_wallet` |
| `m_slot` | this slot's buys | `burst_slot` |
| `m_wave` | this buy wave | `burst_wave` |
| `m_position` | our trade | `position` (read from `PositionCtx`, never the coin) |

Modules are under `engine/src/metrics/`. The v1 group names (`m_flow_ix`,
`m_flow_window`, `m_burst_slot`, ...) map to these references in `v1::map_metric`
([`v1.rs`](../../../engine/src/v1.rs)), the one old-to-new table (section 9).

## 2. Spans

A span is a trailing window, a since-age anchor, or the coin's life (no span written). A
window is `WindowSpec { size, lag, unit }` with `unit` one of seconds, slots, prints;
`WindowUnit::ALL` is the one place the bases are enumerated. Each buffer entry carries a
position already in its own unit (milliseconds, the slot number, the coin's print ordinal),
so the fold, the eviction and the read are one implementation over an `i64` cursor.

| unit | bounds at `now` | what advances it |
| --- | --- | --- |
| seconds | closed `[now - lag - size, now - lag]` | the clock: trades and ticks |
| slots | exactly `size` slots ending `lag` before the current one | a trade, or a tick that carries a slot |
| prints | exactly `size` prints ending `lag` before the current one; `1p` is the print being read | a trade only |

**Why slots.** A slot is what the chain batches in, so a bundle is a slot fact. At ~400 ms a
one-second window straddles two or three slots: it merges bursts that landed separately, and
one transaction in a neighbouring slot poisons a composition read that was clean in the slot
being judged.

**Why prints.** A print is what the tape batches in, and the only basis in which a quantity
is a statement about a trade. `gross_sol >= 10` over one second is ten one-SOL prints or one
ten-SOL print; `[1p]` is the current transaction alone. Paired with `[20p@1]`, a rule reads "a
10-SOL print into a tape whose previous twenty moved almost nothing" with no arithmetic
between the two. A print window is also the one span silence does not move, so a print gate
reads the same on a busy coin and a dead one.

**The lag makes a window causal in its own terms.** A gate on "the state entering this slot"
must not see the slot it fires in: the burst is `[1sl]`, the quiet tape before it
`[30sl@1]`, and neither can leak into the other.

**Slot and print cursors hold on a tick.** A tick is a wall clock and carries no slot unless
the producer supplies one, so a slot window holds its last cursor rather than estimate one
from elapsed time (a guessed cursor is silently wrong, not stale). No amount of silence
evicts a print. Entry decisions are taken at a trade, where both cursors are exact. The slot
cursor only moves forward, so a regressed feed row cannot rewind every slot window on the
coin.

**The slice rides the span's clock.** The two `slice_*_share_pct` metrics take a shorter
window nested in the span: same unit, and the span's lag (`[30s@2, slice 2s]` compares
`2s@2` against `30s@2`). A slice in slots over a span in seconds would be a ratio across two
clocks, so the parser refuses it. The slice reads the coin's own flow windows, registered
for both axes, so a rule that also reads `gross_sol [30s]` and `[2s]` pays nothing extra.
On a print basis `slice_trade_share_pct` is the constant `100 * slice / span`; only the SOL
twin carries information there.

**One grammar.** `WindowSpec::label` / `WindowSpec::parse` spell a window everywhere:
`30s`, `30sl@1`, `20p`. A bare number is seconds. `Span::parse` adds `age60s` and the slice;
`Span::bracket` renders `[30s, slice 2s]`. A persisted label, a chart column, a
`?windows=` query and a sweep axis all round-trip through it.

**Since age is an anchor, not a window.** A window ending at now still holds the launch
scramble while now is inside it; `m_crowd.buyer_count [age60s]` asks for buyers after it.
Its buyer set is capped at one more than the largest threshold any loaded condition names
under that anchor (`arm::anchor_cap`, computed at compile), so every operator stays exact
and a hot coin costs a handful of entries. Pair `buyer_count [age60s] = 5` with
`buyer_is_new [age60s] = 1` to fire once, on the arrival print.

## 3. Tags

A fingerprint's `tags` document (`fingerprints.tags`) names trade lists. Each tag splits
every coin's trades in two: `@name` carries it, `@!name` is the rest. `volume`, `dump`,
`targets` and `working` are names, not kinds: every tag is the same thing with different
matchers. Validated in full on save (`validate_tags`, every error named by its path) and
compiled once per reload (`compile_tags` into `EngineState::fp_tags`), never per event.

### The classifier

`TagState::fold_half`, one verdict per trade, in this order:

1. Off the tag's `side` (absent = both), the trade cannot carry it.
2. It carries the tag when ANY stateless matcher holds (`program`, `ix_shape`,
   `ix_template`, `ix_contains`, `ix_lacks`, `wallet`, `creator`) or its wallet is in the
   sticky set; else `cluster`, checked last because it counts every trade it reads into its
   slot group.
3. Else, under `exclude_creation_slot`, a wallet that buys in the creation slot, and every
   later trade of that wallet, counts on NEITHER side: the creation slot holds the dev's
   birth bundle and snipers, never the audience `@!tag` stands for.
4. Else the trade is the rest.

Both halves keep every total (`SplitTotals`: SOL, prints and transactions per side), so a
metric reads the same way whichever half a condition names, and `tag_share_pct @!tag` is the
rest's share. Excluded trades move no total on either half.

**A reload adopts an edited tag.** Trades already folded keep the half they were folded
under (the totals are running sums; no trade is retained to redo), and the sticky set is
kept: an edit moves a live coin's future, never its past.

### Matchers: what each is for

- **`ix_shape` carries variants or nothing.** It matches the hash of the whole ordered label
  list, so one instruction of difference is a complete miss. One launch bot ships with and
  without a trailing `System Program: Transfer` (the tip) and with `Associated Token: Create`
  vs `CreateIdempotent`: four sequences for one behaviour. A list holding some of them books
  the rest as `@!tag`, and an outsider gate then fires on bot traffic. Audit a list by
  variant, never by example.
- **`program` catches every build a tool compiles.** The head program
  (`template_grain::program_owned`: the first instruction past compute budget, system,
  token, associated-token and memo) keeps its name across builds; the 7ix crew's program
  ships ~25 sequences and keeps adding more, which an exact list books as the rest until
  someone adds them.
- **`ix_contains` / `ix_lacks` are mechanisms, not snapshots.** A marker is one bit the
  producer sets from a fixed vocabulary (`trade_keys::MARKERS`, served in the registry):
  machinery (`AdvanceNonceAccount`, `CreateAccountWithSeed`, `System Program: Transfer`,
  `Pump.Fun: Create`, `Memo Program`) and retail routers (`Axiom Trade`, `Photon`,
  `Bloom Router`, `Trojan Trade`, `Terminal`). Matching is substring containment per label,
  and an unknown marker is an error, never an empty mask. `CreateAccountWithSeed` stays true
  of every future throwaway-account build, where a list cannot: 531 distinct sequences carry
  it on the 08-01..08-21 tape.
- **The two marker matchers judge an unmarked build oppositely.** `ix_contains` tags what it
  names and leaves the rest unjudged; `ix_lacks` tags everything without a router, so
  `@!tag` is exactly the router-built trades. Say the one the rule means: on the 8dtx tape
  the same fires read +0.99 % a trade under `ix_contains: [CreateAccountWithSeed]` and
  +6.86 % under `ix_lacks: [<routers>]`, because the 8,566 fires the first admits and the
  second rejects average -0.68 %. `ix_lacks` fails closed: a loader that drops `ix_labels`
  tags every trade, so an `@!tag` gate fires nothing rather than everything.
- **Fee pins sit beside the hash, never in it.** An `ix_shape` entry may pin `cu_limit`,
  `cu_price`, `tip_lamports`; an absent field is a wildcard, and a pinned field never
  matches an absent reading, so a pinned entry matches nothing recorded before fee capture
  (core `0013`). Pin only constants: `cu_limit` is usually a preset and pins well, `cu_price`
  is often computed per transaction off a fee oracle (it then matches the one transaction it
  was copied from), a tip is an auction bid. Read the Budget column in flow discovery
  before pinning. A `cu_limit` above `MAX_TX_COMPUTE_UNITS` (1,400,000) is refused. Equality,
  never a band: urgency in money is a different quantity (see
  [ingest](../../arch/ingest.md)). Rule save warns on a pinned tag (`fee_pin_warning`).
- **`creator` and `sticky` are wallet terms.** A structural gate ("did this transaction come
  through a named router") turns both off: sticky makes it a property of the sender's
  history and `creator` adds an identity, so the fire set stops matching the one the rule
  was derived on (actor identity lives in machinery, [_!___strategy.md](_!___strategy.md)
  T5). A sell-side list (`dump`) never wants `sticky`: every later sell of a wallet that
  once sold a listed build would count. A `volume` tag converted from v1 carries both,
  because v1 defaulted them on.
- **`cluster`** tags a trade once it is the `min_prints`-th print in one slot with the same
  ix shape, side and fee preset, its SOL within `sol_tol_pct` of the group's first. Read as
  trades land, so the first `min_prints - 1` members stay the rest.

**Tags overlap freely.** A sell may carry `@volume` and `@dump` at once: two classifiers
agreeing on one transaction, not one trade counted twice. Nothing sums across tags, so read
them as two answers.

**Prints vs transactions.** Every leg of a transaction carries the same labels, so a shape
matches all of a bundle's legs or none. `sell_sol @dump` and `sell_count @dump` count every
leg (every leg moves the price); `sell_tx_count @dump` counts leg 0, so `sell_sol /
sell_tx_count` is SOL per transaction. The dump exit is `m_flow.sell_tx_count @dump [1sl] >= 2`:
two dump-built transactions in one slot. The `_tx_count` metrics require a tag: the
untagged flow window stays at 8 bytes per print with no transaction flag, because most
rules read it.

### Levels and built-ins

- **Trade level** (`m_flow`, `m_holdings.profit_sol`): one `TagState` per (fingerprint,
  tag), any matcher.
- **Template level** (`m_slot`, `m_wave`, `m_crowd.unique_ix_templates`): the slot and wave
  keep one buffer per coin and apply the tag when read, through its ix-template view
  (`TagPatterns::templates`), so only `ix_template` and `program` mean anything there, and
  `@!tag` is refused. A tag with neither reads `NaN`; rule save warns (`rule_tag_warning`).
- **Wallet classes** `@bundled` / `@public_app` (`m_holdings.bag_share_pct` only) need no
  config and may not be a fingerprint tag's name. The book is exact per wallet
  (`TradeLite::token_amount`, every leg, floored at zero); a print without an amount breaks
  it to `NaN`. A holder's class is fixed at its first buy, so a table reload moves no
  tracked coin's reading and bumps no `cross_epoch`. Public app reads the previous UTC
  day's build-breadth table (`holder_book::is_public_app`): more than `PUBLIC_MIN_BUYERS`
  buyers AND `app_buys >= PUBLIC_MIN_REPEAT x app_buyers`. The repeat test is what keeps
  the largest one-slot rug source private: a bot swarm buying through ~30 unnamed programs
  looks like a crowd app per program (~2,800 wallets) but its wallets do not come back
  (1.03-1.07 buys per wallet per program a day, against 2.9-16 on the named apps; DFlow
  runs 2.9-3.8, hence 2, not 3). The day-before read is what the engine can know; on the
  holdout it books the same clone the whole-tape count did (hot-tape case file,
  [hot-tape-rule-1.md](node-derivation/hot-tape-rule-1.md), L9/L11/L12). With a day table
  loaded, a replay's grid starts at 00:00 UTC of its first day, not the first print: a
  Python reference of a clock exit must use the same origin.

### Every consumer seeds the creator

The `creator` matcher (and, under `sticky`, the sticky set's origin) needs the coin's
creator wallet. A fold that skips it books the dev buy and the dev dump, a coin's two
largest flows, as the rest, and silently disagrees with the engine.

| consumer | seeds at |
| --- | --- |
| live engine | `reduce.rs`: `TokenCreated { creator_wallet_hash }` -> `TokenTrack::seed_creator` |
| simulate | `engine_sim.rs`: hashed from `tokens.creator_wallet` |
| sweep strategy | `sweep/generic/strategy.rs`: `series.seed_creator` |
| `/metric-series` | `metric_series.rs`: `series.seed_creator` |
| chart overlay | `classifyFlow.ts`: `FlowClassifyOptions.creatorWallet`, from `TokenTradeChart` |

Order is free: `ensure_tag` copies an already-seeded creator and `seed_creator` back-fills
existing states.

**The browser classifier is a preview, not the metric.**
[`classifyFlow.ts`](../../../frontend/src/shared/lib/flow/classifyFlow.ts) mirrors
`fold_half` so charts and the trades table redraw a tag edit without a round trip; both
sides read `engine/fixtures/flow_ix_parity.json` (`tags/state_tests.rs`,
`classifyFlow.parity.test.ts`). It folds a different corpus (PG-only
`/api/tokens/:mint/trades` vs the sealed lake + PG tail) in the chart's display unit, so
compare it to `m_flow.* @tag` only in SOL, and expect drift where PG retention dropped a
coin's early trades or a pre-V0 lake day null-fills `ix_labels` / `wallet`.

## 4. Where a read is computed

[`TokenTrack`](../../../engine/src/metrics/track.rs) is all the metric state one coin
carries and the router every read goes through (`TokenTrack::value`):

- one copy each of the coin-level states (`m_state`, lifetime price and flow, the wave),
  shared by every rule armed on the coin;
- trailing windows deduped by span, **one buffer family per subject** (flow, price, crowd
  wallets, ix shapes), so a rule pays only for the buffers its reads touch;
- one `TagState` per (fingerprint, tag) a loaded rule reads at the trade level, one
  template view per (fingerprint, tag) a slot or wave metric reads;
- opt-in maps opened only while a loaded rule reads them: the print wallet map, the holder
  book, the slot prefix.

**Registration is one walk.** [`Buffers::absorb`](../../../engine/src/metrics/buffers.rs)
maps each `MetricRef` to the buffer it needs. The rule compiler, the engine's per-coin
registration (`TrackRequirements`) and the sweep's series all derive from it, so a read can
never be registered on a buffer it does not read (a deque folded on every trade for nothing)
or miss the one it does (`NaN` forever, which reads like a strict gate). A buffer registered
mid-life knows only the trades after it; `TrackRequirements::adds_to` says when a reload
broke from-birth coverage.

**Separate buffers are separate obligations.** The crowd window's subject is the wallet
column; carrying wallet hashes on the flow deque made every `gross_sol` rule pay a second
push and a map entry per trade. All windows admit a trade through the one
`flow_window::is_foldable` guard, so `m_crowd.trades_per_wallet [W]` and
`m_flow.trade_count [W] / m_crowd.unique_wallets [W]` agree by construction.
`distinct_window::DistinctWindow` is the one two-ended distinct count, shared by the wallet
and ix-shape windows: a key leaves only when its last occurrence leaves, and a lagged
window's back end corrects from an inline scratch (a lagged read is never empty there, so a
heap map would allocate per read).

**A tag read is a cost on every coin.** Each (fingerprint, tag) any loaded rule reads opens
a `TagState` on every tracked coin and folds every trade; a tag nothing reads opens nothing
(`an_unread_tag_opens_no_state`). `EngineState::reload` narrows `fps` to the fingerprints
the rules name and compiles their tags once. Deriving patterns where they are used instead
re-walks the JSON and re-hashes every label sequence on the creation fast lane: measured at
461 us per `TokenCreated` with 115 fingerprint rows, 415 us of it with zero active rules.

**One condition walk, two sources.** `arm::CoinReads` reads the coin from the live
`TokenTrack` or one row of a precomputed `MetricSeries`
([`series.rs`](../../../engine/src/metrics/series.rs)), whose columns (`SeriesColumn` =
`MetricRef` + fingerprint) are track values sampled after each fold. The sweep scan
therefore decides exactly as the fold does.

**`m_position` reads our position, never the coin.** `PositionCtx` (entry, peak, trough,
entry time, stage start, entry depth) is `NaN` before a fill, so position metrics are sell
side only; the parser refuses them in `enter`, directly or through a signal. The peak and
trough seed at the fill (see [armed-trailing-stop.md](armed-trailing-stop.md)).

**A trailing-window read is O(1).** Windows keep running totals over a position-sorted
deque (`flow_window::push_sorted`) and correct only the two out-of-window ends on read. A
full-buffer rescan inside a `value()` is a hot-path regression; never assume the caller
evicted at `now` (`TokenCreated` / `FirstSlotSettled` do not, and a skipped tick leaves
entries un-evicted by design). Every windowed buffer needs an arm in BOTH
`TokenTrack::on_trade` and `TokenTrack::on_tick`: reads are exact either way, so a missing
tick arm shows only as retained state.

**`NaN` is the one "no reading".** Anything unregistered, unreadable or undefined reads
`NaN`, and `NaN` satisfies no condition:

| situation | reads |
| --- | --- |
| a tag the rule's fingerprint does not define | `NaN`; rule save warns (`rule_tag_warning`) |
| a template-level read of a tag with no `ix_template` / `program` | `NaN`; save warns |
| `m_position.*` before a fill | `NaN` |
| `m_print.*` on a tick | `NaN` (a rule on it never fires on a tick) |
| empty span for a ratio (`buy_share_pct`, `trades_per_wallet`, `slice_*`, `tag_share_pct`) | `NaN`; a `0` would pass `<= X` on a dead tape |
| `profit_sol` before a print with a reserve pair, or after a tagged print without a token amount | `NaN` |
| a trade with no labels | no `program`, `ix_shape`, `ix_template` match; `ix_lacks` tags it |
| a trade with no fee reading | matches no entry that pins a fee field |
| creator unseeded (no `tokens` row / `creator_wallet`) | `creator` matches nothing |
| pre-V0 sealed lake days (NULL `ix_labels`) | the rest at runtime; excluded from discovery score denominators |

## 5. Clocks: ticks and `ClockHorizons`

Almost every reading is a function of trade data alone. What moves on a bare tick is bounded
per rule by `arm::ClockHorizons` (computed at compile, unioned at reload into
`EngineState::tick_horizons`), which is what lets `reduce` skip a settled coin's ticks
without skipping a decision:

| field | bounds | from |
| --- | --- | --- |
| `max_window_secs` | trailing windows decaying (slot spans at `NOMINAL_SLOT_SECS`; a print span adds `0.0`, the exact horizon: nothing a tick does moves it) | the newest trade |
| `time_secs` | `m_state.age_sec` thresholds and `age_sec` stage deadlines | creation |
| `stall_secs` | `m_price.stall_sec` thresholds | the last all-time high |
| `held_secs` | `m_position.held_sec` thresholds and `held_sec` deadlines | the entry fill |
| `stage_secs` | `m_position.stage_sec` thresholds and `stage_sec` deadlines | the stage start |

A new metric that moves on a tick needs a field here; a new cross-token input bumps
`cross_epoch`. Mechanism, soundness and the measured ~180x:
[tick-cost-and-settled-tokens.md](tick-cost-and-settled-tokens.md).

## 6. Loader obligations

Offline, the lake omits columns a run did not ask for, and a fold over rows without them
looks like a strategy result rather than a load error. The answer lives on the read:

- `Metric::needs_wallet_identity(tagged)` / `MetricRef::needs_wallet_identity`: without the
  wallet column every trade is one anonymous wallet, and `unique_wallets >= 10` never fires.
  Any tagged read counts (a tag may use `wallet`, `creator` or `sticky`).
- `needs_ix_labels(tagged)`: slot, wave and holdings families, ix-shape counts, any tag.
- `Buffers::needs_slot`: a slot span, or the slot / wave families. Without the column
  `TradeLite::slot = 0` and every slot window is frozen.
- **Fold order is part of the answer.** Canonical order is slot -> tx_index -> leg; a
  loader ordering by timestamp gives print windows, clusters, sticky sets and the creation
  slot a different history from live (95 % of the money sits in same-slot pairs ~0.5 ms
  apart).
- The creator seed (section 3) and the fingerprint axes' primed tallies (section 10).

A new metric that reads a wallet or labels must be added to those two functions: a
wallet-keyed metric in an otherwise SOL-only family is exactly what a family-level list
misses.

## 7. Readings that look like one thing and mean another

None of these is derivable from a definition alone, and each has cost a search run.

| fact | what goes wrong without it |
| --- | --- |
| `m_state.liquidity_sol` is the REAL reserve (`vsol - 30` on the curve), 0 at an empty curve, ~85 at migration | a gate on the virtual 30/115 scale sits ~30 too high |
| `liquidity_sol` reads either venue (on the AMM, the pool's SOL) | a curve-derived upper bound also passes on a drained graduated pool; a curve-only rule adds `m_state.on_curve = 1` (replay carries no `Migrated` event) |
| `m_price.stall_sec` is seconds since the last all-time high, not the last trade | an exit below ~60 fires on ordinary chop; it caps every hold. `m_position.held_sec` is the time stop |
| `m_position.retrace_pct` with no gate is a hard stop from entry (the peak seeds at the fill) | reads as a trailing stop, behaves as a fixed one ([armed-trailing-stop.md](armed-trailing-stop.md)) |
| `m_position.room_taken_pct` is a share of the room to graduation: `room_taken_pct >= 40` is +13 % from vsol 100 and +68 % from vsol 70 | read as pnl it looks far; on a deep pool it is near. The entry depth rides `strategy_positions.extra`; a row without it reads `NaN` |
| `m_flow.buy_share_pct` is percent 0-100 | `>= 0.8` passes every coin |
| a lifetime monotonic read under an upper bound is a one-way door (`m_flow.trade_count <= 140`) | crossed once, the entry disarms as unsatisfiable (`MonoMetricKill`); that is what makes it a maturity gate |
| a windowed `gross_sol [W] >= X` implies `gross_sol >= X` (life) | stacking a lower lifetime floor under it is a no-op clause |
| `slice_*_share_pct` reads 100 on a coin younger than the slice | a true reading of a short life; a rule meaning maturity bounds `m_state.age_sec` itself |
| `m_crowd.unique_wallets` counts people, `m_flow.trade_count` counts prints | one wallet churning and a crowd arriving read alike in SOL; `trades_per_wallet` separates them and survives wallet rotation (a ratio, never an identity). `trade_count` needs no wallet column |
| `take_profit` / `stop_loss` sweep axes reject `null` | to test "none", omit the axis or pass an unreachable value (`1000` TP, `100` SL) |

**Where a lifetime floor earns its place.** A liveness floor is worth ~12.5 pp of mean PnL by
ablation on a broad universe (it holds the `Dead` exit rate down), but a windowed hot gate
risks selecting post-move moments created by the move it gates on, which the entry-timing
diagnostic (`family_search::gates`) catches. When it flags one, swap the windowed gate for
`m_flow.gross_sol >= 30` rather than drop liveness; the same holds for any entry whose
window gate points downward (a quiet-tape gate).

> **Refuted as a selection gate** on `fs3-00` (OOS 07-29..08-09): a crowd floor
> (`>= 20` replacing `gross_sol >= 45`) anti-selects monotonically: -0.75 %/ep against
> -1.22 at 40, -1.97 at 60, -2.04 at 80; stacked on the volume gate it is inert below ~30 or
> worse (-2.47 at 60, -3.94 at 100). At matched fire count the crowd gate beats the volume
> gate by 0.43 pp, inside the +-1.07 pp standard error. The metric stays as a capability; do
> not re-propose it as a selection gate on this family without new evidence.

## 8. Rules read metrics once, compiled

The rule grammar (`enter`, `signals`, `always`, `stages`, lines) lives in
[`rule_params.rs`](../../../engine/src/rule_params.rs) and its parts are defined in the
registry (`RULE_PARTS`). What matters to a metric:

- `enter.event`, `enter.filters` and `enter.final_filters` are each AND; a line's `if` is
  AND; OR is two lines or a signal (OR of AND-groups). Adding a sell line can only make an
  exit fire earlier.
- `CompiledRule::compile` resolves every condition to a `MetricReq` with a dense `read_id`
  once; nothing is looked up by name per event. The same pass fills the rule's `Buffers`,
  its `ClockHorizons` and its monotonic entry kills.
- Held side: one step per print or 200 ms tick, the first line that holds acts; a stage move
  takes effect from the next evaluation, a partial sell moves when its fill lands
  ([partial-exits.md](partial-exits.md)).

## 9. Extending the system

| adding | touches | enforced by |
| --- | --- | --- |
| a metric | a `Metric` variant in family order + its `METRICS` row (name ending in its unit, phrase, summary, example with a number, note, tags, tag level, spans, monotonic, `=` band, hue); the compute arm in its family's module, routed by `TokenTrack::value`; a `Buffers::absorb` arm if it needs a buffer; `needs_wallet_identity` / `needs_ix_labels`; a `ClockHorizons` field if it moves on a tick; regenerate `fixtures/registry.json` | `metric_order_matches_the_enum`, `every_name_ends_in_its_unit`, `every_metric_is_explained_with_an_example`, `registry_fixture_is_current`, `every_metric_is_live_reachable` |
| a buffer (a new windowed state) | a `TokenTrack` field with `ensure_*`, arms in both `on_trade` and `on_tick`, a list on `Buffers` (+ `union`, `ensure_on`) and in `TrackRequirements::adds_to` | the O(1) and brute-force equivalence tests of its module |
| a family | a `Family` variant + `FAMILIES` row, a compute module | the exhaustive matches in `TokenTrack::value` and `Buffers::absorb` (compile-time) |
| a tag matcher or option | a `TAG_FIELDS` row, a `parse_tag` arm, a `TagPatterns` field, the check in `TagState::matches` (stateless) or `fold_half` (stateful, like `cluster`), and the mirror in `classifyFlow.ts` + `tagsDoc.ts` | `the_documented_vocabulary_is_the_parsed_one`, the shared `flow_ix_parity.json` fixture |
| a span kind | `Span` / `SpanUse` / `span_kinds_json`, `check_allowed`, `Buffers::absorb`, `ClockHorizons::absorb_req`, `chart_reads` | `every_metric_is_live_reachable` (its span list), `spans_round_trip` |
| a fingerprint axis | one `AxisDef` + one reader arm ([fingerprint-ranges.md](fingerprint-ranges.md)) | the axis registry's guards |

**Add the smallest thing that carries the finding**: a metric before a buffer, a buffer
before a family, a family only for a subject that has none. Adding tag or span support to an
existing metric is its registry row plus its compute arm.

**Every registered metric is reachable, or it is a gate that never fires.** Each compute
module's `value` ends in `_ => f64::NAN`, so a metric in `METRICS` whose arm was never
written reads `NaN` forever: no panic, no failing test. `every_metric_is_live_reachable`
([test](../../../engine/tests/every_metric_is_live_reachable.rs)) drives every metric, on
every tag level and span kind it accepts, through `EngineState` + `reduce` +
`readout::read_state` over a probe stream and asserts a finite reading. It walks the
registry, so a new metric is covered without touching it; a new tag level or span kind
needs its probe taught one variant.

**Colour.** Each metric's `hue` is its chart and chip colour; `m_flow.buy_sol` /
`sell_sol` are pinned to the candle up/down hues (`CANDLE_UP_HUE` / `CANDLE_DOWN_HUE`). A
new metric takes a hue inside its family's band.

**The v1 converter is permanent and closed.** [`v1.rs`](../../../engine/src/v1.rs) reads
every document written before v2 (rule params, fingerprint `metric_config` and criteria, a
v1 rule bundle, a stored sweep's axes, ix patterns and ladders) into v2 exactly;
`parse_params_any` is the entry point for anything that may still hold v1 (bundle import,
lab combo params, the golden tests' pinned rules). `map_metric` is the old-to-new name table.
It never learns a new metric: v1 is a frozen vocabulary. Historical text stays as written
(`strategy_positions.exit_reason`, `strategy_arms.end_detail`), and the UI shows old labels
raw.

## 10. Fingerprint axes next to metrics

**The test is when a fact can change.** `age_sec` moves every tick, `liquidity_sol` on
every trade. A fact fixed at creation selects WHICH coins a rule arms on, never when it
fires, which is what a fingerprint axis is for. The creation-slot buy total is therefore the
`first_slot_buy_lamports` axis, not a metric: an `AxisPredicate` range
(`{"kind": "range", "min": "6410000000"}` is `>= 6.41 SOL`) expresses strictly more than a
condition list. It is deferred (summed from the creation slot's trades): a fingerprint
configuring it holds the arm at `PendingFirstSlot` until `FirstSlotSettled`, and an unknown
value fails the axis, so an unscreened coin never arms.

Two axes are engine tallies rather than columns, with the same load-time hazard as
`needs_wallet_identity`: the value depends on data the loader may not have asked for, and
the failure looks like a strict gate.

| axis | fact | why |
| --- | --- | --- |
| `prior_launches` | `0` is a real value; unknown is `NaN` | seeding `0` without a creator would widen `= 0` to every coin whose creator the feed failed to resolve |
| | the tally must be primed | a fresh process reads every creator as new: `EngineState::prime_creator_launches`, live from `TokenRepo::creator_launch_counts` at boot, simulate over `[corpus_start - 30d, corpus_start)` |
| | the window is part of the rule | every threshold is denominated in `PRIOR_LAUNCH_WINDOW_DAYS` (30); widening it re-scales every authored condition |
| | unavailable on lake-corpus paths | the lake's tokens dimension has no creator, so grouped sweep, rule search and family search leave it `None` (`lab/src/lake/duck.rs`), which fails a configured axis closed. Use simulate |
| `name_reuse_count` | one counter, `fingerprint::identity_launches::IdentityLaunches` | the engine stamps it at `TokenCreated`; simulate's candidate scan stamps from one built off `tokens` (`fingerprint_axes::stamp_name_reuse_count`), so scan and replay agree |
| | named builds only | a fingerprint reading it must also pin `ix_labels`; the tally holds those builds' launches only |
| | primed with timestamps on both paths | simulate primes every creation of the build over `[since - 30d, until)` (`engine_sim::load_identity_rows`); live primes the last 30 days once per build, on the reload that first names it |
| | the dashboard mirror is SQL | `axis_num_sql` counts the same window with a regex twin of `identity::token_identity_hash`'s normalization; `[[:space:]]` and `char::is_whitespace` can differ on exotic whitespace |

## 11. The hashers are one set

[`trade_keys`](../../../engine/src/metrics/trade_keys.rs) (`ix_hash`, `ix_hash_opt`,
`build_hash`, `wallet_hash`, `marker_bits`, `ix_hash_from_labels_json`) is the only place a
trade's identity is hashed. Every adapter (live producer, lake replay, event log, readout)
calls it, and tags compile their lists to hash sets at reload. No interner, so replay parity
holds by construction. `build_hash` (the ix-shape key of `m_crowd.unique_ix_shapes`) drops
account setup, teardown and memos; it partitions label sequences exactly like the toolkit's
`lake_export.build_core` (`build_hash_partitions_like_the_study_build_core`, and its
`--ignored` twin over a whole lake export).

## 12. Flow discovery: authoring a tag from the tape

`lab/src/strategies/flow_discovery.rs` + `POST /api/strategies/flow-discovery` partitions a
corpus by sweep `GroupKey` and scores each distinct ix structure (`volume_share`,
`wash_symmetry`, `cross_token_recurrence`, `group_lift`, `slot_burst`, `wallet_reuse`,
`wallet_overlap`, `first_slot_gross_sol` / `first_slot_trades`). Apply adds the checked
shapes to a fingerprint tag (`ix_shape`), or binds a new fingerprint promote-style. The
rules the page stands on:

- **`lift_defined`.** `group_lift`'s denominator is the structure's share of the whole
  scored corpus; when the group IS the corpus (a fingerprint-scoped run, one `ALL` group)
  every structure scores `1.0`. Readers skip the lift gate when `lift_defined` is false,
  never fail it, and the Lift column renders `-` (locked by
  `whole_corpus_group_reports_lift_undefined`).
- **The `Auto` composite** (`flowDiscoverySuggest.ts`): the number is the decision
  (`score >= SUGGEST_SCORE`); correlated columns count once (a mean over the families
  Recur, Burst, Wallets, Wash, so one launch bundle tripping two columns is not two votes);
  the verdict never depends on click order (contagion% is display only); small samples
  do not vote (`wallet_reuse` below `SUGGEST_MIN_REUSE_TRADES`). Gates: dust floor,
  `SUGGEST_MIN_TOKENS`, and lift when defined; `suggestExplain` shows every family.
- **Launch presence is not launch purity.** `first_slot_gross_sol / gross_sol` (Launch%)
  is purity, for sorting; `first_slot_trades > 0` (`isFirstSlotPresent`) is presence, the
  *Launch shapes* button's test, with no dust floor and no lift gate (presence is an
  identity claim). An `ix_shape` matcher has no slot predicate, so a checked shape that also
  trades organically tags that tail too, and `sticky` then sweeps those wallets' other
  trades in: the button is a proposal; read Launch% and uncheck mixed rows.
- **The button gates on the corpus, not the draft.** The draft re-seeds from the target
  tag on every run (`seedFromFingerprint`), so a re-run over a new window can have an empty
  diff and a full launch set; `disabled` reads the whole set, the click adds the unchecked
  ones, the hover outlines the full set.
- **Per-token launch set.** The group aggregate cannot say what was in THIS coin's launch:
  it sums every member's creation slot, is cut at `max_structures_per_group` (64), and only
  scores trade rows. `TokenGross.first_slot_ix_labels` carries every distinct shape that
  traded in that coin's creation slot, uncapped and unfloored, ranked by first-slot gross.
- **Unknown is not zero.** `first_slot_trades` / `first_slot_ix_labels` are `Option` on the
  wire; a result cached before a field existed reads `-` and selects nothing.
- **The result carries its own corpus identity.** `DiscoveryResult` echoes `plan`,
  `ix_labels_filter` and `fingerprint_id`; the page rebuilds fingerprint identity from
  these, never its form state (a cached result is routinely an earlier session's), and
  re-attaches the label filter before `bind_flow_discovery` (`withIxLabelsFilter`), which
  otherwise drops the `ix_labels` axis and fires on every coin shape.
- The creation slot offline is `lab::sweep::projection::creation_slot`, the slot of the
  coin's first trade, shared with replay's `FirstSlotSettled`; a coin whose creation slot
  saw no trade reports its first later slot.

## 13. Deliberately not built

- **Cross-coin sticky sets** (a wallet tagged on coin A pre-tagged on coin B of the same
  fingerprint): needs a bounded, log-replayable set in `EngineState`; one false tag poisons
  a whole group. Build only once rotation demonstrably defeats per-coin `sticky`.
- **Since-entry anchors** (flow since our fill, since the creator's first sell): new spans,
  no structural change.
- **Transfer ingestion** for direct wallet linking: an expensive ingest feature, only if the
  wallet and program proxies demonstrably fail.
- **Discovery auto-promote**: blocked on a hand-label kit; even then review-then-apply stays
  the default and auto-promote an opt-in mode, never a background job.
