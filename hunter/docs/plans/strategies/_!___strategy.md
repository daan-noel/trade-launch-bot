# Strategy: the market, the basis, and what is true

The one strategy file. What this market is, who acts in it, the arithmetic every rule obeys,
what is established, what is open, and why.

Seven files, and they do not overlap:

| file | carries | edit rule |
| --- | --- | --- |
| **this one** | the market, the basis, the laws, what is closed by mechanism | rewritten when a finding lands |
| [_!___derive.md](_!___derive.md) | the method: from one paying wallet to a sentence, the gates, how a result is recorded | rewritten when the method changes |
| [_!___inventory.md](_!___inventory.md) | every idea, in Door / Event / Permission / Exit | a row is added or its status changes |
| [_!___workflow.md](_!___workflow.md) | the open queue | rewritten when the open list changes |
| [_!___evidence.md](_!___evidence.md) | the standing measurements with their coordinates, and the ledger of every rule booked | a standing number is added; a closed line is one ledger row |
| [_!___terms.md](_!___terms.md) | every word these files use, and every study code name | a word is registered in the same edit as the idea or finding that needs it |
| [_!___metrics.md](_!___metrics.md) | how the engine's metric system is built and extended; each metric's one definition (unit, tags, spans) lives in the registry (`engine/src/metrics/registry.rs`) | the structure, a landmine or the extension cost changes, in the same commit as the code |

Self-contained on purpose. Section 11 is the only part that names this repository; the rest
travels. A claim here that the evidence file does not support is wrong **here**.

**Every measured number names its book.** A figure is a pointer into
[_!___evidence.md](_!___evidence.md), written `(evidence 4.2)`, and the section there is the
definition: where a number here and the section it points at read differently, the section wins.
A figure marked **(unrecorded)** is one no evidence section holds - a real reading from a run whose
book was never written, kept because it carries the argument, and not something to re-quote as a
result or build a rule on.

Section numbers are never reused: a number is a permanent address, so cutting a section leaves a
gap rather than renumbering the ones after it. A missing number is a section that was cut, not a
page that is missing here.

```
  0. BIG PICTURE       plain language, one page - read this if you read nothing else
  1. THE BASIS         the arithmetic every rule obeys, with the numbers
  2. THE MACHINE       curve, fees, execution, discovery, actors, base rates
  3. THE CAUSAL CHAIN  ten theses, in the order cause runs
  4. WHAT IS TRUE      survival, convexity, and the two positive anchors
  5. THE ACTORS        five decision nodes, 26 instruments, which are reachable
  6. A RULE            six slots, and what belongs in each
  7. LAWS              physics, the seat, cost, honesty, the objective
  8. OPEN AND CLOSED   the ledger, with the empty slot named on every line
  9. CLOSED MISTAKES   one table
 10. SOURCES
 11. APPENDIX          this repository
```

---

# 0. THE BIG PICTURE, IN PLAIN TERMS

One page. Everything after this is the same thing with the numbers attached.

## What we are building

One rule the bot runs: *when the tape shows X, buy 0.2 SOL; when Y happens, sell.* It has to make
money after all costs, on most days, on enough trades to be a book.

## Where the money comes from - there is only one source

Buying a coin on the curve is not buying an asset. It is a bet that **more SOL flows into this
coin than out of it while we hold.** There is nothing else: no spread, no dividend, no volatility
to harvest. And SOL only flows in because **some person or bot decided to spend it.**

> **A rule is a prediction that someone is about to spend. Nothing else is a rule.**

Predicting a price shape instead - "it bounced 15 %", "flow is rising" - is the standing way to
lose, because a price shape is a record of spending that has already happened.

## The equation, in words

```
   what we make  =  (how often we win) x (how big a win is)
                  - (how often we lose) x (how big a loss is)
                  - the toll, 3.2-4 % every round trip
```

Four dials, and that is the whole design space:

| dial | what moves it |
| --- | --- |
| how often we win | **selection** - which coin, which moment |
| how big a win is | **headroom** (a cheap coin can travel further) and the exit |
| how big a loss is | **where we enter**, and the exit |
| the toll | fixed; it cannot be moved |

Two of these dials are selection problems, not one: *which coin goes up*, and *which coin goes to
-50 %*. Halving the loser cost is worth exactly as much as winning nine points more often.

## Three walls

**1. The price already contains everything that landed.** Price here is a formula on the SOL in
the pool, so every buy that landed is already in it. Volume, flow, "it is up 20 %" - all old news
by construction. Four things are **not** in the price, and they are the only raw material:

```
   who made the last buys (which machine)      whether the pushers still hold
   how many INDEPENDENT machines are acting    whether the creator has sold
```

**2. We are 115 ms late.** Deciding to filled is 115 ms. An edge used up inside that window is
closed to us forever, however much it pays its owner. An edge that plays out over seconds and
minutes is fully open. This is why copying a smart wallet's **fill** can never work: their buy and
the swarm behind it are in the price before we can act. Their **decision** is a separate question,
asked at a different seat (5.4, 7.2), and it is where every live rule comes from.

**3. The toll.** 3.2-4 % a round trip. A rule that is only slightly right still loses.

## What is established

- **We can predict which coins survive.** Certain launch builders, plus a website and a telegram,
  give 4-6x - on 30 straight days here, and on an independent study of 832,941 launches.
- **Only one sentence yet names a spike at our seat**: rule 1's frenzy-absorbed sell. The crowd
  that makes a spike arrives through feeds and chat, and none of that is on chain until it is
  already in the price.
- **The prize is real and reachable.** About 1,250 real doubling moves a day, and 90 % of the move
  is still there 115 ms after it starts. We are fast enough; telling which one is the problem.

## What a rule is

Six parts, always tested together:

```
  DOOR        which coins we watch at all      "coins from a builder whose coins live"
  EVENT       the exact print we buy on        "a 0.5 SOL router buy starts a burst"
  PERMISSION  what must already be true        "the dev has not sold"
  EXIT        how we get out                   "cut small if it stalls, ride if it moves"
  RE-ENTRY    may we buy this coin again       "yes, one position at a time"
  SIZE        how much                         "0.2 SOL"
```

**Testing one part alone always fails**, because on its own we are still buying thousands of
random dying coins. So a red number means *this sentence is red*, and the first question is which
part is empty. Write all six down every time, or that question cannot be asked.

## How a dead story produces the next one

A story answers four questions: **who** will spend, **why**, **what on the tape shows it now**,
and **why the money has not landed yet**. The fourth is the scarce one - if the sign and the money
arrive together there is no trade at any speed.

When a story dies, the shape of the failure names which answer was wrong, and that points at the
next story. Only one failure - the sign and the money being simultaneous - actually kills a line.
The generator of a parent is one paying wallet ([_!___derive.md](_!___derive.md)).
[_!___inventory.md](_!___inventory.md) is the idea list on that parent. The routing table
is [_!___derive.md](_!___derive.md) section 10.

## The state of the program

What is **settled** is here. What is **next** goes stale between edits, so it is not duplicated
here: the queue, the clean test and the calendar item all live in
[_!___workflow.md](_!___workflow.md).

| | |
| --- | --- |
| **one rule clears every ship bar** | rule 1 (hot-tape): +4.44 %/trade, 100 a day, 5/5 on unseen days, top 1 % 9.8 %, reproduced ticket for ticket by the engine (evidence 1.22, 1.23) |
| **works, forward-valid** | survival selection: 4-6x on 30 days, with external confirmation |
| **answered** | which coin goes to -50 %: bundle share and agreement among the solo 26, independent and stacking (1.3) |

---

# 1. THE BASIS

Everything in this file is a consequence of this section. If a later sentence contradicts it,
the later sentence is wrong.

## 1.1 There is exactly one source of profit

Our gross on a curve is `(vsol_exit / vsol_entry)^2 - 1`. So `vsol_exit > vsol_entry` requires
that **more SOL arrives while we hold than leaves.** There is no spread to capture, no
volatility to harvest, no carry, no borrow. Every trade is one bet on one quantity: **somebody
else's future net inflow.**

