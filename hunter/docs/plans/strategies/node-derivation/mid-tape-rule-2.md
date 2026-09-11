# Mid-tape one-shot: rule 2 (instrument 9999hu)

The working file for **9999hu**, derived by [_!___derive.md](../_!___derive.md).
One member, never pooled. 88887Q names the same tell and stays off this file (law 27).
9Uq8GV is a different tell: [mid-tape-rule-1.md](mid-tape-rule-1.md).

Numbers: the chain rows below and [_!___evidence.md](../_!___evidence.md) 1.27; the closed line is a
row of evidence 7 (mid-tape 9999hu), and the cut write-ups are in git at `9f8ce4c5`. Code: [toolkit/](toolkit/README.md),
scripts in [mid-tape/](mid-tape/README.md).

Tapes: the **study tape** (2026-08-30 17:48 to 2026-09-06 12:00 UTC, 6.76 days), where every
threshold is read; the **holdout tape** (2026-09-06 12:00 to 2026-09-10), where changes
are only confirmed. Days after 09-10 are the clean test.

---

## 1. Rule 2

```
E  public sell >= 1: a derive 5.2 kill, reaction cost 8.97 % behind its buy (1.27). Public occupancy
   of that class at its fire time is red. Launch first-sell (age p50 ~2 s) is a different book
   and is not this sentence. Not frozen.
P  none. Keep rule takes no cut on stop-outs vs take-profits at the fire (step 9.1). Rule 1's
   established-coin cut is under the floor here (age p50 15 s).
X  working: take profit +15 %, stop -40 %, clock 70 s on the acted pool (step 8.2). Hazard is a
   20-30 s clock, close median -0.2 % against peak 17.8 %: it gives the leftover back. A clock
   is the prior, not the eat. Top 1 % 43 % and capped red: not shipped.
D  none
R  unlimited, one position per coin
S  0.2 SOL
seat  both legs fill at the last print landed 115 ms after the decision print
```

Plain words: (empty until E is a public sentence).

### The book

(empty until a public sentence exists)

### Each slot, in one line

| slot | where it comes from | step, evidence |
| --- | --- | --- |
| E, the trigger | sell >= 1 @ 50-75 ms lift 9.12; 79 % of episodes; behind its buy the reaction cost is 8.97 %, a 5.2 kill (the +17.8 % leftover is read on the ahead tickets) | 5.1, 5.2c; ev 1.27 |
| E, the terms | vs ignored same-coin sells: live frenzy (nb2 9 vs 5, buys2 6.5 vs 1.0), younger (age 16 s vs 145 s). Recipe-count occupancy does not lift. Age <= 16 s occupancy is the launch first-sell (fire age p50 2 s), not its 16 s fire | 6.1-6.2 |
| P | keep rule takes none. Stop-outs are slightly hotter/thicker (vres AUC 0.63, mv60 0.62), not young-thin (hold_n AUC 0.50). Rule 1's holders x age is under the floor | 9.1 |
| X | hazard is a 20-30 s clock (close median -0.2 % vs peak 17.8 %). Working family on the acted pool: tp15 sl40 t70 +1.61 % 7/7 body +4.47, top 1 % 43 %, capped -7.42 | 8.1-8.2 |
| D | none | |

---

## 2. The chain

