# Evidence: every standing measurement, with its coordinate

The numbers behind [_!___strategy.md](_!___strategy.md). The ideas are
[_!___inventory.md](_!___inventory.md). The method, its gates and how a result is written are
[_!___derive.md](_!___derive.md). The open queue is [_!___workflow.md](_!___workflow.md).

This file keeps the measurements a rule, a law or an open line stands on. A closed line keeps
one row in the ledger of section 7; its full write-up is in git at `8b01c18b`. Section numbers
are never reused, so the gaps are those closed write-ups.

**Every result is a coordinate, never a verdict.** A measurement fixes all six slots of a rule
whether or not the study thinks about it, so each block below names all six. `none` in a slot is
a fact about the measurement, not an omission - it is usually the reason the number is red.

```
  D  door         which coins are watched at all
  E  event        the print fired on
  P  permissions  state already true at the event
  X  exit         every exit read, all of them
  R  re-entry     tickets per coin        S  size    the clip
  frame           seat . cost . universe . window
```

**The standing frame.** Unless a block says otherwise:

| term | value |
| --- | --- |
| fill | last print landed by decision + 115 ms, **both legs** (`lag_115`); stress column = end of the decision slot |
| cost | 125 bps a leg, 0.000225 SOL a leg, own impact `B / vsol` on the **virtual** reserve |
| pricing | exact curve arithmetic, `price = vsol^2 / k`, `k = 3.219e16` |
| grain | one print = `(mint, slot, tx_index)`, last leg |
| universe | full tape, every instance of the event, the studied wallet's own prints removed |
| clip | 0.2 SOL |
| occupancy | one position per coin at a time; re-entry otherwise unlimited |

Column meanings used throughout: `first/day` = first-per-mint trades a day (the floor is 50);
`be` = the cell's own realised break-even `L / (W + L)`; `top1 %` = share of net in the top 1 %
of trades (calibration in 2.3); `fit | hold` = the week's two halves.

Vocabulary cutoff **2026-08-30 17:48 UTC**: instruction names change at the decoder upgrade, so
nothing merges across that instant. <!-- pt-ok: cutoff, tape before it carries old names -->

```
  1. THE SEAT AND THE COST      what every number here is priced at
  2. THE PRIZE                  what an up-move is worth, and the tail calibration
  3. SURVIVAL                   what predicts that a coin keeps living
  4. ENTRY POSITION AND EXITS   where W and L come from
  5. NODES AND INSTRUMENTS      the 26, the five nodes, and what is reachable
  6. THE CONJUNCTION SPACE      what a search over terms actually produces
  7. THE BOOKS                  every rule that exists, with its coordinate
  8. THE OUTSIDE RECORD
  9. WHERE THE REST IS
```

---

# 1. THE SEAT AND THE COST

## 1.1 The seat, on real fills

| quantity | value |
| --- | --- |
| decision to own-fill-observed | p50 **115 ms**, p90 228, p99 513 (send path 8 ms, ack to fill 107 ms) |
| `entry_slot - target_slot` | p50 **0**; 52.6 % in the trigger's own slot, 81.6 % within one, 93.4 % within two |
| what explains the spread | trigger staleness, 98.7 % of variance; chain load is not a factor |
| P(same slot) by reaction time | < 50 ms 84 %, 50-100 ms 75 %, 100-200 ms 54 %, >= 200 ms 20 % |

**Direction sets the sign of the latency cost** (fill / trigger price):

| action | price moves | cost |
| --- | --- | --- |
| buy into a flush | toward you | 0.889-0.943 |
| sell into strength | toward you | 1.023-1.027 |
| sell on a stop or trail | away from you | 0.964-0.977 |
| buy a breakout | away from you | about -9.87 % of entry per slot |

The stop fill lands about **3.5 % past the level**, across every threshold tested on 29,357
episodes and reproduced on an independent population.

**Pricing an entry from the first print at or after the deadline is a one-print look-ahead worth
+8 to +12 pp a trade**, concentrated on exits into strength.

Measured on 506 real fills we land a median of **one print** behind the trigger (none at all in
50 % of fills), while the ingest clock counts 1.6-1.8x that many prints inside 115 ms. On a
swarm burst the honest fill is behind the swarm, not behind the print.

## 1.2 The cost

| term | value | who charges it |
| --- | --- | --- |
| protocol + creator fee | **125 bps per leg** - measured as `gross * 10000/10125` on 16,544 of 16,854 round dev buys | pump.fun |
| tip + priority | 0.000225 SOL per leg at a 0.0002 tip | Jito + validator |
| own impact | `B / vsol` per leg, on the virtual reserve | the curve |

Cost is U-shaped in size; the minimum sits at `B* = sqrt(F * vsol)`, about **0.126 SOL** on a
70 SOL pool. Break-even gross at the optimum is about **3.3 %**, and a round trip on a coin that
does not move costs **3.2-4.0 %**.

Charging impact on the real reserve (`vsol - 30`) overcharges by `vsol / (vsol - 30)` - 1.6x at
liquidity 50, 11x at liquidity 3. Measured when simulate last got this wrong: 4.62 pp a trade
instead of 0.66 pp.

The engine reads about **1.25 % high** per trade against the offline kernel:
`pnl_engine = (1 + F) * pnl_study + 2 * F * FIX`. Reconcile before believing a promotion.

## 1.3 Curve physics, measured

- `k = vsol * vtok = 3.219e16` exactly on every print; the graduation wall sits at `vsol`
  114.9-115.0.
- **Windowed flow is the price move**: corr 0.975-0.982 at 5 s / 15 s / 60 s.
  `buyshare = (1 + net/gross)/2`, so it carries information only through `gross`, and on a climb
  `gross/|net|` collapses to 1.000 for the bottom six deciles.
- **Silence freezes price.** Booking a silent exit at -100 % manufactured an **85 pp** effect on
  one cohort where the honest number was about 2 pp.
- **The curve floor caps how far a trade can fall.** `price = vsol^2 / k` and `vsol` never goes
  below its opening 30, so a -50 % price move needs `vsol <= v_entry * sqrt(0.5)` and is
  **mechanically impossible below `v_entry` 42.43**. On the door-v3 event, **80.4 % of 50,411
  fires enter below that line and cannot produce a -50 % trade at all** - so an L-rate compared
  across reserves measures headroom, not prediction. Every L-term is read inside a reserve band.
  `study-kernel/cvx_c0b_control.py`.

---

## 1.4 The sequencing race is the biggest cost term, and it dwarfs latency (R1)

The reconciliation that should have come first: take the roster's OWN buy and sell decisions and
price them through our kernel with our 0.2 SOL clip. `study-kernel/cvx_replay3.py`, 131,339
episodes over 25 wallets, 1.07 % bags reported rather than dropped.

**The episode builder is validated against the roster** (`margin_pct` vs the same episodes rebuilt
here): omegoM 0.76/0.94, 88887Q 1.52/1.70, 9999hu 1.33/1.41, 8dtx2t 2.65/2.67, ADkquS 2.32/2.73,
ocBBRK 0.88/0.90, 4HgMCR 1.10/1.12, AbQcLH 2.03/1.64. Position is tracked to the token from
`K = vsol * vtok` (`tokens = K/v_before - K/v[k]` on a buy), so an episode is a real open-to-flat
position, not an aggregate over a coin.

**Four seats on the same decisions:**

| seat | per trade | days positive | SOL | worst day |
| --- | ---: | :---: | ---: | ---: |
| THEIRS - their money, their clip | +0.96 % | - | - | - |
| **RACE** - our clip, sequenced BEFORE their print on both legs | **+2.77 %** | **8/8** | +718.95 | +20.52 |
| **PEER** - same slot, order the leader's coin flip | **-0.03 %** | 3/8 | -7.19 | -29.12 |
| FOLLOW - after their print, zero lag | -2.82 % | 0/8 | -733.33 | -175.92 |
| FOLLOW - after their print, +115 ms | -3.59 % | 0/8 | -933.37 | -225.85 |

**The cost ladder, in points per round trip:**

| term | points |
| --- | ---: |
| **losing the sequencing race** | **-5.59** |
| pump.fun protocol fee | -2.47 |
| our own impact at 0.2 SOL | -0.76 |
| **the 115 ms itself** | **-0.77** |

*The term this program has spent two months on is the smallest of the four.* Being sequenced
after a print costs seven times what the latency costs, because the print we react to is itself
a 0.2-2.0 SOL order in a 40-60 SOL pool - 1.16 % of the pool at the median episode, and the
round trip of that displacement is what we pay twice.

**Why this invalidates a whole family of readings.** An event anchored on a PRINT is a FOLLOW
model by construction: we can only react to the print after it exists, so we always pay its
impact and always lose its slot. Every node study in section 6 is anchored that way. A state
that has been true for seconds is the only anchor that can be a PEER, and "the state changed at
this print" is a print anchor wearing a state's clothes.

**The ceiling with a perfect oracle, at the PEER seat**, which is the honest one for a rule that
fires on the same condition at the same time:

| node | episodes | per trade | days positive |
| --- | ---: | ---: | :---: |
| deep-age big clip | 1,197 | **+2.62 %** | 6/8 |
| mid-tape one-shot | 14,790 | **+0.71 %** | 7/8 |
| hot-tape re-entry | 93,905 | -0.14 % | 3/8 |
| instant launch | 12,187 | -0.25 % | 4/8 |
| quiet deep-age | 7,849 | -0.16 % | 4/8 |

So two nodes have a positive ceiling at a reachable seat and three do not, and the roster's own
clip is NOT the best clip: our 0.2 SOL beats their 0.2-2.0 SOL on their own decisions
(+2.77 % against +0.96 % at the RACE seat) because impact is `B/vsol` and they are ten times
our size.

**What this does NOT say.** RACE includes riding their own later scale-in buying, which is only
available to someone already in before them. PEER assumes we reach the same slot, which the real
seat does 52.6 % of the time (1.1). And the oracle is their wallet list, which never ships
(7.4 law 20). This is a ceiling on the nodes, not a sentence.

## 1.5 The hot-tape node read as one label: five readings, each withdrawn

Before the split by member (1.11) the six hot-tape wallets were read as one label. Five readings
followed, each withdrawn, and each left a guard in [_!___derive.md](_!___derive.md). The chain rows
are [node-derivation/hot-tape-rule-1.md](node-derivation/hot-tape-rule-1.md) section 2.1.

| reading | what it measured | why it is withdrawn | the guard it left |
| --- | --- | --- | --- |
| "they are slow; we land ahead 68-85 %" | time since the last public buy >= 1 SOL, p50 1.29 s | a waiting time: it reads about a second whether or not anyone reacts; their gap from the previous print is 79 ms | latency is excess intensity from an identified trigger print |
| lull, then wake, within the mint | `wake_z` 0.660, `quiet60_z` 0.418 on 93,588 buys against same-coin non-buys | the wake half discriminates (lift 4.5) and costs +5.98 % to react to at 115 ms; the lull half is reachable and predicts nothing; the conjunction covers 1.2 % of their buys | reaction cost and coverage beside every lift |
| their moment, modelled | a within-mint model reads AUC 0.72 on held-out coins | fired on the tape it books -4.2 to -10.6 %/trade, 0/8, on a cheap fill | imitation is bounded by the median trade (strategy law 28) |
| they are the demand | fires pay +2.37 % 7/8 when one of the six buys inside our hold | they put 2-5 % of the move's SOL in: detectors of a move, not its cause | a member's arrival is a diagnostic split, never a door |
| the exit, searched | 480 causal shapes on 95,135 episodes: best -2.35 % behind their print, +0.21 % ahead, ceiling +53 % | swept with D and P empty, to 1800 s on a 19.9 s hold, six wallets pooled; its first grid read +8.30 % from a lookahead that index order fixes to -2.38 % | strategy laws 23, 24, 26, 27 |

What carries forward as method: the within-mint control (same coin, buy against non-buy, holds
every off-chain factor constant) and features in the coin's own units (`wake` 0.605 against
`wake_z` 0.660).

## 1.11 The node is two animals, and only one of them pays (H7)

The exit search of 1.5 ran with D and P empty, which derive phase 8 refuses, and "shorter beats
longer everywhere" is that refusal's signature rather than a fact about exits. This section
replaces it. Five runs, on the 95,135 position-tracked episodes and on the full 12.47 M print tape.

### The question nobody asked

Their own book is a lottery: median trade **-2.07 %**, net margin 1.10 %, and the best 1 % of
their trades produce **180.6 %** of their net, so the other 99 % collectively lose. A perfect
reproduction of their decisions reads **-0.14 %/trade** at the PEER seat (1.4). *Imitating this
node was never going to pay at any AUC*, which is what the within-mint model of 1.5 measures at
AUC 0.72 and -5 %/trade. The only object worth searching is a selector INSIDE their decisions, and
every reading in 1.5 carries the 93,137 buys as a single label (7.4 law 28).

### The split that changes the verdict (7.4 law 27)

Every episode booked at both seats, `node-derivation/hot-tape/cvx_hot_sep2.py`. RACE is sequenced before their
print, which is the seat a rule anchored on STATE can reach and a rule anchored on their print
cannot:

| wallet | n | RACE cap15 | days | worst day | top1 % | book without its top 1 % | biggest coin | FOLLOW cap60 |
| --- | ---: | ---: | :---: | ---: | ---: | ---: | ---: | ---: |
| **8fStGV** | 3,627 | **+2.28 %** | **8/8** | +0.02 | 47.0 | **+8.77 SOL** | 4.0 % | +0.54 % (top1 362 %) |
| **AbQcLH** | 5,464 | **+2.29 %** | **7/7** | +0.73 | 49.3 | **+12.68 SOL** | 3.3 % | -1.12 % |
| **49uohd** | 3,842 | **+2.24 %** | 6/8 | -0.12 | 53.2 | **+8.06 SOL** | 6.3 % | -3.16 % |
| 64hP97 | 28,378 | +0.98 % | 8/8 | +0.02 | 123.7 | -13.11 | 3.6 % | -4.33 % |
| omegoM | 29,926 | +0.12 % | 3/8 | -2.00 | 917.9 | -58.10 | 23.7 % | -2.63 % |
| sssssw | 21,901 | -0.62 % | 1/8 | -7.04 | - | -65.84 | - | -2.74 % |

Pooled, the three:

| cell | n | first/day | coins | SOL | %/trade | days | worst | win | top1 % | body | max coin |
| --- | ---: | ---: | ---: | ---: | ---: | :---: | ---: | ---: | ---: | ---: | ---: |
| **the three, RACE cap15** | 12,933 | **1,913** | 4,327 | **+58.77** | **+2.27 %** | **8/8** | **+0.01** | 51.3 | 49.8 | **+29.50** | **1.8 %** |
| the three, FOLLOW cap15 | 12,933 | 1,913 | 4,327 | -17.02 | -0.66 % | 1/8 | -5.49 | 42.1 | - | -44.54 | - |
| the other three, RACE cap15 | 80,205 | 11,865 | 10,544 | +35.21 | +0.22 % | 8/8 | +0.05 | 45.5 | 494.3 | -138.84 | 12.1 % |
| all six, RACE cap15 | 93,138 | 13,778 | 11,426 | +93.98 | +0.50 % | 8/8 | +0.05 | 46.3 | 216.6 | -109.62 | 4.5 % |

**This is the first cell in this program that is positive on every day, positive with its top 1 %
of tickets removed, and spread widely enough that its biggest coin carries 1.8 % of the net.** It
is a ceiling and not a rule - the oracle is a wallet list, refused by 7.4 law 20 - and it fails the
tail gate at 49.8 % against the 9-12 % calibration. It also lives entirely at the RACE seat: the
same decisions at our own fill read -0.66 %, so **the seat is worth about 2.9 points here** and no
print-anchored rule can reach it.

The three are a different animal from the other three, and it reads at the decision:

| median at the decision | the two cleanest | the other four |
| --- | ---: | ---: |
| coin up over the last 60 s | **+22.3 %** | +9.8 % |
| seconds since the coin last made a new high | **24.3** | 62.8 |
| below the coin's own peak | **-24.7 %** | -32.0 % |
| public prints in the last minute | 141 | 106 |
| SOL through the pool in the last minute | **72.8** | 45.5 |
| net SOL over the last 5 s | **-0.57** | +0.49 |
| coin age | 144 s | 244 s |
| round trips already taken on this coin | 1 | 3 |

*They buy a pullback inside a live up-move; the others buy a deep dip on an old coin.* The pooled reading
(1.5) has this node buying deep below the peak (`dd_peak` 0.408) because the deep-dip members outnumber the
others eight to one on the tape.

### The re-entry terms replicate, and they are not the edge

`node-derivation/hot-tape/cvx_hot_sep.py` puts the three decision-time facts of the August 64hP study into this
node for the first time. Two replicate on a different tape and a different wallet set: **76.5 %**
of episodes return to a coin already round-tripped, and **61.4 %** of those sit below that
wallet's own previous exit, against 61.3 % in August.

As money they are gradients, not levers. At the FOLLOW seat under `cap60`, round trips already
taken runs -4.07 % at the first to **-1.91 %** at the eighth; depth below their own previous exit
runs **-1.91 %** below -30 % against -2.95 % above +25 %. Inside the band where the curve allows a
real fall (`v < 42.43`), "30 % below their own previous exit" reads **+1.68 %/trade on 7 of 7
days** at the RACE seat on 2,874 episodes, and that is the only slice of the re-entry family
positive anywhere.

**Their winners are not separable from their losers at their own decision time.** Within the coin,
the episodes that reach +100 % against those that do not, ranked over 41 features: `v` 0.112,
`age` 0.142, `ep_idx` 0.215 - reserve, age, and how many times the coin has already been traded,
which is **headroom rather than prediction** (7.4 law 21). With money as the label the ranking is
`v` 0.304 and then nothing above 0.082.

### Three public sentences, and all three are red

| run | E | fires | coins | best cell | days |
| --- | --- | ---: | ---: | ---: | :---: |
| `cvx_hot_lvl.py` | pullback of d % from the coin's own swing high, one ticket per down-leg, taken at the knife / at the turn / after stillness | 357,896 | 40,466 | **-3.35 %** (turn, d 40, cap15) | 0/8 |
| `cvx_hot_up.py` | the portrait above as a standing condition: up m % over 60 s, new high within s seconds, given back at most g %, busy tape | 207,688 | 14,228 | **-3.58 %** (cap15) | 0/8 |
| `cvx_hot_mach2.py` | the count of INDEPENDENT machines printing, alone and beside the price state | 178,065 | 9,729 | **-2.99 %** (cap15) | 0/7 |

Three readings worth more than the reds.

**Buy the turn, never the knife.** At every depth and every exit the turn beats the crossing print:
at d = 40, -3.35 % against -4.71 %; at d = 25, -3.31 % against -8.33 %. The August TURN family
says the same and it survives at this seat.

**Tightening the price state makes the book worse, monotonically.** `m25 st20 dd25 n150` reads
-8.20 % where the loose `m10` reads -4.95 %. A state that describes their moment more exactly buys
a worse book - the within-mint model's result (1.5) reproduced through a completely different
construction.

**The independent-machine count is the only term with a monotone money gradient that is not
headroom.** Under five distinct builds printing in the last 5 s books **-4.08 %/trade** over
85,416 fires; twelve or more books -3.17 %; twelve or more with a de-concentrated build mix books
**-2.99 %**. About **1.1 points**, and it does not cross zero. It is the first time the axis 1.4
calls unpriced enters this node's event at all, and within the coin it is what separates the two
animals: `nb5` 0.586 HIGH, `nb20` 0.565, builds new to this coin 0.558, build concentration 0.443
low, professional builds in the last 5 s 0.554.

### The lookahead this run caught

The level rule's one green cell reads **+5.02 %/trade on 8 of 8 days** behind the early-liveliness
door (reserve at age 60 s of 50 to 70). **All of it is the 15.5 % of its fires that happen before age
60 s**, where that door fact does not yet exist: those 2,287 tickets book +251.42 SOL at
**+54.97 %/trade with an 80.8 % win rate**, and the same cell at age >= 60 s reads **-4.12 %,
1/8**. A door fact dated later than the fire is 7.4 law 24 in the D slot, and the 80.8 % win rate
is the tell.

### Verdict

**The node is open, and it is not one node.** Three of its six members carry a ceiling of
+2.2 to +2.3 %/trade that is positive on every day, survives its own tail, and spreads over 4,327
coins - and it exists only at a seat that decides on state rather than on a print. Nothing public
yet reaches it: the best public event books -3.0 % where their own decisions at the same clip and
the same clock book -0.66 %, so **about two and a half points sit in selection** and the rest in
the seat. Measured and empty: the price path in three constructions, the re-entry terms outside one
reserve band, and the machine count as a standalone threshold. Unmeasured: their trigger print and
therefore their reaction time (1.5), and the machine axis as a fitted vector rather than a
threshold.


## 1.12 The trigger, found: a flipper's sell inside a buying frenzy (H8)

1.11 leaves the node open on one question - are the members that pay reacting to a PRINT or firing
on a STATE? Five runs answer it and fill the event slot. `node-derivation/hot-tape/cvx_hot_trig.py`,
`cvx_hot_seat.py`, `cvx_hot_dump.py`, `cvx_hot_which.py`, `cvx_hot_dump2.py`, `cvx_hot_door2.py`.

### Their reaction, measured properly

For every buy, every public print in the 5 s before it is binned by class and lag, against random
times on the same coin within 60 s. The control absorbs the coin's own arrival rate, so 1.00 is no
relationship and a spike at one lag is a reaction at that latency - the measurement 1.5 demands in
place of a waiting time. Lift at the peak:

| wallet | RACE cap15 (1.11) | reacts to | peak lift | lag | avoids |
| --- | ---: | --- | ---: | --- | --- |
| 8fStGV | +2.28 % | a public SELL >= 1 SOL | **8.12** | 25-200 ms, a plateau | burst starts, 0.01 at 0-25 ms |
| AbQcLH | +2.29 % | a burst start | **9.93** | 25-50 ms | - |
| 49uohd | +2.24 % | big buys, then big sells | 5.51 | 25-300 ms | - |
| omegoM | +0.12 % | sells and burst starts | 5.03 | 25-100 ms | - |
| 64hP97 | +0.98 % | nothing under 200 ms | 1.80 | 300-600 ms | - |
| sssssw | -0.62 % | a burst start (a public buy after a 0.4 s gap; a BUY >= 1 reads 4.4, a +3 % print 5.9) | **9.15** | 75-100 ms | sells, 0.35-0.45 |

**Speed does not separate the members that pay** - sssssw and omegoM react as fast as AbQcLH. **The
side does.** The cleanest member buys the print that pushed price DOWN and avoids burst starts; the
worst member buys the print that pushed it UP and avoids sells. That is strategy 1.5's direction
law read off a professional's own reaction.

### Which sell - the event

8fStGV reacts to only 2.2 % of the sells >= 1 SOL on its own coins. Its reaction lag from them is
p25 48 / p50 **81** / p75 145 ms, so firing on the same sell at our 115 ms we land ahead of it on
**33.5 %** - and it does not matter much: our book on the sells it picks is **+0.38 %/trade, 5/8**,
and **+0.21 %** on the two thirds where we land behind it. Firing on EVERY sell >= 1 on the full
tape books **-4.32 %**. So the edge is which sell, and within the coin it reads:

| at the sell print | the sells it buys | the sells it ignores | rank |
| --- | ---: | ---: | ---: |
| distinct builds printing in the last 5 s | **15** | 7 | 0.757 HIGH |
| public prints in the last 5 s | 35 | 13 | 0.736 HIGH |
| buy SOL in the last 2 s | **3.94** | 0.40 | 0.731 HIGH |
| seconds since the coin made a new high | **5.4** | 80.3 | 0.308 low |
| price move over the last 10 s | **+18.5 %** | -0.4 % | 0.649 HIGH |
| seconds since the SELLER bought this coin | **20.5** | 50.8 | 0.367 low |
| size of the sell | 1.98 SOL | 1.48 SOL | 0.651 HIGH |

**A quick flipper takes a profit into a live buying frenzy - many independent machines buying in
these seconds, the coin at a fresh high - and the frenzy absorbs it.** Spelled in public tape state:

```
E  a public sell >= 1 SOL, landing with >= 15 distinct builds printed in the last 5 s,
   >= 2 SOL bought in the last 2 s, a new high within 20 s, and a seller who bought <= 30 s ago
X  cap15      R one per coin      S 0.2      seat  our fill 115 ms after the sell
```

