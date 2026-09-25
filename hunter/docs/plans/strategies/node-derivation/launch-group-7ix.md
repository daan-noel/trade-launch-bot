# Launch-group: 7ix Create_v2 + ExtendAccount + Buy, CU price 1000

D is a creation instruction sequence, and the subject is the crew behind it: the coins rise for
about half a minute, hold a narrow band, then the crew dumps in one slot or the coin migrates.

```
CU Limit, CU Price, Transfer, Create_v2, ExtendAccount, ATA CreateIdempotent, Buy
```

Tapes: lake tokens 07-22 .. 09-22 (2,829 coins on the sequence, 2,814 at CU price 1000). Trades:
lake 09-01 .. 09-21 for coins born to 09-19, Postgres for coins born 09-20 .. 09-23 16:00 (183
coins; exact parity with the lake on 40 overlap coins). 1,145 coins in all. Study 09-01 .. 09-12,
holdout 09-13 .. 09-23 (the wide split, `STUDY2_END`). Fill: kernel, 115 ms, clip **0.03 SOL** (the
real-test size, fixed leg cost about 1.5 % a round trip).

Scripts (local, `launch-group-7ix/`): `g7_extract.py`, `g7_portrait.py`, `g7_dump.py`,
`g7_tool.py`, `g7_book.py`, `g7_tell.py`, `g7_tp.py`, `g7_door.py`, `g7_pg.py`, `g7_load.py`
(the combined tape and the crew waves), `g7_waves.py`, `g7_tools2.py`, `g7_hyp.py`, `g7_hazard.py`,
`g7_burst.py`, `g7_pnl.py`, `g7_xpnl.py`, `g7_roll.py`, `g7_studybook.py`; the audit: `g7_replay.py`
(independent: imports no study code), `g7_audit2.py`, `g7_giveup.py`, `g7_x2.py`, `g7_entry.py`,
`g7_final.py`, `g7_dumpix.py`, `g7_hybrid.py`; the D/P/E/X search: `g7_all.py`, `g7_tier3.py` (+ `b`,
`c`), `g7_crewix.py`, `g7_axiom.py`, `g7_axiom2.py`, `g7_facts.py`, `g7_single.py`, `g7_combo.py`, `g7_hold.py`; the final pass: `g7_cache.py`, `g7_ride.py`, `g7_x3.py`, `g7_late.py`,
`g7_ride2.py`, `g7_x4.py`, `g7_single2.py`, `g7_final2.py`; the leak pass: `g7_rule.py` (the rule in
one module), `g7_leakA.py`, `g7_sigwin.py`, `g7_leakA2.py`, `g7_birth.py`, `g7_retune.py`; candidate 4's audit: `g7_book4.py` (the study
ticket list), `g7_audit4.py` (independent replay); engine parity: `g7_engine_ref.py` (the rules as the engine
spells them), `g7_engine_sim.py` (lab simulate driver), `g7_parity.py`.
Tables in `data/g7_*.parquet`.

---

## 0. Checklist - read this first, every session

| step | state | where |
| --- | --- | --- |
| 1 Pick | **done** | the user's pick: 7ix at CU price 1000, split by max_cost |
| 2 Portrait | **done for the coins and the crew** | chain 1-3 |
| 3 Their exit | **done for 0.13 / 0.65 / 4.16** | chain 8-12: the crew takes profit on its own paper PnL; 1.3 and 6.5 dump at a loss |
| 4 Test each clause | partly | chain 5-7, 13-16 |
| 5 Our version | **one candidate** | section 1 |
| 6 Fit D, P, X | **partly** | X fitted (chain 14); E per split (chain 15); P not read |
| 7 Coverage | this file, section 3 | |
| 8 Prove | **audit partly run** | section 4: V1, V3, V4, U1, F1, F3, F4, F7 pass; F2 and T6 open |
| 9 Record | this file | |

---

## 1. Sentence

```
E  0.13 / 4.16: first print at age >= 1 s. 0.65: first print after the creator's first sell, age >= 1 s
P  none
X  out when crew profit >= target, or crew profit >= 0.6 x target AND outsider buys over the last
   3 s >= 1 SOL; else a 120 s clock
D  7ix, CU price 1000, max_cost in {0.13, 0.65, 4.16} SOL
R  one open per coin
S  0.03 SOL
seat  both legs fill at the last print landed 115 ms after the decision print
```

