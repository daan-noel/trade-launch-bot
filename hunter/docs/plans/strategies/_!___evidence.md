# Evidence: every standing measurement, with its coordinate

The numbers behind [_!___strategy.md](_!___strategy.md). The ideas are
[_!___inventory.md](_!___inventory.md). A parent is derived by
[_!___derive.md](_!___derive.md). The gates and the queue are
[_!___workflow.md](_!___workflow.md).

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

## 1.5 The hot-tape node is SLOW, and that is the whole opening (H2)

Evidence 1.4 makes the seat the dominant term, so before any event work the six hot-tape wallets
were timed. `node-derivation/hot-tape/cvx_hot_sessions.py` and `cvx_hot_latency.py`.

**First, they share a trigger.** When one opens a position on a coin, another of the six buys
within 2 s far more often than a same-coin time-shuffled null allows:

| wallet | agrees within 2 s | shuffled null | lift |
| --- | ---: | ---: | ---: |
| 49uohd | 43.2 % | 4.7 % | **9.1** |
| AbQcLH | 36.6 % | 4.5 % | **8.2** |
| omegoM | 22.0 % | 2.7 % | **8.1** |
| 64hP97 | 27.8 % | 4.0 % | **7.0** |
| sssssw | 20.8 % | 3.5 % | **6.0** |

The lift decays 9.1 -> 3.7 -> 2.2 across 2 s / 10 s / 60 s, so the trigger is sharp in time. Six
independent readers firing inside the same two seconds is evidence that the trigger is **public
and on the tape**, which is what makes it findable at all.

**Second, their buying is not a burst - it is a refractory period.** Gaps between consecutive
buys by one wallet on one coin are UNDER-dispersed at short lags and OVER-dispersed at medium
ones, the opposite of clustering: 64hP puts 1.8 % of gaps under 5 s where an exponential null
puts 3.9 %, and 56.4 % under 30 s where the null puts 37.9 %. Median gap 48-100 s. At a 5 s
session break 97 % of sessions hold a single buy. `8fStGV` and `omegoM` are the two that do
burst. **So the unit is the individual buy, and "I have not bought recently" is part of the
state** - a term about OUR position, which is shippable.

**Third, and this is the gate: they are slow.** Assumption-free, the spread between first and
last arrival inside a 2 s agreement cluster (10,707 clusters) is **p50 0.429 s**, over 115 ms in
**75.6 %** of clusters, over one slot in 51.7 %, over a second in 23.6 %.

Against four public trigger definitions, the delay to their order landing, and the share where a
115 ms fill from that trigger lands at a print index STRICTLY BEFORE theirs:

| candidate trigger | delay p50 | prints between | **we fill first** |
| --- | ---: | ---: | ---: |
| last public buy >= 1 SOL | **1.288 s** | 9 | **85.0 %** |
| last print that moved price <= -2 % | 0.785 s | 3 | **85.1 %** |
| last burst start (gap >= 0.4 s) | 0.404 s | 2 | 78.7 % |
| last print that moved price >= +1 % | 0.266 s | 1 | 67.8 % |

Per wallet, from the nearest big public buy: 64hP97 **p50 2.183 s** (98.6 % over 115 ms), 8fStGV
1.204, omegoM 1.209, AbQcLH 1.257, 49uohd 0.716, sssssw 0.234.

**So whatever the trigger turns out to be, we land ahead of them 68-85 % of the time.** The
opening is not that we are fast in absolute terms - it is that this node is 4-19x slower than our
seat, and it is profitable anyway, which says **the edge survives at least a second of delay**.
We do not have to be fastest. We have to be right.

*The gate passes. What remains is entirely the event, the door, the permission and the exit.*

## 1.6 The hot-tape event, measured within the mint - and the bind it runs into (H3)

The design that makes this different from every earlier instrument step: **compare their buy
moments against non-buy moments ON THE SAME MINT.** Repeated buys on one coin share the
Telegram channel, the narrative, the creator, the launch build, the hour - all of it constant -
so any surviving difference must be an on-chain timing state. Off-chain data is excluded by
construction, the door is held fixed, and the event is measured alone. 93,588 cases, 374,339
controls, 11,367 strata, every feature node-blind including counts and gaps.
`node-derivation/hot-tape/cvx_hot_event.py`.

**The mint-local basis beats the absolute basis almost everywhere**, which settles how features
should be built from here: a coin measured against its own prior history, not against one ruler
for 11,765 coins. `wake` 0.605 -> `wake_z` **0.660**; `vol20` 0.550 -> `vol20_z` 0.591; `n60`
0.475 -> `n60_z` 0.418; `buy2` 0.614 -> `buy2_z` 0.640.

| feature (mint-local) | rank of a node buy among same-coin non-buys | reads |
| --- | ---: | --- |
| `wake_z` | **0.660** HIGH | the last 2 s are busier than this coin's own last 60 s |
| `buy2_z` | 0.640 HIGH | with real public SOL in them |
| `gap_z` | 0.365 low | prints arriving fast right now |
| `quiet60_z` | 0.418 low | but the last minute was unusually QUIET for this coin |
| `vol20_z` | 0.591 HIGH | and unusually volatile |
| `dd_peak` | 0.408 low | the coin sits deep below its own peak |
| `age` | 0.582 HIGH | and it is old |

**LULL, then WAKE.** Not a flush on a busy tape. It also explains how they can be 0.4-2.2 s slow
(1.5): after a lull there is no crowd to race.

**THE BIND, and it is the result.** The two halves of that story have opposite properties:

| half | features | dwell | reaction cost at 115 ms | within-coin lift |
| --- | --- | ---: | ---: | ---: |
| WAKE | `wake_z`, `buy2_z`, `gap_z`, `vol20_z` | **0.216 s** | **+5.98 %** | **4.5** |
| LULL | `quiet60_z`, `n60_z`, `dd_peak`, `age` | **24-52 s** | **-1.0 %** | ~1 |

Firing on the wake books -1.6 to -5.5 %/trade over eight exit families
(`cvx_hot_book.py`); firing on the lull books -4.1 to -11 %/trade over seven
(`cvx_hot_lull.py`), and a control with the lull term removed reads the same, so the lull adds
nothing on its own. **The half that carries the information has already moved the price by the
time we can act; the half we can reach in time carries no information.**

That is the time-axis twin of the ticket law: *the term that identifies the moment is the term
whose move has already happened.* It is measured here, not argued - the +5.98 % is the price
change between the state a watcher held and our own 115 ms fill.

**And the coverage number says the trigger is still unfound.** The best conjunction covers
**1.2 %** of their buys at a within-coin lift of 4.2. A term that explains one buy in eighty is
not their rule; it is a rare corner of it. The reaction cost proves the same thing from the other
side: if reacting at 115 ms costs 6 %, reacting at their measured 0.4-2.2 s costs more, and they
are profitable, so **they cannot be firing on lull-then-wake at all**.

**What this leaves.** Hard thresholds on the top two or three features throw away the features
with modest deviations that apply to ALL of their buys - `wake_z` sits at 0.660 across 93,137
cases, not across a rare tail. The unused instrument is a **within-mint conditional model on the
full feature vector**, fitted on one half of the coins and read on the other. That is the next
job, and it is the first time in this program the data has been arranged so that such a model
would be measuring the event alone.

## 1.7 The moment IS learnable, imitation is worthless, and the missing slot is the DOOR (H4)

1.6 left hard thresholds throwing the evidence away: `wake_z` sits at 0.660 across ALL 93,137 of
their buys, and cutting at `wake_z >= 2` kept the 2 % of moments where the price had already
jumped. So the whole feature vector was fitted at once, on the within-mint design, split by COIN
(a coin never appears in both halves). `node-derivation/hot-tape/cvx_hot_model.py`, `cvx_hot_model2.py`,
`cvx_hot_score.py`.

**A control-sampling artifact found and priced first.** With PRINT controls, a control's `gap` is
the full inter-arrival interval while a node buy sits INSIDE one - about half of one by
arithmetic. With TIME controls (uniform random seconds, spliced in as zero-size virtual prints),
the bias runs the other way: a random second is usually quiet, so cases read gap 0.156 s against
controls 4.163 s. Neither control is neutral, so the test is whether the signal survives with the
whole gap family deleted:

| fit | within-coin AUC on HELD-OUT coins |
| --- | ---: |
| print controls, every feature | 0.7678 |
| print controls, **gap family removed** | **0.7315** |
| time controls, gap family removed | 0.8220 |
| print controls, gap removed, **coin-relative features only** | **0.7212** |

**It survives.** The gap family is worth 0.036 of AUC, and the two opposite biases bracket the
answer. The last row is the shippable one: every feature is that coin measured against its own
prior history, so a live rule computes the score from the coin's own tape with no stratum and no
lookahead. Its top decile inside a coin holds **44.4 %** of their buys against a 20 % base
(lift 2.22).

**So their moment is learnable, on coins the model has never seen.** That is the first time in
this program anything has reproduced a professional's timing out of sample.

**And it is worth nothing.** Fired on the whole tape at lag_115, five score thresholds x seven
exit families, every cell is red: **-4.24 % to -10.62 % a trade, 0 of 8 days** on 2,772 to 105,295
trades. The reaction cost is **-0.50 % to +1.18 %**, so unlike 1.6 this is not a seat problem -
the fill is cheap and the moment is still not worth having.

That is the sharp form of the result. **We can stand at their moment, ahead of them (1.5), with a
clean fill, and lose five percent a trade.** So the edge is not in the moment.

**Where it is instead.** The within-mint design holds the coin constant, which is what makes it
honest about the event - and blind to the door by construction. Splitting the model's own fires:

| the model fires... | trades | per trade | reaction cost |
| --- | ---: | ---: | ---: |
| on a coin the node TOUCHES | 15,754 | **-2.63 %** | +2.64 % |
| on a coin the node never touches | 8,335 | **-14.21 %** | -2.59 % |

**11.6 points sit in the coin, not the second**, and 54 % of the model's coins are ones they never
go near. The node's edge is a DOOR effect that the event study was designed not to see.

**Next.** The door, at coin level, on its own data: what separates the 11,765 coins they touch
from the 116,068 they do not. It is a separate question with separate evidence, and neither slot
is allowed to borrow the other's.

## 1.8 The door is not a token group - it is THEIR ARRIVAL (H5)

1.7 put 11.6 points in the coin rather than the second. This is the door census that followed,
at coin level, on data the event study never saw: `node-derivation/hot-tape/cvx_hot_door.py`, universe =
32,905 coins with at least 30 prints and 60 s of life (not the whole tape, where dead coins
inflate every concentration), and every outcome field - `mfe`, `his`, `peak_in_first_run`,
`reply_count`, `ath_mc`, `complete` - excluded, because a door built on one is a lookahead.

**The winning group is not creation structure. It is EARLY LIVELINESS.** Base touch rate 35.04 %:

| group | coins | touch rate | share of coins | share of their trades | concentration |
| --- | ---: | ---: | ---: | ---: | ---: |
| reserve at age 60 s in 50-70 | 2,947 | **66.8 %** | 8.96 % | 34.29 % | **3.83** |
| reserve at age 60 s over 70 | 1,190 | 62.5 % | 3.62 % | 13.97 % | **3.86** |
| **reserve at age 60 s under 32** | 16,077 | **15.3 %** | 48.86 % | 6.80 % | **0.14** |
| over 200 prints in the first 60 s | 7,043 | 67.0 % | 21.40 % | 61.95 % | **2.89** |
| 30-80 prints in the first 60 s | 11,510 | 16.9 % | 34.98 % | 10.13 % | 0.29 |
| bundler creation group | 2,230 | 17.7 % | 6.78 % | 1.14 % | **0.17** |
| has a community link | 2,160 | 65.9 % | 6.56 % | 11.54 % | 1.76 |
| creation cgroup, every large group | - | 23-47 % | - | - | **0.58-1.63** |

Creation structure is again close to the market (6.8 found concentration 1.02 across all 26);
`5ix:ix#6f` reaches 3.15 but is 2 % of coins. **Half the tape is dead by age 60 s and carries
7 % of their trades** - that is the real coin filter, and it is a trajectory fact, not a birth
fact.

**And no public door rescues the book.** Applied to the model's own fires (top 1 % score,
cap 60, restricted to fires after age 60 s so the door is already knowable):

| door | trades | per trade | days positive |
| --- | ---: | ---: | :---: |
| none | 18,744 | -6.68 % | 0/8 |
| reserve60 > 40 | 11,561 | -7.73 % | 0/8 |
| over 200 prints in the first 60 s | 7,092 | -7.08 % | 0/8 |
| reserve60 > 40, prints60 > 80, not bundler | 9,090 | **-6.29 %** | 0/8 |
| the same plus a website | 3,298 | -5.60 % | 0/8 |

**Because the 11.6 points were never a property of the coin.** Splitting the same fires by
whether one of the six buys INSIDE our hold:

| | trades | per trade | days positive |
| --- | ---: | ---: | :---: |
| **one of them buys during our hold** | 7,258 | **+2.37 %** | **7/8** |
| they touch the coin but never during our hold | 7,847 | -8.02 % | 0/8 |
| they never appear on the coin at all | 8,984 | -12.71 % | 0/8 |

*The money is incoming demand, and these six ARE the demand.* Being on "their coin" pays nothing
unless they actually turn up while we are holding, and it happens on **30.1 %** of fires. With
arrivals at +2.37 % and non-arrivals at -10.53 %, the book breaks even at an arrival rate of
**81.6 %**.

**And the exit cannot close that gap.** Seven abort-on-no-response families - `abort=(T, g)`
closes at T seconds if price has not risen g percent, which is precisely "did anybody come?" -
across five score thresholds: best **-4.87 %**, none positive on more than 1 of 8 days. Cutting
the non-arrivals early also cuts the arrivals, and the toll is charged per ticket (4.7).

**RETRACTED, same day, on two measurements and one methodological error (1.9).** The reading
above - "the node is incoming demand and these six ARE the demand" - is wrong. Their share of the
SOL that makes the move is **2 % at the median and 5.2 % at the mean**; the public side puts in
4.59 SOL where they put in 0.19. They are DETECTORS of a move other people make, not the cause of
it. And a D+E that is negative closes nothing when P is empty and X is a guessed grid: the ceiling
if every fire exited at its own 120 s peak is **+19.26 %/trade** against the -6.67 % a cap-60 book
reads. See 1.9.

## 1.9 Three retractions, and the two slots that were never searched (H6)

The user rejected 1.8 on three grounds. All three hold, and two are measurable.

**1. A negative D+E closes nothing.** A sentence is a conjunction of four slots. 1.6-1.8 fixed X
to a generic grid and never built P at all, then reported "the node closes". That is not a
verdict, it is an unfinished sentence. *Only a conjunction with all four slots searched can close
a node* (7.4 law 17 says a red number closes a SENTENCE, never a slot - the same error in the
other direction).

**2. The latency claim was invalid and is withdrawn.** 1.5 measured "time since the last public
buy >= 1 SOL" and read p50 1.288 s. On a coin where big buys arrive every couple of seconds that
statistic reads about a second **whether or not anyone is reacting to it** - it is a WAITING time,
not a reaction time, and its wide exponential shape (p10 0.08 s to p90 9.41 s) is what no
relationship looks like. The same file's own `gap` reads **79 ms** from the previous print to
their buy at the median, and 25 % of their agreement clusters are tighter than 121 ms. **Their
reaction latency is unmeasured. "They are slow" and "we land ahead of them 68-85 %" are
withdrawn.** To measure a reaction you must first identify the trigger print; a nearest-event
waiting time cannot substitute.

