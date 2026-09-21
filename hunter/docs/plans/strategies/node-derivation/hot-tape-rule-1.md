# Hot-tape node: rule 1 and the member book

The case file for the hot-tape node, the first node derived end to end by
[_!___derive.md](../_!___derive.md), and its worked example. It holds the sentence that pays
(section 1), the chain of steps that produced it, dead ends included (section 2), what every
member of the node does (section 3), and where the data and code are (section 4). Numbers:
[_!___evidence.md](../_!___evidence.md) 1.4, 1.5, 1.11, 1.14, 1.16 and 1.20-1.27. Code: [toolkit/](toolkit/README.md)
and [hot-tape/](hot-tape/README.md), which also says where each step's script is kept.

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

The volume is set by the event: loosening any entry term adds trades that earn well under rule 1's
own and lose on some days, under both exits (step G15; evidence 7, rule 1c).

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

| slot | where it comes from | step, evidence |
| --- | --- | --- |
| E, the trigger | 8fStGV buys 25-200 ms after a public SELL >= 1 SOL (excess intensity, lift 8.1) | step 21, ev 1.27 |
| E, the terms | the sells it buys against those it ignores on the same coin; then re-read off money on rule 1's own pool, where the 2 s buy term drops out | step 24, ev 1.20 |
| P, established | the stop-outs are young, thin coins; each fold picks 368 / 369 on a count that turns out to be distinct buyers, age 123 / 193 s | steps 32-33, ev 1.20, 1.22 |
| P, room under the wall | a take profit that needs the graduation print is not priceable; a safety term, not a fit | ev 1.20 |
| X | its closing hazard on rule 1's pool (sells hard at +15..+20 %); then one axis at a time by money, finally at the engine's grain and fill: +20 %, the stop inert past -60 %, the clock 90 s | ev 1.20, 1.22 |

---

## 2. The chain

Each row is one step: the question, what the measurement shows, and what it does to the rule.
The dead ends stay: each closes a wrong reading that the next node would otherwise make again.
The step numbers are the scripts' own; each heading names the derive phase. The chain row is the
record of a step: `-` in the ev column means no evidence section carries it, and a cut section's
write-up and every step script are in git at `9f8ce4c5` (or `8b01c18b`).

### 2.1 The node read as one (derive phases 4-6, before the split)

| step | question | what it shows | what it does to the rule | script | ev |
| --- | --- | --- | --- | --- | --- |
| H1 | Is the node's flush-then-bounce moment a public state that pays? | The state (a -20 % drawdown, a -5 % fall in 5 s, buyers accelerating) fires red on the whole tape under 18 exit families | Nothing: a flush is real and cannot pay the toll | `cvx_hottape*.py` | 7 (H1) |
| R1 | What does each cost term take from the roster's own decisions? | Losing the sequencing race costs -5.59 points, the fee -2.47, our impact -0.76, the 115 ms -0.77 | The seat dominates: an event anchored on a print is a follow model | `study-kernel/cvx_replay3.py` | 1.4 |
| 0-1 | Do the six share a trigger? | One buys within 2 s of another at lift 6-9 against a shuffled null; the unit is the single buy | The trigger is public and on the tape, so it can be found. The latency read here is a waiting time and is withdrawn (1.5) | `cvx_hot_sessions.py`, `cvx_hot_latency.py` | 1.5 |
| 2-5 | What separates their buy moments from same-coin non-buys? | Lull then wake (`wake_z` 0.660, `quiet60_z` 0.418); firing on the wake -1.6..-5.5 %, on the lull -4.1..-11 % | The informative half has moved price before we act (reaction cost +5.98 %). Within-coin design kept; hard thresholds dropped | `cvx_hot_event.py`, `cvx_hot_event2.py`, `cvx_hot_book.py`, `cvx_hot_lull.py` | 1.5 |
| 6-9 | Is their moment learnable from the whole feature vector? | Within-coin AUC 0.72 on held-out coins; fired on the tape -4.2..-10.6 % 0/8 with a cheap fill; -2.63 % on coins they touch against -14.21 % elsewhere | The edge is not the moment: 11.6 points sit in which coin | `cvx_hot_model.py`, `cvx_hot_event3.py`, `cvx_hot_model2.py`, `cvx_hot_score.py` | 1.5 |
| 10 | Which coins do their trades live in? | Early liveliness, not creation structure; no public door rescues the book (best -5.60 %); +2.37 % 7/8 when one of them buys inside our hold | Read as "they are the demand" - retracted: they are 2-5 % of the move's SOL, detectors of it | `cvx_hot_door.py` | 1.5 |
| 11-14 | Can an exit reach the convexity they enter? | 480 shapes; flat clocks win; nothing positive | Withdrawn: swept with D and P empty, at 1800 s on a 20 s node. Law 26: read an exit at the actor's hold, on a selected pool | `cvx_hot_exit.py`, `cvx_hot_exit2.py`, `cvx_hot_exit3.py`, `cvx_hot_exit4.py` | 1.5 |

### 2.2 Who pays (phase 4)

| step | question | what it shows | what it does to the rule | script | ev |
| --- | --- | --- | --- | --- | --- |
| 15-16 | Is their tail separable, and at which seat? | Split by member: three book +2.2..+2.3 % 8/8 at RACE cap15 with a positive body and a 1.8 % biggest coin; three are noise. Pooled +0.50 % with a 216 % tail | The node is two strategies: derive the event from the members that pay | `cvx_hot_sep.py`, `cvx_hot_sep2.py` | 1.11 |
| 17-20 | Is the event in the price path, or in the machine count? | The price path is red in three constructions (-3.35 %, -3.58 %, 0/8); the independent-machine count is a gradient (-4.08 to -2.99 %) that does not cross zero | No price-path event; the machine axis stays a candidate for a fitted vector | `cvx_hot_lvl.py`, `cvx_hot_up.py`, `cvx_hot_mach.py`, `cvx_hot_mach2.py` | 1.11, 7 (price-path) |

