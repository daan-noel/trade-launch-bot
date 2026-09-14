# Mid-tape one-shot: rule 3 (instrument 8aaRWu)

The working file for the mid-tape node, derived by [_!___derive.md](../_!___derive.md).
One member, never pooled. Phase 3-4 ranks every roster member on the every-leg study tape.
**8aaRWu** is the live instrument: identity leftover PASSES 5.2; 6.2 occupancy is red; **7.1**
(the door test) is next. Occupancy red and leftover green is not phase 8. **8dtx2t**'s
first-position sentence (rule 3b, section 1b) is red on every fire: its study book read only the
coins that end with >= 60 prints (a future read); every day from 09-01 books -3.1 %/trade. **3Xk2Eu**
leftover exists on the burst family (peak leftover +5.75 to +10.41 %; cost is not a kill). **88887Q**
leftover exists on sell/down (peak +3.13 to +6.45 %). **ApfmkS** is
volume manufacture, not a reader. E is not frozen: occupancy is not a public sentence; D is unread.

Numbers: the chain rows below. Code: [toolkit/](toolkit/README.md), scripts in
[mid-tape/](mid-tape/README.md).

Tapes: the **study tape** is `study_exact` (every leg). Study **fires** stop at 09-06 12:00 even
though the file still holds prints to 09-06 24:00 (those are the holdout's warm-up; derive 2.1).
The last-leg study tape does not carry 3Xk2Eu. The **holdout tape** is `holdout_exact`
(2026-09-06 12:00 to 2026-09-10), where changes are only confirmed. Days after 09-10 are the
clean test. 8aaRWu's 6.1/6.2 tables sit on the full 6.00-day file and on the old candidate
floor: re-read before they count.

---

## 1. Rule 3

```
E  public identity family (union of structure_burst / clip_step_up / buy_after_sells /
   two_struct_slot / tool). Behind our 115 ms fill: cost 1.20 %, peak leftover +8.84 %,
   cover 95.2 %. Class is too wide (0.3 % of family prints acted). 6.1 first terms:
   this print's own step mvk >= 1.29 %, nstruct >= 3, sell_run <= 0. Do not AND.
   6.2 occupancy of those terms on every coin is red 0/6; leftover on each still PASSES.
P  none (age / hold_n look like P: 93 s vs 288 s, 166 vs 406). Frame: age >= 10 s (derive 9)
X  none. Prior: hold p50 25.6 s; his tot +3.39; >20 % losses 19.8 %; RACE +0.80 3/6,
   body -1.32, top 1 % 264 % of his net (his harvest shape, not the 5.2 veto).
D  unread (7.1). Occupancy red 0/6 and acted-only +0.88 % 4/6 is the door test, not a skip.
R  unlimited, one position per coin (re-entry is R; E is not first-on-mint)
S  0.2 SOL
seat  both legs: measured p50 83 ms, 115 ms beside it
```

Plain words: he buys after a public identity-family print. The family covers almost every
buy, first and re-entry alike. Most of those prints are ballast. The ones he takes already
stepped about 1.3 % in a crowded slot, not after sells. Occupancy of that spelling on every
coin is red; leftover still exists. Next is whether that gap is a coin property (7.1).

### The book

(empty until a public sentence exists)

### Each slot, in one line

| slot | where it comes from | step |
| --- | --- | --- |
| E, the trigger | 8aaRWu identity family (union of five). Parent leftover cover 95.2 %, behind cost 1.20 %, peak +8.84 %. Exclusive remainders leftover-legal except buy_after_sells only (cover 3.1 %, a corner not a drop). seed_racer is a race diagnostic, not E | 5.1, 5.2, overlap |
| E, the terms | On its coins, 0.3 % of family prints acted. First terms: mvk >= 1.29 % (rank 0.74), nstruct >= 3 (0.74), sell_run <= 0 (0.35). 6.2 occupancy on every coin: none **-2.97 %** 0/6, mvk **-1.51 %** 0/6 (lifts, still red), nstruct **-2.06 %**, sell_run **-2.14 %**. Sequential AND **-1.26** then **-1.34 %** 0/6 (sell_run does not lift). Control sells **-4.07 %**. Leftover on each spelling PASSES (peak **+8.84 to +13.08 %**). Occupancy age p50 456 s vs his 89 s. Do not freeze occupancy. Do not AND | 6.1, 6.2 |
| P | age 93 s vs 288 s, hold_n 166 vs 406 (candidate, not fitted). Frame: age >= 10 s | 6.1, 9 |
| X | hold p50 25.6 s; his tot +3.39; >20 % losses 19.8 % (a prior, not phase 8) | 3 |
| D | unread. Occupancy red / acted green is 7.1, not "none" | 7 |

## 1b. Rule 3b (8dtx2t, its first position on a coin)

8dtx2t's first positions are 73 % of its positions and carry RACE +13.32 of its +16.34 SOL.
That sentence treats first-on-mint as R = 1; 8aaRWu does not split (derive 3: E is this print).
Derived at the measured seat, 83 ms (evidence 1.1), 115 ms beside it; study fires 09-01 .. 09-06 12:00.

```
E  a public clip_step_up buy (larger than its build's last buy on the coin), its own step
   >= 4.4 %, the reserve after it <= 40 SOL, by a wallet with no earlier print on the coin;
   the coin >= 1 s old (frame: strategy 8.1's launch ramps)
D  public holders <= 46 before it (the toolkit's hold_n, which reads as distinct public buyers)
P  <= 4 public sells >= 1 SOL in the last 30 s; <= 25 prints in the last 5 s
X  take profit +40 %, stop -30 %, clock 17 s
R  one entry per coin
S  0.2 SOL
```

**The book, every lake day** (`mid-tape/mt_d8_replay.py`: the lake, every coin born from 09-01,
every leg, every wallet; engine fills, clock and cost; fires after the engine's dead verdict out):

| window, the terms as derived | tickets/day | %/trade at 83 ms | SOL | days + | at our real fill lags |
| --- | ---: | ---: | ---: | :---: | ---: |
| study, 09-01 .. 09-06 12:00 (fitted) | 1,993 | **-3.15** | -69.1 | 0/6 | -3.02 |
| holdout, 09-06 12:00 .. 09-10 | 2,233 | **-3.35** | -67.3 | 0/5 | -3.33 |
| new days, 09-11 .. 09-13 | 1,441 | **-3.14** | -27.1 | 0/3 | -3.14 |

The engine spelling (any wallet, distinct buyers <= 46, every sell) on every day: 1,958 a day,
-3.27 %/trade, 0/13, 95 % interval over coins -3.60 .. -2.96 %; seat 50 ms -2.55 %, 83 ms
-3.27 %, 115 ms -3.61 %, 200 ms -4.09 %, 300 ms -4.41 %.

**Where the study's +5.00 %/trade (1,167 a day, 6/6) comes from.** The candidate table
(`toolkit/candidates.build`) keeps a coin only when it ends with >= 60 prints and >= 60 s of
life, a count read after the fire. 40.4 % of the rule's fires sit on coins that end shorter; they
book -14.7 / -14.9 / -14.2 %/trade (study / holdout / new days), win 10-13 %, and the coins that
end longer book +5.15 / +4.95 / +2.40 %. With that floor the replay rebuilds the study book to the
ticket (6,417 of 6,417 on the same trigger print, SOL equal to the last digit); without it, the
rule reads -3.10 % 0/6 on the study. Every other change moves the study book by less than 0.1 pp:
the trigger by any wallet, missing labels as no recipe, every sell in nbig30, holders as distinct
buyers, the engine's fills, clock and cost, the dead verdict. Every 6.2 .. 12 read below stands on
that table.

---

## 2. The chain

| step | question | what it shows | what it does to the rule | script | ev |
| --- | --- | --- | --- | --- | --- |
| 3-4 | Shape and who pays, every member, every-leg study | All seven have episodes. FOLLOW red on all (a copy of the fill, not a drop). RACE total green on all. His book at 0.2 SOL: 8dtx2t median -1.65 %, total +26.78, **>20 % losses 0.8 %**, hold 5.7/16.6/91.2 s, 424/day, RACE +16.34 6/6. 88887Q more RACE SOL (+52.94) but >20 % losses 9.3 % and hold p10 2.4 s. 3Xk2Eu 998 episodes, hold p50 50.0 s, >20 % losses 22.0 %. 9Uq8GV hold 9.7/16.0/16.2 s (a clock). A negative median with a positive total and few large losses is a cut, not a tail | Freeze **8dtx2t**: harvest hold, enough tickets, RACE pays, the tightest loss cut. Others stay candidates. Do not pool | `mt_p34.py` | - |
| 4.0 | Is ApfmkS a reader? | Tape share 0.13 % prints / 0.45 % SOL on coins it prints. Lake ix: VolAcc 7.7 %, TransferChecked 40.8 % (unique in the 26), CreateCoinAndBuy 1 mint, 6Vo router 98.6 %. 5.1 peak lift 1.44 | Drop before FIND E. Volume manufacture, not a reader. Next instrument is 8aaRWu | `mt_volmaker.py`, `mt_volmaker_ix.py` | 5.1 |
| 4.0 8aaRWu | Is 8aaRWu a reader? | Tape share 0.19 % prints / 0.14 % SOL. Lake ix: VolAcc 0, TransferChecked 0, 6Vo 0, no_ata 49.8 % (same as other readers). Creator 0. Clip p50 0.28, top-3 8.1 % | Keep. Reader, not volume manufacture. FIND E next | `mt_volmaker.py`, `mt_volmaker_ix.py` | 5.1 |
| 5.1 8aaRWu | What print does it react to? | Every-leg study, 707 episodes, WHO then history then priced. Identity: structure_burst **4.84** @ 25-50 ms, clip_step_up 4.46, buy_after_sells 4.21, seed_racer 4.33 (race diagnostic), tool 2.98, two_struct_slot 2.86. Priced: burst_start **5.95**, up>=1 % 4.68, buy>=0.5 4.34. Sells 0.32-1.28 (avoids). Cover 15-79 %. Fire is that public structure restart, not its buy | 5.1 yes: identity of a structure restart. Identity beats priced. Same priced family as 8dtx2t / 3Xk2Eu, different leftover | `mt_p51_8aaRWu.py` | - |
| 5.2 8aaRWu | Leftover on those prints at 115 ms | Behind row: identity cost **1.19-1.94 %**, peak leftover **+5.31 to +16.47 %**, missed 0. Lead structure_burst cost 1.36, peak +5.31, cover 45.7 %. clip_step_up 1.27 / +8.52 / 63.9 %. buy_after_sells 1.49 / +11.31 / 26.4 %. two_struct_slot 1.19 / +6.95 / 43.1 %, cost_ge2 16.9 %. tool 1.55 / +10.38 / 59.8 %. Exclusive quiet / continuation 1.49 / 1.39; loud restart cost 2.37 % with peak **+7.12 %** (leftover exists; cost is not a kill). seed_racer leftover 1.56 and is still a race diagnostic, not E | 5.1 yes, 5.2 yes: DELAY-legal. Identity classes are one family | `mt_p51_8aaRWu.py` | 1.27 |
| overlap 8aaRWu | Same print, or five events? | Jaccard of family prints on its coins 8-28 %. 52.6 % of family prints carry exactly one flag, 32.7 % two. Fire print (latest family print <= 300 ms before its buy) covers **95.2 %** of buys; 42.1 % of those fires have one flag, 35.7 % two, 17.8 % three, 0.9 % all five. Exclusive remainder of fires: tool 13.7 %, clip 11.7 %, two_struct 8.3 %, burst 6.1 %, buy_after_sells 2.2 % | One family. Do not AND. Do not drop a quieter class | `mt_p61_8aaRWu.py` | - |
| exclusive 5.2 8aaRWu | Leftover on exclusive pieces of the family | Parent union: cover 95.2 %, behind cost **1.20 %**, peak **+8.84 %**, missed 0. Exactly-1 / 2+: 1.20 / 1.33, peak +11.58 / +8.16. Remainders PASS except buy_after_sells only (cover 3.1 %). Burst-not-clip / clip-not-burst / both: 1.44 / 1.29 / 1.49, peak +4.77 / +9.91 / +4.02 | Leftover exists on the parent and on exclusive pieces. buy_after_sells only is a corner, not a drop from the family. Parent is E's class | `mt_p61_8aaRWu.py` | 1.27 |
| 6.1 8aaRWu | Which of those family prints does it take? | 560,105 family prints on its coins; 1,469 acted = **0.3 %** of triggers (several family prints in the 300 ms window). Rank: mvk 0.74 HIGH (1.29 % vs 0.07), nstruct 0.74 HIGH (3 vs 2), sell_run 0.35 low (0 vs 1). clip_step_up 47.6 % vs 28.5 %. structure_burst 28.9 vs 27.1; tool 61.3 vs 62.1; buy_after_sells 12.9 vs 12.6 (nothing among family). Clock 26: acted **+0.90 %**/trade, ignored **-1.19 %**. Age 93 s vs 288 s and hold_n 166 vs 406 are P | First terms mvk >= 1.29 %, nstruct >= 3, sell_run <= 0. Do not AND (52 coins). "Do not add D" is only when leftover exists only on the fires he takes. Occupancy red + leftover green is 7.1 | `mt_p61_8aaRWu.py` | - |
| 6.2 8aaRWu | Public sentence on every coin? | Clock 26. Occupancy: none **-2.97 %** 0/6 n=296,980; mvk >= 1.29 **-1.51 %** 0/6 (lifts); nstruct >= 3 **-2.06 %**; sell_run <= 0 **-2.14 %**. Sequential AND **-1.26** then **-1.34 %** 0/6 (sell_run does not lift). Control sells **-4.07 %** (worse). Leftover on each spelling PASSES (cover 43.8-95.2 %, peak **+8.84 to +13.08 %**). Occupancy age p50 456 s vs his 89 s. Acted-only **+0.88 %** 4/6 body -0.71 (not a sentence); acted nstruct +1.49 % 5/6. Tables sit on study fires past 09-06 12:00 and the old candidate floor | Occupancy is not a public sentence. Do not AND. Next is **7.1**, not phase 8. Re-read before the numbers count | `mt_p62_8aaRWu.py`, `mt_p62_tail.py` | - |
| 5.1 | What print does it react to? | Every-leg study, buys vs same-coin controls. burst_start @ 25-50 ms lift **7.24**; buy>=0.5 7.01; up>=3 % 6.93; buy>=1 5.87. Sells 0.13-0.88 (avoids). Peak lag 25-50 ms on every buy-side class. Fire is that public print, not its buy | E class is a public burst start (a size buy after silence). Reaction is faster than 115 ms | `mt_p51.py` | - |
| 5.2 | Leftover on that print at 115 ms | Acted tickets: latest class print <= 300 ms before its decision; fill 115 ms after that print. Behind row (its buy already in the price): burst_start cover 70.5 %, cost **5.20 %** (98.6 % >= 2), peak +0.86 %. buy>=0.5 cost 4.46 %; up>=3 % 4.55 %; buy>=1 5.34 %. Ahead tickets are the copied fill (burst_start ahead 37.5 %). PASS none | DELAY: the tell and the SOL land in the burst. No D/P/X on this trigger | `mt_p51.py` | - |
| 5.1c / 5.2 | Held state, then leftover on its rising edge | vs same-coin public prints: unpriced top nb2 rank 0.67 HIGH (p50 3). Stronger priced separator is buys5. Rising edge nb2>=3 / nbig2>=1 / nb5>=4 / nbig5>=1: behind cost 5.14-6.70 %, cover 21-32 %. PASS none | The unpriced state is already inside the burst. The crossing is the same 5.2 kill | `mt_p51.py` | - |
| 5.1 / 5.2 exclusive | Same family, split so the trigger print is not the 3 % step | Quiet restart / continuation / loud restart: lift 5.74 / 5.81 / 9.72 @ 50-75 ms, cover 25.7 / 41.9 / 44.8 %. Behind cost **4.23 / 4.48 / 6.11 %**, peak leftover **+0.08 / +1.92 / +1.07 %**. Quiet trigger itself moves +2.27 %; the extra cost is the next prints inside 115 ms | Leftover on the exclusive burst family is thin. Cost is not a kill. 8aaRWu's identity family is the live hill | `mt_p51_excl.py` | - |
| 5.1 88887Q | What print does it react to? | Every-leg study, 4602 episodes, WHO then history then priced. Identity: seller_loss **5.27** @ 50-75 ms, seller_recent 4.16, nonce 4.01, buy_after_sells 3.45. Priced: sell>=1 **8.49**, sell>=0.5 7.25, down>=2 % 7.24. Buy-size / burst_start stay flat (lift 1.22-1.77). Cover 24-84 %. Fire is that public sell/down, not its buy | 5.1 yes: it reacts to a public sell (and WHO/history of that sell). Same family as 9999hu | `mt_p51_88887Q.py` | - |
| 5.2 88887Q | Leftover on those prints at 115 ms | Behind row, all 16 spikes: cost **6.55-7.53 %** (88-98 % >= 2), peak leftover **+3.13 to +6.45 %**, missed 0.0-0.8 %. Identity first (seller_loss 7.53, nonce 7.04, tool 7.10); priced sell>=1 6.99. cost_ge2 88-98 %, so no exclusive split | 5.1 yes, leftover exists (cost is not a kill). Not the live instrument: entry is already 6-7 % | `mt_p51_88887Q.py` | 1.27 |
| 5.1c / 5.2 88887Q | Held state, then leftover on its rising edge | vs same-coin public prints: unpriced HIGH but weak (nw5 rank 0.65, nb2 0.60, p50 4). Rising nb2>=4 / npro2>=1: cover 12.2 / 11.2 %, behind cost 7.74 / 6.99 %, peak leftover still green | The state is not a second tell | `mt_p51_88887Q.py` | - |
| lag ladder 88887Q | Cost / leftover at 50 ms vs 115 ms on seller_loss / sell>=1 / down>=2 % | Cover 53.8 / 61.7 / 80.7 %. At 50 ms, acted cost 0.39 / 0.00 / 0.00 % and ahead 59.5 / 75.9 / 71.6 % (first-in-window). Behind 50 ms still cost **7.27 / 6.57 / 6.95 %**. Behind 115 ms 7.53 / 6.99 / 7.29 %. Peak leftover stays green | 50 ms does not cheapen the behind row. The 0-cost acted median is landing before its buy, not leftover. Seat stays 115 ms (83 ms beside it) | `mt_p51_88887Q_lag.py` | 1.1 |
| 5.1 3Xk2Eu | What print does it react to? | Every-leg study, 998 episodes, WHO then history then priced. Identity: alone_in_slot **5.70** @ 75-100 ms, clip_step_up 4.64, tool 4.49, buy_after_sells 4.27, structure_burst 4.25. seed_racer 4.17 @ 0-25 ms (a race diagnostic). Priced: burst_start **6.21**, up>=3 % 5.18, buy>=1 5.08. Sells 0.15-1.39 (avoids). Cover 16-94 %. Fire is that public burst, not its buy | 5.1 yes: the same burst family as 8dtx2t. Identity names the printer, not a quieter print | `mt_p51_3Xk2Eu.py` | - |
| 5.2 3Xk2Eu | Leftover on those prints at 115 ms | Behind row, all 14 spikes: cost **5.41-11.72 %** (86-100 % >= 2), peak leftover **+5.75 to +10.41 %**, missed 0.0-0.3 %. Identity first (clip_step_up 6.65, nonce 6.63, tool 7.04, structure_burst 7.26); priced burst_start 11.72, cheapest up>=1 % 5.41. Exclusive quiet / continuation / loud 6.40 / 6.33 / 15.49 | 5.1 yes, leftover exists (cost is not a kill). Same family as 8dtx2t. Not the live instrument: entry is already 5-15 % | `mt_p51_3Xk2Eu.py` | 1.27 |
| 5.1c / 5.2 3Xk2Eu | Held state, then leftover on its rising edge | vs same-coin public prints: unpriced HIGH (nb2 rank 0.64, nbig2 0.62, p50 4.5 / 1). Rising nb2>=4 / nbig2>=1 / nbig5>=1: cover 31-36 %, behind cost 10.99 / 14.78 / 14.71 % | The state is already inside the burst | `mt_p51_3Xk2Eu.py` | - |
| lag ladder | Cost / leftover at this bot's fastest fill vs 115 ms | Fastest ACK is 8-10 ms (not a fill). Fastest observed own-fill ~45 ms. Standing seat p50 115 ms. At 50 ms, acted median cost 0.00 / 1.78 / 2.43 % and peak +5.32 / +5.34 / +5.81 % — the 0 is "no later print has landed yet", not a reachable next-print seat. Behind 8dtx2t still cost 4.01 / 4.24 / 5.27 %. At 115 ms behind cost 4.23 / 4.48 / 6.11 % | A faster ACK does not change the 5.2 kill. The 50 ms 0-cost median is first-in-window, which sequencing does not buy | `mt_p51_lag.py` | 1.1 |
| anatomy 8dtx2t | Where do its decision buys land? | It opens 0.2 % of its bursts (other buyers 38.6 %); 94.7 % follow a buy, 74 % in the same slot as the print before, tx_index gap p50 120 (not a bundle). Its buys pile up 25-300 ms after the burst's first print (p50 136 ms), with no rise in the 5 s before it | Its decision is which burst, inside the burst: the burst's first print is the earliest fire | `mt_d8_anatomy.py` | - |
| seat | What is our seat today? | Copy rules' real fills since 09-01: p50 83 ms, 63.7 % in the trigger's slot. At 83 ms a fire on the burst's first print lands before its buy on 49.6 % of the bursts it joins | Rule 3b reads at 83 ms, 115 ms beside it | Postgres | 1.1 |
| 5.1 wide 8dtx2t | The full class scan, and on its first positions only | All buys: clip_step_up 7.24, structure_burst 6.83, burst_start 7.24 @ 25-50 ms; nothing spikes alone at >= 150 ms. First positions (1,853): clip_step_up **7.96** (cover 72 %), burst_start 7.76. 5.2 at 83 ms: ahead of its buy 39.6 %, ahead cost 0.00 %, peak +7.27 %; behind cost 4.52 % | E class = clip_step_up (identity beats priced) | `mt_d8_p51wide.py`, `mt_d8_e.py scan` | - |
| 6.1 3b | Which clip_step_up prints does it take for its first position? | Every class print on its coins while flat vs its first-position picks, same coin: own step 0.74 HIGH (+4.4 vs +1.0 %), size 0.72 HIGH (0.86 vs 0.25 SOL), reserve 0.34 low (40 vs 52), buyer's earlier prints on the coin 0.37 low; age / holders / SOL bought 0.49-0.50 | First terms: step, size, reserve, new wallet | `mt_d8_e.py pick` | - |
| 6.2 3b | Do they hold on every coin? | One term at a time, prior clock 16.6 s, no fire before age 1 s: step >= 4.4 %, reserve <= 40, new wallet each lift the book; size lifts only to 0.5 SOL and lowers it once the step is in; dd hurts. Stacked +0.30 %/trade 4,122 a day; the sell-side control lower at every step | E = step + reserve + new wallet; size and dd dropped | `mt_d8_e.py spell` | - |
| launch frame | Where does the money sit by age at the fire? | Before the frame: age < 0.5 s carries 72 % of the SOL (+13.65 %/trade, 869 a day) at a 0.0 % pick share of its own; 0.5-10 s +5.79 %; >= 60 s +0.26 % | No fire before age 1 s (strategy 8.1); E, D, X and P re-read without it | `mt_d8_e.py seats` | - |
| 7.1 / 7.3 3b | Is the gap a coin property? | E: its coins before its first buy +6.03 %/trade 5/5, after -1.04 %, other coins -0.61 %. Keep rule from 09-02: public holders <= 46 (both folds, above cut_noise); then nothing. Public share, bundled share, the creator's earlier coins and its bag: none | D = public holders <= 46 | `mt_d8_d.py`, `walk` | - |
| 8.1 / 8.2 3b | How does it close, and which exit pays here? | Its first positions: 64.7 % close at -20..0 %, 0.9 % at <= -20 %; hazard 69 % at -20..-10 % inside 5-10 s, 47-70 % under water at 10-30 s. On E + D: walked bracket +40 % / -30 % / 17 s; its own stop-after-5 s and under-water cut never taken; clocks, trail, ride, sell-into-buy, scale-out do not beat it on both halves inside the tail bar | X = +40 % / -30 % / 17 s | `mt_d8_x.py` | - |
| 9 / 12 3b | P, R, S, then re-read on the final pool | P: public sells >= 1 SOL in 30 s <= 4, prints in 5 s <= 25 (keep rule, above cut_noise). R: first entries +5.00 %/trade, 2nd +0.10 %, 3rd-4th -0.9 %. S repriced: 0.3 SOL +81, 0.5 SOL +87, 1.0 SOL negative (an upper bound). X re-walked: +80 % take profit wins fold 2 by 0.13 SOL against a 0.87 floor (not taken); no further P | R = 1 per coin; S stays 0.2 SOL. Every gate but the tail (21.5 %) | `mt_d8_p.py` | 1.1 |
| audit 3b | Does the book survive code that shares nothing with the study, on every lake day? | The fill model prices the state our 251 real buys met in 98.4 % at each fill's own lag (mean +0.36 % dearer at a flat 83 ms). With the table's coin floor the replay equals the study to the ticket; without it every window is red: -3.15 / -3.35 / -3.14 %/trade, 0/6, 0/5, 0/3. The fixed toolkit agrees with the replay to the ticket (616 of 616 on 6,000 sampled coins) | Rule 3b is red on every fire. Its E, D, P and X were read on survivors, on first positions only and from age 1 s. By age at the fire: 1-3 s -3.15 %, 3-10 s -5.28 % (47 % of fires), 10-30 s -3.17 %, 30-300 s -1.3 .. -1.8 %; after 10 s, 33-41 % of fires still sit on coins that end short. Next: one sentence on all its buys (5.1 names clip_step_up on both), age >= 10 s, from 6.1 on the fixed toolkit | `mt_d8_fillcheck.py`, `mt_d8_replay.py` | 1.1, 7 |

### Tried and out

| idea | result | why it is out |
| --- | --- | --- |
| last-leg study tape as the book | 3Xk2Eu 0 episodes | that grain does not carry its prints |
| copy 8dtx2t's fill | FOLLOW -7.22 0/6 | the fill is in the price |
| drop a negative median at pick | 8dtx2t median -1.65 %, total +26.78, >20 % losses 0.8 % | that is a harvester who cuts, not a tail |
| fire at buy minus L | uses its later print as the clock | not a public event (derive 5.1 is the trigger) |
| follow burst start at 115 ms | behind cost 5.20 %, peak +0.86 % | 5.2; he reacts at 25-50 ms |
| follow buy>=0.5 / up>=3 % / buy>=1 | behind cost 4.46 / 4.55 / 5.34 % | the same burst print |
| follow a sell | lift 0.13-0.88 | he avoids sells |
| rising edge nb2>=3 / nbig / nb5 | behind cost 5.14-6.70 % | 5.2; the crossing is the burst |
| follow quiet restart (burst AND NOT up>=3 %) | behind cost 4.23 %, peak +0.08 % | 5.2; the rest of the burst lands in 115 ms |
| follow continuation (size/up3 AND NOT burst) | behind cost 4.48 %, peak +1.92 % | 5.2; same burst, later print |
| follow loud restart (burst AND up>=3 %) | behind cost 6.11 %, peak +1.07 % | 5.2; the overlapping move |
| drop 8dtx2t because top 1 % is 56.4 % of his net | body +11.67, >20 % losses 0.8 % | his close is a cut; the kill at our seat is DELAY |
| follow 88887Q sell>=1 / down>=2 % | behind cost 6.99 / 7.29 % | 5.2; the sell is the move |
| follow 88887Q seller_loss / nonce / tool | behind cost 7.53 / 7.04 / 7.10 % | 5.2; WHO/history of the same sell |
| AND-filter 88887Q's sell family | 88-98 % of behind tickets already cost >= 2 | derive: most tickets already dead, do not AND |
| 88887Q held-state rising nb2 / npro2 | behind cost 7.74 / 6.99 %, cover ~12 % | 5.2; not a second tell |
| follow 3Xk2Eu burst_start / up>=3 % / buy>=0.5 | behind cost 11.72 / 6.54 / 6.16 % | 5.2; same burst family as 8dtx2t |
| follow 3Xk2Eu clip_step_up / structure_burst / tool / alone_in_slot | behind cost 6.65 / 7.26 / 7.04 / 7.36 % | 5.2; WHO/history of the same burst |
| follow 3Xk2Eu seed_racer | behind cost 7.24 %, acted dt 9 ms | a race diagnostic (derive: if it follows that print, it is a race) |
| exclusive quiet / continuation / loud on 3Xk2Eu | behind cost 6.40 / 6.33 / 15.49 % | 5.2; 89-99 % already cost >= 2 |
| AND-filter 3Xk2Eu's burst family | 86-100 % of behind tickets already cost >= 2 | derive: most tickets already dead, do not AND |
| 3Xk2Eu held-state rising nb2 / nbig | behind cost 10.99-14.78 % | 5.2; not a second tell |
| pick ApfmkS as next | VolAcc 7.7 %, TransferChecked 40.8 % of prints; unique in the 26. Tape share 0.45 % SOL. 5.1 flat | volume manufacture, not a reader; derive pick |
| follow 8aaRWu loud restart | behind cost 2.37 %, peak leftover +7.12 % | not a drop: cost is not a kill when leftover remains |
| pick 8aaRWu seed_racer as E | lift 4.33, cover 15.3 %, behind cost 1.56 % | a race diagnostic (derive: if it follows that print, it is a race) |
| AND the five identity classes as E | 5 flags on 0.9 % of fire prints; sequential 6.1 AND is 6.8 % of acted / 52 coins | a rare corner, not its logic (derive 6.1) |
| pick 8aaRWu structure_burst as the 6.1 term | 28.9 % of acted vs 27.1 % of ignored among family prints | 5.1 lift is vs all public prints; among the family it does not separate |
| pick 8aaRWu tool as the 6.1 term | 61.3 % vs 62.1 % | ballast in the union, rank 0.50 |
| pick buy_after_sells only as E | cover 3.1 % of buys | a corner of the family, not a drop of the class |
| freeze 8aaRWu occupancy as E | none -2.97 % 0/6, best term mvk -1.51 % 0/6, age p50 456 s vs his 89 s | occupancy takes first family prints on old coins, not the fires it takes (derive 6.2) |
| AND the 6.1 terms as E | sequential occupancy -1.34 % 0/6, cover 22.3 % of acted; leftover AND cover 22.9 % | a rare corner, not its logic; sell_run does not lift the stack |
| skip 8aaRWu D because occupancy is red | occupancy -2.97 % 0/6, acted-only +0.88 % 4/6 | positive on some coins, negative overall is 7.1 (derive 10); D is next, not phase 8 |
| jump to phase 8 on 8aaRWu's acted pool | leftover green, occupancy red, D unread | walk D then X then P on the spelled E (derive 6.2, 7) |
| split 8aaRWu first-on-mint vs re-entry as two E's | 84.6 % first, 15.4 % re-entry; 5.1 on all 707 episodes, family cover 95.2 % | E is this print; re-entry is R. Split only if 5.1 names a different print (derive 3) |
| hunt leftover at age < 10 s | sniper-eating rugs die there; 8aaRWu age p50 89 s | frame / P, not E. Age < 1 s is the fill-model hole (strategy 8.1) |
| fit 8aaRWu thresholds on study_exact past 09-06 12:00 | study_exact prints run to 09-06 24:00; holdout_exact fires start 09-06 12:00 | study fires stop where the holdout starts (derive 2.1). The 6.2 numbers sit on that overlap and on the old candidate floor; re-read before they count |

---

## 3. The member book

Every-leg study (`study_exact`, 6.00 days). His columns are our 0.2 SOL clip through its
entry reserve and its close. RACE / FOLLOW are a 15 s clock at those seats. `loss>20` is the
share of tickets at <= -20 % of clip.

| member | ep/day | hold p10/p50/p90 | age p50 | his med | his tot | his >20% | RACE tot | RACE 15s >20% | FOLLOW tot | status |
| --- | ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 8dtx2t | 424 | 5.7 / 16.6 / 91.2 | 157 s | -1.65 % | +26.78 | **0.8 %** | +16.34 6/6 | 5.5 % | -7.22 0/6 | **rule 3b** (first position): red on every fire, every lake day (section 1b) |
| ApfmkS | 66 | 2.8 / 7.1 / 33.8 | 204 s | +4.31 % | +3.78 | 6.3 % | +1.97 6/6 | 6.2 % | -1.75 1/6 | **drop**: volume manufacture (VolAcc + TransferChecked), not a reader |
| 9Uq8GV | 136 | 9.7 / 16.0 / 16.2 | 173 s | -0.82 % | +4.31 | 7.7 % | +4.22 6/6 | 9.8 % | -1.93 2/6 | candidate; hold is a 16 s clock |
| 88887Q | 765 | 2.4 / 22.6 / 78.9 | 42 s | +3.26 % | +64.56 | 9.3 % | +52.94 6/6 | 9.1 % | -16.31 0/6 | leftover exists on sell/down; not the live instrument (cost 6-7 %) |
| 9999hu | 591 | 4.1 / 25.3 / 73.5 | 18 s | +0.17 % | +47.99 | 17.9 % | +42.19 6/6 | 16.6 % | -19.59 0/6 | candidate (younger band, loose cut) |
| 8aaRWu | 117 | 2.8 / 25.6 / 109.3 | 89 s | -0.09 % | +3.39 | 19.8 % | +0.80 3/6 | 11.9 % | -1.53 2/6 | **instrument**: 6.2 occupancy red; 7.1 next |
| 3Xk2Eu | 166 | 12.3 / 50.0 / 213.6 | 51 s | -4.28 % | +13.95 | 22.0 % | +8.35 6/6 | 11.5 % | -7.10 1/6 | leftover exists on burst family; not the live instrument (cost 5-15 %) |

8dtx2t's own close takes 0.8 % large losses; the same entries under a 15 s clock take 5.5 %.
The cut is in X, not only in E.

---

## 4. Data and code

| where | what |
| --- | --- |
| [mid-tape/README.md](mid-tape/README.md) | every script, by step |
| `node-derivation/data/mt_p34_exact_book.csv` | phase 3-4 book, every member, every-leg study |
| `node-derivation/data/mt_p51_8dtx2t_lift.csv` | 5.1 excess intensity |
| `node-derivation/data/mt_p51_8dtx2t_gate.csv` | 5.2 leftover on named prints and rising edges |
| `node-derivation/data/mt_p51_88887Q_lift.csv` | 5.1 excess intensity, WHO / history / priced |
| `node-derivation/data/mt_p51_88887Q_gate.csv` | 5.2 leftover on those prints and rising edges |
| `node-derivation/data/mt_p51_3Xk2Eu_lift.csv` | 5.1 excess intensity, WHO / history / priced |
| `node-derivation/data/mt_p51_3Xk2Eu_gate.csv` | 5.2 leftover on those prints, exclusive splits, rising edges |
| `node-derivation/data/mt_p51_8aaRWu_lift.csv` | 5.1 excess intensity, WHO / history / priced |
| `node-derivation/data/mt_p51_8aaRWu_gate.csv` | 5.2 leftover on those prints and exclusive splits |
| `node-derivation/data/mt_p61_8aaRWu_gate.csv` | exclusive leftover on the identity family |
| `node-derivation/data/mt_p61_8aaRWu_rank.csv` | 6.1 within-coin rank, acted vs ignored family prints |
| `node-derivation/data/mt_p62_8aaRWu_left.csv` | leftover on 6.1 terms and their AND |
| `node-derivation/data/mt_p62_8aaRWu_book.csv` | 6.2 occupancy, each term alone, clock 26 |
| `node-derivation/data/mt_p62_8aaRWu_seq.csv` | 6.2 sequential add (not the event) |
| `node-derivation/data/mt_p62_8aaRWu_ctrl.csv` | 6.2 opposite-side control (family sells) |
| `node-derivation/data/mt_p62_8aaRWu_acted.csv` | 6.2 acted-only occupancy (not a sentence) |
| `node-derivation/data/mt_volmaker_tape.csv` | tape share / wash / clips on the 26 |
| `node-derivation/data/mt_volmaker_ix.csv` | instruction mix on the 26: VolAcc / 6Vo / TransferChecked |
| `node-derivation/data/mt_d8_e_spell_buy.parquet` | rule 3b's candidate table: every clip_step_up print on every coin under loose floors, 83 ms |
| `node-derivation/data/mt_d8_p_table.parquet` | rule 3b's E + D rows booked under its X, with the door facts |
| `node-derivation/data/mt_d8_replay_b*.parquet` | the replay: every E print on the lake from 09-01 with every spelling of each term, and the exits of each coin's first fire |
| `node-derivation/data/mt_d8_replay_tickets.parquet` | rule 3b's tickets on every lake day, the engine spelling |
| `node-derivation/data/mt_d8_fillcheck.csv` | our 251 real buys: the state they met against the fill model |

## Open

The open items are in [_!___workflow.md](../_!___workflow.md) section 2, the one queue.