That inflow exists only because **someone still intends to spend.** A rule is therefore a claim
about an actor's remaining plan. It is never a claim about a price path.

## 1.2 The one equation

```
                    E  =  P . W  -  (1-P) . L  -  toll
                          ^   ^            ^         ^
                          |   |            |         |
            SELECTION ----+   |            |         +-- 3.2-4 % a round trip, fixed
            door x event      |            |
            x permission      |            +-- LOSER COST: entry position + exit
                              |
                              +-- WINNER PAYOFF: headroom (vsol) + exit
```

Break-even is `p = L / (W + L)`.

**That number is not a market constant.** Measured on one week of the same fires, it moves from
**29.0 %** under trail50 c600 to **46.4 %** under a 30 s clock (evidence 4.2), and from **14.5 %** at
an arbitrary mid-tape entry (evidence 4.1) to about **0** at an episode trough. A rule is judged against **its
own realised break-even**, never against a fixed percentage carried over from another cell.

## 1.3 There are two selection problems, not one

```
   P-selection : which coin goes UP         - two months of work, every sentence red
   L-selection : which coin goes to -50 %   - ANSWERED: predictable from the launch bundle
```

On the unselected mid-tape parent under trail50 c600 (evidence 4.3): `P = .20`, `W = 84.7 %`,
`L = 31.6 %`, so `E = -8.8 %` a trade. Holding `W` fixed, the book breaks even at `L <= 16.3`.
Holding `L` fixed, it breaks even at `P >= .29`. **Halving the loser cost is worth exactly as
much as lifting the hit rate by nine points**, and the terms that mark a collapse - bundle share,
dev holding, fresh-wallet share, creator sold, create structure - are already in hand.

The documented-project book cannot answer it: its door enters at age >= 300 s and reserve 50-85,
past the window where coins collapse, so 1.2 % of its trades reach -50 %, against 14.1 % on
door-v3 MONEY. **That book has no left tail; the tail is not unpredictable**
([_!___evidence.md](_!___evidence.md) 7, the C1 row).

**Run where the tail is, it is answered: the -50 % trade is predictable at decision time, from
the launch bundle.** On the door-v3 MONEY book, `bundle share < 0.20` - the share of live supply
held by wallets that bought in the creation slot - takes the -50 % rate from **14.1 % to 2.3 %**
and the book from **+5.39 to +10.43 SOL**, and it survives the control that kills every other
term (evidence 3.7). Snipers, fresh-wallet share and buyer count separate only because they
proxy the reserve, and reserve is reachability, not prediction (7.4 law 21). The creator
permission **inverts** on this axis: it buys survival and it buys the coins whose dev still holds
the supply.

**This is the first term in the program that cuts the loser cost and raises the money at the same
time**, and it is red on its own on the full tape (-151 SOL over 13,602 trades, evidence 3.7) - which is what a
conjunction term is supposed to look like.

**A second L-term is independent of it and stronger.** How many of the solo 26 are already in the
coin at our decision print cuts the -50 % rate to a **0.37 lift**, against **0.82** for
activity-matched random wallets and **0.98** for the ten roster wallets under the profit
cut - and it reads the same outside the roster's own selection window, so it is judgement
rather than circularity. Stacked with the bundle term it turns a -14.62 %/trade population into
**+3.86 %/trade** (evidence 6.8). Agreement is a gate on the LOSS only: firing *after* they land
is still -10.4 %/trade (unrecorded), because their impact is already in the price.

## 1.4 What price contains, and what it does not

The consequence for flow terms - that windowed flow on a curve **is** the price move, so there is
no absorption to trade - is 7.1.

Price is a function of the SOL reserve alone, so **every landed print is already in the price.**

```
  IN the price (spent information)      NOT in the price (the only predictive material)
  --------------------------------      -----------------------------------------------
  every buy and sell that landed        which MACHINE class made the last prints
  windowed flow (corr 0.98 with the     how many INDEPENDENT machines are acting
    price move over the same window)    whether the cohort that pushed this coin is still in
  net flow, buy share, volume           whether the creator has sold
  any percent off the low or high       where the coin sits in the feeds
  any zigzag, any breakout              what the launch and the presentation were
```

A condition read off the tape's own **activity** therefore reads spent information, and worse:
activity is purchasable, so it preferentially selects manufactured motion, and manufactured
motion is unwound by whoever paid for it. This is one mechanism and it explains a long list of
separate-looking negatives.

It does **not** cover machine identity, count of independent machines, or cohort state. Those
are unpriced, and they are what an event is built from.

## 1.5 The seat

Decision to fill is **p50 115 ms** against a 400 ms slot, and we land in the trigger's own slot
52.6 % of the time (evidence 1.1).

This is the law. What the seat actually **costs**, decomposed, is 7.2 - and the 115 ms is the
smallest of its four terms, which is the part that surprises people.

```
  the fill is the LAST print landed by fire + 115 ms, on BOTH legs
    - never the first print at or after the deadline (that is a trade that landed after us,
      worth +8 to +12 pp of pure look-ahead)
    - when nothing lands inside the lag, the fill is the trigger's own state
    - a quiet tape costs nothing: an AMM price moves only when someone trades
```

Direction decides the cost of that lag (evidence 1.1):

| action | price moves | fill / trigger |
| --- | --- | --- |
| buy into a flush | toward you | 0.889-0.943 (the lag pays you) |
| sell into strength | toward you | 1.023-1.027 (the lag pays you) |
| sell on a stop or trail | away from you | 0.964-0.977 (fills past the level) |
| buy a breakout | away from you | about -9.87 % of entry per slot |

**Any trigger that waits for adverse movement is adversely selected by construction.** Only
"buy weakness, sell strength" is robust on both legs.

An edge consumed inside about 115 ms of its trigger print is closed to us however well it pays
its owner. An edge that develops over slots and minutes is fully open.

## 1.6 Delay: why a story can exist at all

A rule needs a gap between **the moment intent becomes visible** and **the moment the SOL lands**.
With no gap there is no trade at any seat, however true the signal is.

```
  intent forms  ->  a footprint appears on the tape  ->  the SOL lands  ->  price moves
                    ^                                    ^
                    |                                    |
                    we fire here                         and this must still be ahead of us
                                                         115 ms later
```

The market lines closed in 8.1 fail on that gap rather than on the idea: the swarm lands in the
trigger's own slot, the wave's first buy already carries the impact, a racer bundle after silence
is confirmation that somebody else has already fired. The tell and the money are simultaneous.

The delays that exist in this market, largest first:

| mechanism | magnitude |
| --- | --- |
| a human reacting to a feed, a call or a stream | seconds to minutes - the largest, and untouched |
| a committed budget spent on a schedule | seconds to minutes |
| one operator executing a position in legs | seconds to minutes |
| an off-chain event with an on-chain footprint | seconds to minutes |
| a structural obligation (the wall, a fee tier, migration) | minutes |
| habit: an operator repeating himself on the next coin | minutes to days |
| co-arrival inside one slot | zero - closed to us |

**Enumerate the delay before the tell.** It is the binding constraint, and it is what separates a
story from a description.

## 1.7 The toll

The three cost terms are here and the reserve they are charged against is 7.3; read both before
pricing anything.

125 bps a leg, a fixed 0.000225 SOL a leg, and our own impact `B / vsol` a leg. Cost is U-shaped
in size and the minimum sits at `B* = sqrt(F * vsol)`, about **0.126 SOL** on a 70 SOL pool.
**A round trip on a coin that does not move costs 3.2-4.0 %** (evidence 1.2). A cohort whose median result sits
on the toll is telling you its median coin does not move.

## 1.8 The six statements everything follows from