### 2.3 The event (phases 5-6)

| step | question | what it shows | what it does to the rule | script | ev |
| --- | --- | --- | --- | --- | --- |
| 21 | What print does each member react to, and at what lag? | Excess intensity against same-coin controls: 8fStGV a public SELL >= 1 SOL at 25-200 ms (8.1), avoiding burst starts; AbQcLH a burst start at 25-50 ms (9.9); sssssw, the one that loses, a burst start at 75-100 ms (9.2) | E's class: a public sell >= 1 SOL, bought within 115 ms. The side separates payers from losers, not the speed | `cvx_hot_trig.py` | - |
| 22-23 | Where does our fill land against them? | Firing on each member's trigger at 115 ms: AbQcLH's burst start is a race (lag 47 ms, ahead 5 %); 8fStGV's lag from its sell is p50 81 ms and we land ahead 33.5 % | 8fStGV's event is reachable: the member does not have to be beaten | `cvx_hot_seat.py`, `cvx_hot_dump.py` | - |
| B2c | Does leftover exist at our fill on the sells it buys (derive 5.2)? | Behind its buy: cost 1.41 %, peak +7.46 % at its hold p50 (10.3 s); AbQcLH's burst start costs 4.55 %; the veto alone passes sssssw and the class 8fStGV avoids | E passes 5.2 as the derive writes it; the veto's lines are calibrated here and hold only after phases 4 and 5.1 | `b2_leftover.py` | 1.27 |
| 24 | Which sells does it buy? | It buys 2.2 % of the sells >= 1 on its coins. Our seat on those +0.38 % 5/8 (+0.21 % when behind it); on every sell -4.32 %. Same coin, bought against ignored: 15 recipes in 5 s (7), 3.94 SOL bought in 2 s (0.40), a new high 5.4 s ago (80.3), a seller who bought 20.5 s ago (50.8) | The terms and their first thresholds: >= 15 recipes / 5 s, >= 2 SOL / 2 s, new high <= 20 s, seller <= 30 s | `cvx_hot_which.py` | - |
| 25 | Does it hold as a public sentence on every coin? | Each term lifts the book: -4.26 % (every sell) to -0.68 % (the full event); the same frenzy on a BUY -2.64 % | E is filled: the frenzy-absorbed sell. The dump side wins | `cvx_hot_dump2.py` | - |
| 26-27 | Does a public coin fact carry the rest? | Size, busyness, prior frenzies: best -0.27 %. On its coins the fire pays +6.04 % when it buys inside our hold, -0.62 % when not | D is the empty slot, and part of it is the member's arrival | `cvx_hot_door2.py`, `cvx_hot_arrive.py` | - |

### 2.4 Door and exit (phases 7-8)

| step | question | what it shows | what it does to the rule | script | ev |
| --- | --- | --- | --- | --- | --- |
| 28 | Is "its coins" a coin property? | On its coins E books +4.46 % 7/7 BEFORE its first buy there and -0.68 % after | The gap is its future arrival; a coin list is split at the first buy before it is read | `cvx_hot_door3.py` | - |
| 28b | Which unpriced coin fact passes a day split? | "Few of this coin's earlier frenzy-sells made a new high" passes both folds: +1.13 % 5/7 | A door candidate, later refuted out of sample (step 34) | `cvx_hot_door3b.py` | - |
| 29 | What makes it sell? | Its closing sells trip 25-50 ms after a public BUY >= 1 (5.8); its hazard is a bracket: take profit above +10 %, stop -25..-40 %, time from 40 s | X's family and first levels: take profit +10 %, stop -25 %, 60 s | `cvx_hot_exit5.py` | - |
| 30-31 | The sentence, and does a tape-driven exit beat the bracket? | With the door: +1.22 % 5/7, tail 39.7 %. Twelve tape-driven exits all book below the bracket | The bracket stays; a tighter reaction cuts recoveries | `cvx_hot_exit6.py`, `cvx_hot_exit7.py` | - |

### 2.5 Permission and holdout (phases 9 and 12.1)

| step | question | what it shows | what it does to the rule | script | ev |
| --- | --- | --- | --- | --- | --- |
| 32 | Which frenzies die? | The stop-outs against the take profits at the fire: young, thin coins (age AUC 0.40, holders 0.43); heat, composition and the seller do not separate them | P's facts: age and public holders | `cvx_hot_perm.py` | 1.14 |
| 33 | The permission as a sentence | Each fold picks holders 368 / 369, age 123 / 193 s; holders >= 368 x age >= 158 s: +2.07 % 7/7, the door redundant | P filled | `cvx_hot_perm2.py` | - |
| 34 | Does it hold on unseen days? | 4.5 lake days, same code: +1.90 % 5/5, body +1.73, tail 23.2 %; the door -0.53 % | Rule 1 holds out of sample; the door was fitted and is dropped | `cvx_holdout_export.py`, `cvx_hot_holdout.py` | - |

### 2.6 The second event, not found