| step | question | what it shows | what it does to the rule | script | ev |
| --- | --- | --- | --- | --- | --- |
| 1-4 | Pick / shape / seat | 9999hu: 4,323 episodes / 3,669 mints, re-entry 15.1 %, age p50 17.6 s, hold 3.9/25.0/69.8 s. RACE +5.94 % 7/7 body +34.67; FOLLOW -2.60 % 0/7 | instrument; event must be reached before its print or leftover must survive FOLLOW; do not pool | `mt_p15.py` | - |
| 5.1 | What print does it react to? | sell >= 1 @ 50-75 ms lift 9.12; sell >= 0.5 lift 6.89; buy >= 1 lift 2.68; burst start < 1.3 | E class is a public sell, not a burst start | `mt_p15.py` | - |
| 5.2 | Leftover at 115 ms on acted tickets | hits 79.1 %, dt p50 96 ms, ahead 43.1 %. rcost p50 +5.5 % overall, **0.0 % when ahead**, +9.0 % when behind. Missed 0.1 %. Leftover median p10/p50/p90 **+9.3 / +17.8 / +23.6 %/trade**, 88.6 % cover cost | leftover **lives**. rcost when ahead is 0: the +5.5 % is filling after its buy, not a 5.2 kill (superseded by 5.2c) | `mt_hu_p5.py` | - |
| 5.3 | Diagnostics | clock mean 1/5/15 +4.8 / +3.2 / +3.6; clock 15 median **-2.6**. His-exit median -2.3. Behind leftover still +13.4. Occupancy on its coins clock 25 **-0.94 % 0/8** | a clock and a copy of its close are not the leftover veto. Occupancy red is 6.1 | `mt_hu_p5.py` | - |
| 6.1 | Which of those sells? | 183,865 sell >= 1 on its coins; 3,437 acted = **77 % of its buys**, k==0 0.0 %. Acted: nb2 9 vs 5, buys2 6.5 vs 1.0, age 16 s vs 145 s, shold 4.2 s vs 20.6 s, stall 3.6 s vs 30 s. Acted clock 25 **+1.29 %**; ignored **-0.83 %** | its logic, not a corner. Class is too wide: add terms to E, do not add D | `mt_hu_p5.py` | - |
| 6.2 | Public sentence on every coin? | sell >= 1 occupancy clock 25 **-3.53 % 0/8**. Stacking nb2 / nw5 / ssize makes it worse. Control buy >= 1 none is +0.07 body -455 (k==0 14.7 %). Each term alone: only age <= 16.3 lifts (**+3.85 % 7/7 body +40, top 1 % 71 %**) and that fire's age p50 is **2.0 s**. Age 8-16 s (its fire) **-3.74 % 0/7** | occupancy first-sell is launch, not its decision. Do not freeze age <= 16. Do not AND nb2. Next is phase 8 on the acted pool | `mt_hu_p6.py`, `mt_hu_p6b.py` | - |
| 8.1 | How does it close on the selected pool? | No closing print (buy >= 1 lift 1.18). E-kind closes n=2,555, held p50 **23.5 s**, close pnl p50 **-0.2 %**, peak p50 **17.8 %**. Hazard jumps at 20-30 s across every pnl band, not at a take-profit | X prior is a clock at hold p50. It does not eat the leftover; it gives it back. A copy of its close is the wrong family | `mt_hu_p8.py` | - |
| 8.2 | Which family eats the leftover? | Clock 25 +0.86 5/7 body -6.3. **tp15 sl40 t70 +1.61 % 7/7 body +4.47**, hold 6.3 s, both halves green. Trail / ride / dump / longer clock worse. Sellbuy and scale do not beat it on days and body. Top 1 % 43 %, capped -7.42 | working X on the acted pool. Ship bars fail on the tail | `mt_hu_p8.py` | - |
| 9.1 | What is true at the fire on stop-outs vs take-profits? | Occupied sl 14 % / tp 67 %. Best AUC vres 0.63, mv60 0.62; hold_n 0.50. Keep rule takes none of 28 facts. Rule 1 established-coin n=131-302, under the floor. Named age / stall / nb2 cuts lose a day or SOL | P stays none. The ship fail is the winner tail, not the stops. No D on this parent (5.2 kill, 1.27) | `mt_hu_p9.py` | - |
| 5.2c | Does leftover exist behind its buy (derive 5.2 as calibrated)? | Behind its buy: reaction cost 8.97 % (93 % of tickets over 2 %), peak +12.61 % at its hold p50 (25 s), lag 58 ms; 88887Q 7.01 % | Killed on the cost line: the 5.2 row above read the ahead tickets, where its own buy is the leftover. No D search on this parent; another class or a state | `../hot-tape/b2_leftover.py` | 1.27 |

### Tried and out