1. Profit is someone else's future net inflow while we hold. There is no other source.
2. `E = P.W - (1-P).L - toll`, and break-even `L/(W+L)` moves with the entry and the exit.
3. Cutting the loser cost is worth as much as raising the hit rate, and on a book with a left
   tail it is answered (1.3).
4. Price contains all landed flow. Only who, how many, and whether they are still in is unpriced.
5. Our seat is 115 ms on both legs; edges consumed inside it are closed, edges that develop over
   slots are open. A story needs a delay between the tell and the money, and that delay is the
   scarce ingredient - enumerate it before the tell.
6. A rule is a conjunction of six slots. A red number is an address, not a verdict.

---

# 2. THE MACHINE

## 2.1 The bonding curve

Every pump.fun coin launches on a constant-product curve, `vsol * vtok = k`, opening at about
**30 virtual SOL** and **1.073 billion** virtual tokens, so the opening price is about
0.000000028 SOL. When the curve has taken in about **85 real SOL** the coin **graduates**: the
program withdraws the SOL, takes a fee and seeds a PumpSwap pool.

Consequences every later section uses:

- **Price is a function of the reserve alone.** `price = vsol^2 / k`. Measured here (evidence 1.3): `k` is exact
  on every print at `3.219e16` (SOL x raw units), and the graduation wall sits at `vsol`
  114.9-115.0.
- **Headroom is quadratic.** The gross a position can ever make is `(vsol_max / vsol_entry)^2 - 1`.

| target gross | +50 % | +100 % | +200 % | +300 % | +500 % |
| --- | ---: | ---: | ---: | ---: | ---: |
| **max entry reserve** | 93.8 | 81.2 | 66.3 | 57.5 | 46.9 |

  A book that wants +100 % winners enters below reserve 81 or it is a grinder, not a harvester.
- **The fall is capped the same way, and this one is easy to forget.** `vsol` never goes below
  its opening 30, so a **-50 % price move needs `vsol <= v_entry * 0.7071` and is impossible
  below entry reserve 42.43**; -25 % is impossible below 34.6. A cheap entry does not merely
  reduce the loss, it forbids it. Measured (evidence 1.3): 80.4 % of one event's 50,411 fires enter below 42.43
  and **cannot produce a -50 % trade at all**, which is why an L-rate compared across reserves
  measures headroom rather than prediction (7.4 law 21).
- **A silent coin keeps its price.** No prints means no reserve change; an exit fills at the last
  reserve state. The curve is always the counterparty, so **an exit never needs a later print.**
- **Liquidating `B` tokens against reserve `V` returns `V*B / (k/V + B)`**, never `B * price`.

## 2.2 Fees, and where the dev's money comes from

- **Curve trades pay 1.25 % total.** Measured here (evidence 1.2): the curve-side amount is `gross * 10000/10125`
  on 16,544 of 16,854 round dev buys - 125 bps, not 100.
- **Creators earn a revenue share on every trade of their coin**, dynamic by market-cap tier
  (0.05-0.95 % a trade), and the highest tier sits **just past graduation**.

So the dev has two revenue lines: **selling supply he bought cheap** (his bundle) and **fee
income that peaks once the coin clears the wall and keeps churning.** Graduation is not a
milestone for him; it is where his revenue rate is highest. Both lines need the same input:
transactions from other people.

## 2.3 Execution: slots, ordering, tips

- A slot is about **400 ms**; transactions landing in one slot confirm together.
- **Order inside a block is the leader's.** A priority fee improves the odds, not the position.
- **Jito bundles** are atomic groups of up to five transactions in one slot, ranked by tip. Over
  95 % of stake runs Jito and tips are over 60 % of priority-fee volume, so **position inside a
  slot is a tip auction, not a latency race.**
- Nothing sequences between the legs of a bundle. A price that exists only between two bundled
  buys is not a price anyone else can trade.

## 2.4 Discovery: feeds, terminals, the safety panel

Retail does not find coins on chain. It finds them in **feeds**, and feeds rank by transaction
count, recency, unique buyers and volume. One transaction moves a coin to the top of "recently
traded". **Attention is purchasable with transactions**, and attention converts into retail flow.

Terminals stage discovery by curve position - New Pairs, Final Stretch, Migrated - and their
**safety panel** shows what a human reads before buying: bundle share of supply, dev holding,
insider holding, dev funding source, fresh-wallet buys, bot detection.

Everything on that panel is a claim about **who holds the supply and who is buying**, never
about the price path. That is what a retail buyer reads - and it is the vocabulary the
L-selection problem (1.3) is built from. ("Reader" everywhere else in these files means the
daily-profitable professional of 5.4, never the person at the terminal.)

## 2.5 Actors and their equipment

| actor | what he wants | equipment | fingerprint on the tape |
| --- | --- | --- | --- |
| **dev cohort** (creator, bundle, volume bots) | sell supply, farm fees, reach the wall | bundler (launch + 16-25 pre-buys), volume bot in bump or volume mode, fee/CU presets, wallet rotation, KOL calls | launch build, bundle build, volume build with a fixed clip, and the sell build paired with them |
| **snipers** | the first price | 2-5 ms infrastructure, multi-relay, dynamic tips, pre-signed durable nonces | creation-slot buys; `CreateAccountWithSeed` / `AdvanceNonce` builds |
| **copy / tracker bots** | whatever a labelled wallet does | leaderboard feeds | swarms landing 0-1 slot behind a tracked wallet |
| **readers** (daily-profitable) | the next flow before it lands | a terminal or a private bot, one node each | small clips, one shot or re-entries, exits into strength |
| **retail** | the story | the feed, a KOL, a livestream | named retail routers, no speed markers |

A campaign is staged: **bump first to enter the feeds, volume once organic buyers arrive.**

## 2.6 Base rates

- 8,000-15,000 coins launch per day; **under 2 % graduate.**
- Wash trading is the most common manipulation (74.8 % of flagged cases), run by tiny groups
  (median 2.8 actors). **62.9 % of extraction events follow a visibility-building operation on
  the same coin** (evidence 8). Manipulation is staged: attention first, extraction second.
- Measured here, and unrecorded - no evidence section holds this census: about **0.2 % of active wallets are profitable every day** and take about a
  third of everyone else's losses; roughly half of those are launch operators trading their own
  creations.

---

# 3. THE CAUSAL CHAIN

```
  dev cohort has a PLAN and a BUDGET
       |  every decision is a transaction through a TOOL
       v
  the tape carries a FINGERPRINT for each decision  (launch / bundle / volume / sell build)
       |  price absorbs the SOL of each print the instant it lands
       v
  price contains the FLOW ............ but NOT who, how many, or whether the cohort is still in
       |  that unpriced state is what predicts the NEXT flow
       v
  a READER predicts remaining intent from unpriced state
       |  he fires on the first print after which his condition holds
       v
  he SELLS INTO the flow he predicted
       |  our seat: 115 ms decision-to-fill, the trigger's own slot half the time
       v
  we take the same decision at the same print, and pay the direction-dependent latency cost
```

**T1 - Nearly every price move is manufactured.** Someone pays for it, with a goal and a budget.
The tape is a record of campaigns, not opinions.

**T2 - The business loop is transactions -> feed placement -> attention -> volume -> creator fees
plus supply sales.** The dev is the entrepreneur of that loop; everyone else services it,
parasitizes it, or funds it.

**T3 - Exploitation is staged: visibility first, extraction second.** A coin's life is
launch -> scramble -> (abandon | campaign) -> attention -> (push | extract) -> wall -> fees.
Money sits at the phase transitions, because that is when the next flow becomes predictable.

**T4 - Every dev decision is visible as a fingerprinted transaction.** A stall is a decision
point, and what breaks the silence says what was decided. The gap by itself means nothing; the
machine that acts is the information.

**T5 - Actors are their machinery, and machinery is the only durable axis.** Wallets rotate
daily. Instruction structure, compute-budget order, account-creation pattern, fee preset and
clip size persist and sit on the very print being decided on. **A wallet is the subject of a
study, never a term in a rule.**