### The event against the door

Full tape, age >= 60 s, `cvx_hot_dump2.py`:

| cell | n | first/day | %/trade | days |
| --- | ---: | ---: | ---: | :---: |
| every public sell >= 1 SOL | 128,221 | 18,973 | -4.26 % | 0/8 |
| + 12 builds in 5 s | 15,019 | 2,222 | -2.12 % | 0/7 |
| + 2 SOL bought in 2 s | 9,627 | 1,425 | -1.41 % | 1/7 |
| the full event | 2,219 | 328 | **-0.68 %** | 1/7 |
| the full event, **on 8fStGV's coins** | 1,383 | 205 | **+1.30 %** | **5/7** |
| the full event, on every other coin | 836 | 124 | **-3.96 %** | 0/7 |
| control: the same frenzy on a BUY >= 1 | 2,759 | 408 | -2.64 % | 0/7 |

Each frenzy term lifts the book, monotonically, from -4.26 % to -0.68 % - **the largest public
improvement on this node**, and the dump side beats the pump side at the same frenzy. On the coins
8fStGV trades the event pays, body positive (+1.20 SOL) and its biggest coin 8.2 %; everywhere else
it loses four points. **E is filled; D is the empty slot, and it is worth about five points.**

### The door, searched on public coin facts

Among the 15,028 frenzy fires, the coin facts that separate its coins from the rest, all computed
from prints before the fire: SOL bought into the coin so far (AUC **0.709**, 312 against 187), prior
frenzies on the coin (0.689, 13 against 3), the coin's peak reserve (0.683), prints so far, the
5-minute print rate, wallets so far. Its coins are bigger, busier and have frenzied before. As
money on every frenzy fire each of them is monotone the right way - SOL bought so far runs -5.32 %
in its lowest quintile to -0.74 % in its fourth - and **none crosses zero**. The best two together
on the full event read **-0.27 %, 4/7, 38 tickets a day**: the door facts buy the coin list's
SHARE (81 % of the fires land on its coins) and not its money.

### Half of "its coins" is its own arrival

On its coins, split by what happens inside our 15 s hold:

| | n | %/trade | days |
| --- | ---: | ---: | :---: |
| 8fStGV buys inside our hold | 400 | **+6.04 %** | 6/7 |
| it does not | 983 | -0.62 % | 3/7 |
| none of the six buys inside our hold | 311 | +0.68 % | 5/7 |

The arrival mechanism of 1.5 again, on a named event: the fire pays when the professional confirms the move
after us. What is left on its coins with nobody from the node arriving is +0.68 % on 311 tickets -
small, and the part a public door would have to reproduce.

### Verdict

**The event slot is filled for the first time on this node**: a flipper's sell absorbed by a
multi-machine buying frenzy, bought within 115 ms and held 15 s, dump side not pump side. It is the
cleanest member's own trigger, measured as a reaction and not as a waiting time, and at our seat it
is reachable - we land behind that member two times in three and still book +0.21 % on the sells it
picks. The sentence is red on the full tape at -0.68 % because **D is empty**: public size,
busyness and prior-frenzy facts move the book to -0.27 % and no further, and a large part of what
"its coins" carries is the professional arriving inside our hold, which no door can name.

The "five points in which coin" are the member's future first arrival: on its coins E books
+4.46 % 7/7 before its first buy there and -0.68 % after, the full-tape number (case step 28).


## 1.14 The permission: a frenzy dies on a young, thin coin (H10)

Under the member's own bracket (take profit +10 %, stop -25 %, 60 s, read off its closing hazard,
case step 29) the losers are a do-not-enter problem: the stop-outs average -31 % and no exit cuts
them without cutting more recoveries; twelve tape-driven exits all book below the bracket. `node-derivation/hot-tape/cvx_hot_perm.py` reads 27 facts at the fire -
overhang, heat, composition, the seller - on the trades that end at the stop against the trades
that end at the take profit, public prints only, node wallets dropped. `cvx_hot_perm2.py` books the
result with every permission applied before occupancy.

### What the dying frenzies share

The facts that separate them are all one fact, maturity: coin age (AUC 0.40, the dying frenzy is
younger), the share of the last minute's buys still held (0.42), reserve (0.42), the share of held
tokens on 20 %+ profit (0.43), the number of public wallets holding (0.43), the ten biggest
holders' share (0.56, higher on the dying one). Heat, composition and the seller separate nothing
(0.47-0.53). The stop-out rate runs 31 % at age 60-77 s and 12 % past 417 s.

Chosen on half the days and scored on the other, without the door:

| permission | fold A test | fold B test | stop-outs |
| --- | --- | --- | ---: |
| public wallets holding >= 368 (fold cuts 368 and 369) | +0.90 % 4/4 | +1.21 % 3/3 | 15-16 % |
| age >= 123 / 193 s | +0.79 % 4/4 | +1.95 % 3/3 | 13-15 % |

### The sentence

Frozen before booking: holders >= 368, age >= 158 s (the mean of the two fold cuts).

| sentence, bracket | first/day | %/trade | days | worst day | body | top 1 % | biggest coin | days 0-3 | days 4-6 |
| --- | ---: | ---: | :---: | ---: | ---: | ---: | ---: | ---: | ---: |
| E | 445 | -0.30 % | 2/7 | -1.19 | -4.33 | - | - | -0.05 % | -0.83 % |
| E x door (earlier frenzy-sells failed) | 124 | +1.22 % | 5/7 | -0.23 | +1.23 | 39.7 % | 11.1 % | +1.66 % | +0.25 % |
| E x holders | 227 | +0.95 % | 7/7 | +0.14 | +1.87 | 36.1 % | 6.2 % | +0.92 % | +1.02 % |
| E x age | 210 | +1.25 % | 7/7 | +0.17 | +2.43 | 31.4 % | 5.1 % | +1.33 % | +1.10 % |
| **E x holders x age** | **141** | **+2.07 %** | **7/7** | **+0.22** | **+3.26** | **17.6 %** | **4.6 %** | **+2.23 %** | **+1.77 %** |
| E x door x holders x age | 55 | +2.32 % | 6/7 | -0.07 | +1.46 | 15.0 % | 5.5 % | +2.84 % | +1.15 % |

The door adds nothing once the permission is in: `abs_rate` was mostly a proxy for maturity (its
rank correlation with age is -0.34). Stop-outs fall to 11.7 %. The threshold is a plateau, not a
point: every cell with holders >= 200 and age >= 158 s is positive on 7 of 7 days at +1.5 to
+2.7 %/trade except one at 6/7, and the chosen cell is inside it, not at its top.

### Verdict

**E x holders >= 368 x age >= 158 s x the member's bracket is the best sentence on this node and
the first to clear days, both halves of the days, body and coin concentration together**: +2.07 %
a ticket, 141 a day, every day positive. It fails the tail at 17.6 % against the 15 % bar, and it
is in-sample - the two thresholds were read off these seven days. The tape ends 09-06 12:00; the
days after it are the holdout. The coin-list split survives (+2.83 % on 8fStGV's coins, -0.15 % off
them), so the arrival door is still worth building on top.


## 1.15 The holdout: the sentence holds on days it has never seen (H11)

Every threshold on the sentence was read off the study tape, which ends 2026-09-06 11:59:59 UTC.
The lake's sealed days 09-03..09-10 are converted to that tape's exact format
(`node-derivation/hot-tape/cvx_holdout_export.py`: every one of the 2.65 M rows the two sources share on 09-04
and 09-05 is identical in time, reserve, amount, side and recipe; token creation times are
identical on all 158,849 shared coins). The recipe key is `aa.pxf`'s `build_core`, recovered
exactly: md5 of the instruction labels joined by `|` after dropping ATA creates, account closes
and memos. The same booking code (`cvx_hot_perm2.run`) runs unchanged, fires only from 09-06 12:00
on, the three days before it as warm-up; the six node wallets are dropped by address.
`node-derivation/hot-tape/cvx_hot_holdout.py`, 4.50 days, 7.8 M prints.

| sentence, bracket | first/day | %/trade | days | worst day | body | top 1 % | biggest coin | stop-outs |
| --- | ---: | ---: | :---: | ---: | ---: | ---: | ---: | ---: |
| E | 467 | -1.25 % | 0/5 | -2.07 | -7.27 | - | - | 21.8 % |
| E x door (earlier frenzy-sells failed) | 117 | -0.53 % | 2/5 | -0.81 | -0.98 | - | - | 18.3 % |
| E x holders | 177 | +1.20 % | 4/5 | -0.37 | +1.19 | 37.8 % | 12.8 % | 13.9 % |
| E x age | 263 | +0.54 % | 3/5 | -0.17 | +0.00 | 99.9 % | 15.6 % | 15.7 % |
| **E x holders x age** | **132** | **+1.90 %** | **5/5** | **+0.17** | **+1.73** | **23.2 %** | **7.0 %** | **10.3 %** |

In-sample the same cell read +2.07 %, 141 a day, 7/7, top 1 % 17.6 %. Out of sample it keeps
92 % of its margin, every day is positive (+0.23, +1.02, +0.17, +0.60, +0.23 SOL), both halves of
the holdout are positive (+2.09 %, +1.64 %), and the stop-out rate is lower than in-sample. The
earlier-frenzy door does not hold (-0.53 %, 2/5): it was fitted, and 1.14 already found
it redundant. Neither permission term alone holds as well as the two together.

### Verdict

**The first sentence in the program to hold out of sample**:

```
E  a public sell >= 1 SOL from a seller who bought <= 30 s ago, landing with >= 15 distinct
   recipes printed in the last 5 s, >= 2 SOL bought in the last 2 s, a new high within 20 s
P  age >= 158 s and >= 368 public wallets holding
X  take profit +10 %, stop -25 %, clock 60 s
R  one position per coin at a time     S 0.2 SOL     seat  fill 115 ms after the sell, both legs
```

It does not ship yet on two counts: the tail (top 1 % 23.2 % against the 15 % bar) and the size of
the book (about 0.5 SOL a day at 0.2 SOL a ticket). The next work is the tail and the ticket count
- a second event from the other paying members, and exits scaled by headroom - before paper.


## 1.16 The second event: one leg is a race, the other is not yet spelled (H12)

The other paying operator is two legs on one 15-wallet recipe set, AbQcLH and 49uohd. Steps 35-38
run the method on both triggers (`node-derivation/hot-tape/cvx_hot_which2.py`, `cvx_hot_ev2.py`,
`cvx_hot_ev2b.py`, `cvx_hot_ev2c.py`).

| trigger | its lag p50 | we land ahead | our seat on its picks, cap15 | bracket |
| --- | ---: | ---: | --- | --- |
| A: AbQcLH buys a public burst start | 47 ms | 5.0 % | -1.36 %, 1/7 | -3.19 % |
| B: 49uohd buys a public sell >= 1 SOL | 260 ms | 96.8 % | **+3.50 %, 8/8** | +1.86 %, 8/8 |

**A is a race.** The burst starts it picks are ~1 SOL buys opening a run after a dip (size rank
0.84, the 10 s move -5.4 % against 0.0 %). On the 5 % where our fill lands before it, it pays
(+7.93 %, n 83); on the rest it loses. A 47 ms reaction to a BUY is DELAY about zero at our seat.

