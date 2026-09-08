# Market model and workflow

The foundation. How this market works, who acts in it and why, what a rule is, how a
rule is measured, and the workflow that turns the actor model into money.

Self-contained on purpose: readable with no other file and portable to another
project. Every market fact carries a source (section 8). A number measured on this
bot's own tape is marked **measured here** - it is evidence for a law, never the law.
Section 9 is the only part that names this repository; the rest travels.

```
0. How to read this file
1. The machine            what pump.fun is: curve, fees, Solana, feeds, actors, base rates
2. The causal chain       the ten theses, in the order cause runs
3. The readers            who is profitable, and the one question they all answer
4. Anatomy of a rule      Event, Door, Permissions, Exit, Re-entry, Size
5. Laws                   physics, the seat, cost, measurement honesty, the objective
6. The workflow           census -> dev model -> readers -> story -> money -> engine -> live
7. Closed mistakes        one table, generalized
8. Sources
9. Appendix               this repository: docs, vocabulary, tables, verdict status
```

---

## 0. How to read this file

- Sections 1-2 are the **why**: what is happening under the hood, in the dev's head and
  the reader's head. Nothing downstream is allowed to contradict them.
- Sections 3-4 are the **what**: the shapes a profitable decision takes.
- Section 5 is the **how honestly**: a result that breaks a law here is not a result.
- Section 6 is the **in what order**.
- Section 7 is the list of ways this work has gone wrong, stated as rules.

A sentence in this file is either a sourced market fact, a thesis (a causal claim that
every rule must be consistent with), a law (a measurement rule that every number must
obey), or a measured-here observation. The type is always visible from context.

---

## 1. The machine

### 1.1 The bonding curve