The crew is every print whose ix list carries program `9ddjzqYhSTMHaBrrKukRXRfy4WzHUPjdX88uPXZ7MXyn`,
plus the create signer. Crew profit = what its tokens return if sold into the curve now, minus
its net SOL spent (taken profit included). Outsider = neither. The target is the split's median
crew profit at its first sell wave on 09-01 .. 09-12: 0.13 = **0.83 SOL**, 0.65 = **1.18**,
4.16 = **4.96**. f = 0.6 and X = 1 SOL are the best study cell.

Plain words: the crew sells once its profit is good enough and outside money arrives to sell
into; we sell when that moment is reached.

### The book (independent replay, `g7_final.py`)

| | study 09-01 .. 12 | holdout 09-13 .. 23 | whole tape |
| --- | ---: | ---: | ---: |
| trades | 490 | 210 | 700, 32 a day |
| %/trade at 115 ms | +10.40 | +12.81 | +11.11 |
| 0.13 / 0.65 / 4.16 | | | +10.2 / +10.9 / +12.4 |
| days positive | | | **20/22**, worst day -2.8 % |
| SOL at 0.03 | | | **+2.33**, +0.106 a day |
| win / median | | | 49 % / -0.26 % |
| top 1 % share / biggest coin | | | 0.15 / 0.02 |
| coin bootstrap 90 % | | | +8.3 .. +14.1 %/trade |
| lag 50 / 83 / 200 / 300 / 400 ms | | | +11.5 / +11.2 / +10.9 / +11.0 / +10.8 |

Incumbents on the same coins: crew target alone (chain 12) +7.18 %/trade, 16/22 days;
tp 10 % / clock 20 s about +2 %/trade.

---

### Candidate 4 - the current best (chain 28-32)

