# Hot-tape node: rule 1 and the member book

The working file for the hot-tape node, the first node derived end to end by
[method.md](method.md). It holds the sentence that pays (section 1), the chain of measurements
that produced it, dead ends included (section 2), what every member of the node does (section 3),
and where the data and code are (section 4). Numbers: [_!___evidence.md](../_!___evidence.md)
1.4-1.22 and 6.10. Code: [toolkit/](toolkit/README.md) and the step-numbered scripts in
[hot-tape/](hot-tape/README.md).

The rule runs as **Flip-Catch - Bracket** (this exit: +20 %, -60 %, 90 s). Its entry with the wall
target, **Flip-Catch - Room** (40 % of the room to graduation, -60 %, 90 s), is rule 1b
(evidence 1.24, 1.25). The name is what the entry buys: a fast flipper's sell of 1 SOL or more,
absorbed by a crowded tape.

Tapes: the **study tape** is 08-30 17:48 to 09-06 12:00 UTC (12.47 M prints, 6.76 days); every
threshold below is read off it. The **holdout tape** is the lake's days after it, 09-06 12:00 to
09-10 (4.50 days), in the study tape's exact format (`toolkit/lake_export.py`). Section 1's rule is
read at the engine's grain instead: `study_exact` (lake 09-01 to 09-06 12:00, 5.5 days) and
`holdout_exact` (09-06 12:00 to 09-10), every leg, the engine's clock and spot (evidence 1.22).

---

## 1. Rule 1

Every term is spelled the way the engine computes it (evidence 1.22); every wallet and every leg
of a transaction counts.

```
E  a sell >= 1 SOL (this leg), from a wallet whose last buy of this coin is <= 30 s old, while
   >= 15 distinct recipes (build_core) printed on the coin in the 5 s up to and including it,
   and the coin's spot made a new high <= 20 s ago
P  age >= 158 s, >= 368 distinct wallets other than the creator have bought the coin, and the
   reserve after the sell <= 100 SOL (liquidity <= 70; the take profit fits under the wall)
X  take profit +20 %, stop -60 % (a catastrophe guard), clock 90 s on the 200 ms tick
D  none
R  one position per coin at a time; re-entry after the exit fill
S  flat 0.35 SOL (0.2 booked below)
seat  both legs fill at the last print of either side landed 115 ms after the decision print, in
      its slot or the next observed one (the engine's `LagMs` rule, one rule on both legs)
```

Plain words: a flipper takes profit into a buying frenzy on an established coin that is not near
graduation; buy the flipper's sell, take +20 %.

### The book

| at 0.2 SOL, 115 ms | study, every leg | holdout, every leg (unseen) | ship bar |
| --- | ---: | ---: | ---: |
| tickets a day | 109.8 | 100.0 | - |
| %/trade | +4.51 % | **+4.44 %** | - |
| SOL a day at 0.2 / 0.35 SOL | 0.99 / 1.63 | 0.89 / 1.46 | - |
| days positive, worst day | 6/6, +0.00 | **5/5**, +0.48 | >= 5/7 |
| halves of the days | +3.91 % / +6.01 % | +4.46 % / +4.41 % | both > 0 |
| top 1 % share, biggest coin | 12.8 %, 5.0 % | **9.8 %**, 5.0 % | <= 15 % |
| 95 % interval, resampling coins | +2.46..+6.34 % | +2.19..+6.58 % | - |
| 200 / 300 / 500 ms, both legs | +4.12 / +3.78 / +3.52 % | +3.69 / +3.34 / +2.76 % | - |
| stops, clock exits, take profits | 3.8 % stops | 14 / 166 / 270 tickets | - |

The last-leg tapes agree: +4.26 % 7/7 (08-30..09-05) and +4.99 % 5/5 (evidence 1.22).

### The engine's target

`node-derivation/data/r1_ref_{holdout_exact,study_exact}.parquet`: per ticket the trigger, entry-fill
and exit prints by slot, transaction, leg and time, the reason, and the SOL under the engine kernel.
The replay is `hot-tape/r1_exact.py`; `hot-tape/r1_exact_check.py` rebuilds its tickets with code
that shares nothing with it. Simulate books them one for one (evidence 1.23): 448 of 450 and 603 of
604 on the same trigger, fill and exit prints with the same SOL; the other three sit on coins the
engine had retired as dead. The same code books every day after 09-10 (the clean test).

### Each slot, in one line

