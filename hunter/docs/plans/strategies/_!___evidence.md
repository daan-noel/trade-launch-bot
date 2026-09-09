# Evidence: every standing measurement, with its coordinate

The numbers behind [_!___strategy.md](_!___strategy.md). The method that produced them is
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

---

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
members, and the two largest books in it have never been studied.

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

**Do not establish:** anything about the door slot (both scans run with a demonstrated-episode
door and nothing else), about the untried L-selection axis, about state-conditional exits, or
about the burst-start event of 5.4. Sixteen terms, one week, two events, one clip.

---

# 7. THE BOOKS

Every rule that exists, with the coordinate that produced its number.

| rule | coordinate | book | status |
| --- | --- | --- | --- |
| **Door rule v3 MONEY** | D launch door . E as shipped . P none . X armed/unarmed trail + held . R re-entry . S 0.2 | **SlotEnd** 2,001 fires / +43.56 SOL / +10.89 %/trade / 7 of 7 days; **`lag_115` +4.80 SOL, 4 of 7 days**, about 286 tickets a day | **positive at our seat and unfinished.** Its loss distribution carries 300+ trades worse than -50 % against 55 above +200 %: an entry inside the rug window and a static exit. Reconciled to the published python book trade by trade |
| Door rule v3 RATE | same, tighter door | SlotEnd +30.74 SOL / 629 fires; `lag_115` **-0.53 SOL, 3 of 7** | dead at the seat |
| **Burst-start event** | D none . E burst start . P none . X four families . seat lag_115 | on the traders' coins **+2.6 .. +6.7 %/trade, every exit**; full tape with no door **-1.5 .. -13 %** | **open, blocked on the door** (5.4) |
| Launch-build door | D launch build . E none | 4-6x on all 30 days, 22-day holdout, no decay | **stands as a screen.** A door is not a trade |
| Documented-project rule | D document . E buy >= 0.5 SOL . P age/reserve/builds . X trail 40 c1800 | fitting week +0.40 %; **holdouts +1.46 % and +0.64 %/trade, 4 of 7 days each, +3.77 SOL on 2,156 tickets** | **weakly positive out of sample** - a lead for paper. Every week under 1 SE from zero |
| Campaign-break rule v0 | D campaign machine . E its buy ends a >= 10-slot silence . P vsol 65-100, break_idx >= 4, tape not buy-heavy, below 97 % of the 30-min max . X TP +40 % else 600 s clock . seat **+2 slots** | derivation +3.59 SOL / 1,227 trades / 73.2 % win; **disjoint holdout +2.61 SOL / 1,278 trades / 72.5 %, mint sets fully disjoint**; engine-reconciled | **the only rule that passed a disjoint holdout AND engine reconciliation. Never priced at `lag_115` as this sentence** |
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