**B is reachable and not yet spelled.** Within the coin the sells 49uohd buys land after a fall
(the 10 s move -10.1 % against -1.1 %), in a busy tape, inside a burst, from a seller at a loss
(-3.9 % against +7.3 %) - a capitulation cascade, the mirror of E. The strongest within-coin rank
is 0.37 (E's was 0.26 / 0.76), and spelled in public tape state the cascade is red and does not
lift term by term: -4.26 % for every big sell, -4.62 % with all five terms, -1.72 % with the
established-coin permission. Two readings of its decision point are refuted: the operator's recipe
set being in the coin (rank 0.50) and a public big dip-buy just before the sell (rank 0.49) - its
partner has usually not bought the coin at all.

### Verdict

The second event is not found. Trigger A is DELAY about zero at our seat. Trigger B's money is
real at our seat (+3.50 % on its picks, 8/8) and its selection is not in any single tape fact
tried; the unused form is the within-coin descriptors FITTED as one score (fall, burst, busy tape,
seller at a loss, size), chosen on half the days.


## 1.20 Rule 1, every slot re-derived on its own pool (H16)

`node-derivation/hot-tape/cvx_r1u_cand.py` writes one table per tape: every public sell >= 0.5 SOL on a coin
aged >= 60 s inside loose floors, with its tape facts and its bracket outcome from a fill 115 ms
later. `cvx_r1u.py` books any variant as a mask plus occupancy and reproduces rule 1 exactly (840
trades, +2.73 %, 4.58 SOL study; 525, +2.53 %, 2.66 holdout). A change is chosen on the study tape by
walk-forward (each half of the days picks its cut by SOL with every fit day positive, the other half
scores it, both folds must move the same way and beat the current value), then booked once on the
holdout.

**The member's selection adds nothing inside rule 1.** The sells 8fStGV also buys book +2.00 % study
/ +2.77 % holdout against +2.82 % / +2.49 % for those it skips, and its buying inside our hold does
not lift the trade (+2.54 % / +0.00 % against +2.77 % / +3.46 %). The money is the public event
itself, so every threshold is read off money.

**Thresholds** (`cvx_r1u_step1.py`, four rounds to convergence):

- "bought >= 2 SOL in the last 2 s" drops out: both folds pick no floor, +0.71 and +0.93 SOL on the
  test halves; the study book goes 4.58 -> 6.22 SOL at +2.98 %/trade, tail 13.7 %.
- Holders wants 150-200 in every round and is not taken: the SOL it adds is upside gaps - with gains
  capped at the median take profit, 368 books 2.89 SOL against 2.27 at 200, with a 13.7 % tail
  against 18.9 %.
- The sell size (1 SOL), recipes in 5 s (15), the new high (20 s), the seller's hold (30 s) and age
  (158 s) hold.

**Exits on the graduation print** (`cvx_r1u_grad.py`). When a coin completes its curve inside the
hold, the exit's fill is the completing buy at vsol 115: rule 1 as booked closes 131 of 840 study
trades there (2.10 of 4.58 SOL) and 87 of 525 holdout trades (1.28 of 2.66 SOL), 102 / 70 of them
entered above vsol 107.2, where a +15 % take profit does not fit under the wall. None of the top 1 %
tickets is one of them. The lake holds post-migration AMM prints for 0-1 % of graduated coins, so
those exits are priced at the migration price (the curve's final state, which the PumpSwap pool
opens at). At a 20 % post-migration drop rule 1 as booked is -0.65 % / -1.04 %.

**The reserve at the sell <= 100 SOL** is taken as a safety term: the money alone does not pick a cap
(fold 1 picks 110, fold 2 picks 100 and loses its test half). It leaves 2 study trades and 0 holdout
trades on the graduation print, lifts %/trade from +2.98 to +3.95 and costs 3 % of the study SOL.

**New terms** (`cvx_r1u_step2.py`): 22 facts, each cut at 12 quantiles from both sides - recipes
over 2, 3 and 10 s, buy and sell SOL over 3-10 s, prints and wallets in 5 s, the seller's profit and
the share of its bag, the drop from the high, the 3-60 s move, the reserve, big sells in 30 s, the
sell against the buying. Two pass the walk-forward with +0.04 to +0.18 SOL on a test half, inside
the 0.15-0.18 SOL (SD) a random 5 % of the tickets carries on a test half by chance
(`toolkit/walkforward.cut_noise`): none is taken.

**Exit** (`cvx_r1u_exit.py`, `r1u_exit_axes.py`), one axis at a time over take profit 12-35 %, stop
20-50 %, clock 60-300 s: the take profit holds at +15 % (the folds pick 22 and 25 %, the second
loses), the stop widens to -40 % (folds 35 and 50 %), the clock to 240 s (both folds).

**Re-entry** (`r1u_reentry.py`): a 30 s cool-down after a stop adds +0.33 SOL on 14 affected trades,
a cap of five entries a coin rests on 8; neither is taken. The first entry on a coin books +5.98 %,
later ones +2.6..+5 %.

**Size** (`r1u_size.py`): a clip in proportion to the reserve books as a flat clip of the same median
on the study tape (1.58 against 1.58 SOL a day at 0.35) and better on the holdout (1.16 against
1.07); flat stays.

**The holdout, one change at a time** (`r1u_holdout.py`):

| sentence | study SOL | study %/trade | holdout SOL | holdout %/trade | holdout days | holdout top 1 % |
| --- | ---: | ---: | ---: | ---: | :---: | ---: |
| rule 1 as booked | 4.58 | +2.73 % | 2.66 | +2.53 % | 5/5 | 17.7 % |
| without "bought >= 2 SOL in 2 s" | 6.22 | +2.98 % | 3.02 | +2.39 % | 5/5 | 18.0 % |
| + reserve <= 100 SOL | 6.04 | +3.95 % | 2.39 | +2.71 % | 5/5 | 16.2 % |
| + stop -40 % | **6.49** | **+4.39 %** | **2.98** | **+3.44 %** | **5/5** | **13.1 %** |
| + clock 240 s (stop -25 %) | 6.20 | +4.05 % | 1.83 | +2.08 % | 5/5 | 21.2 % |
| + stop -40 % + clock 240 s | 7.11 | +4.81 % | 1.80 | +2.09 % | 4/5 | 21.6 % |

The 240 s clock fails out of sample and is not taken; with the stop at -40 % the take profit is
re-checked at a 90 s clock and holds at +15 %. **Updated rule 1**: E a public sell >= 1 SOL from a
seller who bought <= 30 s ago, >= 15 recipes in the last 5 s, a new high <= 20 s ago, age >= 60 s;
P age >= 158 s, >= 368 public holders, reserve at the sell <= 100 SOL; X take profit +15 %, stop
-40 %, 90 s. At 0.2 SOL: study 109 a day, 7/7, worst day +0.38, body +5.86, top 1 % 9.8 %,
biggest coin 3.4 %; holdout 96 a day, 5/5, worst day +0.32, body +2.59, top 1 % 13.1 %, biggest
coin 6.1 %, halves +3.27 / +3.66 %. At 0.35 SOL flat: 1.58 / 1.07 SOL a day, every day positive on
both tapes. The holdout is now read once more; the days after 09-10 are the clean test.

The engine is
[node-derivation/toolkit](node-derivation/toolkit/README.md); the chain of every step is
[hot-tape-rule-1.md](node-derivation/hot-tape-rule-1.md) section 2.

## 1.21 Rule 1, audited before the engine (H17)

`node-derivation/hot-tape/r1_replay.py` replays rule 1 print by print, the way a live engine meets
the tape. It shares no code with the candidate table: each wallet's bag, its last buy, the holder
count, the recipes of the last 5 s and the last new high are kept incrementally from the prints
already seen, and each position scans forward for its own exit.

**The two codes agree to the ticket.** With the members left out (the study's spelling) the
replay books the same 739 study and 433 holdout tickets as 1.20, the same exit print on every one,
and the same SOL to 1e-15.

**The engine's target.** A live engine counts every wallet (leaving the members out needs their
addresses) and meets every leg of a transaction (the tapes keep the last leg; 1.1 % of curve
transactions carry several legs, 2.8 % of sells >= 1 SOL sit inside one). The holdout is
re-exported with every leg (`lake_export --all-legs`, tape `holdout_legs`).

| at 0.2 SOL, 115 ms seat | tickets a day | %/trade | SOL | days | worst day | top 1 % | biggest coin | 95 % interval (coins resampled) |
| --- | ---: | ---: | ---: | :---: | ---: | ---: | ---: | --- |
| study, members out (1.20) | 109.3 | +4.37 % | 6.46 | 7/7 | +0.38 | 9.8 % | 3.4 % | - |
| study, every wallet | 116.5 | +3.55 % | 5.58 | 7/7 | +0.32 | 12.7 % | 4.3 % | +2.05..+4.97 % |
| holdout, members out (1.20) | 96.2 | +3.44 % | 2.98 | 5/5 | +0.32 | 13.1 % | 6.1 % | - |
| holdout, every wallet | 101.6 | +3.65 % | 3.34 | 5/5 | +0.35 | 14.1 % | 5.4 % | +1.92..+5.37 % |
| holdout, every wallet, every leg | 103.4 | **+3.26 %** | 3.03 | **5/5** | +0.28 | 15.5 % | 6.0 % | +1.52..+5.01 % |

**The seat.** Both legs moved together; the book decays gently and stays positive, so no money sits
in a same-slot fill:

| every wallet | 115 ms | 200 ms | 300 ms | 500 ms |
| --- | ---: | ---: | ---: | ---: |
| study | +3.55 % 7/7 | +3.31 % 7/7 | +2.91 % 7/7 | +2.31 % 6/7 |
| holdout, every leg | +3.26 % 5/5 | +2.68 % 5/5 | +2.16 % 4/5 | +2.18 % 5/5 |

**Every recorded cause of a study positive dying on implementation, checked:**

| cause | on rule 1 |
| --- | --- |
| a fill priced from a print that landed after it | the fill is the last print landed by decision + 115 ms, never before the decision print; two codes agree |
| a same-slot fill artifact | +1.8..+3.1 % at a 500 ms fill on both legs, in every spelling |
| a down-triggered exit priced at its trigger | the stop fills at the print landed 115 ms after it; 7-10 % of tickets are stops |
| a candidate table read as a universe | the replay walks every print of every coin and finds the same tickets |
| a wallet-identity term | every wallet counted: +3.55 % / +3.65 % |
| a per-day floor read as a mean | every day positive on both tapes in every spelling at 115-200 ms |
| an exit branch that does not resolve to an index | two codes agree on every exit print |
| exits on the graduation print | 0-2 tickets a tape at 115 ms, 4-5 at 300-500 ms |
| a universe filter hiding in the tape SQL | the holdout is every curve row of the lake; the study tape is every print of the coins created 08-30..09-05 - a creation cohort, selected on nothing after the fire. Its last half day (09-06 to 12:00) has no creation times and no fires, so its tickets a day read about 7 % low |
| timestamps | real milliseconds (4.8); 0.001 % of steps inside a coin run backwards |
| a holder book missing a coin's early life | no ticket on a coin created before its tape starts |
| a router credited as a seller | 0.5 % of study prints are proxied |
| our impact | charged twice (the bag is sold into the tape's reserve, which lacks our SOL): selling into the reserve plus our SOL reads +0.4..+0.5 pp higher |

**The holder book fails.** The replay and the toolkit size each bag from the reserve change (`K/v_before - K/v_after`) and count a wallet while its bag is `> 0`; a full exit leaves a positive float residue almost every time. On 289 busy coins (lake 09-08, end of life) the float book reads a median 647 holders, the exact `token_amount` book 96, and 662 wallets ever traded. `holders >= 368` measures about 368 wallets having bought the coin, which no engine can reproduce and which is not a holder count. The term is re-derived before engine work: [hot-tape-rule-1-engine-plan.md](../../roadmap/hot-tape-rule-1-engine-plan.md).

**What the tapes do not test:** the fixed cost per leg is 0.000225 SOL, and every extra 0.001 SOL a
leg costs 1.0 pp at 0.2 SOL (0.57 pp at 0.35); a buy that fails its slippage bound inside a frenzy
is not modelled; the concurrency peak is 2-3 positions. The holdout has been read at every change
(1.15, 1.20): the days after 09-10, replayed untouched, are the clean test.

Tape export: [toolkit/lake_export.py](node-derivation/toolkit/lake_export.py).

## 1.22 Rule 1 re-derived under the engine's exact semantics (H18)

1.21's holder term was float dust, and a second code sharing the idea could not see it. Here every
term is first checked against an independent exact field of the lake, then spelled exactly as the
engine computes it (`node-derivation/hot-tape/r1_exact.py`, each line citing the engine code it
mirrors), and the rule is re-derived with 1.20's keep rule plus the 5 % chance floor and the ship
bars. Plan: [hot-tape-rule-1-engine-plan.md](../../roadmap/hot-tape-rule-1-engine-plan.md).

**The raw fields hold** (lake 09-03..09-10, 13,271,539 curve rows, `r1_terms_audit.py`):

| term input | independent field | agreement |
| --- | --- | --- |
| the print's SOL | the reserve change to the next print | 99.6 % within 2 lamports; 0.2 % of pairs off by more than 1e-6 SOL |
| tokens moved | `token_amount` | 99.6 % within 1 unit |
| spot | `vsol * vtok = K` | every row within 1e-6; a new high by `vsol` and by spot differ on 25 rows |
| age | first print against `created_at` | median 0 s, p99 13.4 s, never before creation |
| recipes, wallets | `ix_labels`, `wallet` | never missing |
| time | `block_time` | microseconds (the tapes round to ms); 72 steps run backward |
| holders | the exact `token_amount` book | **fails** (1.21): the term becomes distinct buyers, creator excluded |

**What each correction does to rule 1 at its 1.20 thresholds** (0.2 SOL, engine cost kernel):

| | study | holdout | holdout, every leg |
| --- | ---: | ---: | ---: |
| the old reading (reproduces 1.21 to the ticket) | +3.61 % 787 | +3.71 % 457 | +3.31 % 465 |
| holders -> distinct buyers | +3.61 % 787 | +3.70 % 455 | +3.35 % 464 |
| recipes counted with the trigger print | +3.38 % 828 | +3.45 % 482 | +3.12 % 491 |
| the engine's entry fill (last BUY by 115 ms) | +2.96 % 814 | +3.06 % 476 | +2.54 % 485 |
| the engine's 200 ms clock and exit fill | +2.96 % 814 | +3.05 % 476, top 1 % 17.0 % | +2.52 % 485, top 1 % 20.1 % |

The dust count read as a buyer count: swapping it moves at most 2 tickets. The cost sits in the
entry fill.

**The engine's entry fill prices states our buy cannot meet.** `LagMs` entry takes the last BUY
landed by the deadline and ignores sells; its exit leg takes the last print of either side. On the
450 holdout tickets of the final rule, the buy-only fill lands on a different print in 29.1 %, at
a mean +6.88 % (median +1.45 %) dearer entry, and in 1.8 % it prices a state inside an unfinished
transaction (a later leg of the same transaction follows). Pricing both legs with the exit leg's
rule is the fix; the rule below is derived under it.

**The grain the rule is derived on.** The study tape keeps the last leg of each transaction. A
study re-cut from the lake at the engine's grain (`study_exact`: every leg, `t_us`, `vtok`, fires
09-01..09-06 12:00, coins born before 09-01 left out) moves the exit:

| derived on | entry fill | the rule it keeps | holdout, every leg | top 1 % | 95 % interval | 200 ms |
| --- | --- | --- | ---: | ---: | --- | --- |
| last-leg study | buy only | 1.20 unchanged | +2.51 % 5/5 | 20.3 % | +0.66..+4.33 | CI crosses 0 |
| last-leg study | either side | seller <= 90 s | +2.31 % 5/5 | 20.7 % | +0.71..+3.87 | +1.96 % |
| every-leg study | buy only | TP +25 %, clock 180 s | +3.42 % 5/5 | 15.9 % | +0.56..+6.24 | CI crosses 0 |
| **every-leg study** | **either side** | **TP +20 %, stop -60 %** | **+4.44 % 5/5** | **9.8 %** | **+2.19..+6.58** | **+3.69 % 5/5** |

Only the last row passes every bar. Its full book:

| at 0.2 SOL | study, every leg | holdout, every leg | study, last leg (08-30..09-05) | holdout, last leg |
| --- | ---: | ---: | ---: | ---: |
| tickets a day | 109.8 | 100.0 | 110.7 | 98.0 |
| %/trade | +4.51 % | **+4.44 %** | +4.26 % | +4.99 % |
| days positive, worst day | 6/6, +0.00 | **5/5, +0.48** | 7/7, +0.44 | 5/5, +0.59 |
| top 1 %, biggest coin | 12.8 %, 5.0 % | 9.8 %, 5.0 % | 12.6 %, 4.3 % | 8.8 %, 4.5 % |
| capped at the median take profit | +3.19 SOL | +2.37 SOL | +3.81 SOL | +2.80 SOL |
| 200 / 300 / 500 ms, both legs | +4.12 / +3.78 / +3.52 % | +3.69 / +3.34 / +2.76 % (4/5) | +3.77 / +3.53 / +3.16 % | +4.20 / +3.79 / +3.18 % |
| at 0.35 SOL | +4.23 % | +4.17 %, 1.46 SOL a day | - | +4.72 % |

On the holdout, 270 take profits average +20.1 % (they fill at the target, not on gaps), 166 clock
exits -15.0 %, 14 stops -67.2 %. The stop is inert: -60 %, -75 % and no stop book +4.51 / +4.59 /
+4.55 % on the study. The every-leg study's walk-forward keeps every entry threshold; the reserve
cap at 110 is refused inside the chance floor.

**A second code agrees to the ticket.** `r1_exact_check.py` rebuilds every fact, fill, exit and
occupancy per coin from the raw prints with pandas, sharing no code with `r1_exact.py`: all 450
holdout tickets on 296 coins plus 300 coins without one, and 465 study tickets on 300 coins, with
no disagreement.

**The engine's target** is `node-derivation/data/r1_ref_{holdout_exact,study_exact}.parquet`: per
ticket the mint, the slot / transaction / leg / time of the trigger, entry-fill and exit prints,
the reason and the SOL under the engine kernel. What the tapes still do not test: the tip above
0.000225 SOL a leg, failed buys, and the untouched days after 09-10.

Scripts: [r1_exact.py](node-derivation/hot-tape/r1_exact.py),
[r1_exact_check.py](node-derivation/hot-tape/r1_exact_check.py).

---

## 1.23 Rule 1 in the engine: simulate books the reference ticket for ticket (H18)

The engine carries rule 1 in its own vocabulary (`node-derivation/data/r1p_rule.json`): three new
metrics (`m_build_window.unique_builds`, `m_print_wallet.since_buy`, `m_state.on_curve`), the
existing ones for every other term, and the `LagMs` fill with the exit leg's rule on both legs. The
replay is the code simulate runs - the lab's lake load, `run_replay` over one `EngineState`, the
engine cost kernel (125 bps a leg, 0.000225 SOL a leg from `.env`) - in
`hunter/lab/examples/hot_tape_rule1_parity.rs`; `hot-tape/r1_engine_parity.py compare` matches its
positions to the frozen tickets of 1.22 on the trigger print `(slot, tx_index, leg)`.

| corpus | reference tickets | engine positions in the fire window | same trigger print | entry fill, exit print, reason | SOL | reference only | engine only |
| --- | ---: | ---: | ---: | --- | --- | ---: | ---: |
| holdout_exact | 450 | 448 | 448 | all equal | equal to 1.5e-16 | 2 | 0 |
| study_exact | 604 | 603 | 603 | all equal | equal to 1.5e-16 | 1 | 0 |

**The three reference-only tickets cannot happen live.** They sit on two coins the engine retired
as dead: `7ieEr...` drained to 0 SOL at age 276 s and printed nothing for 384 s; `2Q8tH...` sat at
0.59 SOL for 300.5 s at age 74 s. The dead verdict (quiet 300 s, liquidity under 30) removes a coin
and every later trade on it is ignored, in simulate and live alike; the reference has no death, so it
saw both coins revive and fire 20 minutes later.

**AMM prints change nothing.** Loading every venue, as live sees the tape, gives a byte-identical
book on both corpora: `on_curve` keeps the rule off graduated pools, and no position was open across
a graduation.

| holdout_exact, engine vs Python | n | %/trade | SOL | days + | top 1 % | 95 % interval |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| 0.2 SOL, 115 ms, engine | 448 | +4.49 % | 4.02 | 5/5 | 9.7 % | +2.27..+6.61 |
| 0.2 SOL, 115 ms, Python | 450 | +4.44 % | 3.99 | 5/5 | 9.8 % | +2.19..+6.58 |
| 0.35 SOL, 115 ms, engine | 448 | +4.22 % | 6.61 (1.47 a day) | 5/5 | 10.2 % | +2.00..+6.33 |
| 0.35 SOL, 115 ms, Python | 450 | +4.17 % | 6.56 (1.46 a day) | 5/5 | 10.3 % | |
| 0.2 SOL, 200 ms, engine | 445 | +3.74 % | 3.33 | 5/5 | 9.8 % | +1.59..+5.83 |
| 0.2 SOL, 200 ms, Python | 447 | +3.69 % | 3.30 | 5/5 | 9.9 % | +1.54..+5.80 |
| 0.2 SOL, 500 ms, engine | 436 | +2.80 % | 2.44 | 4/5 | 13.5 % | +0.55..+4.96 |
| 0.2 SOL, 500 ms, Python | 438 | +2.76 % | 2.41 | 4/5 | 13.6 % | +0.53..+4.88 |

Study, engine: 603 tickets, +4.47 %, 6/6, top 1 % 12.9 % (Python 604, +4.51 %).

Not measured here: the days after 09-10 (the clean test), a tip above 0.000225 SOL a leg, failed
buys, and live paper, which books `worst_case` fills rather than this seat.

## 1.24 Rule 1's exit, re-read on its own trades: nothing beats the bracket out of sample

Rule 1's entry frozen, eleven exit families from the inventory and the stepped trail, each read the
way the engine reads an exit (every print after the fill, the 200 ms tick grid, the `LagMs(115)`
exit leg, the engine kernel, one position per coin). `hot-tape/r1b_exit.py` reproduces rule 1's
frozen tickets exactly through the same evaluator (604 / 604 study, 450 / 450 holdout, same
prints, same SOL), and its docstring carries the bars, fixed ahead of every comparison.

**Where rule 1's book loses (study, 0.2 SOL).** The +20 % take profit books +15.08 SOL on 371
trades; the 90 s clock -6.66 on 210; the stop -2.98 on 23. 85 % of the clock exits reach +3 %
first and half reach +10 %, then give it back to a median -5 %; after a take profit, 79 % of
coins reach +30 % within 120 s. Only 31 trades never rise.

| family (study, 0.2 SOL, 115 ms; rule 1 = 5.44 SOL) | best spec | SOL | fails |
| --- | --- | ---: | --- |
| stepped trail, 198 ladders | peak +15 %, trail 8 % | 4.43 | folds, top 1 % |
| trail while up (`arm_above_pct`) | up 15 %, trail 3 % | 5.52 | folds |
| sell into a buy | up 15 %, a buy of 3 SOL | 5.77 | folds |
| wall target | 0.4 x (115 / vsol)^2 - 1 | 6.35 | top 1 % 15.5 |
| clock | 180 s | 6.19 | fold 1 |
| tighter stop | -30 % | 5.17 | folds |
| cuts under water: new buyers stop, SOL bought, recipes, stall, creator sells | | 4.19 - 5.53 | folds |
| wide stepped trail (round 2) | peak +30 %, trail 30 %, 180 s | 6.83 | top 1 % 25.3 |
| half out (round 2) | half at +20 %, the rest at 0.6 x the wall, 180 s | 6.07 | fold 1 |
| wall target + clock (round 2) | 0.3 x the wall, 180 s | **7.51** | passes every bar |

Round 2 follows round 1's results and is marked as such. The stepped trail loses on
the winners: 192 of them dip 8 % or more after +15 % on the way to +20 %, and the trail sells the
dip behind our seat (-6.4 SOL against +2.3 kept on the trades that time out). A cut under water
sells trades that recover: the winners' own trough is -5.8 % at the median.

**The holdout, booked once on the survivor, refutes it.**

| holdout | rule 1 | wall target 0.3, clock 180 s |
| --- | --- | --- |
| 115 ms | 450, +4.44 %, 3.99 SOL, 5/5, top 1 % 9.8 | 443, +4.17 %, 3.69 SOL, 5/5, top 1 % 16.2 |
| 200 ms | +3.69 %, 3.30 SOL | +3.69 %, 3.26 SOL |
| 500 ms | +2.76 %, 2.41 SOL | +2.77 %, 2.36 SOL |
| 0.35 SOL | 6.56 SOL | 6.05 SOL |

The study gain (+2.07 SOL) does not carry: the longer clock lets losers run to the stop (26 stops
against 14) and the book leans on its tail.

**The two tail-bar failures, read on the holdout AFTER the survivor** (so the holdout chooses
nothing here; the days after 09-10 do):

| book (0.2 SOL, 115 ms) | study | holdout | holdout 200 / 500 ms | holdout at 0.35 SOL |
| --- | --- | --- | --- | --- |
| rule 1 | 5.44 SOL, +4.51 %, top 1 % 12.8 | 3.99, +4.44 %, top 1 % 9.8, body 3.60 | 3.30 / 2.41 | 6.56 |
| wide trail (peak +30 %, trail 30 %, 180 s) | 6.83, +7.53 %, median trade +0.6 %, top 1 % 25.3 | 3.32, +4.96 %, median -0.1 %, top 1 % 23.1 | 3.12 / 2.99 | 5.49 |
| wall target 0.4, clock 90 s | 6.35, +5.84 %, top 1 % 15.5 | **4.57, +5.55 %, top 1 % 13.0, body 3.98** | **3.98 / 3.37** | **7.61** |

The wide trail is a lottery (half the trades lose, five trades carry a quarter of the net) and loses
the holdout, as the tail bar said. The wall target at 0.4 with rule 1's 90 s clock misses the study
tail bar by 0.5 points and beats rule 1 on every holdout line; it differs from the refuted survivor
only in the clock, and a clock past 90 s fails out of sample for the second time (240 s in 1.20).
It is the exit candidate for the days after 09-10, next to rule 1's bracket.

## 1.25 Rule 1b in the engine: the wall target, booked ticket for ticket (H18)

Rule 1b (Flip-Catch - Room; rule 1 is Flip-Catch - Bracket) is rule 1's entry with the exit of
1.24's post-selection read: sell at 40 % of the entry's
room to the graduation wall, stop at -60 %, clock at 90 s. The engine carries the target as one new
position metric, `m_position.room_taken` = `pnl / ((115 / vsol at the fill)^2 - 1)`, in percent, so
the exit is `room_taken >= 40` OR `held >= 90`, with `stop_loss` 60 and no `take_profit`
(`node-derivation/data/r1b_rule.json`). `vsol at the fill` is the depth of the last print folded when
the fill confirms; in simulate that is the fill print, the `v0` of `r1b_exit.py`.
`r1b_exit.py ref` freezes the Python tickets (`r1b_ref_*.parquet`), and the same replay and compare
as 1.23 read them.

| corpus | reference tickets | engine, same trigger print | fill, exit, reason | SOL | reference only |
| --- | ---: | ---: | --- | --- | ---: |
| holdout_exact | 412 | 409 | all equal | equal to 1.4e-16 | 3, all on `7ieEr...` |
| study_exact | 543 | 542 | all equal | equal to 1.5e-16 | 1, on `2Q8tH...` |

The four reference-only tickets sit on the two coins the engine retires as dead (1.23).

| engine book, all venues | rule 1 | rule 1b |
| --- | --- | --- |
| study, 0.2 SOL, 115 ms | 603, +4.47 %, 5.39 SOL, 6/6, top 1 % 12.9 | 542, +5.81 %, 6.30 SOL, 6/6, top 1 % 15.6 |
| holdout, 0.2 SOL, 115 ms | 448, +4.49 %, 4.02 SOL, 5/5, top 1 % 9.7, body 3.63 | 409, +5.63 %, 4.60 SOL, 5/5, top 1 % 13.0, body 4.01 |
| holdout, 200 ms | +3.74 %, 3.33 SOL, 5/5 | +4.96 %, 4.01 SOL, 5/5 |
| holdout, 500 ms | +2.80 %, 2.44 SOL, 4/5 | +4.23 %, 3.40 SOL, 5/5 |
| holdout, 0.35 SOL | +4.22 %, 6.61 SOL | +5.36 %, 7.67 SOL |
| holdout 95 % interval, 115 ms | +2.27..+6.61 | +3.07..+8.28 |

The holdout chose rule 1b's exit (1.24), so this table shows the engine books what Python booked; it
certifies nothing. The days after 09-10 do, rule 1 and rule 1b side by side. A position the live
engine adopts on restart reads its entry depth back from `strategy_positions.extra`.

## 1.26 Rule 1's entry loosened for more trades a day: no term adds money

Rule 1's entry, one term loosened at a time on `study_exact`, each step booked with both exits
(Bracket: +20 %, -60 %, 90 s; Room: 0.4 x the room to the wall, -60 %, 90 s) through `r1b_exit.py`'s
evaluator, which rebuilds rule 1's 604 study tickets on the same trigger print. The trades a step
adds are judged on their own; the eight bars sit in `hot-tape/r1c_loosen.py`'s docstring, fixed
before any step was booked. The holdout is not read (it chose rule 1b's exit, 1.24).

Rule 1 on this tape: Bracket 604 tickets, 109.8 a day, +4.51 %, 0.99 SOL a day; Room 543, 98.7 a
day, +5.84 %, 1.15 SOL a day.

**Every term fails its first step** (0.2 SOL, 115 ms; Bracket / Room):

| term, rule 1 -> first step | added a day | added %/trade | added days + | rule 1 tickets lost a day | net SOL a day | fails |
| --- | --- | --- | --- | --- | --- | --- |
| sell size 1.0 -> 0.75 | 51.3 / 46.0 | +1.75 / +1.47 | 4/6, 4/6 | 23.6 / 22.7 | +0.078 / -0.122 | days, top 1 % 49 / 82 |
| recipes in 5 s 15 -> 14 | 26.7 / 25.1 | +1.79 / +2.60 | 4/6, 4/6 | 9.5 / 9.5 | +0.022 / -0.042 | days, top 1 %, graduation |
| new high 20 -> 30 s | 7.8 / 7.5 | -3.78 / -11.00 | 1/6, 0/6 | 0.4 / 0.4 | -0.045 / -0.145 | loses |
| seller bought 30 -> 45 s | 19.8 / 16.5 | -2.30 / -3.95 | 2/6, 1/6 | 9.8 / 8.5 | -0.052 / -0.148 | loses |
| age 158 -> 120 s | 17.6 / 16.5 | -3.54 / -7.34 | 3/6, 2/6 | 2.4 / 3.3 | -0.148 / -0.289 | loses |
| buyers 368 -> 300 | 23.6 / 22.7 | -0.55 / +1.14 | 2/6, 3/6 | 2.7 / 4.2 | -0.055 / +0.046 | loses / days, top 1 % 82 |
| reserve 100 -> 104 | 25.6 / 30.0 | +1.80 / +2.16 | 5/6, 5/6 | 9.1 / 9.8 | -0.071 / -0.009 | net, graduation 17 / 2 |

**No later step passes either** (`r1c_loosen.py grid`, read past the first failure, chooses
nothing). Sell size 0.5 adds 92 a day at +1.80 % and costs 42 of rule 1's: net -0.045 / -0.226.
Recipes down to 8: the added trades fall to -0.16 / +0.30 %. New high to 90 s, seller to 300 s,
age to 60 s: the added trades lose at nearly every step, to -11 %/trade. Buyers down to 100: the
best net of the grid, +0.086 / +0.076 SOL a day, on added trades of +0.59 / +0.65 % positive 3 of 6
days. Reserve 110: added +2.22 %, 6/6 on the Bracket, and 119 of its 256 added trades exit on the
graduation print, the price 1.20 removed.

**Each first-step band as its own rule** (`slices`: the band only, every other term at rule 1, its
own occupancy, so it displaces nothing):

| band | a day | %/trade | days + | top 1 % | overlaps a rule 1 position |
| --- | --- | --- | --- | --- | --- |
| sell size 0.75-1.0 | 87.5 / 80.4 | +2.90 / +3.06 | 6/6, 6/6 | 24.5 / 28.8 | 70 % / 72 % |
| reserve 100-104 | 30.5 / 33.5 | +1.53 / +2.12 | 5/6, 5/6 | 21.0 / 12.1 | 46 % / 40 % |
| the other five | 16-36 | -7.79 .. +1.74 | 2-4 of 6 | 27-92 where net > 0 | 9-75 % |

The sell-size band pays every day and fails the tail bar; 70 % of its tickets sit on a coin rule 1
already holds, in the same frenzy, so it is mostly a second clip on rule 1's trades (size, not
volume), and the tape cannot price our first buy's impact on the second. It was read after the
walk, so it certifies nothing.

**Reading.** Every added slice earns well under rule 1's own trades (+1.8..+2.6 % at best against
+4.5..+5.8 %) and none is positive every day: rule 1's thresholds sit where the marginal trade is
near break-even, which is why 1.22's walk-forward kept every one. A looser entry also displaces:
it takes the coin earlier and its position blocks a later rule 1 ticket on the same coin (the sell
size and recipe steps lose one rule 1 ticket for every two to three they add). There is no rule
1c; rule 1's volume is set by its event, and more trades a day need a second event (1.16).

## 1.27 Derive 5.2 calibrated on the hot tape's known triggers

`node-derivation/hot-tape/b2_leftover.py` runs derive 5.2 (`toolkit.seat.leftover`) on five member x
trigger pairs of the study tape whose fate is known, the members out of every public print. Acted
ticket: the latest print of the class <= 300 ms before each of the member's decisions; our fill
115 ms after it. Ignored: prints of the class on the same coins that it does not buy within 300 ms,
up to 4 per acted ticket per coin. The anchors were fixed before the run: rule 1's trigger must
pass; AbQcLH's race, sssssw's burst start and the class 8fStGV avoids must be killed. The **behind**
rows are the tickets where our fill lands after the member's buy.

| member x trigger | anchor | covers its decisions | behind: lag p50 | cost p50 | cost >= 2 % | peak at its hold p50 | break-even first | ignored: peak at hold p50 |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 8fStGV x sell >= 1 SOL | pass | 93.2 % | 56 ms | 1.41 % | 38.5 % | +7.46 % (10.3 s) | 59.3 % | +5.23 % |
| AbQcLH x burst start | kill | 38.0 % | 44 ms | 4.55 % | 86.2 % | +1.69 % (19.3 s) | 53.8 % | +0.22 % |
| sssssw x burst start | kill | 57.9 % | 80 ms | 1.18 % | 46.1 % | +1.01 % (21.5 s) | 57.8 % | +0.12 % |
| 8fStGV x burst start | kill | 5.8 % | 91 ms | -4.08 % | 22.5 % | +8.46 % (10.3 s) | 67.5 % | +0.38 % |
| 49uohd x sell >= 1 SOL | read only | 24.8 % | 48 ms | 2.69 % | 62.5 % | +6.42 % (25.4 s) | 54.2 % | +6.95 % |

A driftless price reaches break-even first on 49.1-49.2 % of these tickets. Our fill lands after the
member's closing sell on 0-0.5 %.

- **The absolute read on the behind row passes rule 1's trigger and kills AbQcLH on cost** (4.55 %
  against 1.41 %). The 2 % line sits between them.
- **Alone it passes noise.** sssssw (cost 1.18 %, peak +1.01 %) and the burst start 8fStGV avoids
  both pass. Phase 4 drops sssssw (-0.62 % at RACE, 1/8 days) and 5.1's coverage drops the avoided
  class (5.8 % of its decisions against 93.2 % for its trigger), so the veto holds only in the
  derive order. The coverage line is 10 %, between the two.
- **Ahead tickets count the member's own buy.** On rule 1's trigger 71 % reach break-even first
  when we land ahead against 59 % behind; AbQcLH 87 % against 54 %. The veto reads the behind row.
- **The within-coin peak excess over ignored prints is not the veto.** It reads -1.93 (p5 -2.69) on
  rule 1's trigger: the acted fills pay a rebound, +3.03 % more reaction cost than the ignored prints
  on the same coin, so as a veto it kills rule 1.
- **Break-even first is not the veto.** It sits above the null and above the ignored prints on every
  anchor but the avoided class, sssssw included (57.8 % against 45.2 %): a buy cluster moves price
  up for a while whether or not it pays.
- **The horizon is the hold p50.** At the hold p10 (1-4 s) the peak is below zero on every row; at
  the hold p90 the ignored prints cover the cost too (+4..+18 %).
- **49uohd's sell >= 1, read only:** behind its buy the cost is 2.69 %, a kill on that class at our
  seat; ahead of it (57.5 % of tickets) its own buy is the leftover. Its 5.1 peak is a node print
  (6.4 at 25-50 ms); the sell class is its second band (3.3-3.7 at 150-300 ms, 1.16).

Script: [b2_leftover.py](node-derivation/hot-tape/b2_leftover.py); the function is
`seat.leftover` in [node-derivation/toolkit](node-derivation/toolkit/README.md).