| slot | where it comes from | evidence |
| --- | --- | --- |
| E, the trigger | 8fStGV buys 25-200 ms after a public SELL >= 1 SOL (excess intensity, lift 8.1) | 1.12 |
| E, the terms | the sells it buys against those it ignores on the same coin; then re-read off money on rule 1's own pool, where the 2 s buy term drops out | 1.12, 1.20 |
| P, established | the stop-outs are young, thin coins; each fold picks 368 / 369 on a count that turns out to be distinct buyers, age 123 / 193 s | 1.14, 1.20, 1.22 |
| P, room under the wall | a take profit that needs the graduation print is not priceable; a safety term, not a fit | 1.20 |
| X | its closing hazard on rule 1's pool (sells hard at +15..+20 %); then one axis at a time by money, finally at the engine's grain and fill: +20 %, the stop inert past -60 %, the clock 90 s | 1.13, 1.17, 1.20, 1.22 |

---

## 2. How rule 1 was derived: the chain

Each row is one step: the question, what the measurement shows, and what it does to the rule.
The dead ends stay, because each closed a wrong reading that the next node would otherwise make
again. The step numbers are the scripts' own; the method phase is [method.md](method.md) section 4.

### 2.1 The node read as one (method phases A-B before the split)

| step | question | what it shows | what it does to the rule | script | ev |
| --- | --- | --- | --- | --- | --- |
| H1 | Is the node's flush-then-bounce moment a public state that pays? | The state (a -20 % drawdown, a -5 % fall in 5 s, buyers accelerating) fires red on the whole tape under 18 exit families | Nothing: a flush is real and cannot pay the toll | `cvx_hottape*.py` | 6.10 |
| R1 | What does each cost term take from the roster's own decisions? | Losing the sequencing race costs -5.59 points, the fee -2.47, our impact -0.76, the 115 ms -0.77 | The seat dominates: an event anchored on a print is a follow model | `study-kernel/cvx_replay3.py` | 1.4 |
| 0-1 | Do the six share a trigger? | One buys within 2 s of another at lift 6-9 against a shuffled null; the unit is the single buy | The trigger is public and on the tape, so it can be found. The latency read here is a waiting time and is withdrawn (1.9) | `cvx_hot_sessions.py`, `cvx_hot_latency.py` | 1.5 |
| 2-5 | What separates their buy moments from same-coin non-buys? | Lull then wake (`wake_z` 0.660, `quiet60_z` 0.418); firing on the wake -1.6..-5.5 %, on the lull -4.1..-11 % | The informative half has moved price before we act (reaction cost +5.98 %). Within-coin design kept; hard thresholds dropped | `cvx_hot_event.py`, `cvx_hot_event2.py`, `cvx_hot_book.py`, `cvx_hot_lull.py` | 1.6 |
| 6-9 | Is their moment learnable from the whole feature vector? | Within-coin AUC 0.72 on held-out coins; fired on the tape -4.2..-10.6 % 0/8 with a cheap fill; -2.63 % on coins they touch against -14.21 % elsewhere | The edge is not the moment: 11.6 points sit in which coin | `cvx_hot_model.py`, `cvx_hot_event3.py`, `cvx_hot_model2.py`, `cvx_hot_score.py` | 1.7 |
| 10 | Which coins do their trades live in? | Early liveliness, not creation structure; no public door rescues the book (best -5.60 %); +2.37 % 7/8 when one of them buys inside our hold | Read as "they are the demand" - retracted: they are 2-5 % of the move's SOL, detectors of it | `cvx_hot_door.py` | 1.8, 1.9 |
| 11-14 | Can an exit reach the convexity they enter? | 480 shapes; flat clocks win; nothing positive | Withdrawn: swept with D and P empty, at 1800 s on a 20 s node. Law 26: read an exit at the actor's hold, on a selected pool | `cvx_hot_exit.py`, `cvx_hot_exit2.py`, `cvx_hot_exit3.py`, `cvx_hot_exit4.py` | 1.10 |

### 2.2 Who pays (phase A)

| step | question | what it shows | what it does to the rule | script | ev |
| --- | --- | --- | --- | --- | --- |
| 15-16 | Is their tail separable, and at which seat? | Split by member: three book +2.2..+2.3 % 8/8 at RACE cap15 with a positive body and a 1.8 % biggest coin; three are noise. Pooled +0.50 % with a 216 % tail | The node is two strategies: derive the event from the members that pay | `cvx_hot_sep.py`, `cvx_hot_sep2.py` | 1.11 |
| 17-20 | Is the event in the price path, or in the machine count? | The price path is red in three constructions (-3.35 %, -3.58 %, 0/8); the independent-machine count is a gradient (-4.08 to -2.99 %) that does not cross zero | No price-path event; the machine axis stays a candidate for a fitted vector | `cvx_hot_lvl.py`, `cvx_hot_up.py`, `cvx_hot_mach.py`, `cvx_hot_mach2.py` | 1.11 |