| step | question | what it shows | what it does to the rule | script | ev |
| --- | --- | --- | --- | --- | --- |
| 35-38 | Do the other paying operator's triggers give a second event? | Its burst-start leg reacts in 47 ms (we land ahead 5 %): a race. Its dip-buy leg is reachable (+3.50 % 8/8 on its picks) and red in every public spelling | No second event; rule 1 stays the node's one sentence | `cvx_hot_which2.py`, `cvx_hot_ev2.py`, `cvx_hot_ev2b.py`, `cvx_hot_ev2c.py` | 1.16 |

### 2.7 Rule 1 tuned (phase 12, first pass)

| step | question | what it shows | what it does to the rule | script | ev |
| --- | --- | --- | --- | --- | --- |
| 39 | Is the exit right on rule 1's own pool? | On holds that pass P the member sells hard at +15..+20 %, not +10 %; 90 s beats 60 s | X: +15 % / -25 % / 90 s: +2.73 % study, +2.53 % holdout | `cvx_hot_exit8a.py`, `cvx_hot_exit8.py` | - |
| 40 | Does the book rest on a few tickets, and what clip? | The top 1 % are take profits filled past the target in a frenzy spike; capped at the median take profit the book stays positive. 0.5 SOL roughly doubles SOL a day | The tail passes in its purpose; S is a range | `cvx_hot_size.py` | - |
| 41 | Does anything at the fire remove the stop-outs, or predict the member's arrival? | 20 facts, both tapes: best 0.055 from 0.5, not the same fact on the two tapes; a sell-reactive buyer count AUC 0.60 / 0.45 | D stays empty; the stop-outs are the sentence's price at this seat | `cvx_hot_door4.py` | - |

### 2.8 Every slot re-derived on rule 1's own pool (phase 12)

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
| G8 | Does each change hold out of sample, on its own? | See 1.20's table: 2.66 -> 3.02 -> 2.39 -> 2.98 SOL on the holdout, 5/5 at every kept step | Rule 1 before the engine audit (G9-G12) | `r1u_holdout.py` | 1.20 |
| G9 | Does it hold the way an engine meets the tape? | A print-by-print replay sharing no code with G1 reproduces the G8 tickets one for one (739 / 433, same exits, same SOL); counting every wallet and every leg it books +3.26 % 5/5 on the holdout, positive at a 500 ms fill | The engine's target of section 1; rule 1 unchanged | `r1_replay.py` | 1.21 |
| G10 | Does each term mean what its name says? | Checked against the lake's exact fields: the holder book is float dust (647 against 96 real holders); the count it makes is distinct buyers | P spells distinct buyers, creator excluded | `r1_terms_audit.py` | 1.21, 1.22 |
| G11 | What does the engine compute differently? | Recipes counted with the print, raw time, the 200 ms clock, the engine's fills: at the old thresholds the every-leg holdout reads +2.52 %, top 1 % 20.1 %; the buy-only entry fill is the cost, and it prices 29 % of entries at a print our buy cannot meet | The seat takes the exit leg's rule on both legs | `r1_exact.py audit` | 1.22 |
| G12 | Re-derived at the engine's grain and fill | Every-leg study, keep rule with the chance floor and bars: entry kept, exit +20 % / -60 % / 90 s; +4.44 % 5/5 on the every-leg holdout, top 1 % 9.8 %; a second code rebuilds every ticket | The rule of section 1 | `r1_exact.py derive/confirm`, `r1_exact_check.py` | 1.22 |
| G15 | Does a looser entry add trades that pay? | Each term loosened alone, both exits, the added trades judged on their own: every term fails its first step (added trades +1.8..+2.6 % at best against +4.5 %, never every day; stall, seller, age lose); a looser entry displaces rule 1 tickets through occupancy; the sell size band 0.75-1 SOL as its own rule pays 6/6 at +2.9 % with a 24.5 % tail, 70 % of it on coins rule 1 holds | No rule 1c; more trades need a second event | `r1c_loosen.py` | 7 (rule 1c) |

### 2.9 The left tail (phases 8-9 on rule 1's pool; [left-tail plan](../../../roadmap/hot-tape-rule-1-left-tail-plan.md))