**T6 - Price contains the flow, but not who.** See 1.4.

**T7 - Profit is future net inflow, and it exists only because someone still intends to spend.**
See 1.1.

**T8 - Readers are derivable.** A daily-profitable trader runs a decision procedure over public
data, so it can be reconstructed. A failed reconstruction indicts the hypothesis, never the
existence of the logic.

**T9 - The seat decides reachability, and direction decides its cost.** See 1.5.

**T10 - Everything rots; the pipeline is the asset.** Fee rules change, launch clients rotate,
metas turn over in weeks. No rule is an asset. The census and the dev model that re-derive rules
are, and their refresh is part of the system.

---

# 4. WHAT IS TRUE

## 4.1 Survival is predictable, out of sample, repeatedly

That a coin keeps living:

| selector | label | lift | evidence |
| --- | --- | ---: | --- |
| launch-build door | reaches reserve 60 with a late peak | **4-6x on all 30 days** | 3.1 - 736,800 coins, 22-day untouched holdout, no decay |
| launch door + initial buy >= 2 SOL | produces a playable big move | 4.63x per coin (**1.21x per print**) | 3.2 - holdout |
| metadata document + age + reserve + 8 professional builds | net money on any >= 0.5 SOL buy | -9.41 % to +1.39 % along the ladder | 3.3 - two positive holdout weeks, both under 1 SE from zero |
| creator-sold blacklist | worse on every book | -4 to -8 % a trade, 0/7 days, four weeks | 3.4 - a permission, never an edge |

Independently confirmed at 7x this sample (evidence 8): a survival analysis of **832,941 launches** finds
telegram at a multivariate Cox HR of **5.40**, website 1.19, and all three links present at
17.4x on graduation - which is survival. Its causal caveat is our thesis in the authors' words:
a telegram channel "may proxy for creator effort, discoverability to buy-side bots and traders,
or selection by creators who already expect to succeed."

## 4.2 The prize is real and reachable

The census (evidence 2.1), and the ceiling it implies (evidence 2.2):

| quantity | value |
| --- | --- |
| up-episodes on one week | 100,676 on 42,106 coins |
| **playable** big episodes (age-0 launch ramps excluded) | **8,749 = about 1,250 a day** |
| of the move that survives a 115 ms fill | **90.7 %** |
| median **playable** episode duration | **87 s** |
| base rate | one playable big episode per **542 prints** |
| the trough book: fire at every real episode low, `tp100 / trail50 / cap1200` | **+26.99 %/trade, 8/8 days, 2,755 tickets a day** |

The trough book uses hindsight twice (it knows the low, and that the episode is real), so it is a
**ceiling, not a rule.** What it establishes is that the shape is reachable at our seat and that
the exit for it is settled. **Age-0 launch ramps are not tradeable**: they are consumed in about
two slots and they are where the -50 % tail lives.

## 4.3 No selector tried on the tape predicts convexity

Every axis tried against "this up-move is starting" lands in the same place. On the two event
families measured on money - the zigzag turn and a machine print in the dip - **663 conjunctions
above the ticket floor (522 and 141, evidence 7) produce zero cells that are simultaneously positive, tail-robust and
positive on both halves of the week.** Choosing the best ten cells on the first half of a week
and scoring them on the second turns +74.9 SOL into -40.5.

What a conjunction **does** do is recover the toll: it moves an unselected parent from -4.55 % a
trade (evidence 4.2) to about zero. It buys "this coin is not decaying" - worth roughly the toll - and not
"someone is about to arrive."

**Read that as a wall on the terms currently computable, not as a law of the market.** It covers
sixteen terms, one week, two event families, one clip. It does not cover the door slot, the
L-selection on a book that has a left tail, state-conditional exits, or the survival door against the burst-start print.

## 4.5 Why survival is predictable and a spike is not

Survival is a property of the dev's **intent and equipment**, and intent is visible before it
acts: a launch build, a presentation, several independent operators committing. A spike is a
property of **a crowd arriving in the next sixty seconds**, and the crowd arrives through feeds,
KOL calls and the coin's story - none of which is on chain until it is already in `vsol`.

That is why the metadata document works where it works and not where it does not. It is evidence
that the dev **intends to keep promoting**. It is not evidence that anyone is **responding right
now**.

---

# 5. THE ACTORS

## 5.1 The one question

Every profitable reader answers the same question with a different actor:

> **Whose money is still coming, and what on the tape says so?**

Each distinct answer is a **decision node**. Entry age, tape density, pullback versus strength,
repeat entry and holding time are the observable shadows of the answer.

## 5.2 The instrument set is the solo 26, node by node

A roster row is an address, not a trader. Grouped by **which coins each wallet buys**, 41 of 77
daily-profitable rows collapse into 7 machines - the largest one 16 addresses splitting the mint
space by `ord(mint[0]) % 8`, two per shard, verified on 13,587 of 13,587 mints with zero
violations (evidence 5.1). **26 wallets have no co-selection partner and a margin that clears its own standard
error** (evidence 5.2). They are the only rows where one row is one opinion, and they are the only instruments
used, except a row that manufactures volume stays a roster row and is dropped at pick (derive 4.0;
evidence 5.1). The roster itself is the solo-trader list in appendix 11.1.

| node (every column through `best 1 %`: evidence 5.2) | wallets | net SOL | margin | median trade | lose > 20 % | best 1 % of trades = | open to us? | studied? |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | --- |
| hot-tape re-entry | 6 | 938.2 | 1.10 % | -2.07 % | 13.6 % | 180.6 % of net | **rule 1** ([_!___workflow.md](_!___workflow.md) 1). Three of the six read +2.2 to +2.3 %/trade at the RACE seat; pooled +0.50 % with a 216 % tail (evidence 1.11) | yes |
| mid-tape one-shot | 7 | 427.9 | 1.73 % | -3.34 % | 15.9 % | 122.9 % | **open** ([_!___workflow.md](_!___workflow.md) 2); the best ceiling with volume, +0.71 %/trade 7/8 at the PEER seat (evidence 1.4) | all seven members measured (evidence 7) |
| instant launch | 6 | 205.8 | 1.31 % | -2.50 % | 9.1 % | 116.0 % | oracle ceiling -0.25 % at the PEER seat (evidence 1.4) | partly |
| quiet deep-age | 4 | 110.3 | 1.12 % | -2.50 % | 5.9 % | 112.2 % | oracle ceiling -0.16 % at the PEER seat (evidence 1.4); red at our seat: the burst it follows lasts ~80 ms | yes |
| deep-age big clip | 3 | 68.2 | **4.03 %** | **-1.23 %** | **4.5 %** | **32.3 %** | **best ceiling: +2.62 %/trade 6/8 days** at the PEER seat, but only 1,197 episodes (evidence 1.4) | yes |

Three facts hold across all five:

- **Every node has a negative median round trip.** All five are convexity harvesters carried by a
  tail, so a take-profit inverts every one of them.
- **Four of the five are extreme lotteries.** Their best 1 % of trades produce 112-181 % of all
  net profit, which means the other 99 % collectively lose. `deep-age big clip` is the exception
  at 32.3 %, and it is the only node whose ordinary trades pay for themselves.
- **Hold is reactive, not a timer, in every node.** `p90 / p50` runs 3.0x to 5.3x (evidence 5.2). **No
  profitable node exits on a clock**, and every exit this repository has scored is a clock or a
  fixed trail.

## 5.3 Agreement rises with age

Co-selection lift between two of the 26, against the geometric mean of the pair's entry age
(Spearman 0.794 on 325 pairs, evidence 5.3):

| both enter at | pairs | median lift |
| --- | ---: | ---: |
| under 30 s | 60 | **0.17** (they anti-select) |
| 30-120 s | 125 | 0.96 |
| 120-400 s | 108 | 2.26 |
| over 400 s | 32 | **3.50** |