### 2.3 The event (phase B)

| step | question | what it shows | what it does to the rule | script | ev |
| --- | --- | --- | --- | --- | --- |
| 21 | What print does each member react to, and at what lag? | Excess intensity against same-coin controls: 8fStGV a public SELL >= 1 SOL at 25-200 ms (8.1), avoiding burst starts; AbQcLH a burst start at 25-50 ms (9.9); sssssw, the one that loses, a burst start at 75-100 ms (9.2) | E's class: a public sell >= 1 SOL, bought within 115 ms. The side separates payers from losers, not the speed | `cvx_hot_trig.py` | 1.12 |
| 22-23 | Where does our fill land against them? | Firing on each member's trigger at 115 ms: AbQcLH's burst start is a race (lag 47 ms, ahead 5 %); 8fStGV's lag from its sell is p50 81 ms and we land ahead 33.5 % | 8fStGV's event is reachable: the member does not have to be beaten | `cvx_hot_seat.py`, `cvx_hot_dump.py` | 1.12 |
| 24 | Which sells does it buy? | It buys 2.2 % of the sells >= 1 on its coins. Our seat on those +0.38 % 5/8 (+0.21 % when behind it); on every sell -4.32 %. Same coin, bought against ignored: 15 recipes in 5 s (7), 3.94 SOL bought in 2 s (0.40), a new high 5.4 s ago (80.3), a seller who bought 20.5 s ago (50.8) | The terms and their first thresholds: >= 15 recipes / 5 s, >= 2 SOL / 2 s, new high <= 20 s, seller <= 30 s | `cvx_hot_which.py` | 1.12 |
| 25 | Does it hold as a public sentence on every coin? | Each term lifts the book: -4.26 % (every sell) to -0.68 % (the full event); the same frenzy on a BUY -2.64 % | E is filled: the frenzy-absorbed sell. The dump side wins | `cvx_hot_dump2.py` | 1.12 |
| 26-27 | Does a public coin fact carry the rest? | Size, busyness, prior frenzies: best -0.27 %. On its coins the fire pays +6.04 % when it buys inside our hold, -0.62 % when not | D is the empty slot, and part of it is the member's arrival | `cvx_hot_door2.py`, `cvx_hot_arrive.py` | 1.12 |

### 2.4 Door and exit (phases C-D)

| step | question | what it shows | what it does to the rule | script | ev |
| --- | --- | --- | --- | --- | --- |
| 28 | Is "its coins" a coin property? | On its coins E books +4.46 % 7/7 BEFORE its first buy there and -0.68 % after | The gap is its future arrival; a coin list is split at the first buy before it is read | `cvx_hot_door3.py` | 1.13 |
| 28b | Which unpriced coin fact passes a day split? | "Few of this coin's earlier frenzy-sells made a new high" passes both folds: +1.13 % 5/7 | A door candidate, later refuted out of sample (step 34) | `cvx_hot_door3b.py` | 1.13 |
| 29 | What makes it sell? | Its closing sells trip 25-50 ms after a public BUY >= 1 (5.8); its hazard is a bracket: take profit above +10 %, stop -25..-40 %, time from 40 s | X's family and first levels: take profit +10 %, stop -25 %, 60 s | `cvx_hot_exit5.py` | 1.13 |
| 30-31 | The sentence, and does a tape-driven exit beat the bracket? | With the door: +1.22 % 5/7, tail 39.7 %. Twelve tape-driven exits all book below the bracket | The bracket stays; a tighter reaction cuts recoveries | `cvx_hot_exit6.py`, `cvx_hot_exit7.py` | 1.13 |

### 2.5 Permission and holdout (phases E-F)