Every pump.fun token launches on a constant-product bonding curve, `x * y = k`, with
`x` the virtual SOL reserve and `y` the virtual token reserve. The curve opens with
**about 30 virtual SOL and 1.073 billion virtual tokens**, so the opening price is
about 0.000000028 SOL per token. When the curve has taken in **about 85 real SOL**
(historically about $69k of market cap) the token **graduates**: the program withdraws
the SOL, takes a graduation fee and seeds a pool on **PumpSwap**, pump.fun's own AMM
(Raydium before March 2025). ([DEXTools](https://www.dextools.io/tutorials/what-is-pump-fun-solana-memecoin-launchpad-2026),
[Flashift](https://flashift.app/blog/bonding-curves-pump-fun-meme-coin-launches/),
[SolTokenCreator](https://www.soltokencreator.io/blog/pump-fun-graduation-explained))

Consequences that every later section uses:

- **Price is a function of the SOL reserve alone.** With `vsol * vtok = k`,
  `price = vsol / vtok = vsol^2 / k`. Price moves only when someone trades, by exactly
  the SOL that trade moves.
- **Graduation is a wall at `vsol ~= 115`** (30 virtual + 85 real). Measured here: the
  curve constant is exact on every print, `k = vsol * vtok = 3.219e16` (SOL x raw
  units), and the wall sits at 114.9-115.0.
- **Headroom is quadratic.** The gross a position can ever make is
  `(vsol_max / vsol_entry)^2 - 1`: from `vsol` 55 to the wall is +337 %, from 75 it is
  +135 %, from 100 it is +32 %.
- **A silent token keeps its price.** No prints means no reserve change; the exit fills
  at the last reserve state, not at zero. The curve is always the counterparty, so an
  exit never needs a later print.

### 1.2 Fees, and where the dev's money comes from

- **Curve trades pay 1.25 % total**: 0.30 % to the token's creator, 0.95 % to the
  protocol. Measured here: the curve-side amount is `gross * 10000 / 10125` on 16,544
  of 16,854 round dev buys - 125 bps, not 100.
  ([Froglabs](https://froglabs.io/blog/pump-fun-fees-explained/),
  [SolTokenCreator](https://www.soltokencreator.io/blog/pump-fun-fees-explained))
- **Creators earn a revenue share on every trade of their coin** (since May 2025), on
  the curve and after graduation. The share is **dynamic by market-cap tier, 0.05-0.95 %
  per trade**, and the highest tier sits **just past graduation** (roughly the $88k-$300k
  band). After graduation the pool's total fee itself declines with market cap, from
  1.25 % at the bottom tier toward 0.30 % at the top.
  ([CoinDesk](https://www.coindesk.com/markets/2025/05/13/pumpfun-launches-revenue-sharing-for-coin-creators-in-push-to-incentivize-long-term-activity),
  [CoinMarketCap](https://coinmarketcap.com/academy/article/pumpfun-creators-earn-dollar2m-in-first-day-under-new-fee-structure),
  [Yahoo Finance](https://finance.yahoo.com/news/pump-fun-fee-model-hands-125849600.html),
  [pump.fun fee docs](https://pump.fun/docs/fees))
- Fees are claimable from a dashboard, shareable across wallets, and assignable to a
  community-takeover admin.

So the dev has two revenue lines: **selling supply he bought cheap** (his bundle) and
**fee income that peaks once the token clears the wall and keeps churning**. Graduation
is not a milestone for him; it is where his revenue rate is highest. Both lines need the
same input: transactions from other people.

### 1.3 Solana execution: slots, ordering, tips

- A slot is **about 400 ms**. Transactions landing in one slot confirm together.
- **Order inside a block is the leader's.** A priority fee raises the chance of
  inclusion and improves ordering on standard submission, but the scheduler makes a
  loose guarantee, not a strict one, and an account contended across execution threads
  is ordered unpredictably. ([Solana proposal](https://github.com/solana-labs/solana/blob/master/docs/src/proposals/fee_transaction_priority.md),
  [RPC Fast](https://rpcfast.com/blog/solana-transaction-fees-explained),
  [AllenHark](https://allenhark.com/blog/why-solana-transactions-late-priority-fees))
- **Jito bundles** are atomic groups of up to five transactions, executed in order in one
  slot; the block engine ranks bundles by tip. More than 95 % of stake runs the
  Jito-Solana client and tips are over 60 % of all priority-fee volume, so **position
  inside a slot is a tip auction**, not a latency race.
  ([Chainstack](https://chainstack.com/jito-explained-bundles-tips-mev-solana/),
  [RPC Fast](https://rpcfast.com/blog/jito-explained-bundles-tips-mev-solana))
- Nothing sequences between the legs of a bundle. A price that exists only between two
  bundled buys is not a price anyone else can trade.

### 1.4 Discovery: feeds, filters, the safety panel

Retail does not find tokens on chain. It finds them in **feeds**, and feeds rank by
**transaction count, recency, unique buyers and volume**. One transaction moves a token
to the top of "recently traded" instantly. Attention is therefore **purchasable with
transactions**, and attention converts into retail flow.
([OpenLiquid](https://openliquid.io/tools/pump-fun-volume-bot/),
[Alphecca](https://alphecca.io/en/blog/pump-fun-volume-bot),
[OpenPR](https://www.openpr.com/news/4398886/the-silent-algorithm-behind-pump-fun-why-2026-is-the-year-of))

Terminals stage discovery by curve position - **New Pairs -> Final Stretch (about to
graduate) -> Migrated** - and let a trader filter on holder concentration, dev holdings,
sniper count, insider bundles, liquidity, volume, transaction count and pro-trader
presence. Their **safety panel** shows the things a human reads before buying: bundle
share of supply, dev holding, insider holding, dev funding source, fresh-wallet buys,
bot detection. ([Axiom docs](https://docs.axiom.trade/axiom/finding-tokens/pulse),
[Axiompedia](https://axiompedia.com/guides/trading/axiom-pulse-explained),
[Terminalpedia](https://terminalpedia.com/guides/trading/terminal-trenches-guide))

Everything on that panel is a claim about **who holds the supply and who is buying** -
never about the price path. That is what a reader reads.

### 1.5 Actors and their equipment

| actor | what he wants | his equipment | his fingerprint on the tape |
| --- | --- | --- | --- |
| **Dev cohort** (creator + bundle wallets + volume-bot wallets) | sell supply, farm fees, reach the wall | bundler (launch + up to 16-25 pre-buys), volume bot in **bump** mode (many tiny buys, games frequency ranking) or **volume** mode (fewer larger buys, games volume ranking), fee/CU presets, wallet rotation, KOL calls | launch build, bundle build, volume build with rotating wallets and a fixed clip, and the sell build that pairs with them |
| **Snipers** | the first price on the curve | 2-5 ms infrastructure, multi-relay, dynamic tips, pre-signed durable nonces | creation-slot buys; `CreateAccountWithSeed` / `AdvanceNonce` builds |
| **Copy / tracker bots** | whatever a labelled "smart" wallet does | leaderboard feeds | swarms landing 0-1 slot behind a tracked wallet |
| **Readers** (the daily-profitable population) | the next flow before it lands | a terminal or a private bot, one node each | small clips, one shot or re-entries, exits into strength |
| **Retail** | the story | the feed, a KOL, a livestream | named retail routers, no speed markers |

([Smithii bundler](https://smithii.io/en/pump-fun-bundler-bot/),
[cicere bundler](https://github.com/cicere/pumpfun-bundler),
[OpenLiquid](https://openliquid.io/tools/pump-fun-volume-bot/),
[jumpbit bump bot](https://jumpbit.io/en/solana/pumpfun-tools/pumpfun-bump-bot),
[RPC Fast snipers](https://rpcfast.com/blog/how-to-launches-snipe-pump))

A campaign is staged: **bump first to enter the feeds, volume once organic buyers
arrive** ([Alphecca](https://alphecca.io/en/blog/pump-fun-volume-bot)).

### 1.6 Base rates

- 8,000-15,000 tokens launch per day; **under 2 % graduate**.
  ([Cryptopolitan](https://www.cryptopolitan.com/pump-fun-graduating-tokens-break-to-1-15-of-new-launches/))
- Wash trading is the most common manipulation (74.8 % of flagged cases), run by tiny
  coordinated groups (median 2.8 actors, half by one actor), recurring on the same token
  about 3.6 times. **62.9 % of extraction events (pump-and-dump, rug) follow a
  visibility-building operation on the same token.** Manipulation is staged: attention
  first, extraction second.
  ([arXiv 2507.01963](https://arxiv.org/html/2507.01963v1))
- Most of the volume is bots, and most launches show pump-and-dump or rug-consistent
  patterns. ([AssureDefi](https://www.assuredefi.com/blog/meme-coin-rug-pulls-pump-dumps-how-to-spot-and-prevent-fraud),
  [Webopedia](https://www.webopedia.com/crypto/learn/pump-fun-scam-tactics/))
- Measured here: over a rolling week, about 0.2 % of active wallets are profitable
  every day and take about a third of everyone else's losses; roughly half of those are
  launch operators trading their own creations.

---

## 2. The causal chain and the theses

```
  dev cohort has a PLAN and a BUDGET
       |
       |  every decision is a transaction through a TOOL
       v
  the tape carries a FINGERPRINT for each decision   (launch / bundle / volume / sell build)
       |
       |  price absorbs the SOL of each print the instant it lands
       v
  price contains the FLOW ...............  but NOT WHO, how many, or whether the cohort is still in
       |
       |  that unpriced state is what predicts the NEXT flow
       v
  a READER predicts remaining intent from unpriced state
       |
       |  every print is a decision point; he fires on the first print after which his condition holds
       v
  he SELLS INTO the flow he predicted
       |
       |  our seat: 115 ms decision-to-fill, the trigger's own slot half the time
       v
  we take the same decision at the same print, and pay the direction-dependent latency cost
```

**T1 - Nearly every price move is manufactured.** Someone pays for it, with a goal and a
budget. The tape is a record of campaigns, not opinions. Reading it means asking who is
paying and what he wants.

**T2 - The core business loop is transactions -> feed placement -> retail attention ->
volume -> creator fees + supply sales.** The dev is the entrepreneur of this loop.
Everyone else services it (volume bots), parasitizes it (snipers, readers) or funds it
(retail).

**T3 - Exploitation is staged: visibility first, extraction second.** A token's life is
a sequence of phases - launch -> scramble -> (abandon | campaign) -> attention ->
(push | extract) -> wall -> fees. Money sits at the phase transitions, because that is
when the next flow becomes predictable.

**T4 - Every dev decision is visible as a fingerprinted transaction.** A stall is a
decision point (abandon, dump, or spend on a campaign), and what breaks the silence
says what was decided. It is the cleanest decision node, not the only one: attention
arriving on a busy tape, a pullback while the cohort still holds, and a push toward the
wall are decision nodes too, and none of them has a gap near it. The gap by itself
means nothing - most gaps precede death; the machine that acts is the information.

**T5 - Actors are their machinery, and machinery is the only axis that exists.** Wallets
rotate daily; addresses and funding graphs are hindsight, off the tape, and defeated by
any careful operator. Instruction structure, compute-budget order, account-creation
pattern, fee preset and clip size persist, and they are on the very print being
decided on. The dev cohort is therefore observable only as its builds: the launch
build, the bundle build, the volume build with its rotating wallets, and the sell build
that pairs with them. A wallet is the subject of a study, never a term in a rule.

**T6 - Price contains the flow, but not who.** The SOL of every print is in `vsol` the
instant it lands, so any price-path or flow-window condition reads spent information.
What `vsol` does not contain: the machine class behind the last prints, how many
independent machines are acting, whether the machine that pushed this token has started
selling it, and where the token sits in the feeds. That **unpriced state** is the only
thing that predicts the next flow. Every rule that reads priced state alone dies; every
rule that survives carries an unpriced term.

**T7 - Profit on a curve comes only from flow that has not landed, and that flow exists
only because someone still intends to spend.** A rule is a claim about an actor's
remaining plan - "this machine has decided to push and has not begun to extract" -
never a claim about the price path.

**T8 - Readers are derivable.** A daily-profitable trader runs a decision procedure over
public data: at every print he evaluates the state, fires when his condition first
holds, and sells into the flow he predicted. His procedure consumes only what the tape
shows, so it can be reconstructed. A failed reconstruction indicts the hypothesis, never
the existence of the logic.

**T9 - The seat decides reachability, and direction decides its cost.** Decision to
fill is about 115 ms against a 400 ms slot; we land in the trigger's own slot half the
time. A chosen position inside a slot is not purchasable with speed. Buying into a
falling price and selling into a rising one are nearly free at this seat; buying a
breakout and selling on a stop pay for every millisecond. An edge consumed within
~115 ms of its trigger print is closed to us however well it pays its owner; an edge
that develops over slots and minutes is fully open.

**T10 - Everything rots; the pipeline is the asset.** Fee rules change, tools update,
launch clients rotate, metas turn over in weeks. No rule is an asset. The census and the
dev model that re-derive rules from the actor model are, and their refresh is part of
the system, not maintenance.

---

## 3. The readers: who is profitable, and why

### 3.1 The one question

Every profitable reader answers the same question with a different actor:

> **Whose money is still coming, and what on the tape says so?**

Each distinct answer is a **decision node**. Entry age, tape density, pullback versus
strength, repeat entry and holding time are the observable shadows of the answer, which
is why grouping readers by habit recovers nodes rather than styles.

### 3.2 The nodes

| node | the answer | dev-side reading | event class | exit shape | latency sign | open to us? |
| --- | --- | --- | --- | --- | --- | --- |
| **Launch insider** | "I am the launch" | is the cohort | creation slot | sells into the scramble | none - he is first | **no**: 92 % of entries at or within one slot of creation (measured here) |
| **Launch reactor** | "snipers and retail chase every launch for a few seconds" | none - he races | first prints after creation | 3 s | racing | **no**: consumed inside ~2 slots |
| **Crowd continuation** | "the feed just put this in front of strangers; they keep arriving" | attention has arrived, cohort still pushing | strength on the busiest tape, ~15 s in | scales out over ~40 s | entry pays | open where the arrival lasts slots, not milliseconds |
| **Attention arrival** (mid-tape) | "several independent machines just bought in one slot on a curve with headroom" | real attention, not one bot; budget still being spent | multi-tool slot, real reserve small | one shot; cut in seconds on no follow-through, ride minutes on follow-through | entry pays modestly | open |
| **Re-entry dip scalper** | "the crowd is here, the cohort is still in, this flush is impatience; the next wave restores" | no extraction yet, feed placement intact | a sell flush reaching depth on a hot, still-rising token | sells into the next wave; re-enters on the next flush, repeatedly | **both legs favourable** | open by construction; thin per ticket |
| **Campaign rider** | "the machine that pushes this token restarted after silence; he decided to spend" | campaign decision made, extraction not yet visible | one campaign build breaks a silence | ride minutes; leave on the extraction tell or at the wall | entry ~free (silence), exit into strength | open; validated once here |

Measured here, on a week of daily-profitable readers (levels are tautological - the
population is selected for profit - only the ranking carries): crowd continuation runs
the widest margin on spend, the two launch nodes are closed, and the scalper node runs
the thinnest margin while trading ~13x more often. Wallets that take exactly one buy and
one sell per round trip on 95 %+ of their trades earn about an eighth of the margin of
wallets that scale out. **Every node has a negative median round trip**: all of them are
convexity harvesters carried by a tail, so a take-profit inverts every one of them.

### 3.3 What a reader is for

A reader is an **instrument**. He tells which decision node exists, which machine
signatures carry follow-through, and which ones a professional declines. He is never
the rule, and copying him is closed at every fill: his own buy moves the curve by
`buy / vsol`, the swarm that follows him lands inside his slot, and both are in the
price before a copier can act.

To read him honestly: locate his **decision** print, which sits at least his own
reaction time before his fill (his build tells his speed class; his slot-lag histogram
tells his reaction). Anything that lands inside his reaction time is co-arrival, not
trigger. His own prints stay out of every candidate pool. His response rate to a
candidate event **names** the event; it is never a gate and never money.

---

## 4. Anatomy of a rule

A rule is one causal sentence, then four machine terms plus two policies. The sentence
comes first; a term without a reason is forbidden.

```
  STORY        who acts, why now, why flow CONTINUES after our entry

  EVENT        the first print after which a condition on UNPRICED state holds
               (machine class of the run, count of independent machines, cohort state,
                position in the token's life). Wallet-free. We fire on that print.
  DOOR         which tokens we watch at all: creation-time and machine facts
               (launch build, creator initial-buy band, campaign machine present)
  PERMISSIONS  state that is already true at the event, or we skip
               (age, depth and headroom, phase, cohort still in, not at the high,
                pre-tape not one-sided, tape density)
  EXIT         the harvest shape, from the story, settled after the gates
  RE-ENTRY     open again on the next qualifying event on the same token
  SIZE         near sqrt(F * vsol), or a fixed fraction of vsol
```

- **The event is not "he bought."** It is what anyone watching the tape sees. It is
  named by machine build, count and size over the run inside the slot - never by wallet,
  never by exact hash, never by "anything in front of him".
- **The door is not the trigger.** A token that fails the door stays off; one that passes
  is still waiting for the event. The cohort's state on the token is a door and a
  permission, and it is expressed as machinery: the machine that pushed this token has
  or has not begun selling it.
- **Permissions are not the trigger.** They are already true when the event prints.
- **The exit is chosen from the story, after the gates.** The unfiltered pool is mostly
  dying tokens, so any exit swept on it returns the shortest clock.
- **Re-entry is unlimited** until a budget caps trade count. Each qualifying event on the
  same token is a fresh ticket.

Two exit laws exist, and a book runs on exactly one:

| book | law | shapes that satisfy it |
| --- | --- | --- |
| **Harvester** (primary) | never cap the right tail; abandon fast when follow-through fails | armed trail (fires from strength), cause-based (flow reversal before price turns), clock as the unbiased fallback |
| **Scalper** (alternate) | sell into the wave you predicted, then re-enter on the next flush | small target resolved on prints, or a reaction to the next up-wave |

A take-profit on a harvester book caps the trades that carry it (measured here: 2-8 %
of trades carry 56-77 % of the profit). A wide trail on a scalper book gives back the
wave. The two do not mix.

---

## 5. Laws

### 5.1 Physics of the curve

- `price = vsol^2 / k`, exactly. Reserve-space and price-space rules are one policy.
- `vsol` on a print is the reserve **after** that trade. The state a transaction meets at
  time `T` is the last print at or before `T`.
- Liquidating a bag of `B` tokens against reserve `V` returns `V * B / (k / V + B)`,
  never `B * price`.
- **Silence freezes price.** A token with no prints during the hold exits at the entry
  reserve less the toll. Booking it at -100 % invents a loss the curve cannot produce,
  and it always flatters gates that select for activity.
- **An exit needs no print.** A clock exit fills against the last observed state. The
  migration case is the one exception, and it is a fraction of a percent of tokens.
- **Windowed flow on a curve is the price move over that window** (correlation ~0.98,
  measured here). Net flow, buy share and their multi-window conjunctions add nothing to
  a price-shape condition; only counts and machine identity are orthogonal to price.

### 5.2 The seat

Measured here, on real fills:

| quantity | value |
| --- | --- |
| decision to own-fill-observed | p50 **115 ms**, p90 228, p99 513 (send path 8 ms, ack to fill 107 ms) |
| `entry_slot - target_slot` | p50 **0**; 52.6 % in the trigger's own slot, 81.6 % within one, 93.4 % within two |
| what explains the spread | trigger staleness, 98.7 % of variance; chain load is not a factor |
| P(same slot) by reaction time | < 50 ms 84 %, 50-100 ms 75 %, 100-200 ms 54 %, >= 200 ms 20 % |

Laws that follow:

- **The fill is the last print that has landed by `fire + 115 ms`, on both legs.** Never
  the first print at or after the deadline - that is a trade that landed after us, and
  pricing from it is a one-print look-ahead worth +8 to +12 pp per trade, concentrated on
  exits into strength. When nothing lands inside the lag, the fill is the trigger's own
  state.
- **A latency correction applied to one leg is not a latency correction.** The exit
  reaction is the same reaction as the entry.
- **Landing in the trigger's slot is reachable; a chosen position inside it is not.**
  Slot models that drop the trigger's slot are pessimistic by half a slot; models that
  take the very next print assume an ordering privilege no latency buys. Neither is the
  verdict.
- **Direction sets the sign of latency cost** (measured here, fill / trigger price):

| action | price moves | cost |
| --- | --- | --- |
| buy into a flush | toward you | 0.89-0.94 - the lag pays you |
| sell into strength (target) | toward you | 1.023-1.027 - the lag pays you |
| sell on a stop or trail | away from you | 0.964-0.977 - fills past the level |
| buy a breakout | away from you | about +8-10 % of entry per slot |

  So a stop is adversely selected by construction - the trade that triggers it is the
  leading edge of a move that keeps going. A clock is unbiased. Only "buy weakness,
  sell strength" is robust on both legs.

- **A fill model changes which terms matter, not only the level.** A term that is worth
  nothing at an in-slot fill (price already off its high) is the difference between a
  book and nothing at 115 ms; a term that dominates in-slot (burst purity) is modest at
  115 ms. **Search at the fill you ship at.** A rule derived at one fill and shipped at
  another is a different rule.
- **Do not optimise the send path.** 8 ms against a 386 ms slot. The lever is rule
  shape: a rule that stamps a stale trigger cannot land near it.

### 5.3 Cost

| term | value (measured here) | who charges it |
| --- | --- | --- |
| protocol + creator fee | 125 bps per leg | pump.fun |
| tip + priority | fixed SOL per leg, from the environment (0.000225 at a 0.0002 tip) | Jito + validator |
| own impact | `B / vsol` per leg, on the **virtual** reserve | the curve |

- Cost is U-shaped in size: fixed cost dominates small orders, impact dominates large
  ones; the minimum sits at `B* = sqrt(F * vsol)`, about 0.126 SOL on a 70 SOL pool at
  today's tip. Break-even gross at the optimum is about **3.3 %** per round trip; on an
  unmoved token at `vsol` 50 the toll reads about **-3 %**.
- Impact is charged on `vsol`, never on the real reserve (`vsol - 30`): the real one
  overcharges by `vsol / (vsol - 30)`, worst exactly where shallow-pool rules trade.
- A cohort whose median result sits on the toll is telling you its median token does
  not move. No gate that fires on the median token can pay.
- A flat slippage term does not exist and must not: the fill model already prices which
  print we transact against, and a size-blind term changes sign with buy size, which
  reorders a grid instead of shifting it.

### 5.4 Measurement honesty

1. **Full tape.** Score every instance of the event on every token, never a candidate
   list, never "the tokens a reader traded". A derivation's candidate table is not a
   universe.
2. **Decision-time facts only.** Nothing that happens after the decision print is a
   term, a filter or a label. "Tokens he bought" is a hindsight label, because his buy
   comes after the event being scored. A his-versus-control gap survives only if the
   control is conditioned on the same forward event and the gap remains.
3. **The reader's own prints stay out of the candidate pool.** Putting them in makes the
   thermometer read "he fires when he buys".
4. **Chain order, not wall clock.** Every metric and gate sorts by `(slot, tx_index)`;
   the fire print is located by its key and scans are bounded by slot. The ingest
   timestamp is the reaction clock only. A quarter of the prints in a reader's own slot
   land behind him; counting them inflates every rank.
5. **Both legs at the seat, full costs, curve pricing.** Section 5.1-5.3, every time.
6. **The event is never refuted alone.** An event is named by the story and by reader
   response; its money is read only on a story-derived conjunction. An unconcentrated
   pool is red by construction. A greedy search gated "positive at every step" cannot
   start from a negative universe and reports an empty space where a conjunction passes.
7. **Every term carries one causal sentence.** That is the whole defence against
   curve-fitting: the space of story-derived conjunctions is small enough for a holdout
   to mean something.
8. **Freeze, then a disjoint holdout, never trimmed.** The rule is written before
   untouched time is read. Shortening the sentence because a prefix scores better held
   out is fitting the holdout. A block that is red means the story is wrong; the rule is
   never patched to fix a block.
9. **Score the worst day, not the best.** Thresholds chosen on one day describe that
   day. A post-hoc pocket filtered out of a fired set is a different population from a
   rule that waits for it; nothing is promoted without an engine run.
10. **Report the artifact detectors beside every result:** the zero-lag column (the
    ratio is the artifact size), the gap-to-next-print distribution of the selected
    trades (money concentrated under ~50 ms is an artifact), the share of entries with
    no print in the hold, and the trail's trigger rate (a trail that fires on 18 % of
    trades is a clock with a trail's name).
11. **Lift belongs to the event, money to the gates.** Requiring doors and permissions
    to concentrate on a reader is cloning by another route; the largest single money
    term ever measured here anti-tracks the reader it was found beside (lift 0.69).
12. **Per-trade engine reconciliation before belief.** The offline book and the engine
    book agree trade by trade at the same fill and cost model, or the offline number
    is not a number.
13. **The finding sets the metric.** A rule is implemented in the terms it was derived
    in; when the nearest engine metric differs in basis, grain or unit, the metric
    system is extended, never approximated.

### 5.5 The objective

**Rank on total net SOL.** Mean percent chases thin high-percentage pockets; the same
event ranked in SOL reads green at 100+ trades a day. Win rate, median and mean are
description.

**Accept a daily-positive book.** The population worth copying loses on some days and
wins on most, and its losing days are small. A rule is accepted on the share of days
that end positive and on the size of its worst day relative to a normal one, not on
expectancy alone. This is a harder bar than positive expectancy, and it disciplines
the exit: convex per trade, consistent per day through count. A book that is positive
in aggregate and red most days is a lottery ticket, not a rule.

**Graduation rate and headroom explain a sentence; they are not the objective.** A
proxy fitted on the pool is exploited by any search: it finds the cell where the proxy
is wrong. Rank on real money; use `P(reach the wall)` and `(vsol_max / vsol_entry)^2 - 1`
to check that a sentence holds both, because a cut that raises one while collapsing
the other is at a corner.

**The runner rate must survive a cut.** A filter that raises expectancy while lowering
the frequency of large outcomes has turned a convex book into a flat one. Blacklists
generalize; whitelists do not.

---

## 6. The workflow

```
                    [ THE CAUSAL CHAIN (section 2) ]
                                 |
                                 v
   +------------------------------------------------------------------------------+
   |  LAWS (section 5) - fixed once, every phase runs under them                   |
   |  physics . seat = last print landed by fire+115 ms, both legs . cost on vsol  |
   |  full tape . decision-time facts . reader's prints out . chain order          |
   |  conjunction-only money . total SOL + daily-positive . freeze -> holdout      |
   +------------------------------------------------------------------------------+
                                 |
                                 v
   +------------------------------------------------------------------------------+
   |  PHASE 1  CENSUS OF MACHINES                            (continuous, weekly) |
   |    classify every BUY and SELL by build fingerprint                          |
   |      (program + instruction name, compute-budget order, ATA, nonce/seed,     |
   |       fee preset, clip)                                                      |
   |    cohort = machinery: launch build <-> bundle build <-> volume build <->    |
   |      the sell build that pairs with them                                     |
   |    species: concentrated (many buys per mint) vs spray (few per mint);       |
   |      racer (seed/nonce) vs named router vs direct                            |
   |    NO wallet clustering, NO funding graph                                    |
   +------------------------------------------------------------------------------+
                                 |
                                 v
   +------------------------------------------------------------------------------+
   |  PHASE 2  DEV BEHAVIOR MODEL                                   (continuous) |
   |    token life as phases: launch -> scramble -> (abandon | campaign) ->       |
   |      attention -> (push | extract) -> wall -> fees                           |
   |    decision nodes, several: silence-break . attention-arrival .              |
   |      pullback-while-still-in . push-to-wall                                  |
   |    per machine: P(follow-through | node, count, size), headroom,             |
   |      recurrence, and the EXTRACTION TELL = the machine that pushed this      |
   |      token starts selling on it                                              |
   +------------------------------------------------------------------------------+
                                 |
                                 v
   +------------------------------------------------------------------------------+
   |  PHASE 3  READER STUDIES - instrument, never target                          |
   |    daily-profitable, non-proxy wallets -> which node each one answers        |
   |    locate the DECISION print: >= his own reaction time before his fill       |
   |    his prints out . response rate NAMES the event, gates nothing             |
   |    racers and readers react to the same upstream event: find it, not them    |
   +------------------------------------------------------------------------------+
                                 |
                                 v
   +------------------------------------------------------------------------------+
   |  PHASE 4  STORY -> RULE                          (no story, no rule)         |
   |    EVENT        completing print of a condition on unpriced state            |
   |    DOOR         creation-time + machine facts                                |
   |    PERMISSIONS  state already true at the event                              |
   |    EXIT         harvester law (never cap the tail, abandon fast) or the      |
   |                 scalper law (sell into the predicted wave)                   |
   |    RE-ENTRY     unlimited until a budget caps it . SIZE ~ sqrt(F * vsol)     |
   |    every term carries one causal sentence                                    |
   +------------------------------------------------------------------------------+
                                 |
                                 v
   +------------------------------------------------------------------------------+
   |  PHASE 5  HONEST MONEY - on the CONJUNCTION only                             |
   |    full tape . lag 115 both legs . full costs . every instance               |
   |    rank on TOTAL SOL . accept only DAILY-POSITIVE with small loss days       |
   |    freeze -> disjoint holdout -> never trim                                  |
   |    red = THIS STORY is wrong -> Phase 2 / 4                                  |
   |    an event is never refuted alone; a node closes when its stories run out   |
   +------------------------------------------------------------------------------+
                                 |
                                 v
   +------------------------------------------------------------------------------+
   |  PHASE 6  ENGINE + PER-TRADE RECONCILIATION                                  |
   |    metrics that say EXACTLY the finding (extend, never approximate)          |
   |    offline book vs engine book agree trade by trade, same fill, same cost    |
   +------------------------------------------------------------------------------+
                                 |
                                 v
   +------------------------------------------------------------------------------+
   |  PHASE 7  PAPER -> SMALL REAL   (live evidence at the real seat, slots kept) |
   +------------------------------------------------------------------------------+
            |                                             |
            |  results feed Phase 2: the dev model is alive
            |  everything rots: Phase 1 re-census weekly; rule health is read
            |  against the census ("that machine changed"), never patched
            +-------------------------------------------------------------->
```

### Phase notes

- **Phase 1** is where the vocabulary of unpriced state is built. Measured here: about
  17,000 buy build templates in a week; brands (Axiom, Terminal, GMGN) are buy/sell
  symmetric and are the terminal crowd; the seed-builder cohort (`AdvanceNonce` +
  `CreateAccountWithSeed`) is the racer layer; a handful of builds are campaign
  machines. **The machine is the grain, never the brand** - and never the program name
  alone: a build's instruction name and its compute-budget order separate a campaign
  client from its generic twin on the same instruction set (one concentrates ~21 buys
  per mint and is green; the other sprays ~2.7 per mint and is red).
- **Phase 2** measures what follows a decision. Measured here: the count of same-tool
  buys in a breaking slot predicts the immediate wave (1 -> 4 buys: 59 % -> 79 % get
  1 SOL within 60 s) and does not predict reaching the wall (flat ~7 %); reaching the
  wall tracks **which machine** acted, and the real campaign machine fires alone on
  98 % of its breaks. Multi-tool bursts mark attention arriving; single-machine breaks
  mark a decision.
- **Phase 3** turns a reader into a node label and a thermometer. Measured here on one
  attention-arrival reader: response rises with silence and with same-tool count
  together (0.04 at no gap and one buy, 0.47 at a 21+ slot gap and three buys) and with
  neither alone; the racer swarm lands between the event and him without disqualifying
  it; the slot is not the event unit (a cell on "one transaction in the slot" discards
  every event strong enough to draw a swarm).
- **Phase 4** is forbidden from starting until the story can be said out loud: *"the
  dev of this kind of launch, at this kind of stall, spends through this tool to
  restart the attention loop, because his fee income peaks past the wall - so flow
  continues after our entry."*
- **Phase 5** reads money on the whole sentence. Measured here: the one rule that passes
  a fully disjoint holdout in this workflow is one campaign machine's silence-break
  with four permissions and a harvest exit; its class widening by behavioural screen
  fails walk-forward, so a door is per machine, and a machine earns allocation only by
  sustained green trailing money at adequate effective n (mints, not trades).
- **Phase 6** is where an offline number becomes a claim. One decision kernel serves
  live-real, live-paper and simulate; a rule that cannot be authored in it is not
  finished.

---

## 7. Closed mistakes

| mistake | the rule |
| --- | --- |
| Price the fill one or two slots after the trigger, or drop the trigger's slot | Last print landed by `fire + 115 ms`, both legs. We land in the trigger's slot half the time |
| Take the next print after the signal as the fill | An ordering privilege no latency buys; report gap-to-next-print beside every result |
| Fill an exit at its trigger level | The first print that crosses it, per print; a stop fills ~3.5 % past the level |
| Book a silent exit at -100 % | The curve freezes price; exit at the last known state less the toll |
| Make a specific actor's landed print the event | The event is a condition on tape state, wallet-free, at decision time |
| Refute a node on its event alone, or on an unconcentrated pool | Money is read on a story-derived conjunction; a red pool refutes nothing |
| Search terms at one fill and ship at another | A fill model changes which terms matter; search at the shipping fill |
| Compare "his tokens" with a control | Forward conditioning: his set is defined by an event after the one scored. Fix `t0` blind |
| Put the reader's own prints in the candidate pool | Out, always; the thermometer is response, not membership |
| Rank on mean percent | Total net SOL, then the daily-positive bar |
| Require gates to lift the reader's response | Lift names the event; gates are ranked on money |
| Trim the sentence on held-out performance | Fitting the holdout; the frozen sentence is the rule |
| Tune on one day | Score the worst day; one day selects nothing |
| Promote a post-hoc pocket without an engine run | A filtered fired set is a different population from a rule that waits |
| Build a term on a wallet, an address cluster or a funding graph | Machinery is the only axis; a wallet is a study subject |
| Use the program name as the grain | Split into exact builds; on direct-program buys the instruction name and compute-budget order are the machine |
| Treat the slot as the event unit | The event is one tool's run inside the slot; a swarm in the same slot does not disqualify it |
| Map an engine metric that is not the same quantity | The finding sets the metric; extend the metric system |
| Charge impact on the real reserve | On `vsol`; the real reserve overcharges shallow pools most |
| Gate on windowed flow, net flow or buy share on a curve | They are the price path; only counts and machine identity are orthogonal to price |
| Gate on quiet before the event | A description entailed by age, not a mechanism; the demand does the work, not the quiet |
| Choose the exit by its own score on the unfiltered pool | The pool is mostly dying tokens; every sweep on it returns the shortest clock. Exit from the story, after the gates |
| Read a longer clock as "the harvest needs room" | Split by whether the tape survives the deadline; holding longer pays only on tokens heading for the wall |
| Use graduation rate as the objective | Rank on money; graduation x headroom explains, and a proxy is exploited by the search |
| Conclude "the trader is closed" | Only a story closes; a daily-profitable trader runs derivable logic |
| Copy a wallet, a fingerprint or a send set | The reader's edge is his own impact plus his slot; both are in the price before a copier acts |

---

## 8. Sources

Curve, graduation, migration:
[DEXTools - pump.fun guide 2026](https://www.dextools.io/tutorials/what-is-pump-fun-solana-memecoin-launchpad-2026) .
[DEXTools - bonding curves](https://www.dextools.io/tutorials/what-is-a-bonding-curve-token-pricing-guide-2026) .
[Flashift - curve mathematics](https://flashift.app/blog/bonding-curves-pump-fun-meme-coin-launches/) .
[SolTokenCreator - graduation threshold](https://www.soltokencreator.io/blog/pump-fun-graduation-explained) .
[Moby - pump.fun 2026](https://moby.win/learn/pumpfun/) .
[crypto.news - how meme coins are made](https://crypto.news/how-meme-coins-are-made-bonding-curves-pump-fun-rug-pulls/)

Fees and creator revenue:
[pump.fun fee docs](https://pump.fun/docs/fees) .
[Froglabs - fees explained 2026](https://froglabs.io/blog/pump-fun-fees-explained/) .
[SolTokenCreator - fees](https://www.soltokencreator.io/blog/pump-fun-fees-explained) .
[CoinDesk - creator revenue sharing](https://www.coindesk.com/markets/2025/05/13/pumpfun-launches-revenue-sharing-for-coin-creators-in-push-to-incentivize-long-term-activity) .
[CoinMarketCap - dynamic creator fees](https://coinmarketcap.com/academy/article/pumpfun-creators-earn-dollar2m-in-first-day-under-new-fee-structure) .
[Yahoo Finance - fee model](https://finance.yahoo.com/news/pump-fun-fee-model-hands-125849600.html) .
[Medium - Project Ascend](https://medium.com/coinmonks/pump-fun-new-revenue-plan-how-project-ascend-is-boosting-creator-earnings-in-the-memecoin-world-32901d90f4ac)

Solana execution:
[Solana - fee transaction priority proposal](https://github.com/solana-labs/solana/blob/master/docs/src/proposals/fee_transaction_priority.md) .
[Helius - fees in theory and practice](https://www.helius.dev/blog/solana-fees-in-theory-and-practice) .
[RPC Fast - priority fees and compute units](https://rpcfast.com/blog/solana-transaction-fees-explained) .
[RPC Fast - landing transactions](https://rpcfast.com/blog/how-to-land-transactions-solana) .
[RPC Fast - Jito 2026](https://rpcfast.com/blog/jito-explained-bundles-tips-mev-solana) .
[Chainstack - Jito bundles and tips](https://chainstack.com/jito-explained-bundles-tips-mev-solana/) .
[Chorus One - do tips land faster](https://chorus.one/reports-research/transaction-latency-on-solana-do-swqos-priority-fees-and-jito-tips-make-your-transactions-land-faster) .
[AllenHark - why transactions arrive late](https://allenhark.com/blog/why-solana-transactions-late-priority-fees) .
[Yavorovych - fees for trading bots 2026](https://yavorovych.medium.com/solana-transaction-fees-explained-for-trading-bots-2026-35ebdde7af4c)

Discovery and the safety panel:
[Axiom docs - Pulse](https://docs.axiom.trade/axiom/finding-tokens/pulse) .
[Axiompedia - Pulse filters](https://axiompedia.com/guides/trading/axiom-pulse-explained) .
[Terminalpedia - trenches mode signals](https://terminalpedia.com/guides/trading/terminal-trenches-guide) .
[OpenPR - the trending algorithm](https://www.openpr.com/news/4398886/the-silent-algorithm-behind-pump-fun-why-2026-is-the-year-of) .
[Bubblemaps](https://bubblemaps.io/) .
[Bubblemaps - holder analysis](https://blog.bubblemaps.io/how-to-analyze-meme-coin-holders-with-bubblemaps/)

Dev equipment and the playbook:
[Smithii - bundler](https://smithii.io/en/pump-fun-bundler-bot/) .
[cicere - open-source bundler](https://github.com/cicere/pumpfun-bundler) .
[OpenLiquid - volume bot](https://openliquid.io/tools/pump-fun-volume-bot/) .
[Alphecca - volume bot staging](https://alphecca.io/en/blog/pump-fun-volume-bot) .
[jumpbit - bump bot](https://jumpbit.io/en/solana/pumpfun-tools/pumpfun-bump-bot) .
[RPC Fast - sniper infrastructure](https://rpcfast.com/blog/how-to-launches-snipe-pump) .
[Medium - insider's guide to every strategy](https://medium.com/@jump_bit/making-money-on-pump-fun-the-complete-insiders-guide-to-every-strategy-2025-5dd121cfa74d)

Base rates and manipulation:
[arXiv 2507.01963 - A Midsummer Meme's Dream](https://arxiv.org/html/2507.01963v1) .
[arXiv 2602.14860 - predicting pump.fun token success](https://arxiv.org/abs/2602.14860) .
[Cryptopolitan - graduation rate](https://www.cryptopolitan.com/pump-fun-graduating-tokens-break-to-1-15-of-new-launches/) .
[AssureDefi - rug and pump-and-dump prevalence](https://www.assuredefi.com/blog/meme-coin-rug-pulls-pump-dumps-how-to-spot-and-prevent-fraud) .
[Webopedia - scam tactics](https://www.webopedia.com/crypto/learn/pump-fun-scam-tactics/) .
[Investing.com - sniping signatures in the first buys](https://investing.com/analysis/how-to-spot-the-next-1000x-solana-memecoin-on-pump-fun-200654552?rand=12354)

---

## 9. Appendix: this repository

The one section that names files, tables and engine vocabulary. Delete it to port the
rest.

### Companion docs

| doc | carries |
| --- | --- |
| [trader-study-contract.md](trader-study-contract.md) | Phase 3 in full: completing print, thermometer vs money, concentration, its own closed-mistakes table |
| [fill-and-cost-models.md](fill-and-cost-models.md) | every `FillModel` and `CostModelKind`, worked on one trade; `lag_115` is the verdict |
| [execution-costs.md](execution-costs.md) | the derivation of section 5.3; the fixed leg cost is env-derived |
| [edge-at-real-latency.md](edge-at-real-latency.md) | the seat laws (section 5.2) with their measurements and status |
| [curve-honest-pricing.md](curve-honest-pricing.md) | section 5.1 with the harness assertions |
| [armed-trailing-stop.md](armed-trailing-stop.md) | the harvester exit that fires from strength; an unarmed retrace is a hard stop |
| [metrics-reference.md](metrics-reference.md) | every engine metric with its one definition |
| [tool-census.md](tool-census.md) | Phase 1-2 tables and first findings |
| [campaign-break-money.md](campaign-break-money.md) | the one rule that passes a disjoint holdout; priced at +2 slots, so a lower bound |
| [8dtx-entry-event-ix.md](8dtx-entry-event-ix.md) | the corrected event unit and the gap x count response table |
| [roster-habit-nodes.md](roster-habit-nodes.md) | the node map behind section 3.2, per-wallet fields, purity cost |
| [wave-node-money.md](wave-node-money.md), [axiom-push-money.md](axiom-push-money.md) | two node studies priced at +1/+2 slots - see verdict status below |

### Engine vocabulary the anatomy maps to

| rule term | engine |
| --- | --- |
| Event on a machine run | `m_flow_ix.ix_patterns` (exact ordered build) or `m_burst_slot.working_templates` (grain), over a slot window `window_size_slots: 1` |
| Silence before it | `m_flow_window` on slots with `window_lag: 1`, so the quiet span cannot read the burst |
| Door on launch machinery | fingerprint axes (`max_cost`, `init_buy`, launch `ix_labels`) or the wildcard fingerprint for "any token" |
| Permissions | `m_snapshot.time`, `m_state.liquidity` (real reserve = `vsol - 30`), `m_price_window.trail` |
| Harvester exit | `m_position.retrace` with `arm_above_pct`; `m_position.held` as the clock |
| Fill and cost | `lag_115` + `pumpfun_impact`, on both legs, in simulate and per-trade reconciliation |
| Re-entry | `reentry` with no cap, one position per token at a time |

Vocabulary cutoff: instruction names change at the decoder upgrade of
2026-08-30 17:48 UTC; a template seen only before it is the same machine under an
older name, and nothing merges across that instant. <!-- pt-ok: cutoff, tape before it carries old names -->

### Study tables (workstation PG, LOGGED schema `census`)

`tool_census`, `tool_census_sell`, `gap_breaks`, `gap_follow`, `mint_summary`,
`money_v2..v4`, `break_path`, `hold_breaks`, `hold_money`, `screen_post`, `screen_pre`,
`axiom_push`, `push_money`, `wave_events`, `w1_*`, `rb_ep2`, `rb_feat`, `rb_k`,
`rb_ctx`, `rb_cslot`. Schema `e8` holds the event-unit study. Rebuild one query at a
time with `work_mem = 128MB`.

### Verdict status under the corrected laws

| result | status | why |
| --- | --- | --- |
| Gap-burst harvest rule (9 gates, harvest exit) | **stands** | 95 ms both legs, three independent blocks, permutation null |
| Campaign-break rule v0 (one machine) | **stands as a lower bound** | disjoint holdout green at +2 slots; re-price at `lag_115` before sizing |
| First-launch impulse rule | **stands offline** | 115 ms both legs, out-of-sample; not yet reconciled on the engine |
| Island A-continuation with a clock exit | **stands** | real kernel at 95 ms, forward block green |
| Copying any wallet | **closed at every fill** | own impact plus in-slot swarm are in the price first |
| Wave node, both branches | **void as a node verdict** | priced at +2 slots; re-open at `lag_115` on a conjunction |
| Axiom push cells | **void as a node verdict** | priced at slot +1; a brand is not a machine anyway |
| Attention-arrival four-term gate "at d = 2" | **void as a seat claim** | d = 2 is the reader's seat; ours is `lag_115` |
| Dump-scalp / turn family at `next_slot_median` | **void as a node verdict** | the model drops the trigger's slot; the reader lands in it |
| Single-gate closures (23 gates on one cohort, 32 gates at end-of-slot fill) | **stand as single-gate facts, void as node verdicts** | section 5.4 law 6 |
| N4 mid-tape node as 3Xk2 / 8dtx / 6J1T run it | **closed at our seat** | racer builds ~130 ms behind a terminal buy with size; behind the swarm every exit is negative on their own picks; the full-tape sentence is red on four weeks at `lag_115` both legs |
| No-initial-buy door + router buy >= 0.5 SOL, clock 45 | **refuted on a disjoint holdout** | the fitting week was one launch machine active two days; the frozen sentence reads red on three earlier weeks |
| Creator-sold blacklist (creator already sold before a mid-tape buy) | **stands as a permission** | -4 to -8 % a trade, 0 of 7 days in each of four weeks, 36k-89k fires a week; sold < holds < never-bought every week |
| Mid-tape entry on any tape-state condition (buy and hold, age 10-900 s, reserve 33-60) | **closed at our seat** | every event, permission, exit and band negative on 0 of 7 days at `lag_115` both legs; random fires -5.5 % on a 30 s clock, the toll is -3.5 %, the rest is token decay |
| Machine cadence as a signal (a bump bot's fixed clip and regular period) | **refuted** | the configuration is readable (next beat within 2x, 70-75 %) but forward arrivals from other machines equal a matched control, and on the same token a random print beats a confirmation print |
| The extraction tell as a permission ("the push machine has not sold yet") | **refuted, and inverted** | not-sold reads -7.2 % a trade against -4.3 % for has-sold; the unsold state marks a fall that has not happened yet |