| step | question | what it shows | what it does to the rule | script | ev |
| --- | --- | --- | --- | --- | --- |
| L1 | Do the paying members avoid the big loss, and how? | Worse than -40 % / -60 %: rule 1 7.3 / 3.6 %; 8fStGV 3.0 / 2.0 % at its seat, 3.6 / 2.1 % at ours; the other members 0.1-5 / 0-1 %. Every member's worst drawdown equals its realised loss: none exits a collapse early | The members avoid the tail by which fires they take, not by an exit | `r1t_members.py` | - |
| L2 | What takes the SOL out of rule 1's tail (44 of 604 study tickets)? | Exact vtok holder book. Not a rug: -10 % at 11 s, -40 % at 49 s (p50), over about 250 transactions; the largest one carries 23 % of the fall. The fire's top-10 holders sell 24 % of the hold's sell SOL (14 % on the rest) and are the largest seller on 35 of 44. Bundle and creator share read 0 at age >= 158 s. At the fire only age (AUC 0.37), top-1 share (0.59) and fresh-60 s share (0.56) move | The loss door's bundle and creator terms have nothing to read on this pool; the tail is a distribution wave | `r1t_anatomy.py` | - |
| L3 | Once under water, does the top holders' selling say which trades go on to -40 %? | At the first print at -10..-30 %, top-10 sold, top-1 sold and sell share read AUC 0.47-0.56; only the speed of the fall moves (0.37), and that is the price path. 8.4 % of the take profits dip to -20 % first; a -25 % cut saves on the tail about what it loses on the 89 that recover | A cohort-state exit has nothing to read beyond price: X3 is not the slot. The remainder is P/D at the fire | `r1t_path.py` | - |
| L4 | Does a cut at the fire on age, top-1 share or fresh-60 s share pay, under both exits? | Keep rule on the study days, applied before occupancy: none is taken under either exit (top-1 <= 0.124 wins one fit half and loses its test half). Age >= 250 s cuts the tail 44 -> 27 (Bracket) and 42 -> 24 (Room), lower on every day, at the same SOL (5.44 -> 5.48; 6.35 -> 6.05): the young coins carry the big losers and enough winners to pay for them | No permission joins rule 1: the tail is its price at this seat on money. Age is a risk dial (fewer big losses, same SOL, 20 % fewer tickets), not a money term | `r1t_perm.py` | - |
| L5 | Is the tail a one-slot rug (many wallets selling in one slot)? | Market-wide a one-slot fall of -70 % or worse from reserve > 60 lands 119-183 times a day, every day 09-01..09-11 (73-115 with >= 10 sellers). In rule 1's study book 4 of the 44 tail tickets are one-slot falls of -50 % or worse (-0.59 of -5.04 SOL). Their sellers are 27-197 wallets on mostly one build, buying one at a time over minutes (not bundled), holding 22-48 % of live supply at the fire. The rest of the tail sits on bundled buyers: supply held by wallets whose first buy landed in a same-build, same-slot group of >= 3, AUC 0.69; `<= 0.286` books +0.52 / +0.21 SOL and cuts the tail 44 -> 31, and the keep rule refuses it (Bracket: one test gain inside chance; Room: the second fold picks tighter and loses) | Rugs are a constant market hazard that rule 1 mostly skips; their fingerprint (a one-build wallet swarm) is too rare on rule 1's pool to fit, so it is read on the market's rugs. Bundled-buyer share stays a candidate, not taken | `r1t_rug.py`, `r1t_perm.py` | - |
| L6 | What predicts the market's one-slot rugs at a rule-1-like print? | 155,093 candidate sells; inside rule 1's band (reserve 60-100, age >= 158 s) 1,646 of 50,913 are followed by a one-slot fall of -50 % or worse within 90 s, on 174 coins. The share of live supply held by wallets whose first buy used a public app build (a build with > 300 buying wallets on the tape) separates them: below 0.6 the rug rate is 20-43 %, at 0.7-0.8 0.2-2.3 %, at 0.8+ 0.8-1.7 %, on both halves of the days (AUC 0.24). Rule 1's frenzy already sits on retail coins: 557 of 604 tickets at 0.8+, 12 below 0.7 | Public share >= 0.7 is a loss door read on the market's rug label, not on rule 1's money: on rule 1 +0.29 / +0.19 SOL, tail 44 -> 41. With bundled-buyer share <= 0.286: Bracket 6.25 SOL, 98 a day, tail 28, top 1 % 9.9; Room 6.76 SOL, tail 27, top 1 % 14.6; both better at 200 ms. A post-selection read: only the holdout, booked once, and the days after 09-10 certify it | `r1t_rugdoor.py`, `r1t_perm.py` | - |
| L7 | Does the frozen pair (public share >= 0.7, bundled-buyer share <= 0.2857) hold on the holdout? | Booked once, pass bar written first. Bracket: 3.99 -> 4.46 SOL, +5.53 %/trade, tail 22 -> 12, 5/5, top 1 % 8.7. Room: 4.57 -> 4.99 SOL, +6.78 %, tail 22 -> 12, 5/5. Better at 200 ms on both | The clone of rule 1 and rule 1b with the loss door; the days after 09-10 certify it | `r1t_holdout.py` | 1.28 |
| L8 | Which bundled groups carry the cut, and do they act as one? | Study only. On rule 1's coins the groups are public-app builds (98 % of tickets hold some, median 0.132 of supply; operator-build groups on 23 %): the public-app part alone reproduces the cut (Bracket 6.20 vs 6.21 SOL, tail 29 vs 29). Within 120 s of the fire a member's sell lands within one slot of another same-group member's sell 37.3 % of the time, 6.4 % with group labels shuffled (538 tickets); the excess is +39.7 pp on tail tickets, +30.1 on the rest | A bundled group is one trigger, not many opinions: wallets that enter in the same slot through the same app (copy-trade followers, or one operator's wallets on a public app) also leave together | `r1t_bundled.py` | 1.28 |
| L9 | Can the engine read the door as frozen? | The frozen breadth counts the whole tape, future days included. One bot swarm's ~490 recipes (L11) sit at the 300 line (study 100-150, holdout 300-500). Every past-only spelling (the 1 or 3 days before, thresholds 50-300) books the same clone on the holdout: tail 10-12, SOL 4.19-4.46 against rule 1's 3.99. The engine spelling: public when the first buy's recipe had > 100 buying wallets the UTC day before, the class fixed at that buy, the bag in each print's `token_amount`. Holdout: Bracket 388 tickets, +5.61 %/trade, 4.35 SOL, tail 11; Room 353, +6.84 %, 4.83 SOL, tail 10. The engine books it ticket for ticket: 388/388, 353/353, and 95/95, 90/90 on 09-11..09-12, same prints, SOL to 1.5e-16; its stored table equals the lake's counts on every day read | The clone runs in the engine: `m_holder_book` over the daily `build_breadth_day_stats`; DB rules `Flip-Catch - Bracket + Door` and `Flip-Catch - Room + Door`, paper, inactive | `r1e_breadth.py`, `r1e_parity.py` | 1.28 |
| L10 | What do the first days after 09-10 say (lake 09-11, 09-12)? | The market prints half the trades (0.94 M on 09-12 against 1.5-2.6 M before), and rule 1 fires 57 a day. Rule 1: 114 tickets, +1.30 %/trade, +0.30 SOL, tail 11, top 1 % 38.9; rule 1b: 109, +0.79 %, tail 13. Clone: Bracket 95, +4.01 %, +0.76 SOL, tail 7, top 1 % 15.1; Room 90, +3.42 %, +0.62 SOL, tail 9, top 1 % 26.0; 2/2 days each. Live paper matches the replay hour for hour after 09-11 21:12 (Bracket 64 against 66); the Bracket run opened nothing 17:41-21:12 while the replay fires 10. The clone refuses both paper stops at -80 % (public share 0.027 and 0.008) | Two days certify nothing, and the clone misses the concentration bar on them. The clean test goes on with the clone beside rule 1 and rule 1b | `r1n_newdays.py`, `r1e_parity.py` | 1.28 |
| L11 | Is "public app" (breadth > 100 the day before) a reliable class? | Read only, all three tapes. The recipes between 100 and 300 wallets are mostly one bot swarm: 20-40 k persistent wallets (4 % new a day against 21-51 % on the named apps) on ~30 unnamed programs, 16 recipes each, one buy per wallet per program a day. Where it holds >= 20 % of supply, 36-47 % of rule-1-like candidate sells rug in one slot within 90 s, against 1.7-5.5 % elsewhere. Its recipes hold 100-275 wallets a day, so at 100 it is private on some days and public on others; per program it is always public. Door rug rate among passes, recipe > 100 / > 200 / per program > 100: study 1.56 / 0.92 / 3.20 %, holdout 1.62 / 0.53 / 2.54 %, 09-11..09-12 5.50 / 5.00 / 6.71 % (bases 3.37 / 2.90 / 6.71 %). The clone's money is flat from recipe > 25 to > 300 on every tape and falls from 500; per program the door adds nothing to the bundled cut | The concept holds and the door's gain is the swarm's coins; 100 sits inside the swarm and 200 clears it at the same money (post-selection). A swarm that re-splits defeats any breadth line: the durable door names it by its structure | `r1e_public.py` | 1.28 |
| L12 | Does repeat use (buys per wallet per app a day) name the swarm? | Named apps 2.9-16.1 buys per wallet every day; the swarm's programs 1.03-1.07. Public when the first buy's app key had > W wallets and >= R buys per wallet the day before. The choice rule, fixed first (study: keeps >= 90 % of the band's non-rug candidates, lowest rug rate among passes), takes W 100, R 3 (1.13 %, R 2 1.19 %). The swarm leaves the door's passes on every tape (holdout 107 -> 0 candidates). SOL, Bracket / Room, recipe > 100 against app > 100 repeat >= 2: study 5.04 / 4.78 against 5.20 / 5.04, holdout 4.35 / 4.83 against 4.34 / 4.81, 09-11..09-12 0.76 / 0.62 against 0.80 / 0.63; tails equal within one. The engine spelling (app > 100, repeat >= 2) books its reference ticket for ticket: holdout 393/393, 358/358, 09-11..09-12 96/96, 91/91, same prints, SOL to 1.5e-16; its stored table equals the lake's app wallets and buys on every recipe, 09-03..09-12 | The swarm is private on every day at the same money. R 2 over the rule's R 3: DFlow runs 2.9-3.8 buys per wallet, so R 3 flips it by day. The engine classes public this way (migration 0018); the + Door rules read it unchanged | `r1e_repeat.py`, `r1e_parity.py` | 1.28 |

### 2.10 What the chain teaches, and where each lesson is now a rule

| lesson | the steps that taught it | the rule |
| --- | --- | --- |
| split the node by member first; the three that pay were inside it the whole time | 0-14, then 15-16 | derive 1, 4; law 27 |
| measure a reaction from its trigger print, never a waiting time | 0-1, 21 | derive 5 |
| the side separates payers from losers, not the speed | 21 | strategy 1.5 |
| a member's coin list is its future arrival | 10, 26-28 | derive 7.1 |
| read an exit on the selected pool, and re-read it when the pool changes | 11-14, 39, G5 | derive 8, 12.7; law 26 |
| re-derive every slot on the sentence's own pool | G2-G5 | derive 12 |
| count the trades on the graduation print | G3 | derive 8, 12.5 |
| the holdout confirms, it never chooses | 28b, 34, G5 | derive 12.1 |
| hand the engine a number it can reproduce: every wallet, every leg | G9 | derive 12.10 |
| check every term against an exact field, and derive at the engine's grain and fill | G10-G12 | derive 12.11 |
| grade the event by leftover, not by its prior clock | 25, B2c | derive 5.2, 6.2 |
| a looser entry is judged on the trades it adds, net of what it displaces | G15 | derive 12.4 |

### 2.11 X, P and D upgraded on the door's pool

| step | question | what it shows | what it does to the rule | script | ev |
| --- | --- | --- | --- | --- | --- |
| U1 | Where do the paying members' 300-560 positions a day sit against rule 1 + Door? | Study 09-02..09-06. 8fStGV: 429 a day, 97 % inside 300 ms of a public sell (rule 1's event class); rule 1 + Door takes 18 a day of them. At our seat under rule 1's exit its candidate entries failing 2+ terms book -2.74 % (122 a day, 18 % worse than -40 %) and its entries on coins under 60 s -1.84 % (158 a day); at a 25 ms seat -1.34 % and +0.63 %. Its own position grosses +3.15 %, before about 2.5 % of fees. AbQcLH and 49uohd enter after a buy 59 % of the time (the burst start, a race, 1.16) | The members' volume is a thin margin at their own seat, not a permission rule 1 misses: no P or D term is named | `r1g_members.py` | - |
| U2 | Does a smarter X pay on the door's pool? | 1.24's families and five new ones (a speed stop, a stop that tightens with time held, an under-water clock, a sell on a public sell, net flow), both exits, r1b_exit's bars: only the 180 s clock passes the study. Its combined survivor (0.3 x the wall, 180 s) books 3.74 SOL on the holdout against 4.34 (Bracket + Door) and 4.81 (Room + Door), tail 11 -> 35; 09-11..09-12 +0.85 against +0.80, tail 13 against 7. The under-water clock (-5 % for 45 s) cuts the study tail 20 -> 13 at +0.09 SOL, under the chance floor. One arbitrary speed spec (-20 % in 5 s, under water) is read on the holdout by the evaluator check; it chooses nothing | No exit joins. A clock past 90 s fails out of sample a third time (1.20, 1.24); an exit that sells under water sells the winners' dip | `r1g_exit.py` | - |
| U3 | Do P and D give more trades with the door on? | Each entry term loosened alone (r1c_loosen's bars): every term fails its first step; the added trades earn +0.5..+4.4 %/trade against +6.2 %, none positive every day. One additive score over the six fitted terms in place of the AND: 3 of 4 test halves lose (Bracket -0.22 / -3.31 SOL, Room -0.93 / +0.50); on every study day it takes 123 a day against 93, at 5.08 SOL against 5.20, top 1 % 17.1 | Rule 1 + Door's thresholds hold; more trades need a second event | `r1g_loosen.py`, `r1g_score.py` | - |
| U4 | Does a clip sized by a prediction at the fire beat a flat clip of the same mean? | Rule 1 + Door's tickets, both exits, re-priced through the engine kernel at each clip: an additive monotone score over the nine facts at the fire, or one fact alone, fitted on one half of the days and mapped to 0.05-0.5 SOL three ways (60 specs). None passes: out of sample the score's rank correlation with the trade is -0.19..+0.27 and flips sign between folds; the sized book beats the flat clip by more than random sizing on both halves nowhere (closest: the seller's buy age under Bracket, +0.35 / +0.38 SOL against 0.24 / 0.42 by chance). Fitted on every day the same specs add +0.2..+2.6 SOL: the fit, not a signal | S stays flat: nothing at the fire ranks rule 1 + Door's trades | `r1g_size.py` | - |
| U5 | Does the supply for sale inside our hold separate the door's trades: holders under water by less than the +20 % target (they sell at breakeven inside it), or a trigger seller who keeps part of his bag? | Exact vtok book with average cost, study from 09-02, Bracket 419 tickets. Under-water supply p50 3.7 % of live supply, nearly all of it inside the target band: the fire sits <= 20 s from a new high. Band share AUC take profit vs rest 0.54, tail vs rest 0.47, the side opposite the story. The seller empties his bag on 318 of 419 (+6.43 % against +5.50 %/trade when he keeps some; Room +7.19 / +5.44 %). Keep rule, both exits: nothing taken; band and under-water share taken only under Room, on the unregistered side (>=), gains +0.06 / +0.29 SOL inside chance (0.11 / 0.21); the seller's kept share picks no cut under Bracket on either fold | No P joins: at a new-high event almost nobody is under water, so a breakeven overhang has nothing to read. It is a term for an event after a pullback | `r1s_supply.py` | - |
| U6 | Does a curved trail or an early-pop cut pay on the door's pool? | Study 09-02..09-06, both exits, r1b_exit's bars; the evaluator books r1e_refrep's tickets exactly (holdout 393 / 358, 09-11..09-12 96 / 91). Curved trail (width max(floor, w50 x (peak / 50)^p), armed at a gain): 225 specs with the target kept, 225 without it, 45 past the grid's wide edge. Every spec that fires loses: the best is -0.79 SOL (Bracket) / -1.01 (Room) with the target, -2.00 / -1.85 without it, both folds negative, and past the edge the pick is a spec that never fires. It takes the tail 20 -> 13, but on the 57 Bracket tickets it closes it books -2.33 SOL against the base's -1.46: 40 losers gain +0.31 SOL, 17 winners give up +0.54. Early pop fails (the gain reaches +a within t1 s, then the price is back at the fill before t2 s), 72 specs: best -0.28 / -0.02 SOL, folds fail, tail unchanged. On Bracket's 22 tickets it closes, 11 losers gain +0.48 SOL and 11 winners give up +0.41, and the re-entries it frees book -0.23. At 0.03 SOL the order is the same | No exit joins. On price alone a winner's early dip and a loser's early fade look the same at this seat, and the tail falls too fast for a trail to sell ahead of it | `r1u_curve_pop.py` | - |
| U7 | Does the frenzy tell when to take a gain on the door's pool? | Study 09-02..09-06, both exits, r1b_exit's bars; the frenzy is the entry's own count (distinct ix structures in 5 s, equal to the candidate's at the trigger on 400 of 400). Target waits on the frenzy (at the target, hold while the count is above k, sell when it falls to k or the gain falls to a share of the target), 36 specs, then k 12-40 past the grid's edge: Bracket at +20 % loses 0.19-0.81 SOL for k <= 18, peaks alone at k 22 (+0.15, top 1 % 9.4) and equals the base from k 26; fold 1 -0.03 fails. Room: 0.3 of the wall at k 22 is +0.38, but 0.3 of the wall without the wait is +0.32, so the gain is the lower target; folds +0.03 / -0.40 fail. Bank when the frenzy ends (up at least +a and the count at k or under), 37 specs: every setting that fires often loses (k >= 6: -0.2 .. -1.3 SOL); the best adds +0.04 SOL on Bracket inside the chance floor (0.08) and +0.12 on Room with 6 exits, folds -0.35 / +0.06. At 0.03 SOL the order is the same | No exit joins. The frenzy thinning while we are up is ordinary chop inside the hold, not the end of the move, and the only frenzy that carries a coin past the target is too rare to pay | `r1u_curve_pop.py` | - |
| U8 | Does a top holder or a bundle selling during the hold tell a loser early enough to sell? | Study 09-02..09-06, both exits, r1b_exit's bars. Exact token_amount bag book from the coin's first print to the fill, proxy wallets out; it books the door's bundled share at the trigger on 300 of 300. Top holders sell (a top-N holder at the fill with >= s % of live supply has sold >= f of its fill bag), 36 specs: every one loses; the best (the largest holder, >= 5 %, 90 % sold) -0.46 SOL (Bracket) / -0.02 (Room), folds fail. It names losers: 18 of its 27 Bracket tickets lose under the base, but selling after the dump gains only +0.12 SOL on them, while its 9 winners give up +0.34. Any top-10 holder selling half fires on 297 of 500 tickets: tail 20 -> 1, SOL +5.20 -> -0.06. Bundle sells together (2-3 wallets of one bundle, the bundle >= s % of live supply, sell in one slot or the next), 12 specs: best +0.00 (Bracket) / +0.01 (Room, 14 exits), folds fail; at s 5 % it closes 38 Bracket tickets, 20 losers gain +0.46 SOL and 18 winners give up +0.73. At 0.03 SOL the order is the same | No exit joins. The dump print is the drop itself: an exit that fills 115 ms after it sells at the bottom, and big holders and bundles also take profit on winners. What they name is only cut at the fire (P, D), where the door already reads who holds | `r1u_curve_pop.py` | - |
| U9 | Does a fact at the fire name the door's big losers, as a refused fire or as a tight exit chosen at the fire? | Study 09-02..09-06, both exits, 20 tickets worse than -40 % each. 15 facts through the trigger print: the price run over 30 / 60 / 300 s, the fall under the coin's high, the deepest fall in 60 s, the largest wallet's share of the 5 s buy SOL, buyers in 5 s, sell / buy SOL over 5 / 30 s, repeat buyers' share and mean buy over 30 s, new buyers and sellers in 30 s, top-1 and top-10 holder share. Only the holder shares hold a side on both halves: top-10 AUC 0.66 / 0.67 (halves 0.59-0.69), top-1 0.64 / 0.66; the top two top-10 quintiles carry 14 of Bracket's 20 tail tickets and still book +3.7 %/trade. The bars are r1u_curve_pop's, each spec a fact, a side and its 10-90 % point over the candidates. Refused (270 specs): the best is Bracket a 60 s fall of 47 % or more (+0.36 SOL, folds -0.22 / -0.85) and Room a 300 s run of +430 % or more (+0.47, tail 20 -> 12, folds -0.31 / +0.00); top-10 >= 0.477 books -0.26 / +0.22. A tight exit on the risky side (sl 15 / 25 / 35 %, clock 30 / 45 / 60 s; 1,620 specs): Bracket 18 % or more under the high -> clock 45 s (+0.34, tail 20 -> 14, folds -0.13 / -0.43), Room 36 sellers or fewer in 30 s -> clock 60 s (+0.42, folds -0.23 / +0.17). Every top spec passes the ship bars, 200 ms and 0.35 SOL; every fold pick loses on the other half | No P and no exit chosen at the fire joins. The facts that name the tail sit on paying trades too, and each half's best spec is a different fact that loses on the other half: nothing at the fire ranks the door's trades (U4), on 20 tail tickets a rule | `r1v_fire.py` | - |
| W1 | Read as a whole trader, what is 8fStGV's own book? | Study 09-02..09-06, its 1,916 closed positions (exact `token_amount` bag). A flat clip (0.302 SOL, p10-p99 0.297-0.323), one buy and one sell every time (no scaling, no partial), one position at a time (max 3), 426 a day on 1,035 coins, 46 % of them a re-entry on a coin it already traded (the 2nd and 3rd entries book as well as the first). Gross +3.15 %/position, median +14.24 %, 67.6 % winners, 3.3 % worse than -40 %, hold p50 11.7 s, top 1 % 17.8 %, 5/5 days. Its best while holding is p50 +16.3 % and it closes at 94 % of that best on the 459 positions that reach +20 %; where the best stays under +10 % it closes at -27 % after 25-32 s. Its own prints priced through our kernel book **+2.68 SOL** over the 4.50 days at its clip, against rule 1 + Door's **+5.20 SOL** on 419 trades | The rule books about twice its source wallet on a quarter of the trades. Its shape - flat clip, one position a coin, all in, all out - is rule 1's already; nothing in its money management is missing from the rule | `r1w_census.py` | - |
| W2 | Does its exit, read off its own closes, beat the bracket? | Its 1,916 closes against 65 specs in 8 families (take profit, stop, clock, abort, sell into a public buy, sell into a price step, curved trail, trail), scored by nearness to its own close, greedy OR to 4 terms, both folds. It is static after all: both folds name **a take profit +14..16 %, a stop -25..-30 % and a 60 s clock**, booking 2.51 SOL against its own 2.68, 0.0177 SOL a position off its close; no trail or print trigger enters, and "a public buy >= 0.3 SOL once up 14 %" is the take profit at another grain. Frozen and booked on rule 1 + Door's fires (nothing fitted on our money): study 5.20 -> 2.46 SOL with the tail 20 -> 5, holdout 4.34 -> 1.94 with 11 -> 1, 09-11..09-12 0.80 -> 0.47 with 7 -> 3. Its stop alone: 4.24 / 2.82 / 0.55 SOL, tail 4 / 1 / 3 | No exit joins. Its -25 % stop is the whole of its small tail and costs a fifth to a third of the book on every tape: a risk dial like age >= 250 s (L4), not a money term. Rule 1's +20 % / -60 % / 90 s is the same shape held looser | `r1w_exit.py`, `r1w_his_exit.py` | - |
| W3 | Under the 6.1 ladder, which sells of the class does it take? | Every public sell >= 1 SOL on its 1,035 coins while it is flat, prints made while it holds dropped (80,187 rows, acted 2,188 = 2.73 %); 183 cuts on 23 print facts (age, holders and the reserve are P), beam ladder on the acted label, both folds, chance = the label shuffled across whole coins. Every depth beats chance in both folds, and the folds name different facts: fold 1 buys5 >= 12.8 then a busy tape and a new high, fold 2 ssize >= 2.29 then nbig30 and sfrac. The ladder finishes at **depth 0**. The one term both folds hold on the same side is the **seller's hold >= 19.6 s** - the other side of rule 1's `shold <= 30`. Booked on the door's fires: >= 10 s gives +8.01 %/trade against +6.20 and the tail 20 -> 8, but 4.05 SOL against 5.20; holdout 3.24 against 4.34 (tail 11 -> 6); 09-11..09-12 0.47 against 0.80. Single facts rank the crowd (buys10 0.72, nw5 0.70, nb10 0.70), age and holders 0.50 | No E and no P joins. Its pick is real but is not one sentence across halves, and its one stable term is a third risk dial: fewer, better, fewer big losses, less SOL. Rule 1's entry already carries the ladder's shape (a big sell into a busy tape at a new high) | `r1w_eladder.py`, `r1w_shold.py` | - |

---

## 3. The member book

The node's six wallets, booked on their own decisions at the RACE seat (sequenced before their
print) under a 15 s clock, and their reaction measured by excess intensity.