| step | question | what it shows | what it does to the rule | script | ev |
| --- | --- | --- | --- | --- | --- |
| 32 | Which frenzies die? | The stop-outs against the take profits at the fire: young, thin coins (age AUC 0.40, holders 0.43); heat, composition and the seller do not separate them | P's facts: age and public holders | `cvx_hot_perm.py` | 1.14 |
| 33 | The permission as a sentence | Each fold picks holders 368 / 369, age 123 / 193 s; holders >= 368 x age >= 158 s: +2.07 % 7/7, the door redundant | P filled | `cvx_hot_perm2.py` | 1.14 |
| 34 | Does it hold on unseen days? | 4.5 lake days, same code: +1.90 % 5/5, body +1.73, tail 23.2 %; the door -0.53 % | Rule 1 holds out of sample; the door was fitted and is dropped | `cvx_holdout_export.py`, `cvx_hot_holdout.py` | 1.15 |

### 2.6 The second event, not found

| step | question | what it shows | what it does to the rule | script | ev |
| --- | --- | --- | --- | --- | --- |
| 35-38 | Do the other paying operator's triggers give a second event? | Its burst-start leg reacts in 47 ms (we land ahead 5 %): a race. Its dip-buy leg is reachable (+3.50 % 8/8 on its picks) and red in every public spelling | No second event; rule 1 stays the node's one sentence | `cvx_hot_which2.py`, `cvx_hot_ev2.py`, `cvx_hot_ev2b.py`, `cvx_hot_ev2c.py` | 1.16 |

### 2.7 Rule 1 tuned (phase G, first pass)

| step | question | what it shows | what it does to the rule | script | ev |
| --- | --- | --- | --- | --- | --- |
| 39 | Is the exit right on rule 1's own pool? | On holds that pass P the member sells hard at +15..+20 %, not +10 %; 90 s beats 60 s | X: +15 % / -25 % / 90 s: +2.73 % study, +2.53 % holdout | `cvx_hot_exit8a.py`, `cvx_hot_exit8.py` | 1.17 |
| 40 | Does the book rest on a few tickets, and what clip? | The top 1 % are take profits filled past the target in a frenzy spike; capped at the median take profit the book stays positive. 0.5 SOL roughly doubles SOL a day | The tail passes in its purpose; S is a range | `cvx_hot_size.py` | 1.18 |
| 41 | Does anything at the fire remove the stop-outs, or predict the member's arrival? | 20 facts, both tapes: best 0.055 from 0.5, not the same fact on the two tapes; a sell-reactive buyer count AUC 0.60 / 0.45 | D stays empty; the stop-outs are the sentence's price at this seat | `cvx_hot_door4.py` | 1.19 |

### 2.8 Every slot re-derived on rule 1's own pool (phase G)