**3. They do not ignite the move - they detect it.** Measured on the 7,734 fires where one of the
six buys inside our hold, decomposing the SOL that entered the pool between our fill and the peak:

| | median SOL into the pool, fill to peak |
| --- | ---: |
| the six wallets | **0.188** |
| everyone else | **4.593** |

Their share of the net move: p25 0.000, **p50 0.020**, p75 0.121, mean 0.052. *Between 95 % and
98 % of the move is other people.* So the +2.37 %/trade on arrivals is not their impact - their
arrival is a MARKER that a real up-move is happening. The question that closes or opens this node
is therefore not "will they turn up" but "is a real move starting", which is the P slot.

**4. And the exit has been leaving everything on the table.** Maximum favourable excursion within
120 s of our fill, on the model's own fires:

| population | p50 | p75 | p90 | p95 | p99 |
| --- | ---: | ---: | ---: | ---: | ---: |
| all fires | 4.13 % | 20.96 % | 53.23 % | 84.89 % | **202.85 %** |
| one of them arrives | 20.73 % | 48.60 % | 96.38 % | 141.32 % | 327.24 % |
| nobody arrives | 0.85 % | 9.00 % | 26.73 % | 45.07 % | 101.94 % |

Time to that peak: p50 **16.1 s**, p90 103.7 s.

**A cap-60 book on these fires reads -6.67 %/trade. Exiting each fire at its own 120 s peak would
read +19.26 %.** Every exit tried to this point is a guessed grid rather than a shape fitted to
that distribution - a payoff where half the fires go nowhere and the top decile runs 53 % to 200 %.

> **RETRACTED, 1.10.** The sentence that followed here read "twenty-six points sit in the exit".
> A perfect-exit ceiling is not money in a slot: the exit slot is now searched over 480 causal
> shapes at both seats and the best of them reads **-2.35 %** behind their print and **+0.21 %**
> ahead of it, against a ceiling of +53 %. The gap is the timing problem, and it does not close.
> See 1.10 and 7.4 law 23.

**So the node is NOT closed.** D and E are measured and negative on their own, which is the normal
state of a conjunction before P and X exist. The order that followed from this - X first, then P,
then E - is what 1.10 executes; X comes back empty, and the two that remain are **P** and a
re-derivation of **E** from a properly identified trigger.

## 1.10 The exit slot, searched (H6-X): the convexity is real and no exit reaches it

> **THE VERDICT IN THIS SECTION IS WITHDRAWN, 1.11.** The grid below is swept on 95,135 episodes
> with **D empty and P empty** - no door, no permission, no selection of any kind. Workflow 5
> refuses exactly that: *an exit swept on an unselected pool returns the shortest clock, because
> that pool is mostly dying coins*. What it returned - flat clocks beating ladders beating trails,
> `cap15 > cap30 > cap60`, shorter beating longer everywhere - is that refusal's own signature, so
> the search measures the pool and not the exit. Two scope errors ride with it. The geometry runs
> to **1800 s** on a node whose median hold is **19.9 s**, and inside their own hold the peak
> available is p50 **+7.6 %**, not +33 %. And the six wallets are pooled although `8fStGV` wins
> 68.3 % of its trades at a median of **+12.02 %** while the other five carry a negative median.
> **X on this node is not searched.** Every number below stands as a coordinate with D and P
> empty; the sentence "the slot is empty" does not. See 1.11 and 7.4 laws 26-28.

1.9 put 26 points in the exit slot and named it the largest unsearched thing on this node. The
slot is now searched - 480 causal shapes, three ladder depths, five flow tells, two seats, on the
convexity the six wallets themselves enter - and **the slot is empty**. The 26 points were the
distance to an oracle, not money on the table (7.4 law 23, written from this run).

Basis: an episode is their position, tracked to the token, opening when it leaves zero and closing
when it returns (the cvx_replay3 builder). **95,135 episodes**, 1.29 % still open at the tape edge
and reported not dropped, median clip 0.524 SOL, median one buy per episode. Entry is their own
buy; the exit families are ours; 0.2 SOL; both legs priced through the kernel. Their wallets are
the instrument (7.4 law 20) - a shape and a ceiling, never a term.

### The shape of what they enter

Our fill 115 ms behind their print, 1800 s of tape after it:

| | p10 | p25 | p50 | p75 | p90 | p95 | p99 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| MFE | 1.26 % | 9.65 % | **33.19 %** | 87.62 % | 180.61 % | 260.19 % | 472.12 % |
| MAE **before** the peak | -39.77 % | -21.73 % | **-7.39 %** | -0.42 % | 0.67 % | 2.21 % | 5.62 % |
| MAE over the whole window | -76.77 % | -66.64 % | -50.20 % | -28.76 % | -10.00 % | -3.43 % | 0.79 % |
| price at the horizon | -75.52 % | -63.95 % | -43.41 % | -4.08 % | 93.40 % | 185.07 % | 416.34 % |
| seconds to the peak | 1.79 | 12.24 | **82.80** | 343.99 | 798.75 | 1226.03 | 1723.19 |

**The drawdown before the peak is the number every guessed grid was missing.** By how big the peak
eventually got:

| peak reached | share of entries | MAE-pre p10 | p25 | p50 | t_peak p50 | t_peak p90 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| under 0 % | 5.2 % | -11.6 % | -5.5 % | -1.9 % | 1.3 s | 21.6 s |
| 0 to 5 % | 12.3 % | -12.5 % | -2.6 % | 0.0 % | 3.2 s | 65.2 s |
| 10 to 25 % | 17.6 % | -34.8 % | -18.3 % | -6.6 % | 35.3 s | 398.2 s |
| 50 to 100 % | 17.3 % | -47.5 % | -30.2 % | -13.9 % | 198.9 s | 952.4 s |
| **over 100 %** | **21.9 %** | **-46.4 %** | **-30.7 %** | -15.0 % | 412.9 s | 1355.4 s |

*A quarter of the entries that eventually double are under water by 31 % first.* Any trail or stop
tighter than about 30 % is a winner-killer, which is the mechanical reason every trail family in
1.5-1.8 read red.

### Continuation: the move has momentum, and it is not enough

At each checkpoint, the position's current level against the maximum still ahead of it:

| at 20 s, currently | n | future max p50 | p90 | P(+25 % more) | P(+50 % more) | future MIN p50 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| under -10 % | 23,099 | -2.3 % | 121.5 % | 31.6 % | 22.3 % | -59.3 % |
| -3 to 0 % | 8,765 | 24.1 % | 151.5 % | 49.2 % | 32.9 % | -46.9 % |
| +3 to 10 % | 13,531 | 36.6 % | 178.4 % | 61.4 % | 40.8 % | -48.4 % |
| +10 to 25 % | 13,440 | 56.1 % | 219.1 % | 78.5 % | 54.0 % | -48.4 % |
| **over +25 %** | 9,084 | **107.3 %** | 310.0 % | 99.3 % | **83.1 %** | -44.3 % |

Monotone at every checkpoint from 5 s to 160 s, and it strengthens with time. **This is a
continuation process** - a take-profit sells the half that was going to keep running. But the last
column is the wall: *the median future minimum is -40 to -60 % in every bucket, including the
winners*. Everything gives it all back. The exit lives between those two facts.

### Their own exit is not the edge

| | p10 | p25 | p50 | p75 | p90 | p99 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| their exit level, our basis | -24.87 % | -11.32 % | -1.97 % | 8.42 % | 25.19 % | 92.69 % |
| peak already available when they left | -0.05 % | 1.64 % | 7.63 % | 19.74 % | 41.16 % | 122.63 % |
| **capture ratio** | -2.00 | -1.52 | **0.07** | 0.65 | 0.85 | 1.00 |

They hold 20.8 s at the median, leave before the window peak in **65.5 %** of episodes, and land
within 10 % of it in **0.6 %**. Their own book is **+0.70 % on spend**. *They are fast, small and
early - they are not skilled exiters, and there is no exit craft here to copy.*

### The search, and the bug it caught

480 shapes: first rung 5-30 %, rung fraction 0.34/0.50, runner trail 20-40 %, dead cut
none/stop25/stop35/cap15/cap30/cap60, and two trail modes (armed at the first rung, or live from
the fill). Fitted on a random 15,000 of half the episodes and re-read on all 47,593 of the other
half. Three-rung ladders and five flow tells (net flow negative over 3 s or 10 s, buy share under
half, no buy print in 3 s, each at four arming levels) on top.

**The first version of this grid returned +8.30 %/trade, 8/8 days, top-1 % concentration 27 %,
leave-one-day-out +599 SOL - and every point of it was a lookahead.** The dead cut was evaluated
against the whole window ("no rung ever fired") instead of against its own deadline, so it cut
only the tickets that were never going to work. Corrected to index order, the same shape reads
**-2.38 %**. Recorded as 7.4 law 24; the corrected result is below.

| seat | best of the 480 | per trade | days | without its top 1 % | perfect-exit ceiling |
| --- | --- | ---: | ---: | ---: | ---: |
| FOLLOW, 115 ms behind their print | cap15 flat | **-2.35 %** | 0/8 | -3.38 % | +53.28 %, 8/8 |
| RACE, sequenced ahead of their print | cap15 flat | **+0.21 %** | 5/8 | **-0.89 %** | +57.52 %, 8/8 |

Every other family is worse, in a strict order: flat clocks beat ladders beat trails beat flow
tells, and **shorter beats longer everywhere** (cap15 > cap30 > cap60; trail 25 > trail 40). The
one shape that survived the first, buggy grid - half off at +10 %, trail the rest 25 % - reads
**-3.49 %** once its trail can no longer become retroactively live.

*The fastest exit wins on a payoff whose median peak is +33 %.* That is the whole finding: the
convexity is real, its timing is not predictable from the price path, and holding for it costs
more than it pays.

### Where it is not empty, and why that is the P slot

Split by facts knowable at the decision, cap15 flat, `ex_top1` being the book with its top 1 % of
tickets removed:

| slice | n | RACE %/trade | days | ex_top1 | FOLLOW %/trade | MFE p50 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| reserve 32-35 | 3,078 | **+1.83 %** | **8/8** | **+0.47 %** | -2.25 % | 11.3 % |
| reserve under 32 | 1,218 | +2.13 % | 7/8 | +0.87 % | -2.25 % | 2.9 % |
| age under 30 s | 8,309 | **+1.97 %** | **8/8** | +0.02 % | -1.33 % | 36.5 % |
| 2+ of the six acting in the last 60 s (not shippable) | 16,203 | +0.70 % | 6/8 | -0.31 % | -1.60 % | 37.4 % |
| reserve 50-70 | 36,218 | +0.15 % | 3/8 | -0.96 % | -2.19 % | 34.2 % |
| nobody else acting | 31,892 | -0.33 % | 1/8 | -1.59 % | -3.03 % | 21.6 % |

Two readings, and both matter:

* **At the FOLLOW seat there is no slice at all.** Not one cut of reserve, age, hour, agreement or
  wallet is positive. Copying this node is closed by the seat, again and now on a searched exit.
* **At the RACE seat the survivors are the SMALL moves.** Reserve 32-35 has the least convexity of
  any band (MFE p50 11.3 % against 34.2 % at reserve 50-70) and the best book. The reachable half
  and the informative half are opposite halves - the same anti-correlation as 1.6, now on the exit
  side of the trade.

**Verdict.** X is searched and closed as a standalone lever, at both seats, on the best entries
available. The exit is not where this node's money is, and the "26 points" of 1.9 is withdrawn as
a quantity. What survives is a **P** candidate in two public terms - *early on the curve
(reserve under 35) and young (under 30 s)* - and the unchanged requirement that E be re-derived
from an identified trigger, because the RACE seat is only reachable by deciding when they decide.

The agreement row is a **thermometer, not a term.** "Two of these six named wallets already
bought" is the roster's mint list used as a gate and is refused outright
([_!___workflow.md](_!___workflow.md) 11, 7.4 law 20). If concurrency is to enter a sentence it
has to be re-derived as a public tape fact - distinct payers, distinct routed builds - and
measured again from scratch; the number above says only that the thermometer reads warm.

Scripts: [node-derivation/hot-tape/cvx_hot_exit.py](node-derivation/hot-tape/cvx_hot_exit.py) (geometry, continuation,
their capture), [cvx_hot_exit2.py](node-derivation/hot-tape/cvx_hot_exit2.py) (the 480-shape grid, ladders,
flow tells), [cvx_hot_exit3.py](node-derivation/hot-tape/cvx_hot_exit3.py) (the two seats),
[cvx_hot_exit4.py](node-derivation/hot-tape/cvx_hot_exit4.py) (the decision-time slices).

## 1.11 The node is two animals, and only one of them pays (H7)

1.10's verdict is withdrawn at the head of that section: its 480-shape grid runs with D and P
empty, which is the one thing workflow 5 refuses, and "shorter beats longer everywhere" is that
refusal's signature rather than a fact about exits. This section replaces it. Five runs, on the
95,135 position-tracked episodes and on the full 12.47 M print tape.

### The question nobody asked

Their own book is a lottery: median trade **-2.07 %**, net margin 1.10 %, and the best 1 % of
their trades produce **180.6 %** of their net, so the other 99 % collectively lose. A perfect
reproduction of their decisions reads **-0.14 %/trade** at the PEER seat (1.4). *Imitating this
node was never going to pay at any AUC*, which is what 1.7 measures at AUC 0.72 and -5 %/trade.
The only object worth searching is a selector INSIDE their decisions, and 1.6-1.10 all carry the
93,137 buys as a single label (7.4 law 28).

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

*They buy a pullback inside a live up-move; the others buy a deep dip on an old coin.* 1.6 reports
this node buying deep below the peak (`dd_peak` 0.408) because the deep-dip members outnumber the
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
a worse book - 1.7's result reproduced through a completely different construction.

**The independent-machine count is the only term with a monotone money gradient that is not
headroom.** Under five distinct builds printing in the last 5 s books **-4.08 %/trade** over
85,416 fires; twelve or more books -3.17 %; twelve or more with a de-concentrated build mix books
**-2.99 %**. About **1.1 points**, and it does not cross zero. It is the first time the axis 1.4
calls unpriced enters this node's event at all, and within the coin it is what separates the two
animals: `nb5` 0.586 HIGH, `nb20` 0.565, builds new to this coin 0.558, build concentration 0.443
low, professional builds in the last 5 s 0.554.

### The lookahead this run caught

The level rule's one green cell reads **+5.02 %/trade on 8 of 8 days** behind the door 1.8 names,
reserve at age 60 s of 50 to 70. **All of it is the 15.5 % of its fires that happen before age
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
therefore their reaction time (1.9 point 2), and the machine axis as a fitted vector rather than a
threshold.

Scripts: [node-derivation/hot-tape/cvx_hot_sep.py](node-derivation/hot-tape/cvx_hot_sep.py),
[cvx_hot_sep2.py](node-derivation/hot-tape/cvx_hot_sep2.py), [cvx_hot_lvl.py](node-derivation/hot-tape/cvx_hot_lvl.py),
[cvx_hot_up.py](node-derivation/hot-tape/cvx_hot_up.py), [cvx_hot_mach.py](node-derivation/hot-tape/cvx_hot_mach.py),
[cvx_hot_mach2.py](node-derivation/hot-tape/cvx_hot_mach2.py).

## 1.12 The trigger, found: a flipper's sell inside a buying frenzy (H8)

1.11 leaves the node open on one question - are the members that pay reacting to a PRINT or firing
on a STATE? Five runs answer it and fill the event slot. `node-derivation/hot-tape/cvx_hot_trig.py`,
`cvx_hot_seat.py`, `cvx_hot_dump.py`, `cvx_hot_which.py`, `cvx_hot_dump2.py`, `cvx_hot_door2.py`.