A coin's identity carries a shared signal a professional can read only once the coin has some
life in it. Agreement predicts the -50 % rate forward, outside the roster's selection window
(evidence 6.8), and it is the one coin-level term that is not saturated: a 1,212-wallet oracle covers 89-98 % of the parent and
moves detection by at most 1.1 points (evidence 7), while 26 independent opinions are a different object.

## 5.4 What a reader is for

A reader is an **instrument**, never a target. He tells which decision node exists and which
machine signatures carry follow-through. **Copying him is closed at every fill**: his own buy
moves the curve, the swarm that follows him lands inside his slot, and both are in the price
before a copier can act - truncate the path one slot before he lands and every "before" cell goes
negative.

Two rules follow:

- **A node is not a wallet list.** Do not close a seven-wallet node on two members whose
  equipment happens to be unreachable.
- **"His fill is unreachable" is not "his decision is unreachable."** They are different slots.

---

# 6. A RULE

A rule is one causal sentence, then six slots. The sentence comes first; a term without a reason
is forbidden.

```
  STORY        who acts, why now, why flow CONTINUES after our entry

  D  DOOR         which coins we watch at all: creation-time and machine facts.
                  Two doors, not one: a P-door (which coin goes up) and an
                  L-door (which coin collapses). They are different questions.
  E  EVENT        the first print after which a condition on UNPRICED state holds -
                  machine class of the run, count of independent machines, cohort
                  state, position in the coin's life. Wallet-free. We fire on it.
  P  PERMISSIONS  state already true at the event, or we skip
  X  EXIT         the harvest shape, derived from the story's failure mode
  R  RE-ENTRY     open again on the next qualifying event on the same coin
  S  SIZE         near sqrt(F * vsol), or a fixed fraction of vsol
```

- **The event is not "he bought."** It is what anyone watching the tape sees, named by machine
  build, count and size over the run inside the slot.
- **The event is one tool's run inside a slot, not the slot.** Filtering to `ntx = 1` filters the
  slot and excludes exactly the events strong enough to provoke a crowd.
- **The completing print is the fire.** Do not wait for the rest of the slot.
- **The door is not the trigger.** A coin that fails the door stays off; one that passes is still
  waiting for the event. A door alone is never a trade, and calling one "closed as a trade" is a
  category error.
- **Permissions are not the trigger.** They are already true when the event prints.
- **The exit comes from the story, and is settled after the gates.** The unfiltered pool is
  mostly dying coins, so any exit swept on it returns the shortest clock.
- **Re-entry is unlimited**, one position per coin at a time, until a budget caps trade count.

Two exit laws exist, and a book runs on one:

| book | law | shapes that satisfy it |
| --- | --- | --- |
| **harvester** | never cap the right tail; abandon fast when follow-through fails | loss-capped ride (unarmed trail), tail-preserving `tp100/trail50/c1200`, state-conditional cut |
| **scalper** | sell into the wave you predicted, then re-enter on the next flush | a small target resolved on prints, or a reaction to the next up-wave |

A take-profit on a harvester book caps the trades that carry it. A wide trail on a scalper book
gives back the wave. **Every selector is read under both**, so no cell's fate is decided by one
exit family.

---

# 7. LAWS

## 7.1 Physics

- `price = vsol^2 / k`, exactly. **A vsol threshold squares into a price threshold**: -25 % vsol
  is `pnl_pct <= -43.75`, a 20 % trail is `retrace_pct >= 36`, a +10 % arm is a `pnl_pct >= 21` gate.
  Forgetting the square is the most common way a validated rule ships wrong.
- `vsol` on a print is the reserve **after** that trade. The state a transaction meets at time `T`
  is the last print at or before `T`.
- **Silence freezes price.** A coin with no prints during the hold exits at the entry reserve less
  the toll. Booking it at -100 % invents a loss the curve cannot produce and always flatters gates
  that select for activity. Measured cost of getting this wrong once: an 85 pp effect manufactured
  out of nothing where the honest number is about 2 pp (evidence 1.3).
- **An exit needs no print.** Migration is the one exception, and it is rare.
- **Windowed flow on a curve is the price move** (corr 0.975-0.982 at 5/15/60 s, evidence 1.3). Net flow, buy
  share and their conjunctions add nothing. Only **counts and machine identity** are orthogonal
  to price. **So there is no absorption on a curve.** Absorption is an order-book idea - buying
  that does not move price because resting size eats it - and a constant-product curve has no
  resting size: every buy moves price by its own arithmetic. `buy SOL / (-price fall)` therefore
  divides the signal by itself and reads backwards, and "price fell AND buyers were the larger
  side" holds on 32 of 95,645 real buys (unrecorded). Any tell of the form "demand arrived without the
  price responding" is closed on this venue before it is measured.

## 7.2 The seat

Section 1.5 is the law. Seven corollaries:

- **A latency correction applied to one leg is not a latency correction.** The exit reaction is
  the same reaction as the entry, and the exit-leg artifact is the larger of the two.
- **Landing in the trigger's slot is reachable; a chosen position inside it is not.** Models that
  drop the trigger's slot are pessimistic by half a slot; models that take the very next print
  assume an ordering privilege no latency buys.
- **`lag_115` is the verdict on a spread tape and a floor on a burst.** When a burst is a swarm
  firing on one print, the honest fill is behind the swarm, not behind the print.
- **A fill model changes which terms matter, not only the level. Search at the fill you ship at.**
- **THE SEQUENCING RACE IS THE BIGGEST COST TERM, AND THE LATENCY IS THE SMALLEST.** Priced on the
  roster's own 131,339 episodes with our clip (evidence 1.4): being sequenced after their print
  costs **-5.59 pp**, the fee **-2.47**, our impact **-0.76**, and the 115 ms itself **-0.77**.
  The print we react to is 1.16 % of the pool at the median episode, and we pay the round trip of
  that displacement twice. Same decisions, four seats: RACE **+2.77 %/trade 8/8 days**, PEER
  **-0.03 %**, FOLLOW **-2.82 %**, FOLLOW+115 ms **-3.59 %**.
- **An event anchored on a PRINT is a FOLLOW model by construction** - the print must exist before
  we can react, so we always pay its impact and always lose its slot. Only a state that has been
  true for seconds can be a PEER, and "the state changed at this print" is a print anchor wearing
  a state's clothes. **Every print-anchored red verdict carries this anchor and is
  re-opened by it.**
- **Our clip is better than theirs.** Impact is `B/vsol`; the roster runs 0.2-2.0 SOL where we run
  0.2, so on their own decisions our book beats theirs (+2.77 % against +0.96 %). Size is a lever
  we already hold.

## 7.3 Cost

| term | value | who charges it |
| --- | --- | --- |
| protocol + creator fee | 125 bps per leg | pump.fun |
| tip + priority | fixed SOL per leg (0.000225 at a 0.0002 tip) | Jito + validator |
| own impact | `B / vsol` per leg, on the **virtual** reserve | the curve |

- Impact is charged on `vsol`, never on the real reserve (`vsol - 30`): the real one overcharges
  by `vsol / (vsol - 30)`, worst exactly where shallow-pool rules trade.
- **Do not subtract our cost from a cohort's net edge** - their number is already net of the same
  fees.
- **A flat slippage term does not exist and must not.** The fill model already prices which print
  we transact against, and a size-blind term changes sign with buy size.

## 7.4 Measurement honesty

These are laws about what this venue lets a measurement mean. Several of them have an operative
form as well - a gate a cell must pass, with a number and an order - and that form lives in
[_!___derive.md](_!___derive.md) 11, which is where a run reads them. A law here says why the gate
exists; derive 11 says what it refuses. Where both appear, the number is derive's.

1. **Full tape.** Score every instance of the event on every coin. A candidate table is not a
   universe; matched negatives come from the full print population at decision time.
2. **Decision-time facts only.** Nothing after the decision print is a term, a filter or a label.
   Measured, and unrecorded: 44.5 of 47 percentage points of apparent selection edge land after the decision
   point and are unharvestable.