| idea | result | why it is out |
| --- | --- | --- |
| copy 9999hu's fill | FOLLOW -2.60 % 0/7 | the fill is in the price |
| occupancy of every sell >= 1 | -3.53 % 0/8 clock 25 | class too wide; first-fire is not the sell it takes |
| copy its close / clock 25 as X | close median -0.2 % vs peak 17.8 %; clock 25 +0.86 body -6.3 | it gives the leftover back |
| trail / ride / dump on the acted pool | +0.12 / -0.39 / +0.35, days 3/7 or 4/7 | they hold the bounce and eat the give-back |
| rule 1 established coin as P (age >= 158, hold_n >= 368) | n=302 / 131, under the per-day floor | his fire is age p50 15 s; that P is a hot-tape story |
| keep-rule P cut (28 facts, before occupancy) | no term taken; folds disagree or the test half loses | stop-outs are not a separable fire-time state |
| age >= 16.3 / stall <= 5 / nb2 >= 9 as P | 6/7 or worse; SOL at or under the base | they drop the bounce that pays |
| nb2 >= 9 / nw5 / ssize on that occupancy | -3.70 to -5.85, each worse | recipe-count occupancy is not its leftover |
| age <= 16.3 occupancy | +3.85 % 7/7, top 1 % 71 %, fire age p50 2.0 s | launch first-sell, not its 16 s fire |
| age 8-16 / 10-25 occupancy | -3.74 / -4.41 % 0/7 | public occupancy at its fire time is red |
| age <= 16.3 and nb2 >= 9 | -3.98 % 0/7 | nb2 kills the launch book too |
| 88887Q as a second instrument | same tell (sell >= 1 lift 8.61) | law 27; one member |
| pool with 9Uq8GV | buy vs sell, age 159 s vs 18 s | two sentences |

---

## 3. The member book

| member | RACE cap15 | FOLLOW cap15 | days R/F | its trigger (peak lift, lag) | status |
| --- | ---: | ---: | :---: | --- | --- |
| **9999hu** | **+5.94 %** | -2.60 % | 7/7 / 0/7 | sell >= 1, 9.12, 50-75 ms | **instrument.** sell >= 1 is a 5.2 kill behind its buy (1.27); working X tp15 sl40 t70 (step 8.2); P none (step 9.1); public occupancy red |
| 88887Q | +5.73 % | -1.81 % | 7/7 / 0/7 | sell >= 1, 8.61, 50-75 ms | same tell; off this file |
| 9Uq8GV | +2.21 % | -1.57 % | 8/8 / 3/8 | buy >= 1 / nb2 | other tell; [mid-tape-rule-1.md](mid-tape-rule-1.md) |
| 8dtx2t | +3.16 % | -1.54 % | 7/7 / 0/7 | burst start | public burst START is C2 |
| 8aaRWu | +1.06 % | -0.67 % | 5/7 / 3/7 | burst start | drop (tail) |
| ApfmkS | +2.81 % | -1.46 % | 7/7 / 2/7 | flat | no print tell |
| 3Xk2Eu | - | - | - | - | no prints |

---

## 4. Data and code

| where | what |
| --- | --- |
| [mid-tape/README.md](mid-tape/README.md) | every script, by step |
| `node-derivation/data/mt_lift_9999hu.csv` | excess-intensity lift |
| `node-derivation/data/mt_hu_left_sellge1.parquet` | 5.2 leftover rows |
| `node-derivation/data/mt_hu_p5_left.csv` | 5.2 leftover gate |
| `node-derivation/data/mt_hu_which_sellge1.parquet` | 6.1 which sells, with act |
| `node-derivation/data/mt_hu_all_sellge1.parquet` | full-tape sell >= 1, clock 25 |
| `node-derivation/data/mt_hu_p6_book.csv` | 6.2 stacked unpriced terms |
| `node-derivation/data/mt_hu_p6b_book.csv` | 6.2 each term alone and age bands |
| `node-derivation/data/mt_hu_p8_close_lift.csv` | 8.1 close-intensity lift |
| `node-derivation/data/mt_hu_p8_closes.parquet` | 8.1 hazard closes |
| `node-derivation/data/mt_hu_p8_book.csv` | 8.2 family ledgers |
| `node-derivation/data/mt_hu_p8_outcomes.parquet` | 8.2 per-exit outcomes |
| `node-derivation/data/mt_hu_p9_auc.csv` | 9.1 stop vs tp AUC |
| `node-derivation/data/mt_hu_p9_terms.csv` | 9.1 keep-rule new_terms |
| `node-derivation/data/mt_hu_p9_named.csv` | 9.1 named inventory P cuts |
| `node-derivation/data/mt_hu_p9_book.csv` | 9.1 P=none ledger |

## Open

The open items are in [_!___workflow.md](../_!___workflow.md) section 2, the one queue.