### Their reaction, measured properly

For every buy, every public print in the 5 s before it is binned by class and lag, against random
times on the same coin within 60 s. The control absorbs the coin's own arrival rate, so 1.00 is no
relationship and a spike at one lag is a reaction at that latency - the measurement 1.9 demanded in
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

The 1.8 mechanism again, on a named event: the fire pays when the professional confirms the move
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

1.13 corrects the "five points in which coin": the coin list carries the member's future first
arrival, and on coins it has already traded E books the full-tape -0.68 %.

Scripts: [node-derivation/hot-tape/cvx_hot_trig.py](node-derivation/hot-tape/cvx_hot_trig.py) (reaction by class and lag), [cvx_hot_seat.py](node-derivation/hot-tape/cvx_hot_seat.py), [cvx_hot_dump.py](node-derivation/hot-tape/cvx_hot_dump.py), [cvx_hot_which.py](node-derivation/hot-tape/cvx_hot_which.py), [cvx_hot_dump2.py](node-derivation/hot-tape/cvx_hot_dump2.py), [cvx_hot_door2.py](node-derivation/hot-tape/cvx_hot_door2.py), [cvx_hot_arrive.py](node-derivation/hot-tape/cvx_hot_arrive.py).

## 1.13 The door and the exit, derived: the first positive sentence on this node (H9)

1.12 freezes E and leaves D empty and X a guessed clock. Four runs fill both from the unpriced side
and from the member's own exit, and correct one reading of 1.12.
`node-derivation/hot-tape/cvx_hot_door3.py`, `cvx_hot_door3b.py`, `cvx_hot_exit5.py`, `cvx_hot_exit6.py`.

### The coin list carries the member's future arrival

1.12 read "five points in which coin" off the split by 8fStGV's coin list. That list is built
from the whole tape, so a coin is on it because the member WILL trade it. Split the frozen E's
fires on its coins by whether the fire comes before or after its first buy on that coin:

| fires of E on its coins | n | %/trade | days |
| --- | ---: | ---: | :---: |
| before its first buy on the coin | 534 | **+4.46 %** | 7/7 |
| after it has already traded the coin | 849 | -0.68 % | 1/7 |

Where it is already on the coin, E books exactly the full-tape number. The "five points" are the
professional's FIRST ARRIVAL on the coin after our fire (its median is one buy per coin, p90
four), not a property of the coin that a public fact can name. **The door's target is that
arrival** - the 1.8 mechanism again. A member's coin list is split by before and after its first
buy before it is read as a door gap.

### The door, from the unpriced side

Thirty facts at the fire, every one from PUBLIC prints before it with the six node wallets dropped,
in five families: how this coin's earlier frenzy-sells resolved; who buys in the frenzy
(professional builds, routers, fresh wallets, build concentration); whether recent buyers still
hold; re-entry machines on the coin; and the seller.

On the broad event (every frenzy-sell, 15,019 fires, -2.12 %) they are gradients and none crosses
zero: professional wallets that already round-tripped the coin run -6.25 % in the lowest quintile
to -0.90 %; the share of the last five minutes' buys still held runs -0.15 % (4/7) when most have
sold to -5.07 % when all still hold - held tokens are the next sellers.

On E itself (2,219 fires, -0.68 %, 1/7) a one-sided cut chosen on half the days and scored on the
other half passes both folds for two facts, each fold picking nearly the same threshold alone:

| cut on E | fold A test | fold B test | full sample |
| --- | --- | --- | --- |
| few of this coin's earlier frenzy-sells made a new high within 15 s (`abs_rate <= 0.333`) | +0.45 % 3/4 | +2.40 % 3/3 | **+1.13 % 5/7**, 91 a day, body +0.24 |
| the seller takes 30 %+ profit on the sale | +0.08 % 3/4 | +1.73 % 2/3 | +0.78 % 5/7, body -0.77 |

`abs_rate` is not headroom: its rank correlation with reserve is -0.00, and it holds inside the
1-2 min and 2-5 min age bands (+2.02 % against -2.46 %, +1.77 % against -0.93 %). It does not
transfer to the broad event (-1.48 %). The event pays where this coin's earlier frenzies were sold
into and failed, and this one makes a new high anyway. The seller's profit is the price in
disguise - a flipper who bought 30 s ago and is up 30 % says the coin rose 30 % in 30 s - and it
makes the broad event worse (-3.00 %), so it is not a filling.

### Their exit, measured the way their entry was

Closing sells of each member against random public prints of the same holding episode:

| member | sells after | peak lift | at the sell | reading |
| --- | --- | ---: | --- | --- |
| 8fStGV | a public BUY >= 1 SOL / a +3 % print | 5.8 at 25-50 ms | +14.4 % since entry, 1.2 % off the in-hold peak, 10 s held | **sells the buy** |
| 49uohd | a public SELL >= 1 / a -2 % print | 14.9 at 25-50 ms | about break-even, 9 % off the peak | stop on a dump |
| sssssw | a public SELL >= 1 / a -2 % print | 13.6 at 75-100 ms | -1 % | stop on a dump |
| AbQcLH | nothing (every lift < 1.1) | - | 19 s held, the tape gone quiet | a timeout |

The member whose trigger E is buys the sell and sells the buy. Its hazard - the chance its next
print is the closing sell, over every public print of every hold - is a bracket: about 0 between
-20 % and +10 %, 6-20 % a print above +10 %, a second spike between -25 % and -40 % (2.0 to 6.9
times the in-hold rate; -20..-25 % is 0.23), time stops from 40 s, and nothing held past 80 s.
**Derived exit: take profit +10 %, stop -25 %, time 60 s.**

### The sentence

Frozen E, each exit with its own occupancy, `cvx_hot_exit6.py`:

| D | X | first/day | %/trade | days | body | top 1 % | biggest coin | win |
| --- | --- | ---: | ---: | :---: | ---: | ---: | ---: | ---: |
| none | cap15 | 328 | -0.68 % | 1/7 | -7.17 | - | - | 45.9 % |
| none | tp10 sl25 t60 | 445 | -0.30 % | 2/7 | -4.33 | - | - | 71.5 % |
| `abs_rate <= 0.333` | cap15 | 91 | +1.11 % | 5/7 | +0.21 | 84.7 % | 21.6 % | 49.3 % |
| `abs_rate <= 0.333` | **tp10 sl25 t60** | **124** | **+1.22 %** | **5/7** | **+1.23** | 39.7 % | 11.1 % | 74.8 % |

Days 0-3 +1.66 %/trade, days 4-6 +0.25 %. A hard -20 % stop books +0.56 % and a -15 % stop
-0.22 %: a stop inside the frenzy's own swing cuts trades that recover. The 75 % win rate is the
bracket's shape (many +10 % exits, few deep losses) and not a lookahead - every exit branch reads
only prints after our fill and resolves to the first index that trips it.

### Tape-driven exits do not beat the bracket

Behind the door, 69 % of the bracket's take-profits run on to +20 % within 120 s and 48 % of its
stop-outs were up +5 % first - but those are peaks, and a frenzy's price swings 5-10 % inside a
few prints. Twelve tape-driven exits, each with its own occupancy (`cvx_hot_exit7.py`):

| exit behind the door | %/trade | days | body |
| --- | ---: | :---: | ---: |
| the bracket, take profit +10 %, stop -25 %, 60 s | **+1.22 %** | 5/7 | **+1.23** |
| half at +10 %, half to +20 %, stop -25 %, 120 s | +1.17 % | 5/7 | +0.95 |
| sell the next public buy >= 1 once up +10 % | +0.91 % | 5/7 | +0.73 |
| bracket + close when buying stops, under water only | +0.82 % | 5/7 | +0.62 |
| bracket + breakeven once up +7 % / +5 % / +3 % | +0.61 / -0.22 / -0.85 % | 6/7 / 3/7 / 1/7 | +0.20 / -1.21 / -2.50 |
| trail 5 % from the peak once up +10 % | -0.28 % | 3/7 | -1.26 |
| bracket + close on a public sell >= 1 under water | -1.08 % | 0/7 | -3.16 |

Every tighter reaction trades a recovery for a protection and loses on net; closing on a big sell
under water contradicts the event itself, which is a frenzy absorbing big sells. The losers are
not an exit problem on this sentence: they are a do-not-enter problem for D or P.

### Verdict

**The first causal sentence on this node that is positive on the full tape**, with a door chosen
on held-out days and an exit read off the member's own hazard. It does not ship: the top 1 % carry
39.7 % of the net against the 9-12 % calibration, the later half of the days fades to +0.25 %, and
the whole book is about 2 SOL in a week at 0.2 SOL a ticket. The coin-list split survives inside
the door (+2.58 % on its coins, -1.31 % off them), and 1.13's first table says why: what is left is
the professional arriving on the coin after us, and the door has to predict that arrival.

Scripts: [cvx_hot_door3.py](node-derivation/hot-tape/cvx_hot_door3.py), [cvx_hot_door3b.py](node-derivation/hot-tape/cvx_hot_door3b.py), [cvx_hot_exit5.py](node-derivation/hot-tape/cvx_hot_exit5.py), [cvx_hot_exit6.py](node-derivation/hot-tape/cvx_hot_exit6.py).

## 1.14 The permission: a frenzy dies on a young, thin coin (H10)

1.13 leaves the losers as a do-not-enter problem: the stop-outs average -31 % and no exit cuts
them without cutting more recoveries. `node-derivation/hot-tape/cvx_hot_perm.py` reads 27 facts at the fire -
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
| E x door (1.13) | 124 | +1.22 % | 5/7 | -0.23 | +1.23 | 39.7 % | 11.1 % | +1.66 % | +0.25 % |
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

Scripts: [cvx_hot_perm.py](node-derivation/hot-tape/cvx_hot_perm.py), [cvx_hot_perm2.py](node-derivation/hot-tape/cvx_hot_perm2.py).

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
| E x door (1.13) | 117 | -0.53 % | 2/5 | -0.81 | -0.98 | - | - | 18.3 % |
| E x holders | 177 | +1.20 % | 4/5 | -0.37 | +1.19 | 37.8 % | 12.8 % | 13.9 % |
| E x age | 263 | +0.54 % | 3/5 | -0.17 | +0.00 | 99.9 % | 15.6 % | 15.7 % |
| **E x holders x age** | **132** | **+1.90 %** | **5/5** | **+0.17** | **+1.73** | **23.2 %** | **7.0 %** | **10.3 %** |

In-sample the same cell read +2.07 %, 141 a day, 7/7, top 1 % 17.6 %. Out of sample it keeps
92 % of its margin, every day is positive (+0.23, +1.02, +0.17, +0.60, +0.23 SOL), both halves of
the holdout are positive (+2.09 %, +1.64 %), and the stop-out rate is lower than in-sample. The
earlier-frenzy door of 1.13 does not hold (-0.53 %, 2/5): it was fitted, and 1.14 already found
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

Scripts: [cvx_holdout_export.py](node-derivation/hot-tape/cvx_holdout_export.py), [cvx_hot_holdout.py](node-derivation/hot-tape/cvx_hot_holdout.py).

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

Scripts: [cvx_hot_which2.py](node-derivation/hot-tape/cvx_hot_which2.py), [cvx_hot_ev2.py](node-derivation/hot-tape/cvx_hot_ev2.py), [cvx_hot_ev2b.py](node-derivation/hot-tape/cvx_hot_ev2b.py), [cvx_hot_ev2c.py](node-derivation/hot-tape/cvx_hot_ev2c.py).

## 1.17 Rule 1's exit, re-read on its own pool (H13)

The bracket of 1.13 was read off 8fStGV's closes on all its holds, before the permission existed.
Re-read on its 753 holds that pass rule 1's permission and start on a big public sell
(`node-derivation/hot-tape/cvx_hot_exit8a.py`): it holds through +8..+12 % (0.5-1.8 % a print), sells hard at
+15..+20 % (12-30 %), stops at -25..-30 % (3-11 %; under 1 % between -20 and -25 %), and the
-20..+5 % band waits for the clock, whose closes land at 60-90 s. Booked on rule 1's fires, each
exit with its own occupancy (`cvx_hot_exit8.py`):

| exit | study %/trade | study top 1 % | holdout %/trade | holdout top 1 % | holdout SOL |
| --- | ---: | ---: | ---: | ---: | ---: |
| take profit +10 %, stop -25 %, 60 s (1.13) | +2.07 % | 17.6 % | +1.90 % | 23.2 % | +2.26 |
| take profit +15 %, stop -25 %, 60 s | +2.15 % | 17.4 % | +2.33 % | 19.1 % | +2.46 |
| **take profit +15 %, stop -25 %, 90 s** | **+2.73 %** | **13.9 %** | **+2.53 %** | **17.7 %** | **+2.66** |
| take profit scaled by headroom (x0.20) | +2.09 % | 30.9 % | +2.10 % | 22.2 % | +2.63 |
| ride from +15 % while public buying continues | +0.72 % | 86.4 % | +2.18 % | 37.8 % | +1.85 |

The 90 s clock is the study tape's pick inside the derived range and the holdout confirms it:
+2.53 %/trade, 117 a day, 5/5 days, body +2.19, biggest coin 6.3 %. The tail is 17.7 % against the
15 % bar.

Scripts: [cvx_hot_exit8a.py](node-derivation/hot-tape/cvx_hot_exit8a.py), [cvx_hot_exit8.py](node-derivation/hot-tape/cvx_hot_exit8.py).

## 1.18 Rule 1's tail and clip

**The tail is upside gaps.** Rule 1 (take profit +15 %, stop -25 %, 90 s): all of the top 1 %
tickets are take profits whose fill landed past the target, +33..+61 % within 1-7 s of a frenzy
spike, spread over coins (the biggest carries 4-6 % of net). With every gain capped at the median
take profit (+12.9 %), the book stays positive - +2.00 SOL study, +1.01 holdout - and the top 1 %
share is 10.3 % / 12.6 %. The rule does not rest on its gaps. The cost of the book is its
stop-outs: 148 tickets at -30.4 %, -9.0 SOL against +14.4 from the take profits.

**The clip.** The path does not depend on the clip, so each ticket is re-priced from its entry and
exit reserves through the kernel (`node-derivation/hot-tape/cvx_hot_size.py`): 0.2 SOL books 0.68 / 0.59 SOL a
day (study / holdout), 0.35 SOL 1.08 / 0.94, **0.5 SOL 1.36 / 1.16 with every day positive on
both tapes**, 0.75 SOL 1.55 / 1.29 with a red study day, 1.0 SOL 1.40 / 1.10 and red days on both.
A replay does not price our buy moving the next prints, so the larger clips are upper bounds.

Script: [cvx_hot_size.py](node-derivation/hot-tape/cvx_hot_size.py).

## 1.19 Rule 1's door and its stop-outs: nothing at the fire separates them (H15)

`node-derivation/hot-tape/cvx_hot_door4.py` reads 20 facts at rule 1's fires, public prints only, on both
tapes: overhang, heat, frenzy composition, the seller, and a new count aimed at the member's kind
of buyer - distinct wallets on the coin that bought within 300 ms of a public sell >= 1 SOL.

- **The stop-outs are not separable once the permission is in.** The strongest fact against the
  take-profits is 0.055 from 0.5 on the study tape and a different fact (0.075) on the holdout.
- **The sell-reactive buyer count is not the arrival door**: AUC 0.60 study, 0.45 holdout for the
  member's first arrival within 90 s.