Crew = Tier 1 | 2 | 3 as in chain 17. A wallet that buys in the creation slot and is not crew is
**neither crew nor outsider** (the creator's birth bundle or a sniper - not the audience).

```
E  first print at age >= 1 s, 0.03 SOL
X  SIGNAL = crew profit >= target, or >= 0.6 x target with outsider buys >= 1 SOL over 3 s
   targets (09-01 .. 12): 0.13 0.82, 0.65 1.16, 4.16 4.84 SOL
   signal before age 20 s: sell                         a quick spike: the crew is cashing out
   signal at age >= 20 s: ride                          outside money is carrying the coin
   riding, first 30 s: outsider buys >= 2 SOL over 10 s: sell     sell into the burst the crew dumps into
   any time: vsol >= 110: sell                          the curve is about to close
   any time: a dump-instruction sell: sell              the crew is out
   any time: 1,000 s after entry: sell                  p95 of the time to graduation
   age 10 s, before any signal: outsider sells >= 1.3 SOL: sell    early buyers are flipping
FINAL door  max_cost 0.13 / 0.65 / 4.16; the name was used by an earlier 7ix coin; <= 6 prints by 1 s
BROAD door  max_cost 0.13 / 0.65 / 4.16; vsol >= 44 at the entry print
```

| | FINAL | BROAD |
| --- | ---: | ---: |
| trades | 267 | 711 |
| %/trade study / recent | **+40.56 / +46.71** | +20.40 / +23.28 |
| days positive / worst day | 18/22 / -27.2 % | **22/22** / +0.1 % |
| SOL at 0.03 | +3.47 | **+4.54** |

Audited by an independent replay (section 5); the doors were chosen on all 22 days and no day
after 09-24 is read, so the forward test is the open line.

### Candidate 3 (chain 22-27)

```
D  7ix, CU price 1000, max_cost in {0.13, 0.65, 4.16}; the name was used by an earlier 7ix coin
P  <= 6 prints on the coin by age 1 s
E  first print at age >= 1 s
X  SIGNAL = crew profit >= target, or >= 0.6 x target with outsider buys >= 1 SOL over 3 s
   signal before age 20 s: sell
   signal at age >= 20 s: ride. Riding: sell at vsol >= 110, on the first dump-instruction sell,
     in the first 30 s of the ride on outsider buys >= 3 SOL over 10 s, or at 1,000 s
   at age 10 s, before any signal: outsiders have sold >= 1.3 SOL -> sell
   no signal by 300 s after entry: sell
S  0.03 SOL
```

| | value |
| --- | --- |
| trades | 267, 12 a day |
| %/trade study 09-01 .. 12 / recent 09-13 .. 24 | **+41.7 / +42.0** |
| days positive / worst day | 19/22 / -12.5 % |
| SOL at 0.03 | **+3.35** |
| win / median | 35 % / -3.5 % |
| top 1 % share / biggest coin / coin bootstrap 90 % | 0.09 / 0.05 / +27.5 .. +57.2 % |
| exits | top 29 at +429 %; ride dump 78 at -34 %; early signal 141 at -6 %; guard 12 at +113 % |

The broad version, door3 + vsol >= 44 at the fire and the same X: 711 trades, +18.5 / +24.9, 21/22
days, worst day -0.2 %, +4.37 SOL, top 1 % share 0.23 (over the 0.15 bar). Study-level numbers:
the gates were chosen on all 22 days and the replay audit of section 4 covers candidate 1 only.

### Candidate 2 (the D/P/E/X search, chain 17-21)

```
D  7ix, CU price 1000, max_cost in {0.13, 0.65, 4.16}, and >= 1 other 7ix birth in the last 10 min
E  first print at age >= 1 s with vsol >= 44
P  (the vsol gate above)
X  crew profit >= target, or >= 0.6 x target with outsider buys >= 1 SOL over 3 s. At that moment,
   if outsider buys so far >= 20 SOL: hold until the first dump-instruction print, vsol >= 110, or
   300 s. Else sell. Clock 120 s.
```

Crew = Tier 1 (9ddjzq) | Tier 2 (creator; a wallet from its first 9ddjzq print on the coin) | Tier 3
(>= 3 same-slot prints, same structure, side, CU limit, CU price, tip, SOL within 10 %). Targets
re-fitted on 09-01 .. 12 under this split: 0.13 0.82, 0.65 1.16, 4.16 4.84 SOL.

| | study 09-01 .. 12 | recent 09-13 .. 24 | whole |
| --- | ---: | ---: | ---: |
| candidate 1 on the new tape (all door3 coins) | +9.18 | +6.82 | 18/22 days, +1.83 SOL |
| + concurrency >= 1, vsol >= 44 at 1 s | +10.77 | +12.93 | 19/22, +1.98 SOL |
| **+ the hold branch** | **+10.62** | **+21.64** | **19/22, +2.40 SOL at 0.03**, 578 trades |

Study-level numbers, not results: the recent days chose the hold threshold (the user accepts a
recent-week fit), and the replay audit of section 4 covers candidate 1 only.

---

## 2. The chain

| step | question | what it shows | what it does to the rule | script |
| --- | --- | --- | --- | --- |
| 1 max_cost | What is the max_cost split? | max_cost = the creator's opening buy x 1.316 on every split (buy plus 31.6 % slippage): 0.13 = 0.10 SOL, 0.65 = 0.49, 1.3 = 0.99, 4.16 = 3.16, 6.5 = 4.94. | The split is the size of the creator's own bag, a behaviour, not a fee knob. It is known in the create transaction, at age 0. | g7_extract.py |
| 2 portrait | How do the coins run, per split? | Median life 290-370 s. The pool is 46-66 SOL at age 10 s, peaks at 52-81 SOL at age 30-39 s, and is back at about 31 by 60 s. Migrate: 0.13 **24 %**, 4.16 14 %, 0.65 10 %, 1.3 5 %, 6.5 1 %. Peak to dump gap p50: 0.65 **0.9 s**, 4.16 0.9 s, 0.13 1.3 s, 1.3 4.3 s, 6.5 **19.1 s**. | The "rise, hold, dump" shape runs on a half-minute clock, not minutes. The user's "6.5 swings wider" reads as a longer gap between peak and dump. | g7_portrait.py, g7_dump.py |
| 3 the crew | Who dumps? | The dump slot carries 18-27 sellers besides the dev, 99 % of its SOL sent through one program, `9ddjzqYhSTMHaBrrKukRXRfy4WzHUPjdX88uPXZ7MXyn`. On 09-10..12 that program touches 190 coins, 174 of them 7ix. On a coin, its wallets buy from age 0.1 s, spend a median 35 SOL before the dump against 15 SOL from everyone else, and sell continuously while they buy. | The crew is visible live by its ix structure, with no wallet identity. The rise is mostly the crew's own money; its profit is the outsiders' money. | g7_dump.py, g7_tool.py |
| 4 the dump moment | What is true just before the dump? | At the last print before the dump slot: price at 99 % of its running peak, outsider buys over the last 3 s 0.48 SOL (0.12 at earlier prints), 0.57 s since the last outsider buy. The dump age spreads from 8 s (p10) to 334 s (p90). | The crew dumps INTO live buying at the peak, in one slot. A trail or a "buyers stop" cut fills after the crash; the exit must act before it. Chain 9-11 read what sets the moment. | g7_tell.py |
| 5 ceiling and waiting | What do a perfect exit and no exit earn? | Entry at age >= 1 s. Leaving at the last print before the dump: **+80 %/trade** mean, 34 % median, 20/20 days (look-ahead ceiling). Sitting through the dump: -12 % mean, **-52 %** median. Clocks 5-30 s: -1.3 .. -7.1 % mean, +1 .. +12 % median. Crew net flow over the last 1-5 s <= 0 as the exit: -1.8 .. -3.4 %. | Almost all the value is in leaving before the dump. The crew churns buy and sell, so its net flow flips long before the dump and cuts too early. | g7_book.py |
| 6 take profit | Does a small target, sized from the reachable gain, beat the dump? | Reachable gain before the dump at our fill: p25 16 %, p50 44 %. TP 10-15 % with a 10-20 s clock is about 0 %/trade pooled (-0.3 .. +0.5), and splits cleanly by max_cost (section 1). | The pooled group is flat; the split carries the sign. | g7_tp.py, g7_door.py |
| 7 why 0.65 differs | Why is 0.65 red when its shape looks like 0.13 and 4.16? | The creator's first sell lands at p50 **1.6 s** on 0.65, 3.9 s on 0.13, **30.6 s** on 4.16. | A creator who sells in the first seconds hits our entry window. The creator's own first sell is the next door or permission to read. | g7_door.py |
| 8 recent tape | Does the crew still use its program on the newest days? | Coins born 09-20 .. 09-23 come from Postgres. The dump slot's SOL through `9ddjzq` is **96.9 %** on 09-01 .. 12 and **96.2 %** on 09-13 .. 23, always its sell instruction `ix#5d583c225b1256c5`. max_cost switches by day: 0.65 fills 09-04 .. 09-12, 0.13 and 4.16 come in bursts. | The crew tag holds on every day. A split is partly a day regime, so every split is read on both periods. | g7_pg.py, g7_tools2.py |
| 9 fixed rules | Is the first sell wave (>= 3 SOL from >= 5 crew wallets in one slot) set by a timer, a price, outsider money, or a crew budget? | At the wave: age IQR/median 1.4-2.6, vsol 0.25-0.39, outsider net SOL in 0.9-38, crew spend 1.4-2.8. First crossing of each q10 / q25 level fires the wave within 5 s on at most 19 % of coins; the pool reaches its wave level a median 25-60 s early and holds. | No timer, price level, budget or outsider-money rule. The band is the crew holding a level while it waits. | g7_hyp.py |
| 10 hazard | What is true in the second before the wave that is not true during the band? | Base rate 1.13 % per print. Outsider buys over the last 3 s >= 1 SOL: lift **3.7-4.1**; none for > 2 s: 0.38. As a trigger alone: recall <= 61 %, precision 4-31 %; 0.65 and 4.16 answer a burst (precision 0.22-0.31, reaction p50 3-4 s), 0.13, 1.3 and 6.5 barely (0.03-0.14). | The crew sells into outside money, but a burst is a helper, not the rule. | g7_hazard.py, g7_burst.py |
| 11 crew profit | Does the crew sell on its own paper profit? | Crew profit at the wave, p10 / p50 / p75 SOL: **4.16 3.95 / 4.90 / 6.32** (IQR/median 0.43), 0.13 0.48 / 1.01 / 1.94, 0.65 0.74 / 1.09 / 1.94; the fall from its running max is ~0 on all three. 1.3 median 0.02, 6.5 -2.24, others -0.16, with a large fall from the max. | 0.13, 0.65 and 4.16 take profit at a target (4.16 the tightest); 1.3 and 6.5 sell at a loss when the coin fails. Two dev logics, split by max_cost. | g7_pnl.py |
| 12 our exit | Does selling at the crew's target beat the incumbent? | Section 1. 1.3 is -14.5 / -24.2 and 6.5 -18.4 / -3.1 under the same exit. | X = the crew's target, on the three take-profit splits only. | g7_xpnl.py, g7_roll.py |
| 13 replay | Does an independent replay rebuild the chain-12 book? | `g7_replay.py` reads the lake and Postgres itself: 704 of 704 tickets, max difference **0.000000 pp**. Exact vtok pricing with the .env fixed costs, the clock at entry + 120 s + lag, lag 50-800 ms, entry age 1.5-3 s: +6.09 .. +7.54 %/trade, 16/22 days on every row. | The chain-12 book is what the rule does. | g7_replay.py |
| 13b when the target fires | Does the target exit land before the crew's dump? | Target reached before the first wave: 349 trades, **+40.0 %**. Wave first, so the exit lands after the crash: 197 trades, **-60.7 %**. No wave 143, +13.1 %. Clock 15, +78 %. | Two thirds of the wave coins pay; the rest are the crew dumping below its median target. | g7_audit2.py |
| 14 below-target dumps | When the crew dumps below target, is it giving up? | Those dumps land early (age p50 14-19 s against 20-55 s), at 0.68-0.84 of the target, with crew profit at its running high (stall p50 0.8 s). Hazard: a stall > 10 s has lift 0.05-0.21; an outsider burst >= 1 SOL in 3 s has lift 3.7-4.1 (chain 10). Exit "target, or f x target with outsider buys >= X over 3 s": f 0.6, X 1 SOL is the best study cell, +9.44 study / +11.34 holdout, 19/22 days; every f 0.6-0.9 at X 1 SOL holds +11.3 .. +12.2 on the holdout. | The crew sells when profit is good enough AND outside money arrives to sell into. X gains the burst clause. | g7_giveup.py, g7_x2.py |
| 15 entry | Does waiting for the creator's first sell help? | Under chain-14 X: 0.65 +7.1 / +15.7 -> **+8.6 / +19.2** (study / holdout); 0.13 +13.5 / +4.7 -> +6.6 / +1.5; 4.16 has almost no creator sell before the wave (9 of 159 coins). | E is split-specific: 0.65 waits for the creator's sell, 0.13 and 4.16 do not. | g7_entry.py |
| 16 combined | The whole sentence | Section 1. | | g7_final.py |
| 17 crew split | Which prints are crew beyond the 9ddjzq program? | Tier 1 is 63 % of all prints, Tier 2 adds 2.2 %. On the 35 % left, the user's volume-cluster test (>= 3 same-slot prints, same structure, side, fees, SOL within 10 %) tags 0.7 % of known-retail prints and catches 6 % of known-crew ones; on Axiom it is 87 % known crew. Axiom ix#00 is public (554k tape prints in 3 days): 4,671 of 6,805 Axiom wallets on 7ix touch one coin; a 9-wallet cluster touches 145-148 coins each. Wallets concentrated on 7ix carry 5.6 % of the leftover SOL, and can be other hunters of this group as much as crew. | Crew = Tier 1 + 2 + 3; what it misses is about 5 % of outsider SOL. | g7_tier3.py, g7_crewix.py, g7_axiom.py |
| 18 labels and facts | What does each coin do, and what is known at birth and at 1 / 3 / 5 / 10 s? | 1,191 coins 09-01 .. 09-24 (Postgres to 09-24 17:24). Early-dump rate 18.8 %, migrate 10.1 %. Under candidate 1's X at 1 s: 0.13 +9.6, 0.65 +6.7, 4.16 +12.3, 1.3 -13.7, 6.5 -6.2, rest -7.0 %/trade. | | g7_facts.py |
| 19 single facts | Which facts separate winners from losers on both periods? | vsol at 1-10 s: the bottom fifth (< ~44 SOL at 1-5 s) books -8 .. -31 %, 53-68 % big losses, no migrations; 211 such coins on recent days against 28 on the study days. Other 7ix births in the last 10 min: none -12.1 % recent / -6.5 % study, four or more +13.4 / +7.4. Crew sells > 10.9 SOL by 10 s: -12.6 / -7.7. Outsider sells > 1.3 SOL by 10 s: -8.8 / -24.5. | The crew commits to a coin in its first seconds or not at all, and it runs the coins it launches in a session; recent days carry more uncommitted launches. | g7_single.py |
| 20 combined | Which D / P / E hold on both periods? | Door3 with concurrency >= 1 and vsol >= 44 at 1 s: section 2 candidate 2. Entry at 3 or 5 s is no better on the worse period; no gate turns 1.3, 6.5 or the rest positive. | D gains concurrency, E a vsol gate. | g7_combo.py |
| 21 hold branch | Should a coin be ridden past the crew's signal? | At the signal, migrators and the rest are close (outsider buys p50 6.1 vs 5.9 SOL, vsol 61.5 vs 49.8). Holding when outsider buys >= 20 SOL: study +10.62 (from +10.77), recent **+21.64** (from +12.93); >= 30 SOL: +10.62 / +16.64, 20/22 days; vsol >= 75 at the signal: +11.83 / +12.97. A giveback cut of 3-6 SOL off the crew's profit high changes nothing. | X gains a hold branch keyed on outsider money already in. | g7_hold.py |
| 22 graduation | When do migrators graduate, and can we sell after it? | 120 of 1,188 coins reach vsol 110; the curve completes at 115.01. Time to graduation p10 / p50 / p90 / p95 / p99: 258 / 485 / 795 / 968 / 2,431 s. After completion 75 % of coins print nothing more on the curve. From first touching vsol 110 to completion: p50 76 s. | A ride sells at vsol 110, not at completion; a ride clock is 1,000 s (p95), never 300 s. | g7_cache.py |
| 23 ride hazard | After the crew signal, what precedes a crash? | Crash within 3 s, base 1.9 %: age < 30 s lift 3.96, > 300 s 0.31; outsider buys >= 1 SOL / 10 s lift 2.3-2.8; outsider SOL in >= 50 lift 0.31. At the signal, coins that reach 110 first signal later (age p50 34 s vs 7 s); outsider SOL in barely differs (6.2 vs 5.5). | Ride on a late signal, not on outsider money. | g7_ride.py |
| 24 ride exit | Ride on a late signal to vsol 110? | Door pool (door3, concurrency, vsol >= 44): signal only +10.77 / +12.93; ride when the signal comes at age >= 20 s, sell at 110 / dump print / clock 300 s: **+17.50 / +32.62**, 20/22 days, +3.80 SOL. Exits: 47 tops at +410 %, 150 ride dumps at -31 %, 375 early signals at -7.7 %. A silence cut and an outsider-SOL ride floor change nothing or hurt. | X rides late signals. | g7_x3.py |
| 25 late entry | Buy only coins that survive the spike window? | Entry at 20-30 s with no signal, dump print or crash yet: +32 .. +65 %/trade but 60 % big losses and +1.9 .. +2.8 SOL against +3.8, 15-17 days positive. | Rejected: fewer coins, a lottery shape. | g7_late.py |
| 26 ride guard | What precedes a ride's crash? | Inside rides (base 1.0 %): riding < 10 s lift 6.19, 10-30 s 2.11, > 300 s 0.06; crew profit at its ride high 2.23; outsider buys >= 3 SOL / 10 s 3.42. Selling in the first 30 s of a ride on outsider buys >= 3 SOL / 10 s: +19.68 / +33.43, 21/22 days, +4.11 SOL. | X gains the 30 s guard. | g7_ride2.py, g7_x4.py |
| 27 gates under the new X | Which facts choose the coins? | All coins, no gate: +11.97 / +6.23. Same sign on both periods, top vs bottom fifth: outsider sells by 10 s -41 / -48; birth hour 21-24 UTC +34 / +29 against 11-15 UTC -9 / +10; name used by an earlier 7ix coin +21 / +26; prints by 1 s (fewer is better) -34 / -12; last 5 coins' result +26 / +17; concurrency +15 / +30. Best by the worse period: door3 + <= 6 prints by 1 s + reused name (candidate 3). The 10 s outsider-sell cut moves the book by < 0.4 points; the crew-sell cut costs 2-3 points. | Candidate 3. | g7_single2.py, g7_final2.py |
| 28 no-signal exit | Is the 300 s no-signal clock measured? | No: a 120 / 300 grid picked it. Replacing it with the ride exits (vsol 110, dump print, 1,000 s): FINAL +42.21 / +39.64 against +41.7 / +42.0, +3.29 SOL against +3.35; BROAD +18.84 / +23.62 against +18.5 / +24.9. With the dump print read before the signal too, the early-signal branch is +2.5 % (FINAL) and +3.1 % (BROAD): its -6 % was the pre-signal dumps. | The no-signal clock goes; every exit carries a reason. | g7_rule.py |
| 29 pre-signal dumps | What precedes the crew dumping before our signal? | FINAL 41, BROAD 130 trades at -52 / -58 %; they land a median 15.5 s after our fill, a quarter within 9 s. Sampled once a second (BROAD, base 0.97 %), lift on both periods: crew profit 0.8-1.0 x target 1.56 / 1.36; outsider buys 1-3 SOL over 10 s 2.17 / 3.18; age 10-40 s about 2. Crew sell and buy flows are not consistent. The signal re-spelt with a 5 or 10 s burst or at 0.7-0.8 x target catches more of them (FINAL 41 -> 14) and loses more (+3.29 -> +0.88 SOL): the earlier signal sells runners as quick spikes. No fact at birth or at 1 s marks them on both periods by more than 0.11 of rate. | Unfixed: before the crew acts, its early dump and a runner's start read the same on the tape. The current signal stays. | g7_leakA.py, g7_sigwin.py, g7_leakA2.py |
| 30 creation-slot buyers | Are the creation-slot buyers outsiders? | 2-3 non-crew wallets per coin buy in the creation slot: SOL p50 0.6-1.8 (**15.8 on 4.16**), 20-40 % of them sell in the crew's dump slot, 10-45 % of what the split called outsider buys. | A spelling error in the split: they are not the audience. | g7_birth.py |
| 31 corrected split | Counted as crew, or as neither? | Under the unchanged rule: as crew FINAL +35.52 / +46.46, 16/22, +3.24 SOL, BROAD +18.86 / +26.22, +4.51; neutral FINAL +41.55 / +45.25, 16/22, +3.46, BROAD +20.67 / +22.25, 20/22, +4.51. | Neutral: they are the creator's bundle or snipers, never the audience; the thresholds are re-measured under it. | g7_birth.py |
| 32 re-tune | Which signal, ride and guard values under the corrected split? | At the crew's first wave (study, door3): crew profit / target p10 / p50 0.65 / 1.00, outsider buys over 3 s p50 0.64 SOL. Grid f 0.5-0.7, burst 0.3-1.0, ride line 15-30 s, guard 2-3 SOL: burst 1.0 leads on both doors (0.3-0.5 fire too early); f 0.5 and 0.6 differ by < 1 point; ride line 20 s leads; guard 2 SOL lifts the day count on both doors (FINAL 16 -> 18/22, BROAD 20 -> 22/22). | Candidate 4. | g7_retune.py |

---

## 3. Coverage and the unread list

| family | state |
| --- | --- |
| D create fingerprint + max_cost split | tried: section 1 |
| D creator's opening buy | tried, as the max_cost split |
| E age, first print | tried: 0.5 / 1 / 2 / 5 s, 1 s best, 5 s loses about 8 points of ceiling |
| E crew's own buy as the trigger | not tried |
| P crew inventory, outsider flow, creator has sold | not tried as a permission |
| X clock, take profit | tried: section 2 steps 5-6 |
| X crew net flow turns | tried, red: the crew churns |
| X trail, buyers stop | not booked: the one-slot dump at the peak fills them after the crash (step 4) |
| X what sets the dump moment | read: crew profit target on 0.13 / 0.65 / 4.16 (chain 11); timer, price, budget, outsider money red as fixed rules (chain 9); an outsider burst is a helper (chain 10) |
| X the 1.3 / 6.5 crew's loss exit | not read: they sell with profit falling from its max |
| X crew profit falling from its max as an immediate cut | read as a hazard: a stall is the calm state, lift <= 0.21 (chain 14) |
| X outsider burst with the crew near target | tried: chain 14, in the sentence |
| E the creator's first sell | tried: chain 15, in the sentence for 0.65 |
| E crew's own buy as the trigger, P on the three splits | not read |
| X the creator's first sell as an immediate cut | not tried |
| R re-entry after a take profit | not tried |

The crew target, f and X are fitted on 09-01 .. 09-12 only and read on 09-13 .. 09-23. The split
choice {0.13, 0.65, 4.16} is the user's, named before any book.

---

## 4. Audit ([../backtest-audit.md](../backtest-audit.md))

| line | state |
| --- | --- |
| V1 replay | pass: `g7_replay.py` imports no study code and matches 704 / 704 tickets exactly |
| V2 terms against an exact field | pass: crew profit reads `token_amount`, `vtok` and `sol_amount` as stored, no float bag |
| V3 windows and days | pass: study, holdout and the per-day list in section 1 |
| V4 complete data | pass: lake print count equals Postgres on every day 09-01 .. 09-19 for these coins |
| U1 no coin kept on its future | pass: every coin born in the window. The life split is a diagnostic: < 60 prints -48.4 %, 60-200 -44.1 %, > 200 +44.7 % (chain-12 exit) |
| U4-U7 | pass: fires sit at age >= 1 s on coins born inside the tape |
| F1, F4, F6, F7 | pass: landed-state fills both legs, clock at its deadline, silence holds the last print, 125 bps + fixed costs + own impact on vsol |
| F2 seat on real fills | **open**: no real fill on this rule yet; the lag ladder 50-400 ms is beside the book |
| F3 age split | pass: entry age 1 / 1.5 / 2 / 3 s all green (chain 13) |
| F5 graduation | counted: 1 of 704 exits at the end of the curve |
| T1 no wallet in a term | pass: the crew is an ix structure plus the coin's own creator |
| T6 the terms exist in the engine | **open**: crew profit (paper profit of the prints carrying one program, plus the creator) and outsider buys over 3 s are not engine metrics |

---

## 5. Audit of candidate 4 (`g7_audit4.py`, imports no study code)

| line | state |
| --- | --- |
| V1 replay | pass: FINAL 267 / 267 and BROAD 711 / 711 tickets, max difference **0.000000 pp**; targets rebuilt 0.825 / 1.155 / 4.835 |
| V3 windows and days | pass: FINAL +40.56 / +46.71, 18/22 days, worst -27.2 %; BROAD +20.40 / +23.28, **22/22**, worst +0.1 %; per-day lists in the script output |
| T3 no look-ahead | pass: Tier 3 read causally (a print is crew once >= 3 matching prints have landed, median from the landed ones): FINAL +39.46 / +46.19, +3.40 SOL; BROAD +20.14 / +24.59, 22/22, +4.59 SOL |
| name reuse | pass: no coin of the universe has an empty name |
| F2 seat | lag ladder under the causal split: FINAL 50 / 83 / 200 / 300 / 400 ms +4.16 / +3.60 / +3.23 / +3.24 / +3.20 SOL; BROAD +5.76 / +4.95 / +4.24 / +4.13 / +4.03 SOL, 21-22/22 days on every row. **Open**: no real fill on this rule |
| F3 entry age | FINAL 1.5 / 2 s +3.37 / +3.34 SOL; BROAD +4.56 / +4.41 SOL |
| concentration | FINAL top 1 % 0.11, biggest coin 0.07, bootstrap 90 % +29.4 .. +59.1 %/trade; BROAD top 1 % **0.22** (over the 0.15 bar), biggest coin 0.03, +14.2 .. +29.0 |
| forward | **open**: the doors were chosen on 09-01 .. 09-24; the rule is re-read unchanged on days after 09-24 |
| T6 engine | **open**: crew tags (Tier 1-3, creation-slot neutral), crew profit against target, outsider buys and sells over 3 / 10 s, prints by 1 s, name reuse are not engine metrics |

---

## 6. Engine parity (candidate 4 in the engine, `roadmap/7ix-crew-rule-engine.md`)

The two rules run in the engine: 3 fingerprints per door (`7ix crew FINAL|BROAD <max_cost>`), each
rule an unsaved simulate draft, 09-01 .. 09-24, `lag_115`, 0.03 SOL, copycat guard off, no caps,
curve prints only. The reference is `g7_engine_ref.py`: the audited replay re-spelt as the engine
computes each term (head-program Tier 1, contagion from any tag, streaming Tier 3, identity name
reuse over a trailing 30 days, the engine's dead-pool exit, no entry while an exit clause holds,
cut10 dropped).

| | FINAL | BROAD |
| --- | ---: | ---: |
| engine tickets / reference tickets | 239 / 256 | 660 / 686 |
| reference-only coins | 17, all born 09-22 .. 09-23 | 26, all born 09-22 .. 09-23 |
| entry fill time equal (<= 0.012 s) | 239 / 239 | 660 / 660 |
| pnl in SOL equal within 0.000005 | 232 / 239 | 650 / 660 |
| engine %/trade, SOL (09-01 .. 09-21 coins) | +41.64, +3.008 | +21.83, +4.355 |

What each difference is:

- **Reference-only coins**: simulate reads trades from the lake, whose last day is 09-21; the
  reference reads Postgres for coins born 09-20 on. Data coverage, not logic.
- **0.000005 SOL on every ticket**: the account-close fee, which the engine charges and the
  reference leaves out. **The engine's %** is `pnl_sol / (0.03 + 0.000227)`, money over capital,
  so it reads 0.9927 x the reference's `pnl_sol / 0.03`.
- **Guard exits 0.04 .. 1.7 s earlier (15 tickets)**: when the ride latches on a print where the
  guard already holds, the engine sells on the next 200 ms tick; the reference re-reads only at
  the next print. The engine's reading is the live one.
- **One coin born 09-21** (`5Zu9...`): the lake's 09-21 is incomplete for it, so the engine never
  sees vsol 110 and holds to the 1,000 s clock.
- **Exit label only**: on the crew's dump print the crew's taken profit also makes the early
  signal true; the engine names the first clause (`time < 20`), the reference names the dump.
  Same print, same fill.

**A bug in the study, found by the parity check**: every rolling sum in the 7ix scripts was
`cum - cum[first in-window index]`, which drops the window's oldest print (outsider buys over
3 s read 0.747 where the window holds 1.014). The reference now sums the closed window; booked
that way candidate 4 reads FINAL +38.42 / +54.55, 18/22 days, +3.47 SOL and BROAD +19.78 /
+29.14, **22/22** days, +4.64 SOL. The audited book of section 5 predates the fix.