# 2. THE PRIZE

## 2.1 The episode census

```
D none   E none (a census, not a rule)   P none   X n/a   R n/a   S n/a
frame  one week, 11.76 M prints, 115,646 coins. An episode is a trough -> peak leg,
       closed when price retraces 20 % from the peak.  study-kernel/episodes.py
```

| quantity | value |
| --- | --- |
| episodes | 100,676 on 42,106 coins |
| episodes reaching +100 % | 21,711 |
| **playable** big episodes (age-0 launch ramps excluded) | **8,749 = about 1,250 a day** |
| episodes peaking inside one slot | 8.7 % |
| of the move that survives a 115 ms fill | **90.7 %** |
| median **playable** episode duration | **87 s** |
| coins producing any playable big move | 4.7 % (5,415) |
| P(big) on a coin's first episode vs a later one | 29 % vs 16 % |
| base rate | one playable big episode per **542 prints** |

**Age-0 is not tradeable.** Those coins die immediately to feed sniper bots, so an entry there is
neither reachable nor survivable, and it is where the -50 % tail lives. The playable band is the
mid tape, mass at 60-300 s. **The playable 87 s governs harvester exit design.**

Zigzag decomposition of the same tape at a 15 % retracement (`cvx_export.py`), used as the parent
for the convexity studies: episodes >= 50 % are 59,656 on 29,580 coins; >= 100 % are 21,217 on
15,339. P(second 50 % episode | first) on keep-create coins with >= 20 prints is **39.8 %**.

## 2.2 The trough book - the ceiling, and what the exit is worth on a good entry

```
D keep-create   E every episode LOW (LOOK-AHEAD: knows the low and that the episode is real)
P none          X tp100/trail50/c1200 . trail50 c600 . clock 30
R unlimited, one per coin   S 0.2   seat lag_115 both legs   universe one week
```

| pool | exit | n | first/day | %/trade | days+ | win | top1 % |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| episodes that rose >= 50 % | **tp100 tr50 c1200** | 28,192 | 2,754.9 | **+26.99** | **8/8** | 43.8 % | **12.2** |
| episodes that rose >= 50 % | trail50 c600 | 24,154 | 2,754.7 | +19.66 | 8/8 | 35.4 % | 48.6 |
| episodes that rose >= 50 % | clock 30 | 30,665 | 2,755.3 | +20.44 | 8/8 | 62.3 % | 22.7 |
| **all** episode lows | tp100 tr50 c1200 | 59,138 | 6,354.5 | **-1.30** | 3/8 | 23.3 % | - |
| **all** episode lows | clock 30 | 77,743 | 6,356.3 | +4.12 | 8/8 | 49.0 % | 71.6 |

Two things this settles:

- **The shape is reachable at our seat**, and the tail-preserving exit is the right one **when
  the entry is good**. What is missing is a decision-time generator for the trough.
- **The ceiling depends on which episodes are counted.** Firing at every episode low, including
  the small ones, is negative under the harvester exit and positive under a 30 s clock. Any
  "fire at the trough" number must say which episode set it means.

## 2.3 The tail calibration - what a real convex book's tail looks like

The prize book above, subsampled to matched trade counts, 200 draws each. This is the reference
the `top1 %` column is read against.

| n | positive draws | top-1 % share of net (p10 / p50 / p90) |
| ---: | ---: | --- |
| 150 | 100 % | 7.2 / **10.5** / 23.0 % |
| 250 | 100 % | 5.3 / 9.3 / 18.3 % |
| 1,000 | 100 % | 8.6 / **12.3** / 16.4 % |
| 3,000 | 100 % | 10.3 / 12.2 / 14.4 % |
| 28,192 | 100 % | 12.2 / 12.2 / 12.2 % |

**A real convex book puts 9-12 % of its net in its top 1 % of trades and is positive in
essentially every subsample.** A cell holding 50-270 % of its net there is not a thin version of
this book; it is noise around zero. Reproduce: `study-kernel/audit_bar.py`.

---

# 3. SURVIVAL

## 3.1 The launch-build door, 30 days

```
D launch build ranked by its TRAILING wall rate (a wall counts only if reached before day D)
E none (a token-level screen)   P none   X none   R n/a   S n/a
frame  2026-08-08 .. 09-07, 736,800 coins. Days 08-09 .. 08-30 are a 22-day holdout no pass
       has fitted on.  scratchpad/x1.sql, x2.py, x5.py
```

**On every one of the 30 days**, door coins reach the soft label (reserve 60 with the peak 60 s
or more after birth) at **8.5-19.6 % against a pool of 1.9-4.3 %** - a **4-6x lift with no decay
across the holdout**.

| door | coins | builds | forward wall rate | pool |
| --- | ---: | ---: | ---: | ---: |
| trailing wall >= 10 %, 30+ coins, outside bundler groups | 1,059 | 9 | 36.8 % | 0.7 % |
| trailing wall 4-10 % | 4,312 | 16 | 8.1 % | |
| **trailing SLOW wall >= 5 %** (wall after 60 s of life) | 1,507 | 10 | 8.6 % slow walls | 0.76 % |

The first row is mostly instant pumps in disguise (depth 60 by age 15 s, then decay, negative at
every entry age). **The slow-wall door excludes them and is the runner selector**, 11x the pool.

**Refresh cadence is daily.** The money inside the door is a rotating client: one or two builds a
day carry it and they live one to two days. Refreshed from the previous day alone it reads the
same forward (8.2 % slow walls on 1,475 coins, 13 builds).

**This is a screen, not a trade.** A door is never a trade; it waits for an event.

The last-leg week tape carries these labels (`study-kernel/cvx_swdoor.parquet`),
joined from `launch_build_day_stats` (previous UTC day's launches of the creation
`ix_labels`, the same feed the engine stamps). `aa.door3.n_prev` is a running total
inside that table and is the wrong window for this door. On the tape: previous-day
stats cover 119,234 of 127,833 tokens (92.6 % of prints). The slow-wall door
(`n_prev >= 20`, `sw_prev >= 5 %`, not bundler) is **7,616 tokens, 34 creation
hashes / 22 cgroups, 6.0 % of tokens and 17.3 % of prints** (~1,130 tokens a day;
7-16 hashes a day). The shipped 8 % cut is 3,906 tokens, 11.1 % of prints. Tokens
with no previous-day rate stay out of the door. Reproduce:
`study-kernel/cvx_door_export.py`. C2's slow-wall cell is unblocked.

## 3.1a The door's ticket supply is a rotating client, and that sets the sample size

```
frame  last-leg week, 6.76 days of prints, doors from 3.1.
       study-kernel/cvx_client_diag.py, cvx_build_holdout.py, cvx_c2_diag.py
```

A one-week print tape holds **34 slow-wall builds (26 at the 8 % cut), and a build stays in
the door a median of 2 days** (p90 five). The door refreshes daily and keeps finding one, so
the 30-day survival lift never decays (3.1) - but the identity behind it turns over every
couple of days.

That decides what a book behind this door actually is. **One creation build,
`fb455027566e5992b8a10792b1d8d7f6` (`5ix:ix#6f`), in the door on days 2-3 only, carries:**

| cell | trades | its trades | net SOL | its share of net |
| --- | ---: | ---: | ---: | ---: |
| C2 slow-wall 5 % + E + P, trail40 c600 (6.4) | 1,006 | 708 | +15.51 | **82.1 %** |
| door-v3 MONEY, shipped exit (7.0) | 1,666 | 710 | +5.39 | **220.7 %** |

It is also why "the creator has not sold" reads 11.7 % of door fires on day 1, **81.5 % on
day 3** and 12.4 % on day 6 while the age band and the reserve band hold 48-56 % and 77-87 %
every single day. The creator field is clean - identified on 95-97 % of tokens every day, and
creator-in is a flat 16-21 % tape-wide once the creator trades. The swing is the client
changing, not the data (`cvx_creator_diag.py`).

**So the out-of-sample unit behind this door is the BUILD, not the day.** A 1,006-trade cell
that is really 19 builds with one of them carrying four fifths of the money is 19 draws, not
1,006, and a day-split puts that one draw entirely inside the fit half. Every day-based
fit/hold number reported behind this door - including 6.4's and 7.0's - is splitting on the
wrong axis. The gate that replaces it is in
[_!___derive.md](_!___derive.md) section 11.

**A week of prints is about one profitable client.** Thirty days would hold roughly 150
slow-wall builds. That, and not another AND, is what the door-behind sentences are waiting on.

## 3.2 Coin-level lift, and the denominator that decides whether a rule can fire on it

| population | lift on "produces a playable big episode" |
| --- | --- |
| door 20+/8 %, coin level | 3.02x fit / **3.17x holdout** |
| door + initial buy >= 2 SOL, coin level | 3.13x fit / **4.63x holdout** |
| door 20+/8 %, **per print** | **0.98x** |
| door + initial buy >= 2, **per print** | **1.21x** |

`door + ib >= 2` is **2.0 % of coins and 6.1 % of prints**, and that ratio is where the lift
goes. **Restate every selection claim per print before using it in a plan.**

## 3.3 The metadata document

```
D document parsed from tokens.meta->>'uri'   E any buy >= 0.5 SOL
P age >= 300 s . reserve 50-85 . >= 8 professional builds
X trail 40 % cap 1800   R unlimited, one per coin   S 0.2   seat lag_115
frame  uri capture begins 2026-08-18, so only two holdout weeks exist
```
<!-- pt-ok: cutoff, the tape before that date has no uri -->

Single-field gradients on a 30 % trail - none crosses zero alone: telegram **6.6 points**,
website 5.8, description > 80 chars 3.3.

The conjunction ladder (mean % of clip a trade):

| step | mean %/trade |
| --- | ---: |
| every documented coin | -9.41 |
| + door (website AND (telegram OR desc > 80)) | -6.42 |
| + age >= 300 s | -3.64 |
| + reserve 45-90 | -1.22 |
| + >= 4 professional builds | +0.75 |
| + >= 8 professional builds | **+1.39** |

Holdouts of the frozen sentence: **+1.46 % and +0.64 % a trade, 4 of 7 days each, +3.77 SOL on
2,156 tickets.** Every week is under 1 SE from zero. The positive region is a **plateau**: every
age x build combination inside reserve 50-85 is +1.5 to +2.4 %, and widening to 40-95 flips it
negative, so the reserve term is load-bearing. Holder-book terms make it worse.

Professional build = this week's buy side, <= 50 wallets and >= 200 prints: one operator's own
tool rather than a retail terminal.

**Weakly positive out of sample. A lead for paper, not a rule.**

## 3.4 The creator permission

Inside the slow-wall door, entry at a fixed age, slot-end fill: **+3.3 / +4.1 / +2.9 / +1.8 /
+2.5 %** at ages 15 / 30 / 60 / 120 / 240 s on a 600 s clock. With the creator already sold, the
same entries read **-4 to -9 %** at every age.

Corpus-wide: creator-sold entries book **-4 to -8 % a trade, 0 of 7 days in each of four weeks**,
36k-89k fires a week; the ranking sold < holds < never-bought holds every week.

**A permission, never an edge** - and it is the only term that raises total SOL in every top cell
of both conjunction scans (section 6).

## 3.5 Coin shape by launch group

Post-cutover, 5+ prints. The bundler launch `3ix:Buy` (dev buy 0.9 SOL) peaks at a median depth
of 78; **39 % reach the wall** and 36 % dump by half from the peak. The four mainstream groups
the readers trade (`6ix:Transfer`, `5ix:BuyV2`, `7ix:Transfer`, `6ix:BuyExactSolIn`, 55k coins)
peak at 34-39, only **7-10 % ever reach 60**, 5-7 % dump by half, median life about 200 s and
9 buyers.

**In that universe a runner is a one-in-twelve event and the door does not select it; the event
must.**

## 3.6 L-door on the documented-project book (C1)

```
D keep-create + documented-project (website AND (telegram OR desc > 80))
E any buy >= 0.5 SOL
P age >= 300 s . reserve 50-85 . >= 8 professional builds
X trail40 c600 (L-label and harvester) and clock 45 (scalper)
R unlimited, one per coin   S 0.2   seat lag_115
frame  last-leg week, 6.76 days, 152 door tokens, 9,036 fires.
      study-kernel/cvx_ldoor.py
L-label  pct = y/0.2*100 <= -50 on trail40 c600
```

The parent on this exit is **+1.21 SOL, 4/7 days, 23.5 first-per-mint a day** (under the
floor). **8 trades of 646 go to -50 % (1.2 %).** Without the top 1 % of trades the book is
**-2.11 SOL**. Fit days 0-3 **+1.72**, hold days 4-7 **-0.51**.

Pre-registered safety-panel cuts (skip HIGH risk) on trail40 c600:

| slice | n | first/day | SOL | days+ | l50 % | n_l50 | n_+200 | fit SOL | hold SOL |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| parent | 646 | 23.5 | +1.21 | 4/7 | 1.2 | 8 | 4 | +1.72 | **-0.51** |
| creator in | 368 | 13.8 | **+3.36** | 4/7 | 1.1 | 4 | 2 | +3.22 | +0.14 |
| creator sold | 278 | - | **-2.15** | - | 1.4 | 4 | 2 | - | - |
| bundle < 0.20 | 532 | 20.0 | +1.02 | 4/7 | 0.8 | 4 | 4 | +1.78 | -0.75 |
| fresh < 0.25 | 432 | 17.6 | +0.61 | 4/7 | 1.6 | 7 | 3 | +0.92 | -0.31 |
| snipe <= 8 | 576 | 20.7 | +1.81 | 6/7 | 1.2 | 7 | 4 | +1.86 | -0.05 |
| dev < 0.10 | 522 | 19.7 | +1.24 | 4/7 | 1.0 | 5 | 4 | +1.64 | -0.40 |
| all five | 175 | 7.1 | +2.79 | 5/7 | 1.1 | 2 | 2 | +2.58 | +0.21 |

Creator-in raises total SOL and is load-bearing in the five-term cut (dropping it returns
+0.94). It does **not** cut the -50 % rate (1.2 % -> 1.1 %). It is the known survival
permission (3.4), not L-selection.

Bundle, fresh, sniper count and dev share do not raise SOL on the hold half. Fresh is
inverted: the lowest-fresh quintile is the worst cell (**-1.89 SOL**, 3.8 % L-rate). Quintiles
on bundle / snipers / dev are not monotone in L-rate; the L-counts are 0-5 trades per bin.

Clock 45 on the parent is **-4.81 SOL, 0/7 days**. All-five is +0.43 SOL, 4/7, 7.1 tickets
a day.

**Scope, and it is load-bearing.** This door enters at **age >= 300 s and reserve 50-85**, which
is past the window where coins collapse: the -50 % tail lives in the first seconds to minutes of
a coin's life (2.1). So the population C1 was run on has almost no failure mode of the kind an
L-door is built to remove - 1.2 % of trades reach -50 % here against **14.1 % on door-v3 MONEY
(C0b: 235 of 1,667)**. **C1 measures that this book has no left tail, not that the left tail
is unpredictable.** The L-axis is answered on C0b's fires.

**This book has no left tail to cut.** The 300+ trades worse than -50 % named in the queue
belong to door-v3 MONEY. Those fires sit on this tape (7.0): 235 of 1,667 at -50 %.
On documented-project, L-selection is empty: 8 trades cannot be a door. Creator-in stays
a permission. The other four L-terms do not ship. C1's kill ("the -50 % tail is
unpredictable at decision time") holds on this coordinate because the tail is too small
to predict.

## 3.7 L-door where the tail exists (C0b), and the headroom trap under it

```
D shipped launch door (7.0)   E door-v3 event   P creator has not sold
X shipped arm21/trail36/stop43.75/c1200 and trail40 c600   R one per token   S 0.2
frame  last-leg week, 49,831 booked fires, 3,709 of them at -50 %.
       study-kernel/cvx_c0b_ldoor.py, cvx_c0b_control.py
```

**The trap first.** On the raw panel the reserve at the fire looks like the strongest
L-predictor ever measured: L-rate **0.01 % in the lowest quintile against 34.10 % in the
highest**, a 4.58x lift on the binary `v >= 40`. It predicts nothing. Below `v_entry` 42.43
a -50 % is arithmetically unreachable (1.3), so the reserve separates trades that *can*
collapse from trades that cannot. This is C1's scope error in the opposite direction, and it
is why every row below is read **inside a reserve band**.

L-rate by decision-time quintile, and then the same terms inside a band:

| term | L-rate in | L-rate out | in band 40-50 | in band 50-70 |
| --- | ---: | ---: | ---: | ---: |
| **bundle share < 0.20** | **1.70 %** | 9.60 % | **10.97 %** vs 25.55 % | **16.92 %** vs 29.83 % |
| dev share < 0.10 | 5.96 % | 9.19 % | 19.48 % vs 25.55 % | 26.95 % vs 29.83 % |
| snipers <= 8 | 6.77 % | 17.21 % | 25.82 % (flat) | 30.70 % (flat) |
| fresh-wallet share < 0.25 | 6.70 % | 11.15 % | 23.58 % (flat) | 25.97 % (flat) |
| buyers so far >= 6 | 9.36 % | 3.06 % | 23.19 % (flat) | 28.97 % (flat) |
| creator has not sold | 9.14 % | 6.09 % | **36.89 %** vs 25.55 % | **38.61 %** vs 29.83 % |

**Bundle share is a real L-term.** It survives the band control - inside 40-50 it cuts the
L-rate 25.55 % -> 10.97 % while raising the `>= +200 %` rate 1.56 % -> 2.50 % - and among the
601 door-v3 MONEY trades that can reach -50 % at all it reads **9.5 % at bundle < 0.20 against
48.3 % at bundle >= 0.20**. Snipers, fresh share and buyer count are headroom proxies: they
separate on the raw panel and go flat inside a band.

**The creator permission inverts on the L axis.** Creator-in *raises* the L-rate inside every
band (1.29-1.44x). It buys survival and it buys the coins whose dev still holds the supply.

On the money, added to the door-v3 MONEY sentence (shipped exit):

| cell | n | first/day | SOL | %/trade | l50 | n at +200 % | top1 % | wo top1 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| parent | 1,666 | 246.5 | +5.39 | +1.62 | 14.1 % | 42 | 357.0 | -13.84 |
| **+ bundle < 0.20** | 724 | **107.1** | **+10.43** | **+7.20** | **2.3 %** | 19 | 80.9 | **+1.99** |
| + snipe <= 8 | 1,481 | 219.1 | +8.24 | +2.78 | 10.7 % | 42 | 214.4 | -9.42 |
| + dev < 0.10 | 1,159 | 171.5 | +8.29 | +3.58 | 14.0 % | 26 | 169.5 | -5.76 |
| + v >= 40 | 724 | 107.1 | +1.58 | +1.09 | 32.5 % | 22 | 477.7 | -5.96 |

Bundle < 0.20 nearly doubles the book while cutting the loser count 235 -> 17, clears the
ticket floor, and is the only cell here that is still positive with its top 1 % removed. It
holds on the harvester exit too (+9.26 SOL, l50 0.8 %). **On the full tape the same term books
-151 SOL over 13,602 trades**: it is money only inside the sentence, which is what a
conjunction term is supposed to look like.

**This is the first term in the program that cuts the loser cost and raises the money at the
same time.** It does not ship - 7.0 is why - but the L axis is answered: the -50 % trade IS
predictable at decision time, on a book that has one, from the launch bundle.

**And it is not alone.** A second, independent L-term - how many of the solo 26 are already in
the coin - is stronger still (L-rate lift **0.37** against a matched-random null of 0.82), and
the two stack: `bundle < 0.20 AND A >= 1` reads an **8.77 %** L-rate and **+3.86 %/trade**
against -14.62 % for the whole reachable population. Controls, transfer and the full table are
in 6.8.

---

# 4. ENTRY POSITION AND EXITS

## 4.1 Break-even is set by the entry

Break-even is `p = L / (W + L)`, and `L` is set by where you enter. Same coins, same
`tp100 / trail50 / cap1200` exit:

| entry quality | cost of a miss | break-even hit rate | required lift over base |
| --- | ---: | ---: | ---: |
| arbitrary moment on a door coin | **-17 %** | 14.5 % | about **78x** |
| at the episode trough | **+2.2 %** | about 0 | about **1x** |

**19 points apart on entry position alone.** Never quote a required lift without the entry
quality it assumes.

## 4.2 Break-even is also set by the exit

```
D none   E zigzag TURN on keep-create coins with one completed >= 50 % episode
P none   X six exits, same fires   R unlimited, one per coin   S 0.2   seat lag_115
frame  one week, 62,316 fires on 11,849 coins.  study-kernel/audit_exits.py
```

| exit | n | %/trade | days+ | win | W | L | **be** |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| clock 30 | 41,488 | -4.55 | 0/8 | 36.0 % | 23.4 | 20.3 | **46.4** |
| trail15 c120 | 60,583 | -4.57 | 0/8 | 27.9 % | 25.5 | 16.2 | 38.9 |
| trail30 c300 | 35,429 | -6.06 | 0/8 | 26.0 % | 49.5 | 25.6 | 34.1 |
| trail50 c600 | 21,767 | -10.74 | 0/8 | 20.2 % | 87.2 | 35.5 | **29.0** |
| tp100 tr50 c1200 | 23,369 | -10.09 | 0/8 | 22.3 % | 87.1 | 37.9 | 30.3 |
| arm15 tr25 c600 | 26,262 | -8.12 | 1/8 | 31.4 % | 44.4 | 32.2 | 42.0 |

**The bar moves 17 points with the exit, on identical fires.** Any book judged against a fixed
break-even percentage is judged against the wrong number.

**A tail-preserving exit does not rescue an unselected pool** - it makes it worse, because `W`
rises from 23 to 87 while `L` rises from 20 to 36 and the win rate falls from 36 % to 20 %. The
exit is a property of the selection, so it is settled after the gates.

## 4.3 The loss-cap family, and why a static one cannot work

```
D none   E machine print in the dip (parent below) and zigzag TURN
P none   X fifteen exits, same fires   R unlimited, one per coin   S 0.2   seat lag_115
frame  one week.  study-kernel/audit_exit2.py
```

Machine-print parent, selected rows:

| exit | %/trade | W | L | win | worse than -20 % | worse than -10 % | above +50 % |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| trail50 c600 | -8.84 | 84.7 | 31.6 | 19.6 % | 56.9 % | 70.3 % | 7.9 % |
| clock 30 | -3.11 | 21.0 | 16.9 | 36.4 % | 19.4 % | 32.2 % | 3.4 % |
| unarmed trail 10, cap 600 | -3.21 | 21.1 | **12.7** | 28.0 % | **7.7 %** | 46.1 % | 2.7 % |
| unarmed trail 20, cap 600 | -4.34 | 36.4 | 18.6 | 26.0 % | 33.6 % | 58.8 % | 5.8 % |
| trail40 + abort 30 s / +5 % | -5.09 | 63.5 | 18.5 | 16.3 % | 32.9 % | 50.0 % | 5.6 % |
| **8dtx, measured over 1,717 coins** | - | - | - | about 34 % | **1.1 %** | **12.0 %** | **4.2 %** |

Three readings:

- **Exit choice moves the same fires by 7.6 points** (-10.74 to -3.11 on the zigzag parent,
  -8.84 to -3.11 here). That is inside the range usually attributed to selection alone, so a
  verdict reached under one exit family is not a verdict.
- **A static loss cap trades the winner for the loser at a losing rate**: `L` falls 60 %
  (31.6 -> 12.7) while `W` falls 75 % (84.7 -> 21.0). The stop fires inside the drawdowns that
  precede the up-move.
- **No static exit reaches the professional's shape.** The best left tail buyable is 7.7 % of
  trades worse than -20 % against his 1.1 %, and it costs the entire right tail. Truncating the
  left tail while keeping `W` requires a condition that separates a drawdown inside an up-move
  from the end of one - a state question, and the state-conditional family is untested.

To break even on the machine-print parent holding `W = 84.7`: `L <= 16.3`. Holding `L = 31.6`:
`P >= .29`.

## 4.4 There is no static early-exit signal

**104 abort cells** across two unrelated populations, two runner exits, four deadlines, three
thresholds: **zero positive, zero positive on both halves.**

| abort | winners kept | loss avoided |
| --- | ---: | ---: |
| 3 s / +10 % | 28.5 % | 59.4 % |
| 10 s / +5 % | 61.6 % | 32.0 % |
| 20 s / +0 % | 82.5 % | 14.3 % |