- **What predicts the member's arrival is the frenzy's size** - public buy SOL in the last 5 s,
  AUC 0.70 / 0.63 - a stronger E, not a door.
- **The cuts that pass both study folds are all a bigger frenzy** (>= 16 recipes, >= 11 SOL bought
  in 5 s, <= 7.75 SOL sold in 5 s). On the holdout they raise the per-ticket margin (+2.96 %,
  +3.10 % against +2.53 %) and lower the book (2.22-2.32 SOL against 2.66): fewer tickets.

D stays empty; rule 1 stays as it is. The stop-outs are the price of the sentence at this seat.

Script: [cvx_hot_door4.py](node-derivation/hot-tape/cvx_hot_door4.py).

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

Scripts: [cvx_r1u_cand.py](node-derivation/hot-tape/cvx_r1u_cand.py), [cvx_r1u.py](node-derivation/hot-tape/cvx_r1u.py),
[cvx_r1u_step1.py](node-derivation/hot-tape/cvx_r1u_step1.py), [cvx_r1u_step2.py](node-derivation/hot-tape/cvx_r1u_step2.py),
[cvx_r1u_grad.py](node-derivation/hot-tape/cvx_r1u_grad.py), [cvx_r1u_exit.py](node-derivation/hot-tape/cvx_r1u_exit.py). The engine is
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

Script: [r1_replay.py](node-derivation/hot-tape/r1_replay.py); tape export
[toolkit/lake_export.py](node-derivation/toolkit/lake_export.py).

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

Scripts: [r1_terms_audit.py](node-derivation/hot-tape/r1_terms_audit.py),
[r1_exact.py](node-derivation/hot-tape/r1_exact.py),
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
engine adopts on restart stores no fill depth: its `room_taken` reads `NaN` and only the stop and
the clock close it.

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
[_!___workflow.md](_!___workflow.md) section 4.

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
[_!___workflow.md](_!___workflow.md) 11 says "his mint list is never the universe". The
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

## 4.9 Ranking by SOL selects the two-day cohort

The C8-C11 walks rank occupancy cells by total SOL, then caption the leader as "closest".
On a week where two days carry one launch client, that ranking returns the client. The
floor and client gates catch the ship decision; they do not catch the search. Runner:
`study-kernel/cvx_day_skew.py`.

Tape: `cvx_prints.parquet`, 12,467,222 prints, 127,833 tokens, 2026-08-30 17:48 UTC through
2026-09-06 12:00 (6.76 days). Hours covered per UTC day from `cvx.py` DAY0: **6.2, 24, 24,
24, 24, 24, 24, 12**. The first and last days are stubs, not missing files. The lake
(`hunter/lake-data/trades/dt=2026-09-01` .. `09-08`) is complete on those dates and quieter
mid-week (Sep 4-6) then recovers Sep 7-8, which this tape never sees.

| series | per-day counts | peak/trough | top2 share |
| --- | --- | --- | --- |
| tape prints | 701k, 2.12M, 2.52M, 2.16M, 1.99M, 1.55M, 1.10M, 321k | 7.8x (stubs in) | 38 % |
| token births | 7,712, 21,906, 24,145, 20,838, 19,901, 16,498, 11,704, 5,129 | **2.1x on full days** (24,145 / 11,704) | 36 % |
| slow-wall births | 0, 1,049, 1,541, 1,140, 1,111, 1,998, 424, 353 | 5.7x | 46 % |
| documented births | 52, 182, 236, 208, 148, 130, 64, **0** | 4.5x | 44 % |
| n_pro>=8 ever | 614, 1,829, 2,489, 1,893, 1,656, 1,149, 756, 249 | 3.3x on full days | 41 % |
| C8 documented × silence × creator | 47, 170, 220, 192, 140, 125, 63 | 4.7x (tracks the door) | 43 % |
| C9 n_pro>=8 × flush × cr+av+cu age>=60 | 21, 72, 170, 143, 58, 35, 39 | **8.1x** | **58 %** |
| C10 slow-wall × rebuy × cr+av age>=60 | 0, 16, 179, 210, 34, 33, 8 | **26x** | **81 %** |
| C11 slow-wall × late × cr+av age>=60 | 0, 2, 78, 93, 11, 3, 2 | **46x** | **90 %** |

**The local tape is not a hole in the middle.** Day 0 of C8 (47 tickets in 6.2 h) is a stub
at ~182/day, not a type fail. Documented supply is 0 on the last 12 h because `cvx_meta`
is the keep+ep50 shortlist and does not cover those 5,129 births - a universe filter, not
a missing day of prints (6.13, strategy 7.4 law 30). "Documented births" in the table
above are births of that shortlist, so C8 tracking them is circular.

**C10 and C11 are not "slow-wall tokens get created more those days".** Slow-wall births
peak/trough at 5.7x; the cells are 26x and 46x, with 81-90 % of tickets in two days. The
conjunction is a client inside the door. C9 is the same shape, milder: n_pro>=8 supply is
3.3x on full days, the cell is 8.1x.

**Ranking is the method bug.** C8: 256 SOL>0 cells, 15 even-floor, SOL leader is the
keep+ep50 launch book (illegal D, 6.13). C9: 210 SOL>0, 3 even-floor (D=none × flush × creator+av+cu,
+3.61, body red). C10: 11 SOL>0, 0 even-floor. C11: 76 SOL>0, 0 even-floor. The SOL
leaders of C9-C11 fail TYPE (workflow 4). They are not the next parent. C7 adds clients of
a type that already prints every day; it does not turn these two-day books into a type.
The TYPE readings (not the SOL leaders) are 4.10.

## 4.10 Re-read C8-C11 under laws 29-31

Runner: `study-kernel/cvx_conj_reread.py`. TYPE pass: on full UTC days (hours
>= 20), tickets every day, peak/trough <= 1.5x the door, top2 <= door + 10 pp.
`D=documented` is dropped (law 30). Silence cells with k==0 share >= 5 % are
dropped (law 31).

**Funnel.** `cvx_meta.parquet`: 18,583 rows, 127,833 tape tokens, overlap
18,583, missing 109,250. Construction is keep+ep50. Website-led documented is
1,020 tokens, all inside that shortlist. `cvx_swdoor.parquet`: 164,569 rows,
overlap 119,234, missing 8,599, slow-wall True 7,616.

**k==0.** C8 silence 58,999 / 150,273 (**39.3 %**). C8 size 7.3 %. C8 burst
0.1 %. C8 restart / dip / two / money, and every C9-C11 event, are 0.0 %.

**The reading is the TYPE-pass, not the SOL leader.**

| walk | slice | TYPE-pass (best SOL>0) | SOL leader (not the reading) |
| --- | --- | --- | --- |
| C8 | full tape | slow-wall × burst × creator × shipped **+10.50**, body **-39.00**, pt 3.9x / door 4.7x | documented × silence is illegal (6.13) |
| C9 | age>=60 | n_pro>=8 × legs × cu_hi × derived **+1.75**, body **-2.08**, pt 4.1x / door 3.3x | after-flush × n_pro>=8 fails TYPE (8.1x, 58 %) |
| C10 | age>=60 | none | slow-wall × rebuy fails TYPE (26x, 81 %) |
| C11 | age>=60 | slow-wall × late × av × trail40 **+5.14**, body **+0.75**, pt 5.7x / door 4.7x, top2 50 / 48 | slow-wall × late × creator+av fails TYPE (46x, 90 %) |

No TYPE-pass mid-tape cell clears floor, tail, body and the client gate together.
C8's TYPE-pass is C2's door on the full tape, body red. C13 arrives-from-another-coin
has no SOL>0 TYPE-pass (6.18). C14 sell-run has no SOL>0 TYPE-pass (6.19).

---

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
RACE +2.21 % 8/8 is the seat that pays. Next is phase 6: which buy >= 1 (or burst starts) he
takes vs ignores on the same coin, spelled as a **state that has held**, not as a follow of
the buy. Empty D stays allowed.

9999hu / 88887Q stay a second instrument (sell >= 1), not this parent. Their behind book is
still slightly red; contrast on that sell is a different sentence.

---


# 6. THE CONJUNCTION SPACE

What a search over decision-time terms actually produces, on money, at the floor, walked forward.
This is the measurement that closes or opens an event family.

## 6.1 The zigzag-turn event

```
D keep-create with one completed >= 50 % episode   E zigzag TURN (+15 % off the new low)
P every 2- and 3-term conjunction of 16 decision-time, wallet-free terms
X clock 30 and trail50 c600, both read   R unlimited, one per coin   S 0.2   seat lag_115
frame  one week, 62,316 fires.  study-kernel/audit_conj.py
terms  creator_in . crowd_gone . crowd_held . pusher_in . 2bld_slot . terminal . router .
       not_racer . cu_hi . gap2s . nep2 . age300 . vband . size05 . document . new_species
```

| exit | cells above the floor | positive SOL | **tail-robust** | positive on both halves | walk-forward: best 10 on days 0-3 -> days 4-7 |
| --- | ---: | ---: | ---: | ---: | --- |
| clock 30 | 522 | 8 | **0** | 1 | +23.39 -> **-18.68** (1 of 10 positive) |
| trail50 c600 | 522 | 17 | **0** | 0 | +74.87 -> **-40.46** (0 of 10 positive) |

Best cells reach about zero, never past it: `creator_in + crowd_gone + size05` books +5.77 SOL
and -10.43 without its top 1 %; `creator_in + router + nep2` books +4.54 and -15.10.

**A conjunction moves this parent from -4.55 % a trade to about zero and stops there.** It buys
"this coin is not decaying", which is worth the toll. Nothing clears money, tail and both halves
together.

## 6.2 The machine-print-in-the-dip event

```
D keep-create with one confirmed >= 50 % episode   E a buy >= 0.5 SOL, not a racer, in the dip
P every 1- to 4-term conjunction of 10 terms   X clock 45 and trail40 c600, both read
R unlimited, one per coin   S 0.2   seat lag_115
frame  one week, 251,540 fires on 13,544 coins.  study-kernel/cvx_harvest.py, audit_conj2.py,
       audit_conj3.py
```

Parent: **-457.29 SOL, -7.08 % a trade, 0/8 days** on trail40 c600.

| scan | cells | positive | tail-robust | both halves | all three |
| --- | ---: | ---: | ---: | ---: | ---: |
| 2-3 terms, floor applied | 141 | 15 | **0** | 1 | 0 |
| 2-4 terms, floor removed | 330 | 68 | 7 | 27 | **5** |

Walk-forward on the floored scan: best 5 on days 0-3 book **+57.67**, then **-19.37** on days 4-7.

The five cells that clear money, both halves and the top-1 % removal all sit at **10-29
first-per-mint a day**, far under the floor, and the tail calibration disqualifies them anyway:
their top 1 % carries **49-87 %** of net and one coin carries **44-70 %**, against 9-12 % for a
real book at matched n.

The document ablation on this parent, which settles it as a convexity term:

| cell | n | first/day | SOL | %/trade | days+ | top1 % | one coin % |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| document alone | 1,772 | 104.6 | -18.19 | -5.13 | 0/7 | - | - |
| creator_in + document | 1,030 | 75.8 | -4.91 | -2.38 | 2/7 | - | - |
| terminal + creator_in + document | 166 | 15.7 | +2.38 | +7.16 | 5/7 | 72.8 | 57.8 |
| **terminal + creator_in, document EXCLUDED** | 1,465 | 126.1 | **+8.83** | +3.02 | 4/8 | 196.8 | 19.1 |

Adding the document removes 90 % of the tickets and keeps 27 % of the money. **It does not add on
this event.**

`creator_in` is in every top cell of both scans and is the only term whose removal always lowers
total SOL.

## 6.3 What the two scans establish, and what they do not

**Establish:** across two independent event families, 663 conjunctions above the ticket floor
produce zero cells that are simultaneously positive, tail-robust and positive on both halves; and
anything chosen in sample collapses out of sample.

**Do not establish:** anything about state-conditional exits - only the static family is
measured. L-selection on door-v3 MONEY fires is 3.7 and answered; on documented-project it
is 3.6 and empty. Burst-start behind every door including slow-wall is 6.4. Campaign-break
is 6.6. Quiet deep-age is 6.5. Slow-wall × token-silence is 6.13 and red (**-91.95 SOL**).

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

## 6.5 Quiet deep-age (C5)

```
D keep-create / documented-project / keep + demonstrated episode, one at a time
E first buy >= 0.5 SOL after >= 10 silent slots on the TOKEN (not tool silence), not a racer,
  not the node's own prints. Separate sentence: returning professional after that build's own
  10-slot silence (the "pusher restarted" spelling).
P age >= 400 s . vsol 33-81 . prior-minute prints <= 20 . creator not sold
X trail40 c600 and clock 45
R unlimited, one per coin   S 0.2   seat lag_115
frame  last-leg week, 6.76 days.
      study-kernel/cvx_quiet.py (book) and cvx_quiet_ladder.py (decision print)
```

On this tape the four wallets buy 7,941 times. They are not the silence-breaker (0.3-1.7 % of
entries). They land 51-163 ms after a public print, the second print of an 80-240 ms burst that
opened after 1-5 s of token silence. GZmUDs: prev is a buy 97 %, a buy >= 0.5 SOL 73 %, burst
gap p50 5.44 s (60.6 % >= 4 s), burst length p50 79 ms. That names E as the first size buy after
token silence. It is not C2 (C2 is a router run after that *tool* went quiet on a live tape).

The "pusher restarted" spelling does not describe GZmUDs here: 0 % of his buys are professional
under the week census, 0.1 % are the episode-1 pusher.

Response within 2 s vs a 0.70 % base: token-silence 2.47 %; token-silence + P 9.13 %; returning-pro
restart + P 6.05 %. The node names the reconstructed event. His mint list is not a gate.

| slice (trail40 c600) | n | first/day | SOL | days+ | top1 % | wo top1 | fit SOL | hold SOL |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| E only: token silence, full tape | 80,598 | 9,508 | **-1758.05** | 0/8 | - | - | -1077.00 | -681.05 |
| E only: returning-pro restart, full tape | 11,105 | 793 | **-111.70** | 0/8 | - | - | -53.82 | -57.88 |
| C5 keep + restart + P | 552 | 37.0 | -1.88 | 1/8 | - | - | -0.81 | -1.07 |
| C5a documented + restart + P | 144 | 7.5 | +0.40 | 4/6 | 93.8 | +0.02 | +0.45 | -0.05 |
| C5 age>=200 restart + P | 622 | 45.7 | +1.02 | 3/8 | 348.1 | -2.52 | +1.62 | **-0.60** |
| C5c keep + token silence + P | 2,407 | 200.6 | **-32.46** | 0/8 | - | - | -16.61 | -15.86 |
| C5d documented + token silence + P | 458 | 32.1 | -3.63 | 1/7 | - | - | -1.25 | -2.38 |
| C5e keep+ep + token silence + P | 1,750 | 120.3 | -9.46 | 0/8 | - | - | -3.72 | -5.74 |

Clock 45 is red on every cell. Ablating creator, quiet, or age on C5c makes the book worse.
The two plus cells are under the floor or carried by the top 1 %, and both fail the hold half.
Slow-wall launch door labels sit on this tape (3.1); that cell is unrun.

The burst they follow lasts ~80 ms. A 115 ms fill lands after it. The leftover of that burst is
not at this seat (DELAY). Doors on this tape do not recover either spelling of E.

**C5 does not ship.** Next story is C6 (agreement as a door), not another AND on silence-break.

---

## 6.6 Campaign-break v0 at lag_115 (C3)