| step | question | what it shows | what it does to the rule | script | ev |
| --- | --- | --- | --- | --- | --- |
| G0 | Does the member's selection still add anything? | Inside rule 1 its picks book +2.00 % / +2.77 %, its skips +2.82 % / +2.49 %; its buying inside our hold does not lift the trade | Every threshold from here is read off money | `cvx_r1u_cand.py` | 1.20 |
| G1 | A table that books any variant | 169,995 / 113,456 candidates; rule 1 reproduced to the ticket | The engine for G2-G8 | `cvx_r1u_cand.py`, `cvx_r1u.py` | 1.20 |
| G2 | Is each threshold right? | Four walk-forward rounds: "bought >= 2 SOL in 2 s" drops out (+0.71 / +0.93 SOL on the test halves); holders wants 150-200 and its extra SOL is upside gaps (capped 2.27 against 2.89 SOL, tail 18.9 %) | E loses its 2 s buy term; the rest hold | `cvx_r1u_step1.py` | 1.20 |
| G3 | Does the book rest on an exit the tape cannot price? | 131 of 840 study trades (2.10 of 4.58 SOL) and 87 of 525 holdout trades (1.28 of 2.66) close on the graduation print; at a 20 % post-migration drop the rule is red on both tapes | P gains "reserve after the sell <= 100 SOL": 2 / 0 trades touch graduation, +3.95 %/trade, 3 % of the study SOL | `cvx_r1u_grad.py` | 1.20 |
| G4 | Does a new fact add money? | 22 facts at 12 cuts each: two pass the folds at +0.04 to +0.18 SOL, inside the 0.15-0.18 SOL a random 5 % of the tickets moves by chance | None taken | `cvx_r1u_step2.py` | 1.20 |
| G5 | Is the exit right on the new pool? | One axis at a time: take profit holds at +15 % (the folds pick 22 and 25 %, the second loses); stop -25 to -40 %; clock 90 to 240 s | Stop -40 % holds out of sample (+0.59 SOL); the 240 s clock fails it (2.98 -> 1.80 SOL, 4/5) and 90 s stays; the take profit re-checked at 90 s holds | `cvx_r1u_exit.py`, `r1u_exit_axes.py`, `r1u_tp_check.py` | 1.20 |
| G6 | Re-entry? | A 30 s cool-down after a stop adds +0.33 SOL on 14 trades; a five-a-coin cap rests on 8 | R unchanged | `r1u_reentry.py` | 1.20 |
| G7 | Size? | A share of the reserve books as a flat clip of the same median on the study tape; 0.35 SOL flat keeps every bar on both tapes, 0.5 SOL passes all but the holdout tail (15.5 %) | S: flat 0.35 SOL | `r1u_size.py` | 1.20 |
| G8 | Does each change hold out of sample, on its own? | See 1.20's table: 2.66 -> 3.02 -> 2.39 -> 2.98 SOL on the holdout, 5/5 at every kept step | The updated rule 1 of section 1 | `r1u_holdout.py` | 1.20 |
| G9 | Does it hold the way an engine meets the tape? | A print-by-print replay sharing no code with G1 reproduces the G8 tickets one for one (739 / 433, same exits, same SOL); counting every wallet and every leg it books +3.26 % 5/5 on the holdout, positive at a 500 ms fill | The engine's target of section 1; rule 1 unchanged | `r1_replay.py` | 1.21 |
| G10 | Does each term mean what its name says? | Checked against the lake's exact fields: the holder book is float dust (647 against 96 real holders); the count it makes is distinct buyers | P spells distinct buyers, creator excluded | `r1_terms_audit.py` | 1.21, 1.22 |
| G11 | What does the engine compute differently? | Recipes counted with the print, raw time, the 200 ms clock, the engine's fills: at the old thresholds the every-leg holdout reads +2.52 %, top 1 % 20.1 %; the buy-only entry fill is the cost, and it prices 29 % of entries at a print our buy cannot meet | The seat takes the exit leg's rule on both legs | `r1_exact.py audit` | 1.22 |
| G12 | Re-derived at the engine's grain and fill | Every-leg study, keep rule with the chance floor and bars: entry kept, exit +20 % / -60 % / 90 s; +4.44 % 5/5 on the every-leg holdout, top 1 % 9.8 %; a second code rebuilds every ticket | The rule of section 1 | `r1_exact.py derive/confirm`, `r1_exact_check.py` | 1.22 |

### 2.9 What the chain teaches

- **Split the node by member first.** Everything before step 15 read six wallets as one and every
  reading was red; the three that pay were inside it the whole time.
- **Measure a reaction from its trigger print.** The first latency number (a waiting time) said
  the node was slow; the excess-intensity histogram gave the true trigger and lag.
- **The side separates, not the speed.** The payers buy the print that pushed price down.
- **A member's coin list is its future.** "Five points in which coin" were its own later arrival.
- **Read an exit on the selected pool, then re-read it when the pool changes.** +10 % became
  +15 % once P existed, and the stop moved again once the reserve term changed the pool.
- **Re-derive every slot on the sentence's own pool.** The 2 s buy term and the -25 % stop came
  from earlier pools; on rule 1's own pool both were costing money.
- **Count the trades on the graduation print.** Half of the old book's SOL was booked at a price
  the tape cannot see.
- **The holdout confirms, it never chooses.** The door of step 28b and the 240 s clock of G5 both
  won in sample and failed out of it.
- **Hand the engine a number it can reproduce.** A study that leaves the members out, or keeps one
  leg per transaction, books a sentence no engine can run; replay it print by print with every
  wallet and every leg before any engine work (G9).
- **Check every term against an exact field, not a second code.** Two replays sharing the
  bag-from-reserve idea agreed on a holder count that was float dust (G10).
- **Derive at the grain and fill the engine runs.** The last-leg tape and the buy-only entry fill
  each moved the answer; only the every-leg study under the corrected fill passes every bar (G12).

---

## 3. The member book

The node's six wallets, booked on their own decisions at the RACE seat (sequenced before their
print) under a 15 s clock, and their reaction measured by excess intensity.