You give up winners as fast as you save losses. Adding an abort to the trough ceiling takes it
from **+15.65 to +9.91**. Three to twenty seconds in, a future +100 % winner and a dud are
indistinguishable **on price and a clock**.

## 4.5 The exit fill is where a book can lie

Holding the entry fixed and moving only the exit fill, on the 30-day launch-door rules:

| exit fill | MAX SOL rule | SAFETY rule | previously shipped rule |
| --- | ---: | ---: | ---: |
| the breaching print (unreachable ceiling) | **+151.10** | +49.79 | +53.17 |
| 50 ms after it | -2.26 | -8.21 | -74.25 |
| **115 ms - our measured p50** | **-29.06** | -18.94 | **-99.70** |
| 400 ms - one slot | -78.86 | -39.10 | -143.51 |

**Break-even sits under 50 ms.** Three measured mechanisms: the breaching print has already
gapped past the level (median reserve 0.978 of it, p10 0.690); **68 % of breaches sit in a slot
holding further prints**, and breach-to-end-of-slot has median 0.995 but mean 0.883 and p10
0.488; the tail carries it.

Removing the reactive exit removes the apparent edge with it, which says the edge was the exit
fill rather than the entry.

## 4.6 The age x reserve grid

10 s to 24 h x reserve 33 to the wall, **49 cells x 6 exits**, buy and hold, no door and no
event: **no cell with 300+ fires is positive.** The least bad is age 300 s+ at reserve 45-75,
which is about the toll. Random mid-tape fires read -5.5 % on a 30 s clock against a -3.5 % toll;
the rest is coin decay, and it is monotone - older coins lose less because the fall already
happened, higher reserves lose less because there is less left above them.

## 4.7 The exit lab on a selected entry (C4)

```
D E P FIXED and untouched: slow-wall 5 % door . burst START . age 60-900 s . vsol 33-81 .
  creator has not sold . two of the solo 26 already in (6.4, 6.8)
X the only thing that moves.  R one per token   S 0.2   seat lag_115
frame  6,345 fires on 362 coins, last-leg week.  study-kernel/cvx_c4_exitlab.py
```

The static abort grid is not rerun (104 cells, zero positive). The cuts here are derived from
the sentence's own two failure modes: **the remaining spend never arrives** -> cut on state,
and **the spend arrives then stops** -> ride and give back a fraction of the peak. Each state
cut is read alone, under a trail, and in a **loss-only** form that can fire only while price is
below the fill.

| exit | n | SOL | %/trade | days+ | worst | W | L | break-even | l50 | top1 % |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| trail40 c600 (incumbent) | 867 | **+13.03** | +7.51 | 6/6 | +0.03 | 103.4 | 31.5 | 23.4 % | 1.7 | 77.1 |
| S1 burst dies g10, loss-only + trail40 | 1,356 | +11.81 | +4.35 | 5/6 | -0.01 | 102.2 | **15.7** | **13.3 %** | 0.6 | 117.1 |
| trail50 c1200 | 670 | +11.35 | +8.47 | 5/6 | -0.52 | **132.8** | 36.2 | 21.4 % | 14.8 | 83.3 |
| **shipped arm21 / trail36 / stop43.75 / c1200** | 850 | +10.98 | +6.46 | **6/6** | **+0.09** | 91.8 | 33.1 | 26.5 % | 7.2 | 86.1 |
| S4 arrivals stall g10, loss-only + trail40 | 1,940 | +9.59 | +2.47 | 4/6 | -0.09 | 101.9 | **10.1** | **9.0 %** | 0.2 | 176.4 |
| tp100 trail50 c1200 | 811 | +5.71 | +3.52 | 5/6 | -0.04 | 98.1 | 38.1 | 28.0 % | 16.0 | 47.6 |
| trail20 c600 | 1,806 | +1.02 | +0.28 | 5/6 | -0.50 | 44.0 | 18.5 | 29.6 % | 0.2 | 898.4 |
| clock 45 (control) | 1,835 | +0.76 | +0.21 | 3/6 | -0.68 | 25.6 | 19.7 | 43.5 % | 3.3 | 760.0 |

**The C4 question is answered: yes, a state-conditional cut holds `W` while `L` falls.** The
arrivals-stall cut takes `L` from **31.5 to 10.1** while `W` moves 103.4 to **101.9**, and
break-even from **23.4 % to 9.0 %**. That is the opposite of the static loss cap, which cuts
`L` by 60 % and `W` by 75 % (4.3). The basis said this book breaks even at `L <= 16.3` holding
`W` fixed; three of these cells are under it.

**And it does not raise total SOL, for a reason worth stating.** Cutting early frees the
position, so the same fires become 1,940 trades instead of 867 - and 1,073 extra round trips at
3.2-4 % is about 7.7 SOL of toll. The book falls 13.03 to 9.59, so the cut earns back roughly
4.3 SOL of the 7.7 it spends. **A better distribution is not automatically more money once the
toll is charged per ticket.**

**The loss-only condition is the whole thing.** The same cuts, allowed to fire above the fill:

| the same state cut, unconditional | SOL | W | L |
| --- | ---: | ---: | ---: |
| S1 burst dies g3 + trail40 | **-14.40** | 8.1 | 5.5 |
| S3 flow reverses 15 s + trail40 | **-15.77** | 14.2 | 6.4 |
| S2 the pusher sells + trail40 | **-19.80** | 9.3 | 6.8 |

`W` collapses from about 100 to 8-14: an unconditional state cut sells the winners on their
first pause. **A cut that can fire while the position is ahead is not a cut, it is a clock.**

**On the client gate - the gate that decides now - the shipped armed trail wins:**

| exit | clients | positive | top share | leave-one-out | bootstrap | fit | hold |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| **shipped arm21/t36/stop43.75/c1200** | 16 | 10 | **66.0 %** | **+3.74** | **94.5 %** | +10.07 | **+0.92** |
| S4 g10 loss-only + trail40 | 16 | 10 | 78.5 % | +2.06 | 92.0 % | +9.51 | +0.07 |
| trail40 c600 (incumbent) | 16 | 9 | 80.6 % | +2.53 | 91.3 % | +12.67 | +0.36 |
| trail50 c1200 | 16 | 9 | 87.8 % | +1.39 | 88.4 % | +11.16 | +0.19 |

**94.5 % is the closest anything in this program has come to the 95 % bar**, on the lowest
top-client share (66.0 %) and the best hold half (+0.92). And the shipped exit is itself
two-regime - its `stop` leg fires on 245 of 850 trades and its trail on 448 - so it cuts while
unarmed and rides once armed, which is the shape the story asks for, conditioned on price
rather than on tape state.

**X is settled for this sentence: the shipped armed trail.** It gives up 2.05 SOL against the
incumbent trail and buys the client gate.

## 4.8 The implementation audit of the frozen sentence

Most of this program's positives died on implementation, so the frozen sentence was pointed at
every recorded cause before being believed. Two of the six checks failed, and both failures
were **mine**, not the market's. `study-kernel/cvx_audit_seat.py`, `cvx_audit_floor.py`,
`cvx_audit_refrozen.py`.

**What passed.**

| check | why it matters | result |
| --- | --- | --- |
| timestamp granularity | if `t_ms` were slot-quantised, `lag_115` would be a slot model wearing a millisecond name | **real ms**: 8 distinct `t_ms` per 9-print slot, span 205 ms |
| the door label | `runners` must be the SLOW wall; the fast-wall door is "instant pumps in disguise, negative at every entry age" (3.1) | `runners` = curve peak >= 60 SOL **and** peak >= 60 s after birth, previous UTC day, the same `launch_build_day_stats` the engine stamps at `TokenCreated` |
| the universe | a filter inside the SQL that builds the universe is a rule term (9) | `aa.pxf` is the full curve tape: **98,338 mints in both** it and `trades` on the shared window, rows within 0.04 % |
| the seat | a down-triggered exit filling into its own cascade killed rule v4 | see below - decay is gentle |
| concurrency | the study holds unlimited positions, the engine caps them | **max 10 open, p50 3, 2.0 SOL of capital**. No divergence |
| pricing | impact on the virtual reserve, exits filled 115 ms after the trigger and never at it | kernel asserts against the project's closed form; `kernel_test3.py` covers the unarmed stop |

**Seat stress**, one leg at a time, on the frozen cell:

| seat | SOL | | seat | SOL |
| --- | ---: | --- | --- | ---: |
| verdict 115 / 115 ms | +10.98 | | entry 460 ms | +9.21 |
| exit 230 ms | +10.30 | | both legs slot_end | +10.94 |
| exit 460 ms | +10.44 | | both legs 460 ms | **+8.71** |
| exit 1 s | +9.13 | | **zero lag** | **+21.74** |

Four times the seat still leaves +8.71, so this is not an exit-fill artifact. Zero lag is
**1.98x** the verdict: the fill model costs half the book, honestly charged.

### The two errors

**1. The ticket floor is a per-day refusal and it was reported as a mean.** First-per-mint
tickets a day for the frozen cell: **16, 177, 210, 24, 28, 9**. That is **2 of 6 days over
fifty**, reported as "53.9 a day, floor ok". The mean is inflated by exactly the two days the
single carrying client was alive (3.1a), so quoting it **launders the client concentration
through the gate**. The same error applies to door-v3 MONEY + bundle (13, 346, 296, 35, 26, 8 -
2 of 6) and to that cell with agreement added (mean 23 a day, under the floor even as a mean).
Door-v3 MONEY itself is clean: 73, 459, 491, 258, 241, 144 - 6 of 6.

**2. `A >= 2` is a wallet-identity gate and is refused four times over.** "At least two of these
26 named wallets already bought this coin" is the readers' mint list used as a gate. The
super-root `CLAUDE.md` says "his coins are never a gate"; 7.4 says "never build a factor on
wallet identity"; 9 says "his mint list is not a gate";
[_!___derive.md](_!___derive.md) 14 says "his mint list is never the universe". The
agreement **measurement** stands (6.8) - it is what establishes that the L axis is real. The
**gate** comes out of the sentence.

### The sentence with the forbidden term removed, every gate per day

```
D slow-wall launch door, 5 % cut      E burst START
P age 60-900 s . vsol 33-81 . creator has not sold
X arm +21 / trail 36 / unarmed stop -43.75 / cap 1200 s
R one per token   S 0.2   seat lag_115
```

| | n | SOL | %/trade | days+ | tickets a day | floor | clients | top client | LOO | bootstrap | top1 |
| --- | ---: | ---: | ---: | ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| shipped exit | 986 | **+13.75** | +6.97 | 5/6 | 16, 177, 210, 24, 28, 9 | **2/6** | 19 | **64 %** | **+4.93** | **95.9 %** | 91.5 % |
| trail40 c600 | 1,006 | +15.51 | +7.71 | 4/6 | same | **2/6** | 19 | 82 % | +2.78 | 91.4 % | 78.3 % |

**Dropping the forbidden term made the sentence better**: +13.75 against +10.98, the top client
falls to 64 %, and the build bootstrap reaches **95.9 %** - the first cell in this program to
clear that bar. It still fails the **tail** gate (top 1 % is 91.5 % of net) and the **per-day
floor** (2 of 6).

### Why the floor cannot be fixed by dropping a term

| variant | tickets a day | floor | SOL | LOO | bootstrap |
| --- | --- | ---: | ---: | ---: | ---: |
| door5 + E + P | 16, 177, 210, 24, 28, 9 | 2/6 | **+13.75** | +4.93 | 95.9 % |
| drop the creator permission | 141, 312, 270, 205, 347, 84 | **6/6** | **-17.26** | -26.29 | 16.1 % |
| drop the age / reserve band | 218, 626, 587, 273, 483, 159, 99 | **7/7** | +10.50 | **-17.37** | 59.9 % |
| door only, no permission | 526, 887, 644, 502, 1034, 228, 166 | 7/7 | -86.74 | -113.62 | 6.7 % |

**The term that makes the money is the term that removes the tickets.** Creator-not-sold holds
11.7 %, 56.9 %, **81.5 %**, 19.0 %, 14.0 %, 18.8 %, 12.4 % of door fires day by day - the client
signature from 3.1a. So on this tape the money and the tickets are the same two days, and there
is **no configuration with both**. That is a sharper statement than "short of clients": there is
not yet a sentence to prove.

# 5. NODES AND INSTRUMENTS

## 5.1 A roster row is an address, not a trader

Grouped by **which coins each wallet picks**, 41 of 77 daily-profitable rows are 7 actors. The
largest is **16 addresses splitting the mint space by `ord(mint[0]) % 8`**, two per shard, an
early leg and a late leg on the same coin: all 56 cross pairs and all 56 within-side pairs share
**exactly zero** coins against expectations of 16-62 each, all 16 trade every day, and the shard
key is right on **13,587 of 13,587 mints, zero violations**.

Consequences: **`operator_id` undercounts badly** (a funding-batch guess gave that actor 16
distinct ids); **wallet grade is inside the noise** (identical code on a hash partition scores
0.76 % to 3.62 % across grades A to D); node medians are weighted by copies.

That actor is **not a volume maker** despite its size: on the coins it trades it owns 0.72 % of
prints and 0.53 % of the SOL, and clears +4.71 % gross over 34,828 round trips on 13,565 coins.
A volume maker owns a large share of his coin's tape and round-trips to about zero minus fees.
This one is a passenger that keeps the difference - a reader run industrially. Exclude it from any
count of independent agreement.

## 5.2 The instrument set: 26 solo traders, five nodes

The 26 with no co-selection partner and a margin clearing its own standard error
(`t` = return on spend / bootstrap SE >= 2). Full anatomy: [solo-traders.md](solo-traders.md).

| node | wallets | trades | net SOL | margin | median trade | lose > 20 % | best 1 % = | entry reserve | open? |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| hot-tape re-entry | 6 | 133,939 | 938.2 | 1.10 % | -2.07 % | 13.6 % | **180.6 %** | 55.5 | open |
| mid-tape one-shot | 7 | 24,381 | 427.9 | 1.73 % | -3.34 % | 15.9 % | 122.9 % | 42.4 | open, needs a door |
| instant launch | 6 | 18,860 | 205.8 | 1.31 % | -2.50 % | 9.1 % | 116.0 % | 32.2 | no |
| quiet deep-age | 4 | 20,610 | 110.3 | 1.12 % | -2.50 % | 5.9 % | 112.2 % | 48.0 | open |
| deep-age big clip | 3 | 1,671 | 68.2 | **4.03 %** | **-1.23 %** | **4.5 %** | **32.3 %** | 61.4 | open |

`best 1 %` above 100 % means the other 99 % of trades collectively lose money.

**Hold is reactive in every node**: `p90 / p50` runs 3.0x to 5.3x, so no node exits on a clock.

The scope rule that defines the roster (exactly one buy and one sell on >= 95 % of trades) is
expensive and its cost is monotone in purity: wallets under 50 % purity median 13.38 % margin on
167 trades; at 95 %+ purity, 1.79 % on 2,100 trades. **One-in-one-out wallets trade about 13x
more often for about 8x less margin.**

**Levels are measured on a population selected for being profitable, so only the ranking
carries.**

## 5.3 Agreement between independent traders rises with entry age

Co-selection lift between two of the 26 against the geometric mean of the pair's entry age:
Spearman **0.794** on 325 pairs (`p = 7e-72`).

| both enter at | pairs | median lift | pairs at lift >= 2 |
| --- | ---: | ---: | ---: |
| under 30 s | 60 | **0.17** | 0.0 % |
| 30-120 s | 125 | 0.96 | 17.6 % |
| 120-400 s | 108 | 2.26 | 71.3 % |
| over 400 s | 32 | **3.50** | 96.9 % |

Under 30 s independent professionals **anti-select** each other. Over 400 s, 97 % of pairs agree.
The pool alive at 400 s is smaller and more selected, so read the direction and the ordering, not
the level. **Whether the agreement predicts anything forward is unmeasured.**

## 5.4 The mid-tape one-shot node at our seat

```
D (row 1) the coins these traders picked - HINDSIGHT, not a shippable door
  (row 4) none
E burst START = the router or terminal buy of 0.5-1 SOL that opens the burst
P none   X clock 20 . clock 45 . tp25 c120 . trail30 c600
R unlimited, one per coin   S 0.2   seat lag_115 both legs
frame  one week 09-01..09-07, 11.76 M prints; three holdout weeks 08-11..08-31
```

| decision print | our fill lands after them | clock 20 | clock 45 | tp25 c120 | trail30 c600 |
| --- | ---: | ---: | ---: | ---: | ---: |
| **burst start** | 42-46 % | **+2.6 .. +6.2 %** | **+3.5 .. +6.7 %** | **+1.8 .. +4.8 %** | **+2.7 .. +3.8 %** |
| the print before them | 77-90 % | -0.6 .. -2.4 % | +0.3 .. -3.8 % | -1.4 .. -3.2 % | -0.4 .. -4.4 % |
| their own print | 100 % | -1.8 .. -3.9 % | -0.8 .. -4.2 % | -2.4 .. -3.3 % | -1.6 .. -5.5 % |

**The same event on the full tape with no door: -1.5 to -13 % a trade, 0 of 7 days,
34,097 fires at >= 1 SOL.**

**The seat, the event and the exit are all fine. The door is the missing term.** Every exit
family is positive on the burst-start print, net of costs, at 115 ms on both legs; firing later
in the burst destroys it. Permissions applied one at a time to the full-tape version (creator not
sold, dip, active tape, silence-break) do not turn it positive, and all four together reach
-1.3 %, which is the toll. **No door has ever been applied.**

The three wallets studied here run the node through durable-nonce racer builds landing about
130 ms behind the opening buy. That is a statement about their equipment. The node has seven
members; the two largest books are measured as instruments in 6.9: they do not name burst START,
and leftover on their coins at this seat does not match this table.

## 5.5 Copying is closed, and this is the mechanism

For every entry of two attention-arrival readers and 8dtx: the same coin at their moment (A), at
random prints before (B) and after (C) their entry, and a random pool coin at matched age and
depth (D), 120 s clock:

| set | reader 796 | reader 1416 | 8dtx |
| --- | ---: | ---: | ---: |
| A their moment | -6.0 % | -0.7 % | -1.7 % |
| B same coin, before them, reader arrives inside the window | -3.2 / +1.1 % | **+13.7 / +13.4 %** | +8.2 / +2.9 % |
| **B same, path truncated at the reader's arrival slot** | **-7.0 / -8.0 %** | **-2.3 / -4.7 %** | **-5.7 / -7.8 %** |
| C same coin, after them | -10.5 % | -10.4 % | -11.3 % |
| D pool, matched age and depth | -11.9 % | -15.5 % | -10.5 % |

**The whole profit on their coins is the window that contains their arrival.** Cut the path one
slot before they land and every "before" cell is negative. **Selection is not the cause for these
readers; the flow on their coins is the swarm's own buying.**

On the machine census, **36.1 % of the wallet-based k=2 fires are one machine** (457,510 of
1,268,395); on the machine event, 938,789 fires read -7.6 / -10.2 / -12.7 % at 60 / 120 / 300 s,
every term negative on every one of 8 days.

## 5.6 The wave node, priced at every seat

219,190 events on top-20 mass terminal builds.

| seat | pnl SOL | win |
| --- | ---: | ---: |
| land first (look-ahead) | +44.41 | 57.0 % |
| land second (trigger = first buy >= 0.5 SOL after the gap) | -24.07 | 41.7 % |
| land last in the breaking slot | -31.16 | 39.3 % |
| slot +1 | -35.98 | 38.1 % |
| slot +2 | -35.87 | 37.9 % |

**The whole edge is the wave's own price impact between its first and last buy.** Beating every
follower after seeing the first big buy still loses. **A tape-observable trigger cannot be ahead
of the print that triggers it.**

## 5.7 The machine census

Schema `aa`: 15,998 machines over 12.8 M prints; **408 machines with 1,000+ prints carry 95 % of
prints.** Key = ordered instruction list with token-account create/close and memo removed
(`build_core`) plus whether the buyer pays his own fee. **Nothing that costs nothing to change is
in the key**: not the priority price, not the tip, not the clip, not the wallet.

Split-half stability on 750 machines with 200+ prints in both halves: clip corr **0.89**, depth
0.84, sell share 0.999, wallet count 0.97, age 0.63.

Co-landing separates a tool from a farm: 4 % of buy slots hold two of the machine's wallets at
3-9 wallets and 12-14 % at 10+ (tools: many users, same event); a farm reads 20-85 %.

| role | rule | machines | prints | wallets | clip | co-landing | net SOL |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| tool_terminal | 1,000+ wallets, router labels | 33 | 5.89 M | 245,901 | 0.21 | 7 % | -45,533 |
| tool_bot | 1,000+ wallets, no router labels | 75 | 3.89 M | 611,867 | 0.19 | 7 % | +206,384 |
| racer | nonce or seed on half its prints | 311 | 1.06 M | 10,939 | 0.64 | 3 % | +60,539 |
| farm | 3-999 wallets, co-landing 20 %+ | 47 | 227 k | 9,555 | 0.63 | 51 % | -120,195 |
| custom_operator | 10 wallets or fewer, 500+ prints | 76 | 198 k | 194 | 0.53 | 2 % | -1,168 |
| creator | half its prints by the coin's creator | 53 | 179 k | 43,654 | 5.70 | 2 % | -294,599 |
| bundler | half its buys in the creation slot | 83 | 114 k | 13,934 | 1.50 | 47 % | -178,580 |

**Count predicts the wave, not the campaign.** N same-template buys in a breaking slot raises
immediate follow-through monotonically (Terminal 1 -> 4: **59 % -> 79 %** get >= 1 SOL in 60 s)
while **P(reaches the wall) stays flat at about 7-8 %**. So multi-tool bursts mark attention
arrival; a single-machine break marks a decision. Baseline over 295,518 silence breaks:
P(>= 1 SOL follows in 60 s) = 48.1 %, P(reaches the wall after) = 6.83 %.

Brands are not machines: Axiom, Terminal and GMGN are buy/sell symmetric (the terminal crowd);
the seed-builder cohort is the racer layer. **A build's instruction name and its compute-budget
order separate a campaign client from its generic twin on the same instruction set** - one
concentrates about 21 buys per coin and is green, the other sprays about 2.7 per coin and is red.

## 5.8 Mid-tape derive, phases 1-5 (instrument 9Uq8GV)

```
D  none (member coins are a ceiling, never a door)
E  named by excess intensity, not yet spelled publicly
P  none
X  clock 15 (seat probe, not the exit)
R  one per coin   S  0.2   seat  lag_115 both legs, and RACE (sequenced before its print)
frame  study tape 2026-08-30 17:48 to 2026-09-06 12:00 UTC, 6.76 days.
      node "mid-tape one-shot", seven roster wallets. 3Xk2Eu has 0 episodes.
      node-derivation/mid-tape/mt_p15.py
```

The node is not one strategy. Split first (derive phase 1, law 27). Every member that prints
is green at RACE under a 15 s clock and red at FOLLOW. Copying the fill is closed here the
same way it is on hot-tape.

| member | eps | mints | 2+ % | re % | age p50 | hold p10/p50/p90 | clip | RACE cap15 | days | body | FOLLOW cap15 |
| --- | ---: | ---: | ---: | ---: | ---: | --- | ---: | ---: | :---: | ---: | ---: |
| 9999hu | 4,323 | 3,669 | 15.6 | 15.1 | 17.6 s | 3.9 / 25.0 / 69.8 | 1.20 | **+5.94 %** | 7/7 | +34.67 | -2.60 %, 0/7 |
| 88887Q | 4,933 | 3,674 | 24.4 | 25.5 | 42.7 s | 2.3 / 21.9 / 76.0 | 1.20 | **+5.73 %** | 7/7 | +39.63 | -1.81 %, 0/7 |
| 8dtx2t | 2,782 | 2,081 | 26.1 | 25.2 | 143 s | 5.4 / 16.1 / 88.4 | 0.66 | +3.16 % | 7/7 | +11.77 | -1.54 %, 0/7 |
| 9Uq8GV | 1,024 | 849 | 16.3 | 17.1 | **159 s** | **10.7 / 16.0 / 16.2** | 0.59 | +2.21 % | 8/8 | +2.51 | -1.57 %, 3/8 |
| ApfmkS | 525 | 422 | 18.2 | 19.6 | 181 s | 3.1 / 8.4 / 34.7 | 0.97 | +2.81 % | 7/7 | +2.21 | -1.46 %, 2/7 |
| 8aaRWu | 753 | 637 | 14.4 | 15.4 | 84.5 s | 2.9 / 25.5 / 116.4 | 0.28 | +1.06 % | 5/7 | **-1.39** | -0.67 %, 3/7 |