The door is an `ix_patterns` fingerprint, not a `build_core`. Seed
[`campaign-break-29d9aacb`](../../../../scripts/seed-campaign-break-rule.sql) is the
four-instruction buy (CU price, CU limit, ATA CreateIdempotent, Pump.Fun Buy). On this
tape that is `ixh = 29d9aacbcbb4d1a8a8ad0975968a7626`: **42,178 prints**, n_ix 4, tool
PF.Buy. The same 8-char prefix is 0 of 15,915 `build_core` hashes; that is a different grain.

```
D  ixh 29d9aacbcbb4d1a8a8ad0975968a7626 (wildcard; the machine, not a wallet)
E  that buy ends a >= 10-slot buy silence (engine: 10 slots, lag 1, buy_count = 0)
P  vsol 65-100 . break_idx >= 4 . tagged buys on this coin >= 15 . 120 s buy-share <= 60 %
   . vsol < 97 % of the 30-minute vsol max
X  frozen: TP +40 % else 600 s clock. trail40 c600 and clock 45 beside
R  one per token   S 0.2 (0.05 beside)   seat lag_115; slot_end, lag_0, slot+2 beside
frame  last-leg week. 42,145 machine buys -> 13,411 quiet fires on 505 coins -> 2,625
       permissioned fires -> 256 trades.  study-kernel/cvx_c3_frozen.py
```

| cell (lag_115, B=0.2) | n | first/day | SOL | %/trade | days+ | win | top1 % | wo top1 | fit | hold |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| E only, frozen exit | 933 | 74.9 | **-20.47** | -10.97 | 2/8 | 53.7 | - | - | - | - |
| FROZEN D+E+P, TP40 c600 | 256 | **18.6** | **-0.60** | -1.18 | 4/8 | 66.8 | -84.3 | -1.11 | +2.37 | **-2.98** |
| same fires, trail40 c600 | 199 | **18.5** | +4.18 | +10.50 | 6/8 | 47.7 | 18.5 | +3.41 | +4.19 | **-0.01** |
| same fires, clock 45 | 874 | 18.6 | -4.52 | -2.59 | 2/8 | 50.8 | - | - | -0.18 | -4.34 |

Every permission is load-bearing on the frozen exit (dropping any one makes the book worse).
break_idx and tagged>=15 are almost the same cut (256 vs 257 vs 259). A 0.05 clip reads the
same per trade (-1.45 % / +10.31 % / -2.89 %), so this is not impact.

**Seats on the frozen conjunction, TP40 c600:** lag_0 **-0.34**, slot_end **-0.53**,
lag_115 **-0.60**, slot+2 **-0.89**. The recorded +2-slot book does not appear on this
tape. Trail40 at lag_0 is +6.57 / 7/8 and at slot+2 is +0.67; the leftover at 115 ms is
the trail column, not the derived exit.

**Gates on the frozen exit:** floor fails (18.6 first/day). Money fails. Hold fails.
Trail40 on the same fires is a neighbourhood, not the sentence: it still sits under the
floor, hold is **-0.01**, and 89.5 % of its +4.18 is `payer_id = -1` (the column is empty
before 2026-09-01 16:24). Leave-one-payer-out on trail40 is +0.44; the bootstrap is
positive in 85.5 %, not 95 %.

**C3 does not ship.** The mint-disjoint holdout at +2 slots (1,278 trades, one machine,
two windows) is n = 1 under the client gate (3.1a). Re-pricing the same fingerprint at
`lag_115` does not recover it.

The concentration-species widening of the same event (`cvx_c3_campaign.py`) is a different
sentence and is also red: 1,330 trades, **-15.57 SOL**, 0/8 days, machine bootstrap 0.7 %.
Two of those seven machines are 0.1-SOL terminals booking -6.7 % a trade. Do not widen
the machine class.

## 6.7 Deep-age big clip (G1)

```
D keep-create / documented-project / slow-wall 5 %, one at a time
E a non-racer buy >= 1.0 SOL, not the node's own print. Neighbourhood: >= 0.5, and
  first-of-burst of those.
P age >= 200 s . vsol 45-80 . prior-minute prints >= 20 (live tape) . creator not sold
X trail40 c600 and clock 45
R one per token   S 0.2   seat lag_115
frame  last-leg week.  study-kernel/cvx_bigclip.py
```

On this tape the three wallets buy 1,210 times (D9Uite 265, pau23U 675, 9RNZnq 270).
D9Uite: clip p50 **1.48 SOL**, age p50 **257 s**, n60 p50 **88** (a live tape, not C5's
quiet 10), **53.6 % burst-start himself**, prev buy >= 0.5 on **12.5 %** of entries,
prev buy >= 1.0 on **6.0 %**, prev_dt p50 **482 ms**. He is not chasing a public size
print. Half the time he *is* the burst.