3. **The reader's own prints stay out of the pool.** His response rate names an event; it never
   gates and it is never money.
4. **Chain order, not wall clock.** Sort by `(slot, tx_index)`. A quarter of the prints in a
   reader's own slot land behind him.
5. **Both legs at the seat, full costs, curve pricing.**
6. **Money is read on the conjunction, never on a term alone.** An unconcentrated pool is red by
   construction, so a single term's red number carries no information about the term.
7. **Every term carries one causal sentence.** That is the whole defence against curve-fitting.
8. **Freeze, then a disjoint holdout, never trimmed.** A red block means the story is wrong.
9. **Score the worst day, not the best.**
10. **Report the artifact detectors beside every result** (the list: derive 11).
11. **Lift belongs to the event, money to the gates.**
12. **Measure lift on the grain the book collapses to.** Per-print and per-coin lift can invert
    (2.68x per print against 0.74x per coin on the same event; unrecorded). **A 4.63x per-coin lift is 1.21x
    per print** (evidence 3.2).
13. **Per-trade engine reconciliation before belief**, against the published book, never against a
    recomputation - a recomputation shares the bug you are hunting.
14. **The finding sets the metric.** Implement a rule in the terms it was derived in; when the
    nearest engine metric differs in basis, grain or unit, extend the metric system.
15. **The universe SQL can hide a rule term.** A filter in the query that builds the universe is a
    rule term. **A check reporting exactly zero differences is the tell that the check is not
    running.**
16. **Count actors, not addresses.** Collapse wallets into machines by coin list first: zero
    overlap against a large expectation is one queue split across addresses; near-total overlap is
    one queue racing itself.
17. **A red sweep refutes the sentence, not the slot.** A slot closes on a measured mechanism or a
    conjunction search, never on a number.
18. **Count CLIENTS, not trades.** Trades behind a door are not independent draws: one creation
    build launches many coins over a day or two and every trade on them shares a creator, a
    machine and a window. Measured (evidence 3.1a): **one build carries 82 % of the best cell's net and 221 % of
    the next one's**, and a week of prints holds 34 slow-wall builds with a median life of two
    days, so a 1,006-trade cell is 19 draws. **A cell that dies when its best client leaves is
    that client's book**; the client gate is [_!___derive.md](_!___derive.md) 11.
19. **A per-day gate is checked per day.** The ticket floor is a refusal on each day, and a
    mean hides exactly the shape that breaks it: tickets of `16, 177, 210, 24, 28, 9` average
    against a reported mean of 53.9, and clear fifty twice in six days (evidence 4.8). Because those two big days are the single carrying
    client's own lifetime (law 18), the mean **launders the client concentration through the
    floor** - two failures cancelling into an apparent pass. The gate: derive 11.
20. **A term must be spellable without a wallet.** If it can only be written as "these named
    wallets did something", it is not a term, however well it measures. It stays a thermometer.
21. **Compare a rate only where the outcome is reachable.** `price = vsol^2 / k` with the curve
    floored at 30, so a -50 % move is arithmetically impossible below entry reserve 42.43, and
    80.4 % of one event's fires sit below it (evidence 1.3). Reserve then reads as the strongest -50 %
    "predictor" ever measured and predicts nothing. Hold the reachability band fixed, then read
    the term.

22. **A roster margin is NET of the venue fee; a study cell's price move is not.** The solo
    sheet's `margin` is `sol_out*0.9875 - sol_in*1.0125` over spend
    Comparing it to a cell's gross move is a
    **2.5 pp** error in the flattering direction, and it turns "we did not find their edge"
    into "we found it and the toll ate it" - two findings with opposite next steps. Convert
    one basis to the other before any sentence compares the two: `move = (margin + 2.5)/0.9875`.

23. **The distance to a perfect exit is not money on the table.** A perfect-exit ceiling answers
    "how much convexity exists", never "how much an exit can take", and the two differ by the
    whole of the timing problem: on the hot-tape node the ceiling is **+53 %/trade** and the best
    of 480 causal exit shapes is **-2.35 %** (evidence 1.5), because the peak is at a different index on every
    ticket. Quote a ceiling only beside a **searched** exit, never as a gap to be closed.

24. **An exit family is causal only if every branch is decided in INDEX order.** The common bug is
    a cut whose liveness depends on whether some other branch fires LATER in the window - "close
    at 30 s unless the position ever reaches +30 %" keeps exactly the tickets that go on to
    work. Measured cost of that one bug here (evidence 1.5): **+8.30 %/trade against a true -2.38 %**, on a
    held-out half, 8/8 days, with every robustness column green. A rung, a stop, a clock and a
    trail must each resolve to an index, and the smallest index wins.

25. **A multi-leg exit is priced leg by leg, or it is not priced.** Each sell leg pays its own
    125 bps, its own 0.000225 SOL and its own impact on the virtual reserve, so a three-rung
    ladder starts 0.22 % behind a single clip at B = 0.2. A scale-out that is not charged its
    extra fixed legs will always look better than the exit it replaces.

26. **An exit is read at the horizon its actor trades, on a pool a door has already selected.**
    Two ways make an exit search meaningless. Sweeping it with
    D and P empty returns the shortest clock, because an unselected pool is mostly dying coins -
    that is a standing refusal, and its signature is "shorter beats longer everywhere". And
    measuring the excursion over a horizon the actor never holds describes a payoff nobody in the
    node is exposed to: on the hot-tape node a read to **1800 s** against a median hold of
    **19.9 s** (evidence 1.5) reports a "+33 % median MFE" that is +7.6 % inside the hold that exists (both unrecorded).
    Read the geometry at the actor's own hold, and settle the exit after the gates.

27. **Pooling a node's members averages away the one that is a different animal.** Five of the six
    hot-tape wallets carry a negative median trade and a 180 % top-1 % share; `8fStGV` wins
    **68.3 %** of its trades at a median of **+12.02 %** on a 0.10 SOL clip (all three unrecorded: 1.11 books the member as a cell, never its own win rate). Pooled, its exit
    discipline disappears into 95,135 episodes dominated by two wallets. Split by member before
    any statement about how a node behaves - the mirror of "no node closed on a subset of its
    members".

28. **Imitating an actor is bounded by his MEDIAN trade, never by his net.** On a lottery book -
    best 1 % of trades producing 112-181 % of net - copying every decision copies the median, and
    every one of these five nodes has a negative one (-1.23 % to -3.34 %, evidence 5.2). Measured: a perfect
    reproduction of the hot-tape node's decisions reads **-0.14 %/trade** at the PEER seat (1.4),
    so no amount of AUC on "when do they buy" can pay. **Price what a perfect copy is worth before
    building a model of the moment**; if it is not positive, the only object worth searching is a
    selector INSIDE their decisions.
29. **Rank money among cells that look like a type.** Total SOL on a week where two days carry
    one launch client selects that client every time. A door is a general type only if its
    token-birth supply tracks the tape's births on full UTC days; a cell is a general type only
    if its per-day tickets track that door. Peak/trough and the two-fattest-days share of
    tickets sit beside every book ([_!___derive.md](_!___derive.md) 11). The SOL leader of a
    26x / 81 %-in-two-days book (evidence 7, C13) is a named client,
    not the walk's reading, and more calendar days do not turn it into a type.

30. **A sidecar left-join with missing = False is a universe filter.** The file's construction
    query is a rule term. A left-join that fills missing with False turns "not in this file" into
    "fails the door." Print sidecar rows, tape tokens, overlap, and the construction filter before the door
    is scored. Measured (evidence 7, C8; the 18,583 and the age split are not in that row): a sidecar holding the 18,583 keep+ep50 mints; documented x silence x
    creator x trail40 books +47.54; the same event on keep-create without that file is -1,034.87
    (age < 60 s: -1,007.34). The last-12 h documented supply of 0 is the same hole.