**Shape.** All six that print are mostly one episode per mint (re-entry 15-26 %). None is a
hot-tape re-entry scalper. 9Uq8GV's hold is a clock: p50 16.0 s and p90 16.2 s. 8aaRWu's RACE
body is red and its top 1 % is 188 % of net: noise under law 28. 9999hu's age p50 is 17.6 s:
younger than the mid-tape window.

**Trigger (excess intensity, 5 s lookback, same-coin controls).**

| member | peak class | lag bin | lift | cases |
| --- | --- | --- | ---: | ---: |
| **9Uq8GV** | **buy >= 1 SOL** | 75-100 ms | **13.01** | 1,048 |
| 9Uq8GV | burst start | 50-75 ms | 8.14 | 1,048 |
| 9Uq8GV | sell >= 1 | 50-75 ms | 0.32 | 1,048 |
| 8dtx2t | burst start | 50-75 ms | 7.49 | 2,859 |
| 8aaRWu | burst start | 75-100 ms | 9.20 | 759 |
| **9999hu** | **sell >= 1 SOL** | 50-75 ms | **9.12** | 4,428 |
| **88887Q** | **sell >= 1 SOL** | 50-75 ms | **8.61** | 5,167 |
| ApfmkS | (flat, all lifts ~1.3) | 300-400 ms | 1.28 | 535 |

9Uq8GV names a public **buy**. He avoids sells (lift 0.3). 9999hu and 88887Q name a public
**sell**, the same class as rule 1's 8fStGV. They are not the same strategy as 9Uq8GV. ApfmkS
does not name a print (derive: a state, or not usable).