Response within 2 s vs a 0.052 % base on any >= 0.5 non-racer: buy >= 1.0 **0.043 %**;
E+P (>=1, live, age200, v, creator) **0.052 %** (no lift); E+P >= 0.5 **0.087 %**;
token-silence (C5's E) **0.029 %** (below base). The node does not name this event.

| slice (trail40 c600) | n | first/day | SOL | days+ | top1 % | wo top1 | fit SOL | hold SOL |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| E only: buy >= 1.0, full tape | 86,892 | 8,731 | **-1963.63** | 0/8 | - | - | - | - |
| E + P (>= 1) | 1,840 | 136.6 | -12.22 | 1/8 | - | - | - | - |
| keep + E>=1 + P | 1,706 | 124.7 | **-13.19** | 1/8 | -105.5 | -27.10 | **-7.44** | **-5.75** |
| keep + E>=1 START + P | 1,429 | 111.9 | -14.37 | 1/8 | - | - | - | - |
| keep + E>=0.5 + P (widen actor) | 2,047 | 147.7 | -21.55 | 1/8 | - | - | - | - |
| documented + E>=1 + P | 217 | 16.1 | +1.75 | 4/7 | 77.8 | +0.39 | - | - |
| slow-wall 5 % + E>=1 + P | 603 | 32.7 | +4.69 | 4/6 | 116.4 | **-0.77** | - | - |

Clock 45 is red on every cell that clears the floor. Ablating creator or live-tape, or
moving to C5's quiet permission, makes keep + E>=1 worse. Zero-lag on keep + E>=1 + P is
**-1.91 SOL**: the sentence is red even without the 115 ms seat. lag_115 is **-13.19**.

The two plus cells sit under the floor and are carried by the top 1 %. They are a door
neighbourhood, not a new story; slow-wall behind burst-start is already C2.

**G1 deep-age big clip does not ship.** WHAT: a public size print is not this node's tell
(response equals the base). DELAY is not the C5 failure: he starts the burst half the
time, and when he does not, the previous print is an ordinary live-tape print (~0.5 s
back), not a size tell. The on-tape version of "few coins deserve size" is **which coin**
(C6, agreement of 2+ of the 26), not which print. Do not AND more terms onto size-buy.

---

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
"his mint list is not a gate", and [_!___workflow.md](_!___workflow.md) 11 says the same. The
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

## 6.13 Four-slot inventory walk (no slot frozen)

```
D  none . keep . slow-wall 5 % . documented-project . keep+ep . n_pro>=8 . first-buy>=2
   one at a time
E  burst START . token-silence >=10 slots . returning-pro restart . dip print .
   size>=1 . 2-build slot . MONEY (2nd non-creator buyer, age 5-300 s)
P  none . creator . age/v . creator+age/v . cu_hi . creator+age/v+cu .
   crowd_gone . creator+age/v+crowd
X  clock 45 . trail40 c600 . shipped armed trail . burst-dies g5 + trail40
R  unlimited, one per token   S 0.2   seat lag_115
frame  last-leg week, 6.76 days, 2,212,402 fires, 1,528 occupancy cells.
      study-kernel/cvx_conj4.py, cvx_conj4_diag.py, cvx_conj4_rank.csv
```

Legal product, not a 16-term AND-grid and not a spoken-story seed. Doors stay one at a
time. 256 cells print plus.

**Leader:** documented-project × token-silence × creator-in × trail40.

| cell | n | first/day | SOL | %/trade | days+ | worst | d50 | top1 % | wo top1 | fit | hold |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| documented × silence × creator × trail40 | 1,622 | 141.6 | **+47.54** | 14.65 | **7/7** | **+0.86** | 6/7 | 40.2 | **+28.43** | +34.21 | **+13.33** |
| same, shipped | 1,438 | 141.6 | +42.75 | 14.86 | 6/7 | -0.44 | 6/7 | 36.9 | +26.96 | +30.56 | +12.19 |
| same, clock 45 | 3,512 | 141.6 | +21.67 | 3.09 | 7/7 | +0.75 | 6/7 | 72.0 | +6.07 | - | - |
| documented × silence × none × trail40 | 2,129 | 144.3 | +42.28 | 9.93 | 6/7 | -0.20 | 6/7 | 52.7 | +20.01 | +31.33 | +10.95 |
| documented × two × creator × trail40 | 1,622 | 143.8 | +35.85 | 11.05 | 7/7 | +0.47 | 7/7 | 47.8 | +18.70 | +22.85 | +13.00 |
| documented × size × creator × trail40 | 1,574 | 143.2 | +32.68 | 10.38 | 7/7 | +0.47 | 6/7 | 45.9 | +17.67 | +19.40 | +13.28 |
| documented × MONEY × creator × trail40 | 767 | 113.5 | +22.67 | 14.78 | 6/7 | -1.11 | 6/7 | 35.4 | +14.64 | +15.84 | +6.83 |
| documented × burst × creator × trail40 | 1,179 | 93.1 | +2.15 | 0.91 | 4/7 | -1.55 | 5/7 | 397.2 | **-6.39** | - | - |
| slow-wall × burst × creator+age/v × trail40 | 1,006 | 68.7 | +15.51 | 7.71 | 4/6 | -0.55 | **2/6** | 78.3 | +3.37 | - | - |
| slow-wall × silence × creator × trail40 | 6,631 | 859.3 | **-91.95** | -6.93 | 2/7 | -44.53 | 7/7 | - | -167.77 | - | - |

Tickets on the leader: `47, 170, 220, 192, 140, 125, 63`. Peak/trough 4.7x, top2 43 % -
tracks documented births (4.5). Day 0 = 47 is a 6.2 h stub (~182/day), not a type fail
(4.9). Documented supply on the last 12 h is 0 (`cvx_meta` does not cover those births).
lag_0 trail40 is **+144.85** (gap < 50 ms on 40.7 % of entries, median gap 84 ms).
Clients (creation build): 36, top share 70.8 %, leave-one-out **+13.90**, bootstrap
99.7 %. Top token 5.1 %. Trail fires 50 %. Silent fill 0.5 %.

**The plus is age < 60 s.** Split of the same fires, trail40, occupancy:

| slice | n | SOL | %/trade | days+ | wo top1 | LOO | boot |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| age < 60 s | 901 | **+48.23** | 26.77 | 7/7 | +36.15 | +11.73 | 99.7 % |
| age >= 60 s | 1,006 | **-0.82** | -0.41 | 3/7 | -7.79 | -3.40 | 43.4 % |
| age >= 300 s | 750 | -1.85 | -1.23 | 3/7 | -7.32 | -3.82 | 42.5 % |
| clock 45, age >= 60 s | 2,560 | **-10.88** | -2.12 | 0/7 | -15.31 | -11.08 | 0.4 % |

**The door is not the spelled door.** `cvx_meta.parquet` is exactly the 18,583 keep+ep50
mints from `cvx_tokens.parquet` (`cvx_meta_export.py`). `reindex(tape).ok.fillna(0)` turns
"not in that file" into "not documented." Every one of the 1,020 documented tokens completes
a >= 50 % episode later in the week. That fact does not exist at the fire. Same kernel,
same E/P/X:

| D (silence x creator x trail40) | n | SOL | body (no top 1 %) |
| --- | ---: | ---: | ---: |
| spelled documented (`cvx_meta` website cut) | 1,622 | **+47.54** | **+28.43** |
| all keep+ep50 (the meta file, no website cut) | 19,930 | +150.44 | **-111.44** |
| keep-create only | 48,710 | **-1,034.87** | -1,455 |
| keep, age < 60 s only | 44,548 | **-1,007.34** | - |

**The event is not the named event.** `gap_tok = 10**6 if i == 0`. Of the 1,622 occupancy
trades, 818 are local index 0 (first print of the token) and book **+46.08**; the other 804
book +1.46. Age < 1 s: 801 trades, +45.33. The 10-slot gap never runs on the half that is
the result.

Neighbourhood: every other D on silence x creator x trail40 is red or ~0. `age/v` as P
turns the leader to **-4.13**. Burst-dies as X turns it to **-2.63**. Creator-in is
load-bearing but not the whole book (dropping it leaves +42.28). Silence, two-build,
size and MONEY are interchangeable behind documented+creator; burst START is not.

**What this is:** buy-at-create on tokens that later complete a 50 % episode, with a
website cut inside that file. Not documented-project. Not token-silence.

**What this is not:** a mid-tape harvester, and not a shippable launch rule. Age >= 60 s
is red under both exit families. The inventory has no mid-tape 4-tuple that pays. The
short slot for that target is E (or a D that is not this file).

**Does not ship.** The spelled sentence is not what was scored (strategy 7.4 laws 30 and
31). Tail (top 1 % = 40 % vs 9-12 %) would fail even if the door were legal. Do not
reuse `cvx_meta.parquet` as documented-project on C9-C11 either: those walks read the
same file.

## 6.14 Four new events, four-slot walk (C9)

```
D  none . keep . slow-wall 5 % . documented-project . keep+ep . n_pro>=8 . first-buy>=2
   one at a time
E  staged-leg first buy (S1) . unfinished-budget (S3) . price under own exit (S4) .
   after-flush first buy (S12)
P  none . creator . age/v . creator+age/v . cu_hi . creator+age/v+cu .
   crowd_gone . creator+age/v+crowd
X  clock 45 . trail40 c600 . shipped armed trail . derived tell
   (firing machine sells for S1/S3/S4; dump resumes a new low for S12)
R  unlimited, one per token   S 0.2   seat lag_115
frame  last-leg week, 6.76 days, 61,537 fires, 1,476 occupancy cells
      (748 full tape + 728 age>=60). study-kernel/cvx_conj5.py, cvx_conj5_diag.py,
      cvx_conj5_rank.csv
```

C8 events are not re-walked. Machine census on this tape: 233 professional builds,
44 staged-leg, 46 campaign, **2 rebuy-bots**. Fire counts: after-flush 53,225
(age p50 159 s, 70 % age>=60); staged-leg 7,882 (age p50 60 s); unfinished-budget
338 (age p50 **0 s**); rebuy 92.

Ranking slice is age >= 60 s (the C8 plus is age < 60 s).

| cell | n | first/day | SOL | %/trade | days+ | worst | d50 | top1 % | wo top1 | fit | hold |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| slow-wall × flush × creator × derived | 1,646 | 81.2 | **+10.22** | 3.11 | 3/6 | -0.29 | **2/6** | 154.3 | **-5.55** | +9.89 | +0.34 |
| n_pro>=8 × flush × creator+av+cu × trail40 | 693 | 79.6 | +8.73 | 6.30 | **5/7** | -0.79 | 4/7 | 83.9 | **+1.41** | +6.70 | **+2.03** |
| same, shipped | 687 | 79.6 | +9.05 | 6.59 | 4/7 | -0.58 | 4/7 | 92.6 | +0.67 | +7.38 | +1.68 |
| slow-wall × legs × creator+av × shipped | 185 | 24.3 | +5.42 | 14.66 | 3/6 | -0.38 | 2/6 | 46.6 | +2.89 | - | - |
| keep × budget × creator × clock45 (full tape) | 308 | 45.6 | +2.41 | 3.92 | 4/7 | -1.09 | 4/7 | 51.2 | +1.18 | - | - |
| open2 × rebuy × av × derived | 29 | 4.3 | +0.26 | 4.44 | 3/6 | -0.27 | 0/6 | 247.0 | -0.38 | - | - |

**After-flush × n_pro>=8 × creator+av+cu × trail40** is the first mid-tape cell
that clears the client gate: 33 creation builds, top **32.0 %**, leave-one-out
**+5.93**, bootstrap 98.5 %, p5 +1.60. Walk-forward both halves plus. Tickets
`21, 72, 170, 143, 58, 35, 39` - over fifty on 4 of 7 days. Tail 84 % of net in
the top 1 % (calibration 9-12 %). Gap p50 82 ms, gap<50 on 41 %. Trail fires 72 %.
n_pro>=8 as D on burst START is red (6.11); the same door on after-flush is this
cell. The door is load-bearing: age>=60 none × flush × creator is **-7.91**.

Slow-wall × flush is C2's door on a new event: 21 clients, top **82.1 %**, LOO
+1.83, boot 88.4 %, tickets `15, 190, 257, 43, 24, 20`. Same rotating-client
shape. Derived tell (dump resumes) raises SOL vs trail40 (+10.22 vs +7.80) and
still leaves the body red.

**Per event, age >= 60 s:**

- **S12 after-flush** - the only new E with mid-tape tickets. Pays behind slow-wall
  and n_pro>=8. Does not ship (floor, tail). Closest cell above.
- **S1 staged-leg** - plus only behind slow-wall, first/day 24, tickets
  `3, 64, 83, 10, 3, 1`. C2's door in costume. 12 clients, top 52 %.
- **S3 unfinished-budget** - launch (age p50 0 s). Mid-tape occupancy is 2 trades.
- **S4 rebuy-under-exit** - starved under is_pro: 2 machines, 92 fires, ~0 SOL.
  The professional-build cut (n>=200, nw<=50) ∩ sell-then-buy is empty. C10
  drops that cut (6.15).

**Does not ship.** Floor (no cell clears every day), tail (closest body is +1.41
SOL). Client gate and walk-forward both clear on the n_pro>=8 after-flush cell.
Do not AND onto that parent.

## 6.15 S4 as sell-then-buy, not is_pro (C10)

```
D  none . keep . slow-wall 5 % . documented-project . keep+ep . n_pro>=8 . first-buy>=2
   one at a time
E  price under own exit: first print after >= 10 empty slots with reserve below a
   rebuy-bot's last sell, that build not having bought back. A rebuy-bot is a
   build that sells and later buys on >= 25 % of the coins it sells (min 10
   coins sold). No n>=200 / nw<=50 cut
P  none . creator . age/v . creator+age/v . cu_hi . creator+age/v+cu .
   crowd_gone . creator+age/v+crowd
X  clock 45 . trail40 c600 . shipped armed trail . derived tell (that bot sells)
R  unlimited, one per token   S 0.2   seat lag_115
frame  last-leg week, 6.76 days, 272,191 fires, 448 occupancy cells
      (224 full tape + 224 age>=60). study-kernel/cvx_conj6.py, cvx_conj6_rank.csv
```

C9's class is 2 machines (6.14). The emptying cut is `nw<=50` on sell-then-buy:
is_pro 233; is_pro & n_sell>=10 26; is_pro & n_sell>=10 & frac>=0.25 **2**.
Without is_pro the same rate cut is **30** machines (median 5,009 buy-prints,
median 958 wallets). `n>=200` alone leaves 21; `nw<=50` alone leaves 11.
Fires 272,191; age p50 390 s; 86 % age>=60.

Ranking slice is age >= 60 s.

| cell | n | first/day | SOL | %/trade | days+ | worst | d50 | top1 % | wo top1 | fit | hold |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| slow-wall × rebuy × creator+av × trail40 | 831 | 70.3 | **+4.47** | 2.69 | 3/6 | -0.66 | **2/6** | 181.3 | **-3.63** | +5.60 | **-1.13** |
| same, shipped | 804 | 70.3 | +4.14 | 2.58 | 3/6 | -0.66 | 2/6 | 210.3 | -4.57 | +3.66 | +0.48 |
| slow-wall × rebuy × creator+av+crowd × shipped | 265 | 31.7 | +3.85 | 7.27 | 3/6 | -0.59 | 2/6 | 103.7 | -0.14 | +4.54 | -0.68 |
| none × rebuy × creator+av × trail40 | 4,594 | 515.2 | **-82.70** | -9.00 | 0/8 | -24.14 | 7/8 | -50.0 | -124.03 | - | - |
| slow-wall × rebuy × creator+av × derived | 1,366 | 70.6 | **-7.17** | -2.63 | 0/6 | -3.55 | 2/6 | -104.0 | -14.63 | - | - |

**Slow-wall × rebuy × creator+av × trail40** is C2's door on this event: 21
creation builds, top **87.2 %**, leave-one-out **+0.57**, bootstrap 81.8 %,
p5 -2.86. Tickets `16, 179, 208, 33, 32, 7` - over fifty on 2 of 6 days.
Body **-3.63**. Gap p50 615 ms, gap<50 on 19 %. Trail fires 61 %. Age p50 204 s.
The door is load-bearing (none is **-82.70**). Derived tell (that bot sells)
is red against trail40.

**Does not ship.** Floor, tail, client gate, walk-forward. The plus is the
slow-wall launch client, not a rebuy leftover. Do not AND onto this parent.
Do not restore `nw<=50` (that is the emptying cut). Tape-only mid-tape E in
the backlog (S1, S3, S4, S12) is scored. C11 is the unused S1 moment (late-leg).

## 6.16 Late-leg of a staged-leg machine (C11)

```
D  none . keep . slow-wall 5 % . documented-project . keep+ep . n_pro>=8 . first-buy>=2
   one at a time
E  late-leg: first non-racer buy >= 0.5 SOL of a staged-leg machine's
   second-or-later buy-cluster on this token. Cluster break = 10 empty slots of
   that machine. One fire per (token, machine). Not C9 S1 (first cluster)
P  none . creator . age/v . creator+age/v . cu_hi . creator+age/v+cu .
   crowd_gone . creator+age/v+crowd
X  clock 45 . trail40 c600 . shipped armed trail . derived tell (that machine sells)
R  unlimited, one per token   S 0.2   seat lag_115
frame  last-leg week, 6.76 days, 6,204 fires, 448 occupancy cells
      (224 full tape + 224 age>=60). study-kernel/cvx_conj7.py, cvx_conj7_rank.csv
```

C9 S1 is the first cluster (6.14). This walk is the unused moment of the same
44 staged-leg machines. Fires 6,204; age p50 195 s; 75 % age>=60; 29 unique
machines.

Ranking slice is age >= 60 s.

| cell | n | first/day | SOL | %/trade | days+ | worst | d50 | top1 % | wo top1 | fit | hold |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| slow-wall × late × creator+av × derived | 241 | 27.8 | **+6.93** | 14.38 | 5/6 | -0.25 | **2/6** | 23.1 | **+5.33** | +6.59 | **+0.34** |
| same, trail40 | 240 | 27.8 | +6.75 | 14.06 | 5/6 | -0.32 | 2/6 | 23.7 | +5.15 | +6.48 | +0.27 |
| same, shipped | 240 | 27.8 | +4.94 | 10.29 | **6/6** | 0.01 | 2/6 | 33.8 | +3.27 | +4.37 | +0.57 |
| none × late × creator+av × derived | 651 | 82.4 | +4.72 | 3.62 | 3/7 | -0.79 | 3/7 | 101.2 | **-0.06** | +5.46 | **-0.74** |
| n_pro>=8 × late × creator+av × derived | 570 | 70.9 | +4.88 | 4.28 | 4/7 | -0.72 | 3/7 | 86.3 | +0.67 | +5.74 | -0.85 |

**Slow-wall × late × creator+av × derived** is C2's door on S1's unused moment:
11 creation builds, top **68.9 %**, leave-one-out **+2.15**, bootstrap 99.5 %,
p5 +1.15. Tickets `2, 78, 92, 11, 3, 2` - over fifty on 2 of 6 days. Body
**+5.33** (top 1 % = 23 %, inside the 9-12 % band's neighbourhood). Gap p50
176 ms, gap<50 on 28 %. Age p50 219 s. Derived tell raises SOL vs trail40
(+6.93 vs +6.75). The body of the plus is the door: none's body is **-0.06**.

**Does not ship.** Floor. Client leave-one-out and walk-forward clear; eleven
clients. TYPE fails on this SOL leader. The TYPE reading of the same walk is
slow-wall × late × av × trail40 **+5.14**, body **+0.75**, peak/trough 5.7x
against the door 4.7x, top2 50 % against 48 % (4.10). Do not AND onto either
cell. Do not move S1's event again.

## 6.17 Second-attempt burst, this coin only (C12)

```
D  none . keep . slow-wall 5 % . keep+ep . n_pro>=8 . first-buy>=2
   one at a time. documented is not a door (cvx_meta is keep+ep50)
E  second-attempt burst: first non-racer buy >= 0.5 SOL of an ix structure's
   second-or-later burst on this coin. Burst break = that structure silent
   >= 10 slots. One fire per (coin, structure). Not C8 restart (operator
   structures, every return). Not C11 (staged-leg class)
P  none . creator . age/v . creator+age/v . cu_hi . creator+age/v+cu .
   crowd_gone . creator+age/v+crowd
X  clock 45 . trail40 c600 . shipped armed trail . derived tell (that structure sells)
R  unlimited, one per token   S 0.2   seat lag_115
frame  last-leg week, 6.76 days, 105,497 fires, 384 occupancy cells
      (192 full tape + 192 age>=60). study-kernel/cvx_conj8.py, cvx_conj8_rank.csv
```

Census: first-burst buys 1,499,804; second-burst starts 2,068,498; fires 105,497;
k==0 **0**; 699 distinct structures; age p50 83.8 s; 57.4 % age>=60. Funnel:
cvx_meta 18,583 / 127,833 not scored; cvx_swdoor overlap 119,234 / 127,833.

Ranking slice is age >= 60 s. Rank TYPE first, then SOL.

| cell | n | first/day | SOL | %/trade | days+ | worst | d50 | top1 % | wo top1 | fit | hold |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| slow-wall × second × creator+av × trail40 | 817 | 67.6 | **+17.78** | 10.88 | 4/6 | -0.99 | **2/6** | 54.6 | **+8.07** | - | - |
| same, shipped | 799 | 67.6 | +17.31 | 10.83 | **6/6** | 0.04 | 2/6 | 59.2 | +7.07 | - | - |
| n_pro>=8 × second × creator × trail40 | 2,936 | - | **+3.12** | 0.53 | 4/8 | - | **7/8** | 935 | **-26.04** | +10.73 | **-7.61** |

**TYPE reading:** n_pro>=8 × creator × trail40. Peak/trough **3.7x** against the
door 3.3x, top2 54 % against 45 %. Tickets `53, 171, 344, 377, 203, 142, 102, 2`
(full days all >= 50; day 0 and day 7 are stubs). 48 creation builds, top
**330 %**, leave-one-out **-7.18**, bootstrap 57.4 %, p5 -16.58. Gap p50 178 ms,
gap<50 on 29 %. Age p50 372 s. Body **-26.04**. Hold **-7.61**.

**SOL leader** (not the reading): slow-wall × creator+av × trail40 +17.78, d50
2/6, body +8.07. C2's door on this event.

**Does not ship.** The TYPE-pass clears the floor on full days and fails tail,
body, hold, and the client gate. Do not AND onto the slow-wall SOL leader. Do
not restore the staged-leg class (that is C11). Short slot remains E.

---

## 6.18 Arrives from another coin (C13)

```
D  none . keep . slow-wall 5 % . keep+ep . n_pro>=8 . first-buy>=2
   one at a time. documented is not a door (cvx_meta is keep+ep50)
E  arrives from another coin: first non-racer buy >= 0.5 SOL on this coin by an
   ix structure whose latest print is a buy on a different coin, slot gap < 10.
   One fire per arrival. Not C12, not C11, not the S9 door
P  none . creator . age/v . creator+age/v . cu_hi . creator+age/v+cu .
   crowd_gone . creator+age/v+crowd
X  clock 45 . trail40 c600 . shipped armed trail . derived tell (that structure
   sells) . leave (sells, or prints on another coin)
R  unlimited, one per token   S 0.2   seat lag_115
frame  last-leg week, 6.76 days, 280,791 fires, 480 occupancy cells
      (240 full tape + 240 age>=60). study-kernel/cvx_conj9.py, cvx_conj9_rank.csv
```

Census: switches 7,585,976; from-buy 4,195,500; live (gap < 10) 2,397,238;
live-gap p50 **1.0 slot**; fires 280,791; k==0 **5,443 (1.9 %)**; 300 distinct
structures; age p50 138.5 s; 63.0 % age>=60; leave-time finite 100 %. Funnel:
cvx_meta 18,583 / 127,833 not scored; cvx_swdoor overlap 119,234 / 127,833.

Ranking slice is age >= 60 s. Rank TYPE first, then SOL.

| cell | n | first/day | SOL | %/trade | days+ | worst | d50 | top1 % | wo top1 | fit | hold |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| slow-wall × arrive × creator+av × trail40 | 1,029 | 71.3 | **+16.46** | 8.00 | 4/6 | -1.81 | **2/6** | 73.6 | **+4.35** | - | - |
| n_pro>=8 × arrive × creator × trail40 | 3,851 | - | **-4.64** | -0.60 | 2/8 | - | **7/8** | - | **-42.07** | - | - |
| none × arrive × none × trail40 | 28,654 | - | **-436.93** | -7.62 | 0/8 | - | 7/8 | - | **-664.32** | - | - |

**TYPE reading:** none. No SOL>0 cell passes TYPE. Wide TYPE-pass D×P cells
(none / keep / n_pro>=8 × none or creator or av) are **-4.64 to -437 SOL**.
The plus is slow-wall × creator+av: fire tickets `0, 18, 180, 211, 36, 30, 8, 0`,
peak/trough **26.4x** against the door 4.7x, top2 **81 %** against 49 %. C2's
door. `leave` is red (best **-3.04**).

**Does not ship.** DELAY = 0: live-gap p50 is 1.0 slot. The tell and the SOL
land together. Do not AND onto the slow-wall SOL leader. Do not walk clip-step-up
as the next Event (that tell is the extra SOL). Short slot remains E.

---

## 6.19 First buy after a run of sells (C14)

```
D  none . keep . slow-wall 5 % . keep+ep . n_pro>=8 . first-buy>=2
   one at a time. documented is not a door (cvx_meta is keep+ep50)
E  first buy after a run of sells: non-racer buy >= 0.5 SOL that breaks
   >= 3 consecutive sell prints on this coin. Any buy breaks the run; only
   a qualifying buy fires. One fire per sell-run. Not after-flush, not K=1
P  none . creator . age/v . creator+age/v . cu_hi . creator+age/v+cu .
   crowd_gone . creator+age/v+crowd
X  clock 45 . trail40 c600 . shipped armed trail . derived tell (that
   structure sells) . low (vsol revisits the sell-run low)
R  unlimited, one per token   S 0.2   seat lag_115
frame  last-leg week, 6.76 days, 122,486 fires, 480 occupancy cells
      (240 full tape + 240 age>=60). study-kernel/cvx_conj10.py, cvx_conj10_rank.csv
```

Census: sell-runs >=2/3/4/5 = 1,004,972 / 598,709 / 386,838 / 263,653;
fires 122,486; k==0 **0**; sell-gap p50 **1.0 slot** (37.1 % gap 0); n_sells
p50 4.0; sell_sol p50 2.18; age p50 92.6 s; 57.6 % age>=60; 1,415 structures;
next-print gap p50 **81 ms**, gap<50 **41.9 %**. Funnel: cvx_meta 18,583 /
127,833 not scored; cvx_swdoor overlap 119,234 / 127,833.

Ranking slice is age >= 60 s. Rank TYPE first, then SOL.

| cell | n | first/day | SOL | %/trade | days+ | worst | d50 | top1 % | wo top1 | fit | hold |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| slow-wall × sellrun × creator+av × shipped | 850 | 64.4 | **+10.40** | 6.12 | 5/6 | -0.22 | **2/6** | 90.2 | **+1.02** | - | - |
| n_pro>=8 × sellrun × creator × trail40 | 3,164 | - | **-5.40** | -0.85 | 3/8 | - | **6/8** | - | **-34.47** | +2.26 | **-7.65** |
| slow-wall × sellrun × cu_hi × derived | 2,116 | - | **-5.24** | -1.24 | 2/6 | - | **6/6** | - | **-21.63** | +3.23 | **-8.47** |
| none × sellrun × none × trail40 | 23,553 | - | **-288.58** | - | 0/8 | - | 7/8 | - | **-480.96** | - | - |

**TYPE reading:** none. No SOL>0 cell passes TYPE. Closest TYPE-shaped cells
(slow-wall × cu_hi; n_pro>=8 × creator) are **-5.24 to -9.08**, and the n_pro>=8
cell clears the floor on full days. Wide TYPE cells (none / keep / n_pro>=8 ×
none × trail40) are **-194 to -289 SOL**. The plus is slow-wall × creator+av:
fire tickets `0, 23, 159, 189, 35, 25, 5, 0`, peak/trough **37.8x** against the
door 4.7x, top2 **80 %** against 49 %. C2's door. `low` is red as a TYPE-pass.

**Does not ship.** Leftover of the bounce is 81 ms (next-print p50); 41.9 % of
next prints land inside 50 ms. The sell-run tell precedes the fire by 1 slot
(not C13 DELAY = 0). Do not AND onto the slow-wall SOL leader. Short slot
remains E.

---

## 6.20 First operator after only creator and seed racers (C15)

```
D  none . keep . slow-wall 5 % . keep+ep . n_pro>=8 . first-buy>=2 .
   group-live-now (sibling of the same creation fingerprint printed in
   the last 10 slots at this coin's first print)
   one at a time. documented is not a door (cvx_meta is keep+ep50)
E  first operator after only creator and seed racers: first >= 0.5 buy
   of a non-creator, non-seed operator structure (n>=200, nw<=50) on a
   coin whose prior prints are only the creator and seed racers
   (CreateAccountWithSeed). One fire per coin. i==0 cannot fire
P  none . creator . age/v . creator+age/v . cu_hi . creator+age/v+cu .
   crowd_gone . creator+age/v+crowd
X  clock 45 . trail40 c600 . shipped armed trail . derived tell (that
   structure sells) . follow (no new operator for >= 10 slots, below fill)
R  unlimited, one per token   S 0.2   seat lag_115
frame  last-leg week, 6.76 days, 1,399 fires, 165 occupancy cells
      (105 full tape + 60 age>=60). study-kernel/cvx_conj11.py, cvx_conj11_rank.csv
```

Census: first operator-structure buy 51,170; of which k==0 582; killed by
another actor first 42,603; first buy < 0.5 SOL 6,586; fires **1,399**;
fire k==0 **0**. Age p50 **0.0 s**; 0.6 % age>=60. Prior p50 1.0 (creator
p50 1, seed p50 0). Prior-gap p50 **0 slots** (89.1 % gap 0). 46 structures.
Next-print gap p50 **2 ms**, gap<50 **74.0 %**. Same-structure next buy
481/1,399, p50 **1 ms**. Funnel: cvx_meta 18,583 / 127,833 not scored;
cvx_swdoor / c_build 119,234 / 127,833; group-live-now 78,452 tokens,
births peak/trough 2.5x, top2 41 % against tape 2.1x / 40 %.

Ranking slice is age >= 60 s. Rank TYPE first, then SOL.

| cell | n | first/day | SOL | %/trade | days+ | worst | d50 | top1 % | wo top1 | fit | hold |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| none × firstop × creator × clock45 (age>=60) | 7 | 1.0 | **+0.01** | 0.75 | 2/3 | -0.00 | **0/3** | 105.7 | **0.00** | - | - |
| live × firstop × creator × clock45 (age>=60) | 4 | 0.6 | **+0.01** | 1.26 | 3/3 | 0.00 | **0/3** | 109.7 | **0.00** | - | - |
| none × firstop × none × clock45 (full tape) | 1,360 | - | **-50.75** | -18.66 | 0/8 | - | **7/8** | -21.2 | **-61.51** | - | - |
| live × firstop × none × clock45 (full tape) | 829 | - | **-36.17** | -21.81 | 0/7 | - | **5/7** | - | - | - | - |

**TYPE reading:** none. Age>=60 is 8 fires; no cell has tickets on every full
UTC day. The plus is noise around zero. `follow` is red (best age>=60 **-0.01**).
`n_pro>=8` is empty (the event is the first operator). group-live-now is a
TYPE-shaped door and does not save the book.

**Does not ship.** DELAY = 0: the operator lands in the creator's slot, and
the next print is 2 ms later. The "only creator and seed racers" cut is a
launch filter (42,603 of 51,170 first-operator arrivals already have another
actor). Do not AND. Short slot remains E.

---

## 6.21 First run of an operator structure on this coin (C16)

```
D  none . keep . slow-wall 5 % . keep+ep . n_pro>=8 . first-buy>=2 .
   group-live-now
   one at a time. documented is not a door (cvx_meta is keep+ep50)
E  first run of an operator structure: first >= 0.5 buy of a non-creator,
   non-seed operator structure (n>=200, nw<=50) that has never printed
   here, after at least one prior print that is neither the creator nor a
   seed racer. One fire per (coin, structure). i==0 cannot fire.
   Not C15, not C9 S1, not C12, not C8
P  none . creator . age/v . creator+age/v . cu_hi . creator+age/v+cu .
   crowd_gone . creator+age/v+crowd
X  clock 45 . trail40 c600 . shipped armed trail . derived tell (that
   structure sells) . follow (no new operator for >= 10 slots, below fill)
R  unlimited, one per token   S 0.2   seat lag_115
frame  last-leg week, 6.76 days, 42,893 fires, 560 occupancy cells
      (280 full tape + 280 age>=60). study-kernel/cvx_conj12.py, cvx_conj12_rank.csv
```

Census: new operator-here 192,964; of which k==0 582; no-other-yet (C15
shape) 7,984; first buy < 0.5 SOL 141,505; fires **42,893**; fire k==0 **0**.
Age p50 **6.4 s**; 22.0 % age>=60. n_other p50 36; n_op_before p50 5.
Prior-gap p50 **0 slots** (68.7 % gap 0). 118 structures.
Next-print gap p50 **56 ms**, gap<50 **47.2 %**. Same-structure next buy
9,666/42,893 (22.5 %), p50 **8,758 ms**. Funnel: cvx_meta 18,583 / 127,833
not scored; cvx_swdoor / c_build 119,234 / 127,833; group-live-now 78,452.

Ranking slice is age >= 60 s. Rank TYPE first, then SOL.

| cell | n | first/day | SOL | %/trade | days+ | worst | d50 | top1 % | wo top1 | fit | hold |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| slow-wall × firstrun × cu_hi × derived | 491 | 65.8 | **+2.27** | 2.32 | 3/6 | - | **4/6** | 219.2 | **-2.71** | +5.20 | **-2.93** |
| n_pro>=8 × firstrun × creator+av+cu × derived | 331 | 49.0 | **+9.14** | 13.81 | 4/7 | -0.94 | **2/7** | 39.2 | **+5.56** | - | - |
| none × firstrun × none × trail40 | 6,899 | - | **-75.51** | - | - | - | **7/8** | - | **-128.36** | - | - |

**TYPE reading:** slow-wall × cu_hi × derived **+2.27**. Tickets
`40, 101, 120, 75, 92, 17`; peak/trough **7.06x** against door 4.7x (at the
1.5x bar); top2 **50 %** against 49 %. Floor 4/6. Body **-2.71**. Hold
**-2.93**. Top client **158 %**, LOO **-1.32**, boot 65.4 %. `follow` on
the same D×P is **-3.65**.

The SOL leader is n_pro>=8 × creator+av+cu × derived **+9.14**, body
**+5.56**, tail 39.2 %. Tickets `7, 33, 86, 104, 49, 15, 20`; peak/trough
**6.9x** against door 3.3x, top2 **62 %** against 45 %. TYPE fails. Floor
2/7. Not the reading.

**Does not ship.** Remaining spend, when it exists, is 8.8 s (22.5 % of
fires). Next-print leftover is 56 ms. Do not AND onto the slow-wall TYPE
reading or the n_pro SOL leader. Short slot remains E.

---

# 7. THE BOOKS

Every rule that exists, with the coordinate that produced its number.

## 6.10 Hot-tape re-entry: the flush is real, and it cannot pay the toll (H1)

```
D none - the ticket supply is thousands of unrelated coins, which is the point
E state STRICTLY BEFORE the print: dd <= -20 % . fall5 <= -5 % . acc >= 2 . n20 >= 20
  . age >= 120 s . reserve 42.43-85
P none, then S1-S3 below
X eighteen families, from a 10 s clock to the shipped armed trail
R one per token   S 0.2   seat lag_115, anchored at the print the decision SEES
frame  last-leg week, 6.76 days.  node-derivation/hot-tape/cvx_hottape.py, cvx_hottape_book.py,
       cvx_hottape_sel.py
```

The node is six wallets, 95,645 buys on 11,765 coins on this tape. Their decision moment is
reconstructed in public tape state only, against an unbiased 2 % sample of every buy.

**Two hypotheses died and one survived a control.**

| term | top-bin lift vs every buy | verdict |
| --- | ---: | --- |
| absorption - SOL bought per percent of price given up, 5 s | 1.39 at the LOWEST bin, 0.66 at the highest | **refuted, and inverted** |
| deceleration - the last 2 s of the fall against the 5 s pace | 0.57 where the fall has stopped, 1.48 where it still accelerates | **refuted, and inverted** |
| buy-flow ACCELERATION - SOL bought last 5 s over the 5 s before | **2.15**, and **2.16** with the node's own SOL removed | survives |
| drawdown from the coin's peak so far | 2.04 below -50 %, monotone to 0.28 at the peak | survives |
| the fall itself, 5 s | 2.02 at -20..-10 %, 0.01 at flat | survives |
| coin age | 1.83 above 1800 s, monotone from 0.27 under 30 s | survives |

**Why absorption cannot exist here, and it is physics, not a fitting failure.** Absorption is an
order-book idea: buying that does not move price because there is resting size to eat. A
constant-product curve has no resting size - every buy moves price by exactly its own arithmetic,
and windowed flow **is** the price path (7.1). So `buy SOL / (-price fall)` divides the signal by
itself, and the conjunction "price fell AND buyers were the larger side" is nearly a
contradiction: it holds on **32 of 95,645** node buys. The whole family is closed on the venue,
not on this cell.

**Two ways this cell could have been manufactured, both closed before the book was read.**

- The first pass put the node's own buy inside its own 5 s flow window and read a 2.44 lift. Every
  feature is now built from prints `0..i-1` and the reserve `v[i-1]`.
- The fire anchor. The state at index `k` is what a watcher holds when print `k-1` lands, so the
  fill is the last print by `t[k-1] + 115 ms`. Anchoring at `k` fills later, and on a falling tape
  a later fill is a **cheaper** buy - an optimism of about half a print, pointing exactly the way
  this cell looks.

**The book. Eighteen exit families, every one negative.**

| exit | trades | SOL | per trade | days positive | hold p50 |
| --- | ---: | ---: | ---: | :---: | ---: |
| clock 10 s | 17,204 | -97.02 | **-2.82 %** | 0/8 | 9.7 s |
| clock 20 s | 14,869 | -91.77 | -3.09 % | 0/8 | 19.7 s |
| tp 5 / stop 10 / cap 60 | 23,285 | -164.97 | -3.54 % | 1/8 | 3.7 s |
| trail 15 / cap 60 | 17,790 | -131.60 | -3.70 % | 0/8 | 23.4 s |
| shipped armed trail | 7,651 | -119.23 | -7.79 % | 0/8 | 164.8 s |

The **ticket floor passes 7 of 8 days** on 3,535 coins and 75 clients, so the supply problem that
closed the frozen sentence does not exist here. The money is the whole failure.

**The arithmetic that closes it, and it is NOT the toll.** Decompose the clock-20 book into the
price move and the four cost terms:

| term | per round trip |
| --- | ---: |
| the price move we capture | **+0.29 %** |
| pump.fun protocol fee, 125 bps x 2 | -2.47 % |
| tip + priority, 0.000225 SOL x 2 at 0.2 SOL | -0.22 % |
| our own impact, `B/vsol` x 2 | -0.70 % |
| **booked** | **-3.10 %** |

**At a zero fee this cell still books -0.63 %.** The toll is not what kills it.

The like-for-like comparison has to be on the price move, because the roster's `margin` column is
already net of the same 125 bps (`rb-solo-nodes.py:140`, `net = sol_out*0.9875 - sol_in*1.0125`).
The node's 1.10 % net implies a price move of **+3.65 %** a round trip. Ours is **+0.29 %**.
*We are 3.4 points short on the move itself: the reconstruction does not find what they find.*
The state terms discriminate their buys from the tape at lift 2.0-2.2, and that is not the same
thing as capturing their return.

**Selection cannot close a 3x gap, measured once on pre-registered cuts** (`cvx_hottape_sel.py`,
clock 20 s and clock 60 s, about sixty cells):

| cut | trades | per trade | days positive |
| --- | ---: | ---: | :---: |
| all | 14,869 | -3.09 % | 0/8 |
| slow-wall launch door | 2,880 | -2.91 % | 0/6 |
| not a bundler cgroup | 14,721 | -2.95 % | 0/8 |
| creator has not sold | 2,250 | **-2.56 %** | 0/8 |
| drawdown x acceleration, 20 cells | - | -0.63 % .. -5.25 % | - |
| busyness, buy share, flush size, hour | - | every bin negative | - |

The only non-negative cells are `n = 11` and `n = 218`. The hour-of-day control is flat, as a
control should be, which says the red is the cell and not a regime.

**The seat premise that chose this node did not hold.** The node was picked because the recorded
buy-flush direction factor is 0.889-0.943, a 6-11 % discount at 115 ms. On these fires the buy leg
fills at **mean -0.66 %**, median **0.00 %**, cheaper than the decision print only **22.5 %** of
the time. The direction table is measured on dense burst prints; a flush on an aged coin at 20-80
prints per 20 s does not print densely enough for the lag to pay. **A direction factor is a
property of a tape density, not of a side.**

**Where the missing move is, and the ceiling that closes the node (`cvx_hottape_near.py`).** The
event names no wallet and fires on the whole tape, so the first question is whether it lands where
the node lands. It does, without being told to:

| | |
| --- | ---: |
| fires on a coin the node never touches | **3.5 %** |
| fires within 5 s of one of their buys | **47.2 %** |
| fires within 60 s | 90.4 % |
| of THEIR buys, the share the rule fires on at all | **4.28 %** |

*The state is their habitat.* A pure tape-state event reproduces their coin list and roughly half
their seconds with no wallet named - which is what a public term for a private habit should look
like, and is the first time in this program it has happened.

The money then grades cleanly by that distance, same clock-20 exit throughout:

| population | trades | price move | per trade | days positive |
| --- | ---: | ---: | ---: | :---: |
| every fire | 14,869 | +0.29 % | -3.09 % | 0/8 |
| on a coin they never touch | 524 | **-20.37 %** | -23.12 % | 0/7 |
| on a coin they do touch | 14,345 | +1.05 % | -2.35 % | 0/8 |
| **within 2 s of one of their buys** | 4,921 | **+2.24 %** | **-1.21 %** | 1/8 |
| 5-30 s away | 4,724 | +0.30 % | -3.08 % | 0/7 |
| over 120 s away | 563 | -0.22 % | -3.57 % | 0/8 |
| within 60 s and BEFORE them | 6,894 | +1.09 % | -2.31 % | 0/8 |
| within 60 s and AFTER them | 5,126 | +0.82 % | -2.58 % | 1/8 |

Two readings. The 3.5 % of fires on coins they avoid carry a **-20 %** move and are most of the
gap between +0.29 % and +1.05 %, so **coin choice is worth about 0.8 points and moment is worth
about 1.2 more**. And arriving *before* them beats arriving after by 0.27 points - **front-running
this node is not a lever**, which also says their impact is not what the trade is made of.

**The ceiling. Give the rule a perfect oracle for both the coin and the second - their own wallets,
unshippable by 7.4 law 20 - and read every exit family:**

| exit | trades | per trade | days positive |
| --- | ---: | ---: | :---: |
| clock 10 s | 4,921 | **-1.17 %** | 1/8 |
| clock 20 s | 4,921 | -1.20 % | 1/8 |
| clock 60 s | 4,921 | -1.81 % | 3/8 |
| tp 10 / cap 60 | 4,921 | -2.16 % | 1/8 |
| trail 20 / cap 120 | 4,921 | -2.53 % | 1/8 |
| shipped armed trail | 4,921 | -4.27 % | 1/8 |

**Every one negative.** Even handed their coin and their second, at our size and our seat, this
node does not pay. Their move at that second is +3.65 % after their own impact; ours is +2.24 %
gross, +1.54 % after ours. The residual 2.1 points is exit choice - they pick when to sell and we
read a fixed clock - and no fixed exit in the grid recovers it. *That is the closure: not "we
cannot find their moment", but "their moment, at our seat, is not worth 3.4 % of toll".*

**Coordinate.** D empty, E named and public, P three tried, X eighteen tried, R and S fixed. The
sentence is closed, and the ceiling closes the node behind it. What is NOT closed: the same node with a size that is small against the flush,
and any E that fires seconds AFTER the flush ends rather than inside it - both untested, and both
need a reason before a run.

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
| **Door rule v3 MONEY (C0b)** | D shipped 8 % launch door . E 2nd non-creator buyer at age 5-300 s . P creator in . X arm21/tr36/stop43.75 c1200 . R one per token . S 0.2 | **`lag_115` +5.38 SOL, 3/6 days, 247 first/day, top1 358 %, hold -1.93**; 235 trades at -50 %. lag_0 +38.18 / 6/6. SlotEnd +5.22 | **does not ship.** Floor and money clear; tail and hold fail. Creator-in is load-bearing. L-axis book exists (7.0) |
| Door rule v3 RATE | same, tighter door | SlotEnd +30.74 SOL / 629 fires; `lag_115` **-0.53 SOL, 3 of 7** | dead at the seat |
| **Burst-start event** | D none . E burst start . P none . X four families . seat lag_115 | on the traders' coins **+2.6 .. +6.7 %/trade, every exit**; full tape with no door **-1.5 .. -13 %** | **open, blocked on the door** (5.4). Public doors on this tape: 6.4, red |
| C2 documented x burst START | D document . E burst start . P age/v/creator . X trail40 c600 | **-1.27 SOL, 4/7, 45.3 first/day**; hold +0.71 on 107 trades | **does not ship** |
| C2 keep+ep x burst START | D keep + demonstrated episode . E burst start . P age/v/creator/crowd-gone . X trail40 c600 | **+9.24 SOL, 4/8, 75.2 first/day, top1 78.6 %**; fit +9.59 hold **-0.35** | **does not ship** |
| C5 quiet deep-age, token-silence burst start | D keep / document / keep+ep . E first buy >= 0.5 after >= 10 token-silent slots . P age>=400 / v / quiet / creator . X trail40 c600 | keep+P **-32.46 SOL, 0/8, 200 first/day**; document **-3.63**; keep+ep **-9.46**; both halves red | **does not ship.** DELAY: the burst lasts ~80 ms |
| C5 quiet deep-age, returning-pro restart | D keep / document . E returning professional after 10-slot build silence . P age>=400 / v / quiet / creator . X trail40 c600 | keep+P **-1.88 SOL, 1/8, 37/day**; document +0.40 (7.5/day, top1 94 %, hold -0.05) | **does not ship.** Does not describe GZmUDs on this tape |
| G1 deep-age big clip, public size buy | D keep / document / slow-wall . E non-racer buy >= 1.0 SOL . P age>=200 / v 45-80 / live n60>=20 / creator . X trail40 c600 | keep+P **-13.19 SOL, 1/8, 125 first/day**; document +1.75 (16/day, top1 78 %); slow-wall +4.69 (33/day, top1 116 %, wo-top1 -0.77) | **does not ship** (6.7). Response equals base. Size print is not the tell |
| Mid-tape instruments, all seven members | D cgroup / keep / not-dump / not-3ix / sw5 . E named by response . X clock 45 / tp30 / trail40 . seat lag_115 | 9Uq8GV ceiling +4.12 %/trade, names buy>=1; 8aaRWu +1.72 % clock 45 off slow-wall, lift 1.33; ApfmkS names nothing, self-start 45 %, followed +4.90 % unnamed; 9999hu/88887Q leftover ~0 | **does not ship** (6.9). All members measured. Door still missing. Slow-wall cell is C2 |
| Mid-tape episodes 1/2/3+ four unpriced facts | D n_pro / n_pro_60 / n_hold / creator in / last-pro . E burst START . P age/v . X clock 45 / trail40 . seat lag_115 | all red; closest n_pro_60>=2 + creator in **-0.92 %/trade, 1/8**. Ep1 state does not separate return coins. Gap p50 56 s | **does not ship** (6.11). Four facts are the market on this event |
| Mid-tape holder book at episode open | D never_sold / dev / sold_back / pro_hold / last_pro_hold / top1 . E burst START . P age/v . X clock 45 / trail40 . seat lag_115 | all red; last_pro_hold>=0.15 lift 3.50 then **-3.57 %/trade**. Ep1 book does not name return coins | **does not ship** (6.12). Tokens remaining is not D on this event |
| **C8 four-slot inventory walk** | D documented . E token-silence >=10 slots . P creator in . X trail40 c600 . R unlimited . S 0.2 . seat lag_115 | **+47.54 SOL, 7/7, hold +13.33, LOO +13.90, boot 99.7 %**; tickets 47/170/220/192/140/125/63 (tracks documented births, 4.9); plus is **age < 60 s** (+48.23); age>=60 **-0.82** | **does not ship** (6.13). Launch book, not mid-tape. Day 0 = 47 is a 6.2 h stub. Tail 40 %. Inventory has no mid-tape 4-tuple that pays |
| **C9 four new events** | D n_pro>=8 . E after-flush first buy . P creator+age/v+cu . X trail40 c600 . R unlimited . S 0.2 . seat lag_115 . age>=60 | **+8.73 SOL, 5/7, hold +2.03, LOO +5.93, boot 98.5 %, top client 32 %**; tickets 21/72/170/143/58/35/39; peak/trough 8.1x, top2 58 %; wo top1 **+1.41**; top1 84 % | **does not ship** (6.14, 4.9). TYPE fails. Client gate clears. Floor 4/7. Tail. Not the next parent |
| **C10 S4 sell-then-buy** | D slow-wall 5 % . E price under own exit (n_sell>=10, rebuy frac>=0.25, no is_pro) . P creator+age/v . X trail40 c600 . R unlimited . S 0.2 . seat lag_115 . age>=60 | **+4.47 SOL, 3/6, hold -1.13, LOO +0.57, boot 81.8 %, top client 87 %**; tickets 16/179/208/33/32/7; peak/trough 26x, top2 81 %; wo top1 **-3.63**; top1 181 % | **does not ship** (6.15, 4.9). TYPE fails: two-day client inside slow-wall. Body red |
| **C11 late-leg** | D slow-wall 5 % . E first >=0.5 buy of staged-leg cluster 2+ . P creator+age/v . X trail40 + that machine sells . R unlimited . S 0.2 . seat lag_115 . age>=60 | **+6.93 SOL, 5/6, hold +0.34, LOO +2.15, boot 99.5 %, top client 69 %**; tickets 2/78/92/11/3/2; peak/trough 46x, top2 90 %; wo top1 **+5.33**; top1 23 % | **does not ship** (6.16, 4.9, 4.10). TYPE fails. Body pays. Plus is C2's door on two days. TYPE reading: slow-wall × av × trail40 **+5.14**, body **+0.75** |
| **C12 second-attempt burst** | D n_pro>=8 . E first >=0.5 buy of this ix structure's second burst on this coin . P creator in . X trail40 c600 . R unlimited . S 0.2 . seat lag_115 . age>=60 | **+3.12 SOL, 4/8, hold -7.61, LOO -7.18, boot 57.4 %, top client 330 %**; tickets 53/171/344/377/203/142/102/2; peak/trough 3.7x, top2 54 %; wo top1 **-26.04**; top1 935 % | **does not ship** (6.17, 4.10). TYPE passes. Floor passes on full days. Body, tail, hold, client gate fail. SOL leader is slow-wall (+17.78, d50 2/6), not the reading |
| **C13 arrives from another coin** | D none / keep / slow-wall / n_pro>=8 . E first >=0.5 buy here after a live buy on a different coin (gap < 10) . P standing . X trail40 / derived / leave . R unlimited . S 0.2 . seat lag_115 . age>=60 | SOL leader slow-wall × creator+av × trail40 **+16.46, 4/6, d50 2/6, top1 73.6 %, body +4.35**; TYPE 26.4x / 81 %. Wide TYPE cells **-4.64 to -437**. leave red | **does not ship** (6.18). READING none. DELAY = 0 (live-gap p50 1.0 slot). k==0 1.9 % |
| **C14 first buy after a run of sells** | D none / keep / slow-wall / n_pro>=8 . E first >=0.5 non-racer buy after >=3 consecutive sells . P standing . X trail40 / derived / low . R unlimited . S 0.2 . seat lag_115 . age>=60 | SOL leader slow-wall × creator+av × shipped **+10.40, 5/6, d50 2/6, top1 90.2 %, body +1.02**; TYPE 37.8x / 80 %. Closest TYPE cells **-5.24 to -9**. Wide TYPE cells **-194 to -289**. low red | **does not ship** (6.19). READING none. Leftover 81 ms (gap p50). k==0 0 |
| **C15 first operator after only creator and seed racers** | D none / keep / slow-wall / n_pro>=8 / group-live-now . E first >=0.5 non-creator non-seed operator buy; prior prints only creator and seed racers . P standing . X trail40 / derived / follow . R unlimited . S 0.2 . seat lag_115 . age>=60 | SOL leader none × creator × clock45 **+0.01, 7 trades, d50 0/3**; full-tape none × none × clock45 **-50.75**, 1,360 trades, 0/8. Age p50 0 s. follow red | **does not ship** (6.20). READING none. DELAY = 0 (prior-gap 0 slots, next-print 2 ms). k==0 0 |
| **C16 first run of an operator structure** | D none / keep / slow-wall / n_pro>=8 / group-live-now . E first >=0.5 operator buy on a coin that already has a non-creator non-seed print . P standing . X trail40 / derived / follow . R unlimited . S 0.2 . seat lag_115 . age>=60 | TYPE reading slow-wall × cu_hi × derived **+2.27, 3/6, d50 4/6, top1 219 %, body -2.71, hold -2.93, LOO -1.32, boot 65 %**. SOL leader n_pro>=8 × creator+av+cu × derived **+9.14**, TYPE fails 6.9x / 62 %. Wide TYPE **-41 to -75** | **does not ship** (6.21). Remaining spend 8.8 s on 22.5 %. Next-print 56 ms. k==0 0 |
| Launch-build door | D launch build . E none | 4-6x on all 30 days, 22-day holdout, no decay | **stands as a screen.** A door is not a trade |
| Documented-project rule | D document . E buy >= 0.5 SOL . P age/reserve/builds . X trail 40 c1800 | fitting week +0.40 %; **holdouts +1.46 % and +0.64 %/trade, 4 of 7 days each, +3.77 SOL on 2,156 tickets** | **weakly positive out of sample** - a lead for paper. Every week under 1 SE from zero |
| L-door on documented-project (C1) | D document + safety-panel cuts . E buy >= 0.5 SOL . P age/reserve/builds . X trail40 c600 | parent **+1.21 SOL, 4/7, 23.5 first/day, 8/646 trades at -50 %**; creator-in +3.36 SOL (hold +0.14); bundle/fresh/snipe/dev fail hold | **L-axis empty on this book.** Creator-in is a permission, not L-selection. Door-v3 L-door unrun on C0b's 1,667 fires |
| Campaign-break rule v0 | D ixh 29d9aacb… . E its buy ends a >= 10-slot buy silence . P vsol 65-100, break_idx >= 4, tagged >= 15, tape not buy-heavy, below 97 % of the 30-min vsol max . X TP +40 % else 600 s clock . seat **lag_115** | **`lag_115` -0.60 SOL, 4/8, 18.6 first/day, hold -2.98**; trail40 +4.18 / 6/8 / hold -0.01 / still 18.5/day; slot+2 TP40 **-0.89**. Species widening -15.57 / 0/8 | **does not ship** (6.6). Floor, money and hold fail. The +2-slot mint-disjoint holdout is n = 1 |
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
| the method, the gates, the campaign queue | [_!___workflow.md](_!___workflow.md) |
| every idea, in Door / Event / Permission / Exit | [_!___inventory.md](_!___inventory.md) |
| the market model, the basis, the open/closed ledger | [_!___strategy.md](_!___strategy.md) |
| the 26 independent traders, ranked, with their five nodes | [solo-traders.md](solo-traders.md) |
| frozen sentences, never edited after their date | [study-kernel/frozen-sentences.md](study-kernel/frozen-sentences.md) |
| the pricing kernel and its acceptance tests | [study-kernel/](study-kernel/) |
| the launch-door reconciliation ladder | [../../roadmap/launch-door-rule.md](../../roadmap/launch-door-rule.md) |
| 44 consolidated refuted study lines | [../../history/2026-09-03-refuted-lines-ledger.md](../../history/2026-09-03-refuted-lines-ledger.md) |
| the audit that produced the coordinate discipline | [../../../../docs/history/2026-09-09-closure-ledger-audit.md](../../../../docs/history/2026-09-09-closure-ledger-audit.md) |

Reproduce scripts live in this directory and in `study-kernel/`. They resolve the repository root
by directory depth, so they are not interchangeable between the two locations. Two JSON files are
**inputs**, not dumps, and must not be deleted: `8dtx-event-structures.json` and
`ixd-create-door.json`.