31. **An event with a vacuous base case is not the named event.** `if i == 0: gap = inf` makes
    ">= 10 empty slots" true of every first print. Report the share of fires at local index 0.
    Measured (evidence 7, C8): 818 / 1,622 of the C8 leader are `k == 0` and carry +46.08 of +47.54.

## 7.5 The objective

**Rank on total net SOL among cells that look like a type.** Mean percent chases thin
high-percentage pockets. Win rate, median and mean are description. Ranking by SOL on a
two-day client cohort selects that cohort; that is law 29, not a pass.

**Accept a daily-positive book**, judged on the share of days that end positive and the size of
the worst day, not on expectancy alone.

**Days positive is necessary and not sufficient.** Report **tail concentration** beside it: the
share of net from the top 1 % of trades and from the single largest coin, against the measured
calibration that a real convex book at this seat sits at **9-12 %** (evidence 2.3). If one coin carries the
result, that is the result. That 9-12 % is the calibration, not the bar: a cell stays a candidate
at **<= 20 %** and ships at **<= 15 %** (derive 11). A share far above 20 % is noise around zero,
not a fat tail.

**Never thin the book below about 50 first-per-mint trades a day.** That floor is a refusal, not a
target, and it is absolute rather than a fraction of the pool.

**Never score against a proxy.** Graduation rate, swing-detection rate and headroom explain a
sentence; they never rank one. A proxy fitted on the pool is exploited by any search: it finds
the cell where the proxy is wrong.

**The runner rate must survive a cut.** A filter that raises expectancy while lowering the
frequency of large outcomes has turned a convex book into a flat one. **Blacklists generalize;
whitelists do not.**

---

# 8. OPEN AND CLOSED

Every line names the slot left empty at measurement. See
[_!___evidence.md](_!___evidence.md) for the coordinate and the numbers.

## 8.1 Closed by a measured mechanism - these stay closed

| line | the mechanism |
| --- | --- |
| copying a wallet's fill, at any seat | truncate the path one slot before they land and every "before" cell goes negative; their impact and the in-slot swarm are in the price first |
| the wave node, all branches | landing first is a look-ahead (+44.41) and landing second is already negative (-24.07), evidence 5.6: a tape-observable trigger cannot precede the print that triggers it |
| the attention-arrival node | the same truncation decomposition; 36.1 % of its k=2 fires are one machine (evidence 5.5) |
| booking a silent exit at -100 % | the curve freezes price |
| pricing a fill from the next print | an ordering privilege no latency buys |
| charging impact on the real reserve | arithmetic |
| the exit-fill inversion on the 30-day door rules | break-even sits under 50 ms (evidence 4.5); the apparent edge is the exit fill |
| supervised models on about 40 tape features | fit/hold collapse, reproduced by the conjunction walk-forward |
| machine cadence as a signal | forward arrivals equal a matched control; a random print beats a confirmation print |
| the 1,212-wallet oracle as a second door | it saturates 89-98 % of the parent (evidence 7) |
| age-0 launch ramps as the prize | unreachable and unsurvivable |

## 8.2 Open, and red at our seat

Every open line, with the slot it needs, is [_!___workflow.md](_!___workflow.md). Every rule
booked and red, with its coordinate and number, is the ledger in
[_!___evidence.md](_!___evidence.md) section 7. A red line there closes that sentence, never
its slots (law 17).

## 8.3 What re-opens the harvester

Not a detection rate. **Any decision-time term that either cuts the loser cost by moving the
entry toward the trough, or raises the win rate above the book's own realised break-even - 29 %
under trail50 c600 (evidence 4.2) - at n >= 1,000 with a top-1 % share at or under about 20 %.**

---

# 9. CLOSED MISTAKES

The checks that catch these before a number is reported are the backtest audit in appendix 11.1;
a number that has not passed it is a study read.

**This section is an index, not a source.** Most rows restate in one line something a law in 7.4
or a section above states in full - that is the point: it is the form you scan before a run, not
the form you learn from. Where a row and the section it comes from read differently, the section
is the definition and the row is the shorthand. Nothing here is the only home of a rule.

| mistake | the rule |
| --- | --- |
| Close a slot because a sentence is red | A red number is an address. A slot closes on a mechanism or a conjunction search |
| Score a term alone | Money is read on the conjunction; an unconcentrated pool is red by construction |
| Score against a proxy label | Money is the score. Detection, graduation and swing rates describe, never rank |
| Quote a fixed break-even bar | It moves 17 points with the exit and 14.5 with the entry. Use the cell's own |
| Decide a selector's fate under one exit family | Read every cell under both, with tail concentration beside each |
| Sweep the exit on an unselected pool | It returns the shortest clock every time; the pool is mostly dying coins |
| Use a 30 s or 45 s clock as the primary exit on a harvester story | Shorter than the 87 s median playable move. That is a scalper exit |
| Treat "carried by the tail" as a verdict | Report the share against the 9-12 % calibration |
| Accept a cell chosen in sample | Walk it forward; the best ten cells on one half turn +74.9 into -40.5 |
| Close a node on a subset of its members | A seven-wallet node is not two racers |
| Read "his fill is unreachable" as "his decision is unreachable" | Different slots |
| Copy a wallet, a fingerprint or a send set | The reader's edge is his own impact plus his slot |
| Make a specific actor's landed print the event | A condition on tape state, wallet-free, at his decision time |
| Build a term on a wallet, an address cluster or a funding graph | Machinery is the only axis |
| Count wallets as independent traders | Collapse to machines by coin list first; 41 of 77 rows are 7 machines |
| Score money on his coins only | Full tape. His mint list is not a gate |
| Compare "his coins" with a control | Forward conditioning; truncate the path at his arrival |
| Read a candidate set as a universe | Matched negatives come from the full print population |
| Keep or drop a coin on its future (its print count or life) | No coin floor anywhere: a >= 60 prints floor turns a -3.1 %/trade rule into +5.00 % |
| Report a coin-level lift as if a rule could fire on it | Restate per print. 4.63x per coin is 1.21x per print |
| Price the fill one or two slots after the trigger, or drop the trigger's slot | Last print landed by `fire + 115 ms`, both legs |
| Take the next print after the signal as the fill | +8 to +12 pp of pure look-ahead |
| Fill an exit at its trigger level | A stop fills about 3.5 % past the level |
| Book a silent exit at -100 % | The curve freezes price |
| Quote a book without naming its fill model | `SlotEnd` +43.56 as published is `lag_115` +5.38 on the same rule (evidence 3.7) |
| Search terms at one fill and ship at another | A fill model changes which terms matter |
| Treat the slot as the event unit | The event is one tool's run inside the slot |
| Use the program name as the grain | The instruction name and CU order are the machine |
| Gate on windowed flow, net flow or buy share on a curve | They are the price path |
| Fix a bad entry with a cleverer static exit | 104 cells, zero positive; a static cap cuts `W` faster than `L` |
| Add a take-profit to a convex book | It caps the trades that carry it |
| Trim a frozen sentence on held-out performance | Fitting the holdout. Write a new sentence |
| Tune on one day | Score the worst day |
| Reconcile against a recomputed target | Compare against the published book |
| Ship a vsol threshold onto a price metric unsquared | -25 % vsol is `pnl <= -43.75` |
| Charge impact on the real reserve | On `vsol` |
| Subtract our fees from a cohort's net edge | Their number is already net |
| Map an engine metric that is not the same quantity | The finding sets the metric; extend the system |

---