**DELAY at 115 ms** (reaction on the member's own episodes, clock 15). On-trig is a ceiling:
it is his coins, not a door.

| member | trigger | hits | dt p50 | ahead | on-trig | when behind |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| 9Uq8GV | buy >= 1 | 92.5 % | 68 ms | 30.2 % | 0.00 % | **-1.66 %** |
| 9Uq8GV | burst start | 75.1 % | 125 ms | 51.9 % | +4.09 % | **-1.77 %** |
| 9Uq8GV | sell >= 1 | 13.7 % | 302 ms | 71.4 % | +20.03 % | +0.49 % |
| 9999hu | sell >= 1 | 79.1 % | 96 ms | 43.1 % | +3.63 % | -0.67 % |
| 88887Q | sell >= 1 | 76.1 % | 113 ms | 49.4 % | +3.05 % | -0.74 % |
| 8dtx2t | burst start | 83.0 % | 105 ms | 47.0 % | +2.61 % | -0.90 % |

9Uq8GV's sell >= 1 row is a rare corner (14 % of episodes), not his logic: excess intensity
on sells is below 1. Drop it.

**Verdict on 9Uq8GV.** FOLLOW of his named print is not reachable: buying into a buy, lag p50
68 ms, the book is 0 when we fire on it and **-1.66 % when we land behind**. Burst start on
his coins is the 6.9 ceiling (+4.09 % here at clock 15); when we land behind it is still red.
RACE +2.21 % 8/8 is the seat that pays. Phase 6 (5.9) spells which buy >= 1 it takes: no public
DELAY-legal sentence. Empty D stays allowed.

9999hu / 88887Q stay a second instrument (sell >= 1), not this parent. Their behind book is
still slightly red; contrast on that sell is a different sentence.

---

## 5.9 Mid-tape derive, phase 6 (9Uq8GV, which buy >= 1)

```
D  none
E  public buy >= 1, terms from within-coin contrast; not frozen
P  none
X  clock 15 (its hold; not the exit)
R  one per coin   S  0.2   seat  lag_115
frame  study tape, 6.76 days. node-derivation/mid-tape/mt_p6.py
      window check mt_p6_win.py; unpriced re-book mt_p6_unpriced.py
```

**6.1 Within-coin contrast** (buy >= 1 on its 873 coins, 64,365 triggers, clock 15). Reaction
window 75 ms matches its lag p50 of 68 ms: 914 acted, **87 % of its 1,050 buys** (its logic, not
a corner). k==0 is 1.1 %. The same rank holds at 100-300 ms.

| fact | acted | ignored | rank | dir |
| --- | ---: | ---: | ---: | --- |
| stall (s since new high) | 72.6 | 27.8 | 0.676 | HIGH |
| dd (% below high) | -37.2 | -21.7 | 0.339 | low |
| mv60 | +6.6 | +25.1 | 0.369 | low |
| buys10 (SOL) | 4.56 | 7.26 | 0.393 | low |
| hold_n | 216 | 260 | 0.592 | HIGH |
| nb5 | 6 | 8 | 0.433 | low |

It takes the stalled dip, not the frenzy. Those first three facts are in the price (strategy
1.4). Reaction cost on the acted print: **+9.21 % p50, +22.2 % p90**. Ignored prints: +0.13 %.
Clock 15 on its coins: acted **-1.55 %/trade**, ignored **+1.77 %**. Copying which print it
takes is worse than taking the ones it skips, at our seat.

Burst start: 597 acted (57 % of its buys). The separator is ssize 1.44 vs 0.10 and mvk 6.6 vs
0.3: the burst it takes is the same >= 1 SOL buy. Reaction cost +11.7 % p50.

**6.2 Public sentence**, every coin, occupancy one per coin, clock 15. Terms added from acted
medians, price-path first. Each term must lift; none does. Sell >= 1 control is worse, as
required, and still red. Held buying-state (rising edge of 1 SOL bought in 2 s, no 1 SOL print
required) is the same shape.

| cell | n | %/trade | days | body | cover of its acted | rcost p50 |
| --- | ---: | ---: | :---: | ---: | ---: | ---: |
| buy >= 1, none | 140,384 | -0.06 | 4/8 | -485.8 | 100 % | 0.0 |
| + dd <= -38.9 | 48,023 | -2.24 | 0/8 | -327.7 | 50 % | 0.0 |
| + stall >= 69.4 + quieter flow | 18,218 | -2.00 | 0/8 | -99.9 | 8.6 % | 0.0 |
| sell >= 1 + same terms (control) | 27,376 | -4.87 | 0/8 | -299.7 | - | 0.0 |
| held buying-state, none | 171,758 | -0.81 | 0/8 | -810.9 | 100 % | 0.0 |
| held buying-state + same terms | 14,170 | -1.54 | 0/8 | -63.0 | 8.6 % | 0.0 |

Occupancy first-fire hides the +9 % reaction cost of the print it actually takes (rcost p50
reads 0 on the first buy >= 1 of the coin). k==0 on that first-fire row is 11.8 % (law 31);
the book is red either way. Coin-list split of the tight conjunction: its coins -1.24 % 2/8,
every other coin -2.21 % 0/8. Not a door.

**Unpriced terms only** (buys10, holders, recipes, age; dd/stall/mv out). `buys10 <= 4.56` is
+0.82 % 6/8 and a tail: body **-221.5**, top 1 % **217.7 %**. `hold_n >= 216` then **-1.91 %
0/8**. No legal filling.

**Verdict.** FOLLOW of 9Uq8GV's named buy has no public DELAY-legal sentence: the print it
takes has already moved ~9 %, its which-terms are price-path or a tail, and the held-state
spelling of those terms is red. Do not AND onto buy >= 1. This trigger is closed. The RACE
seat still names a state (5.10).

---

## 5.10 Mid-tape derive, phase 5 again (9Uq8GV, the state)

```
D  none
E  rising edge of unpriced state at its buy; not frozen
P  none
X  clock 15 (its hold; not the exit)
R  one per coin   S  0.2   seat  lag_115
frame  study tape, 6.76 days. node-derivation/mid-tape/mt_p5b.py, mt_p5b_nb2.py
```

Phase 4 is RACE-only: the event is a state before its print, not FOLLOW of buy >= 1 (5.9).
5.1 and 5.2 on that print are kept; this run goes back to 5.1 for a state. Price-path is
not a candidate (strategy 1.4).

**5.1 Full lift.** No unpriced class has peak lag >= 115 ms and lift >= 2. High-lift cells
are 0-100 ms (buy >= 1 13.01, pro 11.35 at 0-25 ms, burst start 8.14, new_build 5.84).
Sells stay below 1. Lags past 400 ms are all below 1: it does not fire on a state that has
held for seconds. The 150-200 ms buy >= 1 cell (12.92) is a second print in the same burst,
not a second reaction time.

**5.2 Missed print classes** (clock 15, max_trig 1 s).

| class | hits | dt p50 | dt>115 ms | ahead | on-trig | behind |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| pro | 38.5 % | 16 ms | 27 % | 27 % | +2.17 % | **-2.21 %** |
| new_build | 50.0 % | 24 ms | 28 % | 28 % | +1.29 % | **-2.68 %** |
| pro < 1 SOL | 33.6 % | 16 ms | 29 % | 29 % | +3.62 % | **-1.69 %** |
| new_build < 1 SOL | 39.7 % | 25 ms | 31 % | 31 % | +3.34 % | **-1.40 %** |

All four are races. Kill.

**5.1c State contrast** (its 1,048 buys vs 4,188 same-coin random public prints, window
+/- 60 s). Strongest separators are **priced**: buys5 5.64 vs 2.42 (rank 0.714), mv10
+14.2 % vs +0.6 % (0.635). Unpriced: **nb2 5 vs 3** (0.638). last_pro / last_new / npro2
are weak. It fires into a live 2 s multi-recipe burst; the burst's size and 10 s move are
already in the price.

**5.2 Rising-edge unpriced states** (max_trig 5 s). Behind stays red on every row. On-trig
is its subsequent buy (not a ceiling).

| state | hits | dt p50 | dt>115 ms | ahead | on-trig | behind |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| nb2 >= 4 | 63.7 % | 308 ms | 61 % | 61 % | +9.74 % | **-2.41 %** |
| nb2 >= 5 | 52.3 % | 299 ms | 60 % | 60 % | +9.07 % | **-2.55 %** |
| nb2 >= 3 | 69.1 % | 309 ms | 61 % | 61 % | +9.24 % | **-3.65 %** |
| npro5 >= 1 | 42.5 % | 399 ms | 57 % | 57 % | +10.29 % | **-2.70 %** |
| npro2 >= 1 | 52.8 % | 275 ms | 57 % | 56 % | +9.94 % | **-1.71 %** |
| nb5 >= 2 / 3 | 27-35 % | 164-187 ms | ~58 % | ~58 % | +6 to +9 | **-6.3 %** |

**5.3 / 6.2** Occupancy one per coin, clock 15. k==0 is 0.0 % on nb2 (law 31); 1.2 % on
npro5 >= 1.

| cell | n | %/trade | days | body | k==0 |
| --- | ---: | ---: | :---: | ---: | ---: |
| nb2 >= 4, every coin | 175,033 | -1.86 | 0/8 | -1,151 | 0.0 |
| nb2 >= 4, its coins | 23,920 | -0.38 | 2/8 | -80 | 0.0 |
| nb2 >= 5, every coin | 136,648 | -1.66 | 0/8 | -864 | 0.0 |
| nb2 >= 5, its coins | 18,656 | +0.03 | 3/8 | -48 | 0.0 |
| npro5 >= 1, every coin | 169,561 | -1.56 | 0/8 | -1,025 | 1.2 |
| npro5 >= 1, its coins | 22,450 | -0.46 | 1/8 | -80 | 0.4 |
| npro2 >= 1, every coin | 180,257 | -1.67 | 0/8 | -1,122 | 1.1 |
| npro2 >= 1, its coins | 23,930 | -0.49 | 1/8 | -87 | 0.4 |

5.3 occupancy with clock 15 is red on every fire of the class. That column is not leftover
existence on the fires he takes (derive 5.2).

**Verdict.** 9Uq8GV co-arrives into a 2 s independent-recipe burst. The print neighbor
(buy >= 1, pro, new_build) is DELAY-dead: reaction cost / race (16-68 ms). The unpriced
rising edge (nb2, npro) has lag ~300 ms next to its own buy; every-fire clock 15 occupancy
is red, including on its coins. RACE +2.21 % 8/8 is its fill. Leftover existence on acted
tickets (peak leftover vs cost, derive 5.2) is unread. Empty D stays allowed on a
leftover-legal parent.

---

## 5.11 Mid-tape derive, instrument 9999hu (leftover then which sells)

```
D  none
E  public sell >= 1; leftover on acted tickets; public occupancy not frozen
P  none
X  clock 25 (its hold p50; a prior, not the exit)
R  one per coin   S  0.2   seat  lag_115
frame  study tape, 6.76 days. node-derivation/mid-tape/mt_hu_p5.py, mt_hu_p6.py, mt_hu_p6b.py
      working file node-derivation/mid-tape-rule-2.md
```

Pick is 9999hu: highest RACE on the node (+5.94 % 7/7, body +34.67), a public tell that is
not C2 (sell >= 1 lift 9.12 @ 50-75 ms). 88887Q is the same tell and stays off (law 27).
Shape: 4,323 episodes, age p50 17.6 s, hold 3.9 / 25.0 / 69.8 s (5.8).

**5.2 Leftover existence** on acted tickets (our fill 115 ms after the trigger, MFE through
hold p10-p90 net of both-leg cost). Sell >= 0.5 is the nearby class.

| class | hits | dt p50 | ahead | rcost p50 (ahead / behind) | missed | leftover median p10/p50/p90 | covers cost | gate |
| --- | ---: | ---: | ---: | --- | ---: | --- | ---: | --- |
| sell >= 1 | 79.1 % | 96 ms | 43.1 % | +5.5 % (**0.0** / +9.0) | 0.1 % | **+9.3 / +17.8 / +23.6** | 88.6 % | live |
| sell >= 0.5 | 88.3 % | 86 ms | 37.9 % | +6.2 % (0.0 / +8.9) | 0.1 % | +8.6 / +16.9 / +22.0 | 87.7 % | live |

Reaction cost when we land **ahead** of its buy is 0. The +5.5 % p50 is filling after its
print. Missed-episode share is 0.1 %. Not a race (ahead 43 %, lag 96 ms). Leftover after
our fill covers cost on 88.6 % of tickets. Behind leftover median is still +13.4 %/trade.

**5.3 Diagnostics** (not a veto), sell >= 1 acted.

| column | number |
| --- | --- |
| clock mean 1 / 5 / 15 s | +4.81 / +3.17 / +3.61 %/trade |
| clock 15 **median** | **-2.59 %/trade** |
| his-exit mean / median | +3.92 / **-2.30** |
| leftover qs (p10 / p50 / p90) | -0.74 / +17.82 / +79.89 |
| occupancy on its coins, clock 25 | -0.94 %/trade 0/8, body -219, k==0 0.0 % |

A hold-matched clock and a copy of its close are red in the median. Peak leftover is not.

**6.1 Which sells** (sell >= 1 on its 3,773 coins, reaction window 200 ms, clock 25).
183,865 triggers; 3,437 acted = **77.4 % of its 4,439 buys**; k==0 0.0 %.

| fact | acted | ignored | rank | dir |
| --- | ---: | ---: | ---: | --- |
| nb2 | 9 | 5 | 0.717 | HIGH |
| buys2 (SOL) | 6.54 | 0.99 | 0.690 | HIGH |
| age (s) | 16.3 | 145 | 0.368 | low |
| shold (s) | 4.22 | 20.6 | 0.365 | low |
| stall (s) | 3.58 | 30.1 | 0.384 | low |
| hold_n | 64 | 292 | 0.364 | low |
| mvk (%) | -8.0 | -5.2 | 0.301 | low |

It takes the live frenzy-absorbed sell on a young coin, not a stalled dump. Clock 25 on its
coins: acted **+1.29 %/trade**, ignored **-0.83 %**. The fires it takes are better at our
seat; occupancy of the whole class is not.

**6.2 Public occupancy**, every coin, clock 25. Stacked unpriced acted medians do not lift.

| cell | n | %/trade | days | body | cover of acted | k==0 |
| --- | ---: | ---: | :---: | ---: | ---: | ---: |
| sell >= 1, none | 135,030 | -3.53 | 0/8 | -1,388 | 100 % | 0.0 |
| + nb2 >= 9 | 21,829 | -3.70 | 0/7 | -253 | 54 % | 0.0 |
| + nw5 >= 27 | 13,778 | -5.35 | 0/7 | -197 | 37 % | 0.0 |
| + ssize >= 1.88 | 8,563 | -5.85 | 0/7 | -130 | 19 % | 0.0 |
| control buy >= 1, none | 112,831 | +0.07 | 4/8 | -455 | - | 14.7 |

Control buy >= 1 none has k==0 14.7 % (law 31) and a 2,861 % top 1 %. Tight spec on its
coins -0.78 % 4/7 vs every other coin -8.84 % 0/7: a gap, both red, not a door.

Each term **alone** (occupancy): only age <= 16.3 is green, and that fire is not its fire.

| cell | n | %/trade | days | body | top 1 % | fire age p50 |
| --- | ---: | ---: | :---: | ---: | ---: | ---: |
| nb2 >= 9 | 21,829 | -3.70 | 0/7 | -253 | - | 26.4 s |
| shold <= 4.22 | 34,032 | -0.61 | 1/8 | -202 | - | 32.0 s |
| buys2 >= 6.54 | 22,051 | -1.96 | 0/8 | -192 | - | 7.8 s |
| **age <= 16.3** | 18,156 | **+3.85** | **7/7** | +41 | **70.9 %** | **2.0 s** |
| age 0-8 s | 16,500 | +4.30 | 7/7 | +50 | 64.6 % | 1.8 s |
| **age 8-16 s** | 12,948 | **-3.74** | **0/7** | -156 | - | 9.6 s |
| age 10-25 s | 14,376 | -4.41 | 0/7 | -189 | - | 12.5 s |
| acted-only (not a sentence) | 2,393 | +0.87 | 5/7 | -6 | - | - |

Age <= 16.3 occupancy is the **launch first-sell** (age p50 2 s, top 1 % 71 %). The band
around 9999hu's own fire (8-16 s, age p50 of the fire 9.6 s) is red. age <= 16.3 and
nb2 >= 9 is -3.98 % 0/7.

**Verdict.** Leftover exists at our seat on the sells 9999hu takes. The class as occupancy
does not name those sells: first-fire is a launch sell, and recipe-count terms make that
book worse. Do not freeze age <= 16 as this sentence. Phase 8 on the acted pool is 5.12.
Empty D stays allowed.

---

## 5.12 Mid-tape derive, instrument 9999hu (phase 8, acted pool)

```
D  none
E  public sell >= 1, the fires 9999hu takes (not occupancy, not a public sentence)
P  none
X  hazard-named clock ~25 s, then families against that bracket
R  one per coin   S  0.2   seat  lag_115
frame  study tape, 6.76 days. node-derivation/mid-tape/mt_hu_p8.py
      working file node-derivation/mid-tape-rule-2.md
```

**8.1 Closing print.** Excess intensity, cases=close, controls=hold: 4,389 closes vs 17,511
hold prints. No class spikes. Peak lifts: burst_start 1.56 @ 50-75 ms, buy >= 1 1.18 @
75-100 ms, sell >= 1 0.89. It does not close on a public print.

**8.1 Hazard** on the pool the sentence selects (buy within 200 ms of a public sell >= 1).

| pool | n | close pnl p25/p50/p75 | held p25/p50/p75 | peak p50 | tp (>=+8) | stop (<=-20) | between |
| --- | ---: | --- | --- | ---: | ---: | ---: | ---: |
| E | 2,555 | -15.5 / **-0.2** / +18.8 | 7.9 / **23.5** / 41.5 s | **17.8 %** | 36.7 % | 19.2 % | 44.1 % |
| other | 1,730 | -15.6 / -3.7 / +12.6 | 20.2 / 28.0 / 45.2 s | 12.3 % | 30.1 % | 19.2 % | 50.8 % |

On E, hazard jumps at **20-30 s across every pnl band** (~2-3.5 % a print), not at a profit
band. The close median is flat against a 17.8 % peak: it gives the leftover back. The
bracket prior is a clock at its hold p50, a loose stop, not a take profit.

**8.2 Families** on the 3,437 acted sell >= 1 fires, own occupancy, fill 115 ms after the
sell. Clock 25 is the 6.2 prior.

| cell | n | %/trade | days | body | top 1 % | h1 / h2 | hold | mix |
| --- | ---: | ---: | :---: | ---: | ---: | --- | ---: | --- |
| clock 25 (prior) | 2,393 | +0.86 | 5/7 | -6.30 | 253.7 | +0.25 / +2.43 | 24.6 s | time 100 |
| clock 70 | 2,338 | -0.01 | 5/7 | -16.10 | - | -1.11 / +2.80 | 69.1 s | time 100 |
| tp10 sl25 t25 | 2,442 | +1.33 | 5/7 | +3.57 | 44.9 | +1.51 / +0.85 | 1.8 s | tp 69 sl 18 |
| tp10 sl25 t70 | 2,441 | +1.26 | 6/7 | +3.22 | 47.5 | +1.45 / +0.76 | 1.8 s | tp 72 sl 22 |
| **tp15 sl40 t70** | 2,433 | **+1.61** | **7/7** | **+4.47** | **42.8** | **+1.70 / +1.35** | 6.3 s | tp 67 time 20 sl 14 |
| tp20 sl25 t70 | 2,434 | +1.25 | 6/7 | +2.73 | 55.1 | +1.44 / +0.74 | 6.4 s | tp 55 sl 32 |
| sellbuy arm10 sl25 t70 | 2,441 | +1.31 | 5/7 | +3.25 | 49.1 | +1.34 / +1.22 | 2.7 s | tp 70 sl 23 |
| scale half10 rest20 t70 | 2,434 | +1.36 | 6/7 | +3.68 | 44.6 | +1.55 / +0.87 | 6.5 s | tp+tp 55 |
| trail arm10 give5 sl25 t70 | 2,438 | +0.12 | 3/7 | -3.94 | 799 | +0.29 / -0.34 | 3.7 s | tp 72 |
| ride arm10 sl25 t70 | 2,432 | -0.39 | 3/7 | -12.36 | - | -0.83 / +0.80 | 6.6 s | tp 71 |
| tp15 sl25 t70 + fade | 2,442 | +1.08 | 5/7 | +2.33 | 55.8 | +1.41 / +0.20 | 3.2 s | tp 52 fade 34 |
| tp15 sl25 t70 + dump | 2,444 | +0.35 | 4/7 | -1.35 | 178 | +0.57 / -0.21 | 1.4 s | dump 50 |

tp15 sl40 t70 per-day n: 123 / 480 / 657 / 505 / 381 / 186 / 101. Capped book -7.42 SOL.
Trail, ride, dump, and a longer clock are worse than the prior. Sellbuy and scale do not
beat the wide-stop bracket on days and body together.

**Verdict.** Leftover on the fires it takes is a short bounce (take-profit hold 2-6 s), not
its 25 s close. A clock copies the give-back. The working X is tp15 sl40 t70: 7/7, both
halves, body +4.47. Top 1 % 43 % and a negative capped book fail the ship bars. Next is
phase 9 on this pool (stop-outs vs take-profits at the fire). Do not freeze age <= 16. Do
not AND nb2 onto occupancy.

---

# 6. THE CONJUNCTION SPACE

What a search over decision-time terms actually produces, on money, at the floor, walked forward.
This is the measurement that closes or opens an event family.

## 6.4 Door x burst START (C2)

```
D keep-create / documented-project / keep + demonstrated episode, one at a time
E first router or Terminal buy >= 0.5 SOL of that tool's run after >= 2 slots of that tool quiet
P age 60-900 s . vsol 33-81 . creator not sold . crowd hold < 50 % (with the episode door)
X trail40 c600 and clock 45
R unlimited, one per coin   S 0.2   seat lag_115
frame  last-leg week, 6.76 days, 291,339 burst-start fires.
      study-kernel/cvx_burst.py
```

| slice (trail40 c600) | n | first/day | SOL | days+ | top1 % | wo top1 | fit SOL | hold SOL |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| E only, full tape | 56,958 | 4,660 | **-1151.14** | 0/8 | - | - | -729.82 | -421.32 |
| D documented-project | 1,826 | 111.7 | -8.55 | 0/7 | - | - | -7.31 | -1.25 |
| C2a documented + P | 421 | 45.3 | -1.27 | 4/7 | - | - | -1.98 | +0.71 |
| C2b keep+ep + P + crowd gone | 712 | **75.2** | **+9.24** | 4/8 | 78.6 | +1.98 | +9.59 | **-0.35** |
| C2b without crowd | 2,613 | 224.0 | -1.24 | 2/8 | - | - | +7.34 | -8.59 |
| C2b without creator | 3,553 | 375.0 | -19.57 | 2/8 | - | - | -10.14 | -9.43 |

The event with no door is red, as 5.4 says. Documented-project does not recover it (C2a under
the floor, hold +0.71 on 107 trades). The demonstrated-episode sentence clears the floor and
prints +9.24 SOL on the fitting week; the plus is the top 1 % of trades, 4 of 8 days, and the
hold half is **-0.35 SOL**. Clock 45 on C2b is +0.61 SOL, 4/8, without the top 1 % **-3.32**.
Creator-in is load-bearing. Crowd-gone is load-bearing on the fit half and does not save the
hold half. The slow-wall door against the same event is below, and it is the cell that
changes the verdict.

### The slow-wall door against the same event - the cell C0a unblocked

```
D slow-wall launch door (3.1), 5 % and 8 % cuts   E P X R S as above
frame  same 291,339 burst-start fires.  study-kernel/cvx_c2_slowwall.py
```

| slice (trail40 c600) | n | first/day | SOL | %/trade | days+ | top1 % | max tok | l50 | wo top1 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| keep + P (best public door) | 3,474 | 341.4 | -23.14 | -3.33 | 1/8 | - | - | 5.9 % | -58.13 |
| slow-wall 5 % + E, no P | 8,646 | 1,279.3 | -92.35 | -5.34 | 1/7 | - | - | 4.6 % | -182.34 |
| slow-wall 5 % + E + age/v, no creator | 2,971 | 439.6 | -15.04 | -2.53 | 2/6 | - | - | 2.8 % | -47.58 |
| **slow-wall 5 % + E + P** | 1,006 | 68.7 mean, **2/6 days over fifty** | **+15.51** | **+7.71** | 4/6 | 78.3 | 11.4 % | 2.5 % | **+3.37** |
| slow-wall 8 % + E + P | 908 | 60.2 | +11.57 | +6.37 | 4/6 | 92.5 | 15.3 % | 2.2 % | +0.87 |
| slow-wall 5 % + E + P + episode | 955 | 62.3 | +14.66 | +7.67 | 4/6 | 82.0 | 12.1 % | 2.2 % | +2.64 |

**The door the program trusts is the one that recovers the event.** The same event books
-5.34 %/trade behind the door alone and **+7.71 %/trade** behind door + permission, a 13-point
swing, on 1,006 trades over the ticket floor. Clock 45 on the same cell is +0.81 SOL: the
harvester exit is the one the story derives, and the scalper is the control. Every term is
load-bearing - dropping the creator costs 30 points a trade.

**And it does not ship, for a reason that is not the sentence.** 708 of the 1,006 trades and
82.1 % of the net are one creation build alive for two days (3.1a). Leave-one-build-out and a
build bootstrap:

| cell | builds | positive | leave-one-out worst | bootstrap positive | p5 |
| --- | ---: | ---: | ---: | ---: | ---: |
| slow-wall 5 % + P, trail40 | 19 | 9 | **+2.77** | 89.8 % | -1.70 |
| slow-wall 5 % + P, clock 45 | 19 | 5 | -2.34 | 55.2 % | -4.36 |
| slow-wall 8 % + P, trail40 | 13 | 5 | -1.17 | 72.2 % | -2.56 |

The 5 % cell survives its best client leaving, which no earlier cell has done, and lands at
89.8 % rather than 95 % on the build bootstrap. The tail gate fails (top 1 % = 78.3 % against
9-12 %) though the body pays: the other 996 trades are +3.37 SOL, about +1.7 %/trade net.

**C2 does not ship on any door here, and the slow-wall cell is the closest the program has
come.** What it is short of is builds, not terms - 19 draws on a week against roughly 150 on
thirty days. Next story is quiet deep-age (G1), not another AND on burst-start.

## 6.8 The solo 26 as a door, and as agreement (C6)

```
D (a) none  (b) the 26's creation-seq set  (c) slow-wall launch door
E the door-v3 event (49,831 fires, a real left tail) and the burst START
P as each parent defines   X shipped and trail40 c600   R one per token   S 0.2
A = distinct solo-26 wallets that bought STRICTLY BEFORE the decision print
frame  last-leg week. 131,862 solo buys on 22,442 coins.
       study-kernel/cvx_c6_agreement.py, cvx_c6_control.py, cvx_c6_oos.py, solo26.json
```

### The creation-sequence door is not a door

The 26 touch **143 distinct creation sequences**, and those cover **95.3 % of tape coins and
97.3 % of prints**. Their own coins sit inside it 97.3 % of the time against a pool of 95.3 %:
**concentration 1.02**. Node by node it is the same - 86.0-94.8 % of coins, concentration
1.03-1.13, the best being deep-age big clip on 3 wallets and 979 coins.

| as a door on burst start + P (trail40 c600) | n | first/day | SOL | %/trade | days+ | clients | bootstrap |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| seq door + P | 3,654 | 360.5 | **-23.07** | -3.16 | 1/8 | 65 | 13.4 % |
| slow-wall door + P | 1,006 | 68.7 mean, 2/6 days | +15.51 | +7.71 | 4/6 | 19 | 89.8 % |

**The creation structures these traders trade are the market, not a filter.** 95.2 % of
slow-wall coins are already inside the seq door, so it adds nothing to the door that works.
It is usable only as a weak EXCLUDE - it removes 4.7 % of coins - and it is not a door.

### Agreement is the strongest L-term measured, and it is judgement

A is 0 on 88 % of fires, 1 on 5,011, 2 on 713, 3+ on 113. Read inside a reserve band, so every
trade in a row can actually reach -50 % (1.3):

| band | base L-rate | A >= 1 | A >= 2 | A >= 3 |
| --- | ---: | ---: | ---: | ---: |
| 42.43-50 | 36.05 % | **13.73 %** | **10.79 %** | **6.82 %** |
| 50-70 | 29.83 % | 13.32 % | 9.35 % | - |
| 70+ | 37.30 % | 9.09 % | - | - |

Monotone in A, in every band, a 3-5x reduction. **Three controls, and it survives all three.**

| arm | in the roster's selection window | outside it (09-04 ..) |
| --- | ---: | ---: |
| **solo 26**, A >= 1 lift | **0.40** | **0.37** |
| **solo 26**, A >= 2 lift | 0.33 | **0.18** |
| rejected 10 (same roster, failed `t >= 2`) | 0.91 | 0.98 |
| matched-random wallets, mean of 20 draws | 0.77 (p5 0.66, p95 0.87) | 0.82 (p5 0.64, p95 0.92) |

The random null is **not 1.00**: "any wallet bought this coin before us" is already worth about
20 % off the L-rate, and that part is not judgement. The 26 sit at 0.37-0.40, far outside the
null's 5th percentile, and the ten roster wallets that failed the profit cut sit inside it.

**And it transfers.** The 26 were chosen on 08-27..09-03, which overlaps this tape, so the
in-window number is circular. On 09-04 onward - outside the selection window entirely - the
lift is **0.37 and 0.18**, unchanged, while rejected stays at 0.98 and random at 0.82. On 193
A >= 1 trades and 62 A >= 2 trades, so the count is small and the effect is large.

### It stacks with bundle share, and together they turn the sign

Among the 9,892 door-v3 trades that can reach -50 %:

| slice | n | L-rate | %/trade | SOL |
| --- | ---: | ---: | ---: | ---: |
| all | 9,892 | 34.23 % | -14.62 | -289.19 |
| bundle < 0.20 | 1,156 | 16.09 % | -5.42 | -12.53 |
| A >= 1 | 1,439 | 13.48 % | -5.06 | -14.57 |
| A >= 2 | 356 | 10.39 % | -3.37 | -2.40 |
| **bundle < 0.20 AND A >= 1** | 513 | **8.77 %** | **+3.86** | **+3.96** |

Two independent decision-time terms - one about how the coin was built, one about who is
already in it - and only together do they turn a -14.62 %/trade population positive.

### On the best cell it buys every gate except total SOL

| slow-wall + P, plus... | n | first/day | SOL | %/trade | days+ | worst day | l50 | hold SOL | bootstrap |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| nothing | 1,006 | 68.7 mean, 2/6 days | **+15.51** | +7.71 | 4/6 | -0.55 | 2.5 % | -0.28 | 89.8 % |
| A >= 1 | 931 | 59.6 | +14.90 | +8.00 | 4/6 | -0.12 | 1.8 % | +0.22 | 91.2 % |
| **A >= 2** | 867 | **53.9** | +13.03 | +7.51 | **6/6** | **+0.03** | 1.7 % | **+0.36** | 91.1 % |
| nodes >= 2 | 732 | 48.1 | +10.96 | +7.49 | 5/6 | -0.04 | 2.0 % | +0.80 | **93.2 %** |

`A >= 2` costs 16 % of the SOL and buys a book that is positive on every one of six days, a
positive hold half and a lower loser rate. **None of that can be used.**

**`A >= 2` is refused, not refuted.** It is the readers' mint list used as a gate and a factor
built on wallet identity: the super-root `CLAUDE.md` says "his coins are never a gate", 7.4 of
[_!___strategy.md](_!___strategy.md) says "never build a factor on wallet identity", 9 says
"his mint list is not a gate", and [_!___derive.md](_!___derive.md) 14 says the same. The
"53.9 first-per-mint a day" quoted beside it is also wrong as a gate reading: it is a mean over
a cell that clears the floor on **two of six days** (4.8).

**Removing the term makes the sentence better**, which settles the question: +10.98 -> **+13.75
SOL**, top client 66.0 % -> **64 %**, bootstrap 94.5 % -> **95.9 %**. Agreement stays what it is
worth being - the thermometer that says the L axis exists, and the thing an ix-structure twin
has to reproduce.

**What agreement does not fix is the client.** The top creation build still carries 80.6 % of
the net, and the cell's tickets stay on the same two days. That is the
constraint in 3.1a and it is unchanged.

---

## 6.9 Mid-tape one-shot instruments: 9999hu, 88887Q, 9Uq8GV, 8aaRWu, ApfmkS, create cgroup

```
D unnamed (cgroup / ix_count census vs tape; hindsight of their coins is a ceiling, not a door)
E named by response vs base; default hypothesis burst START
P none
X clock 45 . tp30 c600 . trail40 c600   (ceiling only)
R one per token   S 0.2   seat lag_115
frame  last-leg week, 6.76 days, 12.47 M prints, 127,833 tokens
      node buys 14,866 on 7,876 coins. 3Xk2Eu has 0 prints on this tape.
      study-kernel/cvx_midtape.py
```

Loop [1] only. Their prints stay out of every public event. Their mint list is never D.

### Create cgroup is the market, not a door

| grain | node share | tape share | concentration |
| --- | ---: | ---: | ---: |
| 6ix | 44.5 % | 38.3 % | 1.16 |
| 5ix | 26.8 % | 30.7 % | 0.87 |
| 7ix | 11.0 % | 10.7 % | 1.04 |
| 3ix | 4.7 % | 6.2 % | 0.76 |
| excl dump | 20.4 % | 10.4 % | **1.97** |
| excl 3ix | 3.3 % | 4.2 % | 0.79 |
| excl dead | 0.1 % | 1.9 % | 0.05 |
| bundler cgroup | 1.1-1.7 % of 9999hu / 88887Q | 4.1 % | ~0.3 |
| slow-wall 5 % | 16.9-17.1 % | 6.0 % | ~2.8 |
| `5ix:ix#6f` | 4.9 % | 0.8 % | **6.24** |

Include of 5ix / 6ix / 7ix is C6 at coarser grain: concentration about 1. Underweight of 3ix is
0.76, too weak to filter. They **overweight** dump-factory coins. Slow-wall is 17 % of their
book, not the door they live in. `5ix:ix#6f` is one creation fingerprint (the client that
carries C2), not a group.

### Burst START is 8dtx's tell, not the two largest books

Response within 2 s, own prints never events, lift vs a 3 % random phase-buy base:

| event | n | 9999hu lift | 88887Q lift | 8dtx2t pct / base |
| --- | ---: | ---: | ---: | ---: |
| base_buy | 132,623 | 1.00 | 1.00 | 0.217 |
| burst START | 201,342 | **1.02** | **1.14** | 0.520 (lift **2.40**) |
| buy >= 1 | 342,133 | 1.87 | 1.79 | 1.75 |
| keep buy >= 0.5 | 551,387 | 1.96 | 1.86 | 2.60 |
| silence break | 38,373 | 0.61 | 0.66 | **4.52** |

9999hu lands 12 prints / 637 ms into a burst (p50), self-start 0.2 %, named burst-start 18 %.
88887Q is 6 prints / 404 ms, named burst-start 10 %. 8dtx2t is 2 prints / 135 ms and **does**
name the opening size print (prev >= 0.5 on 53 % of entries).

DELAY at 115 ms is alive for the two largest: only 13 % / 19 % of their bursts are already
over. 8dtx2t is 45 % over. The unused members are the better seat. They still do not name
burst START.

Half of 9999hu is younger than the mid-tape window (age p25 4.2 s, p50 16.4 s). Age >= 15 s
leaves 2,275 of 4,439 entries.

### Ceiling on their coins, age >= 15 s, lag_115 (hindsight D)

| wallet | decision | clock 45 | tp30 c600 | trail40 c600 |
| --- | --- | ---: | ---: | ---: |
| 9999hu n=2,275 | burst start | -0.56 SOL, -0.12 %, 3/7 | -4.65, -1.02 %, 2/7 | **+1.78, +0.39 %, 4/7** |
| 9999hu | print before them | -12.02, -2.64 %, 0/7 | -13.55, -2.98 % | -9.08, -2.00 % |
| 9999hu | own print | -18.31, -4.02 %, 0/7 | -21.52, -4.73 % | -15.46, -3.40 % |
| 88887Q n=3,435 | burst start | **-14.16, -2.06 %, 2/7** | -20.95, -3.05 % | -28.97, -4.22 %, 0/7 |
| 88887Q | print before them | -18.85, -2.74 %, 0/7 | -20.33 | -35.09 |
| 88887Q | own print | -28.52, -4.15 %, 0/7 | -30.81 | -45.02 |

Copying them is red. Firing one print earlier is red. Burst-start leftover on **their** coins
at this seat is noise (9999hu) or red (88887Q). The +2.6 to +6.7 %/trade ceiling in 5.4 is
8dtx's coins, not this node's two largest books.

### 9Uq8GV names the node's event, and leftover on their coins is real

Response lift: buy >= 1 **4.75**, burst START **3.12**, vs 8dtx2t 1.75 / 2.40. Age p50 154 s,
named burst-start 50.7 %, burst 180 ms p50, 37 % already over by 115 ms. Create cgroup is
still the market (6ix conc 1.18). Dump overweight **2.74**. `5ix:ix#6f` conc 6.53 is the
same client, not a group.

Ceiling, age >= 15 s, n=1,024, lag_115, hindsight D:

| decision | clock 45 | tp30 c600 | trail40 c600 |
| --- | ---: | ---: | ---: |
| burst start | **+8.45 SOL, +4.12 %, 7/8** | +4.18, +2.04 %, 5/8 | +5.70, +2.78 %, 5/8 |
| print before them | -9.13, -4.46 %, 2/8 | -12.06 | -12.26 |
| own print | -10.25, -5.00 %, 2/8 | -13.26 | -13.42 |

Burst-start clock 45 on keep coins +5.19 %/trade (699); on dump +1.43 % (297); on slow-wall
+7.01 % (194); **off slow-wall still +3.45 %** (830). Leftover is not only the slow-wall
door. Copying them is red.

### Public sentence, E = buy >= 1 (the named tell)

```
D none / keep / not-dump / not-3ix / slow-wall 5 %, one at a time
E non-racer buy >= 1.0 SOL, not the node's prints
P age 60-900 s . vsol 33-81 . creator not sold
X clock 45 . tp30 c600 . trail40 c600
R one per token   S 0.2   seat lag_115
```

No 50 % episode. No tighter slow-wall cut.

| D | clock 45 | tp30 | trail40 c600 |
| --- | ---: | ---: | ---: |
| none | -64.03, 0/8, -3.27 % | -91.50, 0/7 | -75.19, 0/8 |
| keep | -55.62, 0/8 | -85.92 | -74.78 |
| not dump | -63.15, 0/8 | -91.76 | -76.77 |
| not 3ix | -55.72, 0/8 | -84.53 | -72.03 |
| **slow-wall 5 %** | +3.48, 4/6, +0.81 %, top1 201 % | +0.66, 3/6 | **+15.68, 5/6, +7.66 %, first/day 25/178/210/44/39/11, top1 74.8 %, wo top1 +3.95** |

The only green public cell is slow-wall + size-buy + P, and it is C2 at a wider event:
same SOL, same per-day floor fail (over fifty on 2 of 6), same tail. It does not ship.

**Verdict:** 9Uq8GV confirms the node's story on their own coins. Create cgroup, keep,
not-dump and not-3ix do not fill D. Slow-wall is the door already measured (6.4).

### 8aaRWu does not name the event; leftover on their coins is a short keep-pop off slow-wall

777 buys on 659 coins, clip p50 0.28 SOL, age p50 77 s, self-start 0.5 %. Named burst-start
12.4 %. Burst 224 ms p50, 32.7 % already over by 115 ms. Create cgroup is the market (6ix
conc 1.18). Dump overweight **2.52**. 3ix is overweight (conc 1.50), not a filter.
`7ix:Buy` conc 22.4 is two launch builds (top 55.6 %), not a group.

Response lift, vs a 3 % random phase-buy base: buy >= 0.5 **1.72**, buy >= 1 **1.66**,
burst START **1.33**, not-3ix 1.79, slow-wall size-buy **2.22**. Silence-break 0.92.
Nothing names the way 9Uq8GV names buy >= 1 (4.75).

Ceiling, age >= 15 s, n=613, lag_115, hindsight D:

| decision | clock 45 | tp30 c600 | trail40 c600 |
| --- | ---: | ---: | ---: |
| burst start | **+2.11 SOL, +1.72 %, 5/7** | -2.05, -1.67 %, 2/7 | +0.21, +0.17 %, 3/7 |
| print before them | -1.54, -1.26 %, 2/7 | -5.63 | -3.04 |
| own print | -2.17, -1.77 %, 2/7 | -6.66 | -3.64 |

Burst-start clock 45 on keep +3.64 %/trade (407); on dump -0.95 % (156); on slow-wall
**-0.89 %** (108); **off slow-wall +2.28 %** (505). Followed (not self-start) +1.63 %.
Copying them is red. Eating 30 % (tp30) is red: the leftover is a short pop.

### ApfmkS names nothing; 45 % self-start; followed leftover is unnamed

538 buys on 433 coins, clip p50 0.97 SOL, age p50 172 s, **self-start 45.0 %**. Named
burst-start 1.9 %. Prev >= 0.5 SOL on 9.5 % of entries. When they are in a burst, 17.9 %
of those bursts are over by 115 ms. Create cgroup is the market. Dump overweight 1.94.
3ix underweight 0.57, too weak to filter.

Response lift: buy >= 0.5 **0.95**, buy >= 1 **1.00**, burst START **0.79**, silence-break
0.13, slow-wall 0.42. Every public event is base. They do not follow a size print.

Ceiling, age >= 15 s, n=521, lag_115, hindsight D:

| decision | clock 45 | tp30 c600 | trail40 c600 |
| --- | ---: | ---: | ---: |
| burst start | **+2.48 SOL, +2.38 %, 6/7** | -2.76, -2.65 %, 2/7 | +0.21, +0.21 %, 4/7 |
| print before them | +2.00, +1.92 %, 6/7 | -2.08 | -0.94 |
| own print | -1.23, -1.18 %, 2/7 | -5.98 | -4.29 |

The burst-start plus is the 288 entries they **follow**: **+2.83 SOL, +4.90 %, 6/7**.
Self-start (233) is -0.73 %. Keep +2.96 %; dump -0.89 %; slow-wall **-3.73 %**; **off
slow-wall +3.26 %**. Copying them is red. Print-before plus is their own arrival in the
window (after-them 29 %), the 5.5 pattern.

**Verdict:** all seven members of the node are measured. 8dtx and 9Uq8GV name the event
and leftover on their coins is real. 9999hu / 88887Q / 8aaRWu do not name it; ApfmkS
starts the burst and names nothing. Create cgroup, keep, not-dump, not-3ix and
7ix:Buy do not fill D. Slow-wall is C2. 8aaRWu / ApfmkS leftover sits **off** slow-wall
on keep coins, so loosening that door is the wrong direction for these two books.

## 6.11 Mid-tape episodes 1 / 2 / 3+: the four unpriced facts are the market

```
D the four facts, one at a time (n_pro, n_pro_60, n_hold, creator in, last buy was pro)
E burst START
P age 15-900 s . vsol 33-81
X clock 45 . trail40 c600
R one per token   S 0.2   seat lag_115
frame  last-leg week, 6.76 days. 14,860 node round-trips (replay3 position builder),
      14.3-26.3 % of mints get a second episode. study-kernel/cvx_midtape_ep.py
```

Episode = position leaves zero to position returns to zero. Snapshot at the OPEN from
prints `0..i-1`, node-blind. Gap close-to-next-open p50 **56 s** (16.5 / 55.8 / 183).
That is on-chain, not a Telegram reaction.

### First vs later on the same mint

| at open | ep1 n=11,377 | ep2 n=2,317 | ep3+ n=691 |
| --- | ---: | ---: | ---: |
| n_pro p50 | 7 | 12 | 17 |
| n_pro_60 p50 | 4 | 3 | 2 |
| n_hold p50 | 7 | 12 | 17 |
| cr_sold p50 | 1 | 1 | 1 |
| last buy was pro p50 | 0 | 0 | 0 |
| age p50 | 42 s | 169 s | 397 s |

n_pro and n_hold **grow** because the coin ages (ep2 >= ep1 on 100 % / 99 % of pairs).
Windowed machines **fall** (n_pro_15 p50 2 -> 1). Later tickets are quieter, not a
re-lit crowded window. cr_sold stays the same on 91.6 % of pairs; last_pro stays the
same on 75.7 % - and the stable value is **0**.

Ep1 state on one-shot coins vs coins they return to is the **same** four-fact
distribution (n_pro p50 7 and 7, n_pro_60 4 and 4). Those facts at first entry do not
name the coins they will re-enter.

### Lift vs a 2 % phase-buy base (ep1, age/v band)

| fact | lift |
| --- | ---: |
| n_pro 4-7 | 1.55; other bins ~0.7-1.0 |
| n_pro_60 | 0.77-1.08 |
| n_hold | 0.72-1.02 |
| creator still in | **1.42** (26 % vs 18 % base; majority of their entries are still after a creator sell) |
| last buy was a pro machine | **2.92** (13.1 % vs 4.5 %) |

### Public sentence, E = burst START

| D | clock 45 | trail40 c600 |
| --- | ---: | ---: |
| none | -438.38, 0/8, -4.83 % | -479.16, 1/8 |
| n_pro >= 2 / 4 / 8 | -4.42 / -3.77 / -2.94 %, 0/8 | -8.36 / -7.17 / -5.99 % |
| n_pro_60 >= 2 | -4.81 %, 0/8 | -8.13 %, 0/8 |
| n_hold >= 1 | -4.70 %, 0/8 | -8.84 % |
| creator in | -2.04 %, 0/8 | -3.95 % |
| last was pro | -4.87 %, 0/8 | -6.27 % |
| n_pro_60 >= 2 + creator in | **-0.92 %, 1/8, -10.24 SOL** | -1.51 %, 1/8 |

Every cut red. Closest to zero is still negative. last-was-pro lift does not transfer
onto burst START as a door.

**Verdict:** multi-trade is real and the gap is seconds. The four unpriced facts do not
fill D: they do not separate return-coins from one-shot coins, they are not a stable
re-checked threshold (windowed count falls; cumulative count is age), and as public
doors on burst START they book -0.92 % to -4.87 %/trade.

## 6.12 Mid-tape holder book: tokens remaining, not last-side

```
D never_sold / dev_share / sold_back / pro_hold / last_pro_hold / top1
E burst START
P age 15-900 s . vsol 33-81
X clock 45 . trail40 c600
R one per token   S 0.2   seat lag_115
frame  last-leg week. Token deltas from K/vsol. Node wallets out of the book.
      14,385 episode opens. study-kernel/cvx_midtape_hold.py
```

6.11 measured "pushers still hold" as last print was a buy. This is the same fact as
balances.

Creator share at ep1 is already ~0 (p50). never_sold falls 0.89 -> 0.80 from ep1 to
ep2; sold_back rises 0.69 -> 0.81. Pro buyers hold 4 % of supply (p50). Ep1 on
one-shot vs return coins is the same book (sold_back 0.71 vs 0.63, top1 0.17 vs 0.15).

Lift vs a 2 % phase-buy base, ep1: last_pro_hold 0.15-0.40 **3.50** (5.3 % of their
opens); pro_hold 0.25-0.50 **2.75** (2.2 %); top1 0.20-0.35 **2.50**; never_sold
>= 0.95 **1.76**. Creator still holding 5-15 %: 1.53.

| D | clock 45 | trail40 c600 |
| --- | ---: | ---: |
| none | -4.83 %, 0/8 | -9.15 %, 1/8 |
| last_pro_hold >= 0.15 | -3.57 %, 1/8 | -4.36 %, 0/8 |
| pro_hold >= 0.25 | -4.70 %, 1/8 | -7.29 %, 0/8 |
| dev >= 0.05 | -2.97 %, 1/8 | -6.94 % |
| never_sold >= 0.80 | -5.28 %, 1/8 | -9.02 % |
| sold_back <= 0.40 | **-11.23 %, 0/8** | -15.99 % |
| top1 <= 0.15 | -4.58 %, 0/8 | -8.41 % |
| dev >= 0.05 + never_sold >= 0.80 | -3.63 %, 2/8 | -7.45 % |

Every cut red. The 3.50 lift on last-pro remaining does not pay as a door on burst
START. sold_back low (still-held supply) is worse than the parent.

**Verdict:** the fourth fact, measured as tokens, does not fill D. Ep1 holdings do not
name return coins. Stop adding holder-book or last-side terms onto this event.

# 7. THE BOOKS

Every rule that exists, with the coordinate that produced its number.

## 7.0 Door-v3 MONEY at lag_115 (C0b)

```
D shipped launch door: previous-day launches >= 20, slow-wall rate >= 8 %, not bundler
E first buy at age 5-300 s that makes non-creator buyers-after-5s = 2
P creator has not sold
X shipped (arm 21 / trail 36 / unarmed stop 43.75 / cap 1200, price = reserve 10/20/-25
  squared) . trail40 c600 . clock 45
R one per token   S 0.2   seat lag_115; SlotEnd and lag_0 beside
frame  last-leg week, 6.76 days.  study-kernel/cvx_money.py and cvx_c0b.py, independently
```

| cell (lag_115) | n | first/day | SOL | days+ | top1 % | wo top1 | n_l50 | fit SOL | hold SOL |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| MONEY D+E+P shipped | 1,667 | **246.7** | **+5.38** | 3/6 | **357.5** | **-13.85** | **235** | +7.30 | **-1.93** |
| MONEY without creator | 2,948 | 436.2 | -15.04 | 2/6 | - | - | 407 | -5.47 | -9.57 |
| slow-wall 5% + E+P shipped | 2,032 | 300.7 | +3.30 | 3/6 | 716.4 | -20.31 | 268 | +8.29 | -5.00 |
| MONEY D+E+P trail40 c600 | 1,667 | 246.7 | +3.18 | 3/6 | 595.6 | -15.75 | 219 | - | - |
| MONEY D+E+P clock 45 | 1,667 | 246.7 | +5.22 | 3/6 | 240.8 | -7.35 | 193 | - | - |
| MONEY D+E+P lag_0 | 1,667 | 246.7 | +38.18 | 6/6 | 50.9 | +18.76 | 76 | - | - |
| MONEY D+E+P SlotEnd | 1,667 | 246.7 | +5.22 | 3/6 | 368.3 | -14.00 | 227 | - | - |

Floor clears (247 first-per-mint a day). Money clears on the fitting week. Tail fails:
the top 1 % of trades is 357 % of net against a 9-12 % calibration, and without those
trades the book is **-13.85 SOL**. Walk-forward fails: hold **-1.93**. Creator-in is
load-bearing (dropping it is -15 SOL). Zero-lag is +38.18 and 6/6 days: most of the
published SlotEnd +43.56 is the fill, not leftover at 115 ms. SlotEnd here equals
lag_115.

The left tail is here: **235 trades at -50 % (14.1 %)** against 42 at +200 %. That is
the L-axis book C1 did not have.

**Reproduced independently.** `study-kernel/cvx_c0b.py` re-derives the same sentence from the
tape and the C0a door labels with no shared code path beyond the kernel, and lands within one
trade: 1,666 fires, +5.39 SOL, top1 357.0 %, wo-top1 -13.84, 235 at -50 %, fit +7.31 /
hold -1.93. The shipped exit is booked there as arm 21 / trail 36 / stop 43.75 on the price -
the reserve thresholds squared, which is the only correct conversion, and `spec['stop']` is
the kernel key that carries the unarmed leg (`kernel_test3.py`).

**The door and the permission are each load-bearing, monotonically:** the event alone is
-10.34 %/trade, + P is -8.00, + door is -2.55, + both is +1.62.

**The build holdout, which is the real gate here (3.1a):** 23 builds, 10 positive, and
dropping the best one takes the book to **-6.50 SOL**. The build bootstrap is positive in
61.7 % of resamples with a p5 of -12.73. Adding `bundle < 0.20` (3.7) is the only version that
survives: 11 builds, leave-one-out worst **+1.42 SOL**, bootstrap positive in 93.8 %, p5 -0.22
- but on the harvester exit the same cell is leave-one-out **-0.10** and 79.8 %, so it does not
hold on both exit families.

**C0b does not ship**, and neither does C0b + bundle. What this coordinate delivered is the
L-axis answer (3.7) and the build-unit correction (3.1a).

---

| rule | coordinate | book | status |
| --- | --- | --- | --- |
| **Rule 1, Flip-Catch - Bracket (hot-tape)** | D none . E public sell >= 1 SOL from a seller who bought <= 30 s ago, >= 15 recipes in 5 s, new high <= 20 s . P age >= 158 s, >= 368 buyers, reserve <= 100 SOL . X +20 % / -60 % / 90 s . R one per coin . S 0.35 . seat lag_115, every leg | every-leg holdout **+4.44 %/trade, 100 a day, 5/5, top 1 % 9.8 %**; simulate books it ticket for ticket | **passes every ship bar** (1.22, 1.23). Rule 1b, the wall target, beside it (1.25). Clean test: the days after 09-10 |
| Zigzag-turn conjunctions | D keep-create + one >= 50 % episode . E +15 % off the new low . P every 2-3-term conjunction of 16 wallet-free terms . X clock 30, trail50 c600 | 522 cells over the floor: 8 / 17 positive, **0 tail-robust**; best 10 on days 0-3 +74.87 -> **-40.46** on days 4-7 | red. A conjunction buys "not decaying", about the toll |
| Machine print in the dip | D keep-create + one >= 50 % episode . E non-racer buy >= 0.5 SOL in the dip . P 1-4 terms of 10 . X clock 45, trail40 c600 | parent **-457.29 SOL, 0/8**; 141 floored cells, 0 tail-robust; best 5 +57.67 -> **-19.37** out of sample; the document adds nothing on this event | red |
| Hot-tape re-entry flush (H1) | D none . E -20 % drawdown, -5 % in 5 s, buyers accelerating . P about 60 pre-registered cuts . X 18 families . seat FOLLOW | captures a +0.29 % move where the node's margin implies +3.65 %; **-0.63 % at a zero fee** | red, and not on cost. Absorption on a curve reads backwards (strategy 7.1) |
| **Door rule v3 MONEY (C0b)** | D shipped 8 % launch door . E 2nd non-creator buyer at age 5-300 s . P creator in . X arm21/tr36/stop43.75 c1200 . R one per token . S 0.2 | **`lag_115` +5.38 SOL, 3/6 days, 247 first/day, top1 358 %, hold -1.93**; 235 trades at -50 %. lag_0 +38.18 / 6/6. SlotEnd +5.22 | **does not ship.** Floor and money clear; tail and hold fail. Creator-in is load-bearing. L-axis book exists (7.0) |
| Door rule v3 RATE | same, tighter door | SlotEnd +30.74 SOL / 629 fires; `lag_115` **-0.53 SOL, 3 of 7** | dead at the seat |
| **Burst-start event** | D none . E burst start . P none . X four families . seat lag_115 | on the traders' coins **+2.6 .. +6.7 %/trade, every exit**; full tape with no door **-1.5 .. -13 %** | **open, blocked on the door** (5.4). Public doors on this tape: 6.4, red |
| C2 documented x burst START | D document . E burst start . P age/v/creator . X trail40 c600 | **-1.27 SOL, 4/7, 45.3 first/day**; hold +0.71 on 107 trades | **does not ship** |
| C2 keep+ep x burst START | D keep + demonstrated episode . E burst start . P age/v/creator/crowd-gone . X trail40 c600 | **+9.24 SOL, 4/8, 75.2 first/day, top1 78.6 %**; fit +9.59 hold **-0.35** | **does not ship** |
| C5 quiet deep-age, token-silence burst start | D keep / document / keep+ep . E first buy >= 0.5 after >= 10 token-silent slots . P age>=400 / v / quiet / creator . X trail40 c600 | keep+P **-32.46 SOL, 0/8, 200 first/day**; document **-3.63**; keep+ep **-9.46**; both halves red | **does not ship.** DELAY: the burst lasts ~80 ms |
| C5 quiet deep-age, returning-pro restart | D keep / document . E returning professional after 10-slot build silence . P age>=400 / v / quiet / creator . X trail40 c600 | keep+P **-1.88 SOL, 1/8, 37/day**; document +0.40 (7.5/day, top1 94 %, hold -0.05) | **does not ship.** Does not describe GZmUDs on this tape |
| G1 deep-age big clip, public size buy | D keep / document / slow-wall . E non-racer buy >= 1.0 SOL . P age>=200 / v 45-80 / live n60>=20 / creator . X trail40 c600 | keep+P **-13.19 SOL, 1/8, 125 first/day**; document +1.75 (16/day, top1 78 %); slow-wall +4.69 (33/day, top1 116 %, wo-top1 -0.77) | **does not ship**. Response equals base. Size print is not the tell |
| Mid-tape instruments, all seven members | D cgroup / keep / not-dump / not-3ix / sw5 . E named by response . X clock 45 / tp30 / trail40 . seat lag_115 | 9Uq8GV ceiling +4.12 %/trade, names buy>=1; 8aaRWu +1.72 % clock 45 off slow-wall, lift 1.33; ApfmkS names nothing, self-start 45 %, followed +4.90 % unnamed; 9999hu/88887Q leftover ~0 | **does not ship** (6.9). All members measured. Door still missing. Slow-wall cell is C2 |
| Mid-tape episodes 1/2/3+ four unpriced facts | D n_pro / n_pro_60 / n_hold / creator in / last-pro . E burst START . P age/v . X clock 45 / trail40 . seat lag_115 | all red; closest n_pro_60>=2 + creator in **-0.92 %/trade, 1/8**. Ep1 state does not separate return coins. Gap p50 56 s | **does not ship** (6.11). Four facts are the market on this event |
| Mid-tape holder book at episode open | D never_sold / dev / sold_back / pro_hold / last_pro_hold / top1 . E burst START . P age/v . X clock 45 / trail40 . seat lag_115 | all red; last_pro_hold>=0.15 lift 3.50 then **-3.57 %/trade**. Ep1 book does not name return coins | **does not ship** (6.12). Tokens remaining is not D on this event |
| **C8 four-slot inventory walk** | D documented . E token-silence >=10 slots . P creator in . X trail40 c600 . R unlimited . S 0.2 . seat lag_115 | **+47.54 SOL, 7/7, hold +13.33, LOO +13.90, boot 99.7 %**; tickets 47/170/220/192/140/125/63 (tracks documented births); plus is **age < 60 s** (+48.23); age>=60 **-0.82** | **does not ship**. Launch book, not mid-tape. Day 0 = 47 is a 6.2 h stub. Tail 40 %. Inventory has no mid-tape 4-tuple that pays |
| **C9 four new events** | D n_pro>=8 . E after-flush first buy . P creator+age/v+cu . X trail40 c600 . R unlimited . S 0.2 . seat lag_115 . age>=60 | **+8.73 SOL, 5/7, hold +2.03, LOO +5.93, boot 98.5 %, top client 32 %**; tickets 21/72/170/143/58/35/39; peak/trough 8.1x, top2 58 %; wo top1 **+1.41**; top1 84 % | **does not ship**. TYPE fails. Client gate clears. Floor 4/7. Tail. Not the next parent |
| **C10 S4 sell-then-buy** | D slow-wall 5 % . E price under own exit (n_sell>=10, rebuy frac>=0.25, no is_pro) . P creator+age/v . X trail40 c600 . R unlimited . S 0.2 . seat lag_115 . age>=60 | **+4.47 SOL, 3/6, hold -1.13, LOO +0.57, boot 81.8 %, top client 87 %**; tickets 16/179/208/33/32/7; peak/trough 26x, top2 81 %; wo top1 **-3.63**; top1 181 % | **does not ship**. TYPE fails: two-day client inside slow-wall. Body red |
| **C11 late-leg** | D slow-wall 5 % . E first >=0.5 buy of staged-leg cluster 2+ . P creator+age/v . X trail40 + that machine sells . R unlimited . S 0.2 . seat lag_115 . age>=60 | **+6.93 SOL, 5/6, hold +0.34, LOO +2.15, boot 99.5 %, top client 69 %**; tickets 2/78/92/11/3/2; peak/trough 46x, top2 90 %; wo top1 **+5.33**; top1 23 % | **does not ship**. TYPE fails. Body pays. Plus is C2's door on two days. TYPE reading: slow-wall × av × trail40 **+5.14**, body **+0.75** |
| **C12 second-attempt burst** | D n_pro>=8 . E first >=0.5 buy of this ix structure's second burst on this coin . P creator in . X trail40 c600 . R unlimited . S 0.2 . seat lag_115 . age>=60 | **+3.12 SOL, 4/8, hold -7.61, LOO -7.18, boot 57.4 %, top client 330 %**; tickets 53/171/344/377/203/142/102/2; peak/trough 3.7x, top2 54 %; wo top1 **-26.04**; top1 935 % | **does not ship**. TYPE passes. Floor passes on full days. Body, tail, hold, client gate fail. SOL leader is slow-wall (+17.78, d50 2/6), not the reading |
| **C13 arrives from another coin** | D none / keep / slow-wall / n_pro>=8 . E first >=0.5 buy here after a live buy on a different coin (gap < 10) . P standing . X trail40 / derived / leave . R unlimited . S 0.2 . seat lag_115 . age>=60 | SOL leader slow-wall × creator+av × trail40 **+16.46, 4/6, d50 2/6, top1 73.6 %, body +4.35**; TYPE 26.4x / 81 %. Wide TYPE cells **-4.64 to -437**. leave red | **does not ship**. READING none. DELAY = 0 (live-gap p50 1.0 slot). k==0 1.9 % |
| **C14 first buy after a run of sells** | D none / keep / slow-wall / n_pro>=8 . E first >=0.5 non-racer buy after >=3 consecutive sells . P standing . X trail40 / derived / low . R unlimited . S 0.2 . seat lag_115 . age>=60 | SOL leader slow-wall × creator+av × shipped **+10.40, 5/6, d50 2/6, top1 90.2 %, body +1.02**; TYPE 37.8x / 80 %. Closest TYPE cells **-5.24 to -9**. Wide TYPE cells **-194 to -289**. low red | **does not ship**. READING none. Leftover 81 ms (gap p50). k==0 0 |
| **C15 first operator after only creator and seed racers** | D none / keep / slow-wall / n_pro>=8 / group-live-now . E first >=0.5 non-creator non-seed operator buy; prior prints only creator and seed racers . P standing . X trail40 / derived / follow . R unlimited . S 0.2 . seat lag_115 . age>=60 | SOL leader none × creator × clock45 **+0.01, 7 trades, d50 0/3**; full-tape none × none × clock45 **-50.75**, 1,360 trades, 0/8. Age p50 0 s. follow red | **does not ship**. READING none. DELAY = 0 (prior-gap 0 slots, next-print 2 ms). k==0 0 |
| **C16 first run of an operator structure** | D none / keep / slow-wall / n_pro>=8 / group-live-now . E first >=0.5 operator buy on a coin that already has a non-creator non-seed print . P standing . X trail40 / derived / follow . R unlimited . S 0.2 . seat lag_115 . age>=60 | TYPE reading slow-wall × cu_hi × derived **+2.27, 3/6, d50 4/6, top1 219 %, body -2.71, hold -2.93, LOO -1.32, boot 65 %**. SOL leader n_pro>=8 × creator+av+cu × derived **+9.14**, TYPE fails 6.9x / 62 %. Wide TYPE **-41 to -75** | **does not ship**. Remaining spend 8.8 s on 22.5 %. Next-print 56 ms. k==0 0 |
| Launch-build door | D launch build . E none | 4-6x on all 30 days, 22-day holdout, no decay | **stands as a screen.** A door is not a trade |
| Documented-project rule | D document . E buy >= 0.5 SOL . P age/reserve/builds . X trail 40 c1800 | fitting week +0.40 %; **holdouts +1.46 % and +0.64 %/trade, 4 of 7 days each, +3.77 SOL on 2,156 tickets** | **weakly positive out of sample** - a lead for paper. Every week under 1 SE from zero |
| L-door on documented-project (C1) | D document + safety-panel cuts . E buy >= 0.5 SOL . P age/reserve/builds . X trail40 c600 | parent **+1.21 SOL, 4/7, 23.5 first/day, 8/646 trades at -50 %**; creator-in +3.36 SOL (hold +0.14); bundle/fresh/snipe/dev fail hold | **L-axis empty on this book.** Creator-in is a permission, not L-selection. Door-v3 L-door unrun on C0b's 1,667 fires |
| Campaign-break rule v0 | D ixh 29d9aacb… . E its buy ends a >= 10-slot buy silence . P vsol 65-100, break_idx >= 4, tagged >= 15, tape not buy-heavy, below 97 % of the 30-min vsol max . X TP +40 % else 600 s clock . seat **lag_115** | **`lag_115` -0.60 SOL, 4/8, 18.6 first/day, hold -2.98**; trail40 +4.18 / 6/8 / hold -0.01 / still 18.5/day; slot+2 TP40 **-0.89**. Species widening -15.57 / 0/8 | **does not ship**. Floor, money and hold fail. The +2-slot mint-disjoint holdout is n = 1 |
| Trough ceiling | D keep . E every real episode low (look-ahead) . X tp100/tr50/c1200 | +26.99 %/trade, 8/8 days, top 1 % = 12.2 % of net | **ceiling, not a rule** - and the tail calibration |
| Real-time trough detector | D door . E up-tick within 2 % of the trailing 15 s low . X several | -7.33 %/trade; precision 5.6 % against about 45 % needed | refuted as built: a price path cannot tell a turning low from a falling knife |
| 30-day launch-door rules (MAX SOL / SAFETY / shipped) | D launch door . X reactive trail priced at the breaching print | `lag_115` **-29.06 / -18.94 / -99.70 SOL** | refuted; the edge was the exit fill (4.5) |
| Campaign-rider family | D campaign class . E camp / camp+no-extraction / +returning+live+creator-in . seat lag_115 | -5.58 % to -10.15 %, 0/8 days | **this family is red. It is not the v0 sentence above** |
| No-initial-buy door + router buy | D creation carries no buy and creator never traded . E router buy >= 0.5 SOL . X clock 45 | fitting week +12.58 SOL, 5/7; three earlier weeks red | refuted on a disjoint holdout; the fitting week was one launch machine active two days |
| Wave node, all branches | every seat | -0.70 to -35.87 SOL | **closed by mechanism** (5.6) |
| Attention-arrival node | every seat, every permission | truncation decomposition | **closed by mechanism** (5.5) |
| Axiom push, cells A and B | seat **slot +1**, with a take-profit on a convex book | -109.83 / -42.67 SOL, 5 of 5 days red | red at a seat worse than ours |
| Copying any wallet | lag_115 | negative in every cell | **closed by mechanism** |
| Machine cadence as a signal | lag_115 | forward arrivals equal a matched control (115 vs 114, 147 vs 149, 208 vs 217); a random print beats a confirmation print, peak ratio 1.72 vs 1.42 | refuted |
| The extraction tell as a permission | lag_115 | "push has not sold" -7.22 % against "has sold" -4.32 %, against -4.80 % for all fires | refuted and inverted. Untested as an **exit** trigger |
| Supervised model, about 40 coin features | token-level, decision-time | fits days 0-3 at +6.1 %, books -3.6 % on days 4-6 | refuted; a gradient-boosted classifier reaching AUC 0.91 and 65x base response in its top 0.1 % still books -10 % |

---

# 8. THE OUTSIDE RECORD

A survival analysis of **832,941 pump.fun launches** (7x this repository's sample) tests exactly
the axis that works here ([arXiv 2607.02823](https://arxiv.org/html/2607.02823)):

| indicator | graduation rate | lift | multivariate Cox HR |
| --- | ---: | ---: | ---: |
| baseline | 0.198 % | 1.00 | |
| twitter / X advertised | 0.227 % vs 0.149 % | 1.52x | 1.31 |
| website advertised | 0.264 % vs 0.158 % | 1.67x | 1.19 |
| **telegram advertised** | **1.485 % vs 0.166 %** | **8.94x** | **5.40** (CI 4.73-6.17) |
| all three present | 1.919 % | **17.4x** | |
| log initial market cap | | | 4.51 |

**The label is graduation, which is survival.** It confirms the survival half of section 3 and
says nothing about spikes, exactly as our own data does. The authors' causal caveat matches our
thesis: a telegram channel "may proxy for creator effort, discoverability to buy-side bots and
traders, or selection by creators who already expect to succeed."

**Two cautions on transferring it.** The label is not our book's label, and on our own survival
book the telegram-first slice reads **-2.30 %, 2 of 7 days** while the website-without-telegram
slice carries **+2.38 %, 4 of 7**. An external HR on graduation does not license a change to a
money sentence without measuring the money sentence.

Wash trading and staging, for the actor model
([arXiv 2507.01963](https://arxiv.org/html/2507.01963v1)): 74.8 % of flagged manipulation is wash
trading, run by tiny groups (median 2.8 actors), and **62.9 % of extraction events follow a
visibility-building operation on the same coin.**

---

# 9. WHERE THE REST IS

| what | where |
| --- | --- |
| the method, the gates, how a result is written | [_!___derive.md](_!___derive.md) |
| the open queue | [_!___workflow.md](_!___workflow.md) |
| every idea, in Door / Event / Permission / Exit | [_!___inventory.md](_!___inventory.md) |
| the market model, the basis, the open/closed ledger | [_!___strategy.md](_!___strategy.md) |
| the 26 independent traders, ranked, with their five nodes | [solo-traders.md](solo-traders.md) |
| frozen sentences, never edited after their date | [study-kernel/frozen-sentences.md](study-kernel/frozen-sentences.md) |
| the pricing kernel and its acceptance tests | [study-kernel/](study-kernel/) |
| the launch-door reconciliation ladder | [../../roadmap/launch-door-rule.md](../../roadmap/launch-door-rule.md) |
| 44 consolidated refuted study lines | [../../history/2026-09-03-refuted-lines-ledger.md](../../history/2026-09-03-refuted-lines-ledger.md) |
| the audit that produced the coordinate discipline | [../../../../docs/history/2026-09-09-closure-ledger-audit.md](../../../../docs/history/2026-09-09-closure-ledger-audit.md) |

Where study scripts and their inputs live: [_!___strategy.md](_!___strategy.md) 11.3.