| member | RACE cap15 | days | its trigger (peak lift, lag) | its exit | status |
| --- | ---: | :---: | --- | --- | --- |
| 8fStGV | +2.28 % | 8/8 | public SELL >= 1 SOL (8.1, 25-200 ms); avoids burst starts | sells a public BUY >= 1 at +14 %; on rule 1's pool sells hard at +15..+20 %, stops at -25..-30 %, clock 60-90 s | **rule 1** |
| AbQcLH | +2.29 % | 7/7 | public burst start - a ~1 SOL buy after a dip (9.9, 25-50 ms) | sells into a quiet tape after ~19 s (no print trigger) | a race: lag 47 ms, we land ahead 5 %, -1.36 % at our seat |
| 49uohd | +2.24 % | 6/8 | a node print (6.4, 25-50 ms); public SELL >= 1 / -2 % print (3.3-3.7, 150-300 ms) | sells right after a public sell (lift 14.9), near break-even | reachable, **not spelled**: +3.50 % 8/8 on its picks at our seat, every public spelling red |
| 64hP97 | +0.98 % | - | nothing under 200 ms (1.8, 300-600 ms) | sells a big buy/sell at 300-600 ms | noise |
| omegoM | +0.12 % | - | sells and burst starts (5.0, 25-100 ms) | sells after a sell or down print | noise |
| sssssw | -0.62 % | - | a public burst start (9.2, 75-100 ms; a BUY >= 1 4.4, a +3 % print 5.9); avoids sells | sells right after a public sell (13.6) | loses: the pump side |

- **Three pay, three are noise.** Pooled, the six read +0.50 % with a 216 % tail; the three that
  pay read +2.27 % 8/8, body +29.50 SOL, biggest coin 1.8 % (1.11).
- **Speed does not separate them; the side does.** sssssw is as fast as AbQcLH. The members that
  pay buy the print that pushed price DOWN; the one that loses buys the print that pushed it UP.
- **AbQcLH and 49uohd are one operator:** the same recipe set (d55b21, e3a28b, d4f288, 3ae630;
  15-66 wallets each). 49uohd is not a follow-up to AbQcLH's buys - its partner has usually not
  bought the coin before its dip-buy.
- **8fStGV uses public-app recipes** (2,103 and 13,534 wallets, the same as 64hP97), so no recipe
  fact stands in for its arrival.
- **49uohd's picks** are a capitulation cascade on the same coin: the 10 s move -10.1 % against
  -1.1 %, busy tape, inside a burst, the seller at -3.9 % against +7.3 %. Spelled publicly: -4.26 %
  to -5.04 %, not monotone; with established coin -1.72 %. The unused form is those descriptors
  fitted as one within-coin score.

---

## 4. Data and code

| where | what |
| --- | --- |
| [hot-tape/README.md](hot-tape/README.md) | every script of the chain above, by step, with its evidence section |
| [toolkit/README.md](toolkit/README.md) | the method as functions; `hot-tape/toolkit_check.py` re-runs steps 16, 21, 24 and 39 through it next to the recorded numbers |
| `study-kernel/cvx_prints.parquet`, `cvx_tok.parquet`, `cvx_ix.parquet` | the study tape and its sidecars (shared with the other studies), from `aa.pxf`, last leg per transaction |
| `node-derivation/data/cvx_holdout_*.parquet` | the holdout tape: `build_core` = md5 of the ix labels joined by `\|` after dropping `Associated Token: Create*`, `*: CloseAccount`, `Memo Program*` (5,000 of 5,000 rows match); curve rows only; time rounded to ms; every shared row identical on 09-04 and 09-05 |
| `node-derivation/data/cvx_r1u_cand_{study,holdout}.parquet` | the candidate tables of phase G (rebuilt by `hot-tape/cvx_r1u_cand.py`) |
| `node-derivation/data/cvx_r1u_final_{study,holdout}.parquet` | the updated rule's tickets (`hot-tape/r1u_holdout.py`) |
| `node-derivation/data/cvx_holdlegs_*.parquet` | the holdout with every leg of a transaction (`python -m toolkit.lake_export cvx_holdlegs 2026-09-03 ... 2026-09-10 --all-legs`), tape `holdout_legs` |
| `node-derivation/data/r1_replay_{study,holdout,holdout_legs}_{node,all}.parquet` | the replay's tickets at the 115 ms seat (`hot-tape/r1_replay.py`): the engine's parity reference |
| `node-derivation/data/cvx_studyexact_*.parquet`, `cvx_holdexact_*.parquet` | the study days and the holdout at the engine's grain: every leg, `t_us` (the engine's clock), `vtok` (its spot); tapes `study_exact`, `holdout_exact` |
| `node-derivation/data/r1_creators.parquet` | each tape coin's creator (Postgres `tokens.creator_wallet`, the source simulate reads) |
| `node-derivation/data/r1_ref_{holdout_exact,study_exact}.parquet` | **the engine's parity reference**: rule 1's tickets, print identities and SOL |