| member | RACE cap15 | days | its trigger (peak lift, lag) | its exit | status |
| --- | ---: | :---: | --- | --- | --- |
| 8fStGV | +2.28 % | 8/8 | public SELL >= 1 SOL (8.1, 25-200 ms); avoids burst starts | sells a public BUY >= 1 at +14 %; read off its own closes it is a static bracket - take profit +14..16 %, stop -25..-30 %, clock 60 s (W2) | **rule 1** |
| AbQcLH | +2.29 % | 7/7 | public burst start - a ~1 SOL buy after a dip (9.9, 25-50 ms) | sells into a quiet tape after ~19 s (no print trigger) | a race, the 5.2 kill: lag 47 ms, we land ahead 5 %, -1.36 % at our seat; cost 4.55 %, peak +1.69 % (1.27) |
| 49uohd | +2.24 % | 6/8 | a node print (6.4, 25-50 ms); public SELL >= 1 / -2 % print (3.3-3.7, 150-300 ms) | sells right after a public sell (lift 14.9), near break-even | **not spelled**: +3.50 % 8/8 on its picks at our seat under a clock, every public spelling red; its sell >= 1 class keeps leftover behind its buy (cost 2.69 %, peak +6.42 %, 1.27) |
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
| [hot-tape/README.md](hot-tape/README.md) | the scripts rule 1 rests on; the chain's step scripts are in git at the commit it names |
| [toolkit/README.md](toolkit/README.md) | the method as functions; `hot-tape/toolkit_check.py` re-runs steps 16, 21, 24 and 39 through it next to the recorded numbers |
| `study-kernel/cvx_prints.parquet`, `cvx_tok.parquet`, `cvx_ix.parquet` | the study tape and its sidecars (shared with the other studies), from `aa.pxf`, last leg per transaction |
| `node-derivation/data/cvx_holdout_*.parquet` | the holdout tape: `build_core` = md5 of the ix labels joined by `\|` after dropping `Associated Token: Create*`, `*: CloseAccount`, `Memo Program*` (5,000 of 5,000 rows match); curve rows only; time rounded to ms; every shared row identical on 09-04 and 09-05 |
| `node-derivation/data/cvx_holdlegs_*.parquet` | the holdout with every leg of a transaction (`python -m toolkit.lake_export cvx_holdlegs 2026-09-03 ... 2026-09-10 --all-legs`), tape `holdout_legs` |
| `node-derivation/data/cvx_studyexact_*.parquet`, `cvx_holdexact_*.parquet` | the study days and the holdout at the engine's grain: every leg, `t_us` (the engine's clock), `vtok` (its spot); tapes `study_exact`, `holdout_exact` |
| `node-derivation/data/r1_creators.parquet` | each tape coin's creator (Postgres `tokens.creator_wallet`, the source simulate reads) |
| `node-derivation/data/r1_ref_{holdout_exact,study_exact}.parquet` | **the engine's parity reference**: rule 1's tickets, print identities and SOL |