# 10. SOURCES

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
[Bubblemaps - holder analysis](https://blog.bubblemaps.io/how-to-analyze-meme-coin-holders-with-bubblemaps/)

Dev equipment and the playbook:
[Smithii - bundler](https://smithii.io/en/pump-fun-bundler-bot/) .
[cicere - open-source bundler](https://github.com/cicere/pumpfun-bundler) .
[OpenLiquid - volume bot](https://openliquid.io/tools/pump-fun-volume-bot/) .
[Alphecca - volume bot staging](https://alphecca.io/en/blog/pump-fun-volume-bot) .
[jumpbit - bump bot](https://jumpbit.io/en/solana/pumpfun-tools/pumpfun-bump-bot) .
[RPC Fast - sniper infrastructure](https://rpcfast.com/blog/how-to-launches-snipe-pump) .
[Medium - insider's guide to every strategy](https://medium.com/@jump_bit/making-money-on-pump-fun-the-complete-insiders-guide-to-every-strategy-2025-5dd121cfa74d)

Base rates, manipulation, survival:
[arXiv 2507.01963 - A Midsummer Meme's Dream](https://arxiv.org/html/2507.01963v1) .
[arXiv 2607.02823 - survival analysis of 832,941 launches](https://arxiv.org/html/2607.02823) .
[arXiv 2602.14860 - predicting pump.fun token success](https://arxiv.org/abs/2602.14860) .
[Cryptopolitan - graduation rate](https://www.cryptopolitan.com/pump-fun-graduating-tokens-break-to-1-15-of-new-launches/) .
[AssureDefi - rug and pump-and-dump prevalence](https://www.assuredefi.com/blog/meme-coin-rug-pulls-pump-dumps-how-to-spot-and-prevent-fraud) .
[Webopedia - scam tactics](https://www.webopedia.com/crypto/learn/pump-fun-scam-tactics/)

---

# 11. APPENDIX: this repository

The one section that names files, tables and engine vocabulary. Delete it to port the rest.

## 11.1 Where things live

| what | where |
| --- | --- |
| the method, the gates, how a result is recorded | [_!___derive.md](_!___derive.md) |
| the checks every backtest number passes before it is a result | [backtest-audit.md](backtest-audit.md) |
| the open queue | [_!___workflow.md](_!___workflow.md) |
| a node's derivation, step by step, and its rule | [node-derivation/](node-derivation/README.md) |
| every idea, in Door / Event / Permission / Exit | [_!___inventory.md](_!___inventory.md) |
| every standing measurement, by coordinate | [_!___evidence.md](_!___evidence.md) |
| the 26 independent traders and their five nodes | [solo-traders.md](solo-traders.md) |
| frozen sentences, never edited after their date | [study-kernel/frozen-sentences.md](study-kernel/frozen-sentences.md) |
| the pricing kernel every offline book runs through | [study-kernel/](study-kernel/) |
| how the metric system is built and extended (each metric's one definition is the registry) | [_!___metrics.md](_!___metrics.md) |
| every `FillModel` and `CostModelKind` | [fill-and-cost-models.md](fill-and-cost-models.md) |
| the cost derivation | [execution-costs.md](execution-costs.md) |
| the harvester exit that fires from strength | [armed-trailing-stop.md](armed-trailing-stop.md) |
| PnL definition, partial exits, fingerprint axes | [pnl-percent-definition.md](pnl-percent-definition.md) . [partial-exits.md](partial-exits.md) . [fingerprint-ranges.md](fingerprint-ranges.md) |
| the launch-door reconciliation ladder | [../../roadmap/launch-door-rule.md](../../roadmap/launch-door-rule.md) |
| 44 consolidated refuted study lines | [../../history/2026-09-03-refuted-lines-ledger.md](../../history/2026-09-03-refuted-lines-ledger.md) |

## 11.2 Engine vocabulary the anatomy maps to

| rule term | engine |
| --- | --- |
| event on a machine run | a fingerprint tag (`ix_shape` exact ordered build, or `ix_template` / `program`) read `@tag [1sl]`, or `m_slot.* @working` |
| silence before it | `m_flow.* [30sl@1]`: a lagged slot span, so the quiet span cannot read the burst |
| door on launch machinery | fingerprint axes (`max_cost`, `init_buy`, launch `ix_labels`) or the wildcard fingerprint |
| permissions | `m_state.age_sec`, `m_state.liquidity_sol` (real reserve = `vsol - 30`), `m_price.trail_pct [span]` |
| crowd after an age threshold | `m_crowd.buyer_count [age60s]`, `m_crowd.buyer_is_new [age60s]` |
| harvester exit | `m_position.retrace_pct` gated on `m_position.pnl_pct` (a line or an armed stage); `m_position.held_sec` as the clock |
| fill and cost | `lag_115` + `pumpfun_impact`, both legs, in simulate and per-trade reconciliation |
| re-entry | `reentry` with no cap, one position per coin at a time |
| episode / swing decomposition | offline only; no engine metric yet |

**Converted-tag trap:** a `volume` tag converted from a v1 fingerprint carries `creator: true`
and `sticky: true`, because v1 defaulted both on. Left alone, every coin tags its own creator
and the metric reads nothing like the finding.

**cgroup** = `<n_ix>ix:<last label after the colon>` from the creation `ix_labels`.
`bundler_group` = cgroup in `{3ix:Buy, 4ix:Buy, 3ix:BuyExactSolIn}`.

Vocabulary cutoff: instruction names change at the decoder upgrade of **2026-08-30 17:48 UTC**;
a template seen only before it is the same machine under an older name, and nothing merges
across that instant. <!-- pt-ok: cutoff, tape before it carries old names -->

## 11.3 Tables and scripts

Workstation PG, LOGGED schema `census`: `tool_census`, `tool_census_sell`, `gap_breaks`,
`gap_follow`, `mint_summary`, `money_v2..v4`, `break_path`, `hold_breaks`, `hold_money`,
`screen_post`, `screen_pre`, `axiom_push`, `push_money`, `wave_events`, `w1_*`, `rb_ep2`,
`rb_feat`, `rb_k`, `rb_ctx`, `rb_cslot`. Schema `e8` holds the event-unit study; `aa` holds the
machine census (`pxf`, `mach_w`, `mach`); `ixd` / `ixp` / `ixq` / `ixr` are the create-door and
campaign scratch schemas. Study schemas are LOGGED; why, and the `count(*)` check:
[hunter/CLAUDE.md](../../../CLAUDE.md).

Offline books all price through [study-kernel/](study-kernel/README.md) (`kernel.py`, `tape.py`,
`cvx.py`, tracked). Study scripts, their outputs and the kernel's acceptance tests stay local to
the workstation and are not tracked, except the few a rule or a gate needs re-run, named in the
root `.gitignore`; they resolve the repository root by directory depth, so a
script is not interchangeable between this directory and `study-kernel/`. These local JSON files
are **inputs and references**, not dumps - keep them: `8dtx-event-structures.json` and
`ixd-create-door.json` (read by scripts), `8dtx-event-detail.json` and `8dtx-event-templates.json`
(8dtx's decision-print ix detail and template list).

## 11.4 Data quirks that cost time

- psql emits booleans as `'t'` / `'f'` - a pandas filter needs `.isin(['t','true'])`.
- `pg_read_file` cannot see Windows paths; inline the list instead.
- The `wallets` table maps almost nothing - the dictionary is **`wallet_dict`**, and every
  per-wallet aggregate excludes `is_proxy`.
- JSON reference files need `io.open(..., encoding='utf-8')`.
- Metadata `uri` capture starts **2026-08-18** (0 % before, 100 % after), so only two holdout
  weeks exist for anything using the document. <!-- pt-ok: cutoff, tape before it has no uri -->
- ipfs.io and dweb.link rate-limit hard; `pump.mypinata.cloud` serves pump.fun CIDs at about 11
  documents a second on 16 threads.

## 11.5 Standing constraints

- **Helius spend needs approval** - never call without asking.
- **Heavy SQL OOMs the 6 GB WSL VM**: one query at a time, `work_mem <= 128MB`, never pipe-mask a
  psql exit code.
- **Do not repair trade data older than two weeks** - record the hole.
- Deploy target is 2 vCPU / 4 GB: never raise cache caps, TTLs or add a pool on the server.
- Never build a factor on **wallet identity**. The durable axis is ix **structure**.
- Every file plain UTF-8, no BOM, no smart quotes, no em-dashes.
