# Launch-group: 6ix Create_v2 + BuyV2 + Transfer

D is a creation instruction sequence. The instrument is not a roster wallet. CU
limit/price is a split, not the key: the same bot can change those presets without
changing the instruction list.

Tapes: lake tokens 2026-07-22 .. 09-20 (1,522,740 coins). Life and peak age on
lake births 09-01 .. 09-20. Leftover on `study_exact`, fires before 09-06 12:00.
D1.2 creator history uses SEQ6_BUY life from 08-25. Lake trades hold 09-01 .. 09-20
(09-21 is not sealed).

Scripts: `launch-group/lg_id.py`, `lg_portrait.py`, `lg_peak.py`, `lg_left.py`,
`lg_nb.py`, `lg_crew.py`, `lg_shape.py`, `lg_hill2.py`, `lg_census.py`.

---

## 0. Checklist - read this first, every session

The nine steps of [../_!___derive.md](../_!___derive.md) on a **coin cohort**: the subject is a
launch type, so "him" is the operators behind it. Two sessions have worked this group; both
reached "D is a type of launch, not a person" independently.

| step | state | where |
| --- | --- | --- |
| 1 Pick | **done** | chain step 1 and section 3b. 41,013 creators on the sequence, 27,802 even with the CU pin. A tool, not a person. Its coins are below the pool on every outcome |
| 2 Portrait | **not done as a person** | chain step 2 reads the COINS as three lines (dead / meh / mover). No operator has been read as a human with habits, which is the order derive 3 asks for |
| 3 Their exit | **not done** | no creator or crew exit has been read from its own sells. Chain step 4 reads the crew's first sell timing only |
| 4 Test each clause | n/a | no portrait, so no clauses |
| 5 Our version | **done, four candidate events** | chain steps 3, 4b, 7, 9 (leftover, crew rebuy, 8dtx analog) and section 3b (price location, buyer identity, rule 1's frenzy shape) |
| 6 Fit D, P, X | **partly** | the creator door is fitted and holds forward (3b). No event survives to fit P and X against |
| 7 Coverage | done | section 6 |
| 8 Prove | not reached | nothing passed step 5 |
| 9 Record | this file, evidence 3.8 | |

**The order was broken in both passes.** Steps 2 and 3 were skipped and the scans ran first.
That is the failure derive 3 exists to prevent, and it is the first thing to fix here.

---

## 1. Sentence

```
E  unread (no DELAY-legal public print on this parent)
P  none
X  none
D  exact create ix: CU Limit, CU Price, Create_v2, ATA CreateIdempotent, BuyV2, Transfer
R  one open per coin
S  0.2 SOL
seat  both legs fill at the last print landed 115 ms after the decision print
```

Plain words: D is a launch type, not a person. CU and creator rotate. No
exact-launch type with >= 300 coins is an 8dtx sleeping-coin universe: the
best sleep-then-wake rate is **3.1 %** (3ix Create_v2+BuyV2). Movers on that
type are already at 41 by age 2 s. A wake plus 5-10 s abort loses the toll.

### The book

No book. Exact launch as D does not make 8dtx's graph.

---

## 2. The chain

| step | question | what it shows | what it does to the rule | script |
| --- | --- | --- | --- | --- |
| 1 identity | Is this sequence one person, or a type of launch? CU and creator are knobs. | 154,801 coins, 61 days, **41,013 creators**, top1 **0.9 %**, top5 3.5 %. 10,626 CU presets on the same labels (HHI 0.35). The named pin CU 500000 / 10000 is 90,162 coins and still **27,802 creators**, top1 1.2 %. Sentinel max-cost on 134,431 / 154,801. Initial buy p50 **0.22 SOL**. Per-day births 14.7x peak/trough, 6 % in two days: a type of tool, not a two-day client. Nearby 5ix without Transfer is 268,090 coins, also a tool. | D = this sequence is a **type of launch**, not a person. CU and creator are knobs the same bot changes. The group is the door, not a crew to clone. | lg_id.py |
| 2 portrait | How do these coins run vs the sized-buy 6ix (`Buy` not BuyV2) vs the rest of the tape? | Lake 09-01..09-20 births: SEQ6_BUYV2 n=73,274, 21,280 creators. **86.23 % never moved** (peak vsol < 35). Peak p50 **30.5**, prints p50 **5**, life p50 152 s, reserve-60 **0.29 %**, wall **0.14 %**. CU pin is the same life. SEQ6_BUY (the other example) is a different product: init 1/3/5 SOL, first-slot 3.4/7.5/13.5, peak p50 41.3, **52.64 % spike-then-dead**, wall 1.74 %, still 5,952 creators. Tape 5 % sample of other creates: never-moved 55.6 %, wall 6.76 %. | SEQ6_BUYV2 coins die because nobody spends after a tiny create-buy. SEQ6_BUY coins pump because the bundle spends 3-8 SOL at birth, then extract back to 30. Up-moves on the first group are rare accidents, not a crew plan. | lg_portrait.py |
| 2b peak age | Is the hill the create slot (age-0 ramp, closed) or a later wave? | SEQ6_BUYV2: age at peak p50 11.3 s; **11.52 %** of coins have peak > 35 and age >= 10 s (83.7 % of the movers). First-slot buy is 70 % of (peak-30) at the median. SEQ6_BUY: age at peak p50 **1.63 s**; 75.3 % peaked by 10 s; only 27 % of movers peak after 10 s; first-slot is 75 % of the hill. | On SEQ6_BUY the hill is the launch bundle (age-0, closed as the prize). On SEQ6_BUYV2 the rare movers do peak after 10 s, so a later tell is not arithmetically impossible. Occupancy leftover has to say whether that hill is still unpriced at 115 ms. | lg_peak.py |
| 3 leftover | After age >= 10 s, does leftover exist at lag_115 on every fire of a public print? | SEQ6_BUYV2, study_exact, fires < 09-06 12:00. First print at age >= 10 s: n=14,876, peak leftover 15 s **-3.94 %**, 87 s **-3.92 %**, share > 0 = 18 %. First buy: -3.80 % at 87 s. Buy >= 0.5 SOL: n=3,377 (~614/day), peak 87 s **+1.51 %** (thin under +2 %), share >= 4 % = 42 %. SEQ6_BUY first print: -3.71 % at 87 s; buy >= 0.5: **+2.01 %** thin. Reaction cost ~ 0 (quiet after scramble). | **5.2 occupancy red** on first print at 10 s: that class is the dead line, not the start of an up-move. | lg_left.py |
| 4 SEQ6_BUY non-bundle | After the birth bag, is a non-bundle buy at age >= 10 s still unpriced? When does the dump land? | SEQ6_BUY tape coins 8,485. First-slot buyers p50 **4**. First crew sell p50 **1.65 s** (95 % by 10 s). First non-bundle buy p50 **0.84 s** (93 % by 10 s). vsol at first age>=10 s print p50 **33.5**. Dump already done at 10 s (peak>=40 and v@10<=35): 23 %. Leftover, lag_115: nb_buy_age10 n=5,925 peak87 **-3.38 %**; nb_buy before the first crew sell n=110 (~20/day) **-3.68 %**; crew sell at 10 s **-3.56 %**. CU 300000 / 3333333 (108,270 of 140,936 coins) is the same book. First-slot buy bands 0-1 / 1-3 / 3-6 / 6-10 / 10+ SOL: first print and nb buy red in every band. | The campaign is an age-0 race. By the harvester frame the bag is out and the extract has started. Non-bundle spend at 10 s is not DELAY. Pinning CU does not change it. First-slot size does not rescue the class. | lg_nb.py |
| 4b crew rebuy | A first-slot wallet buying again at age >= 10 s: leftover of that print, or a tag of coins already running? | crew_buy_age10 n=355 (~65/day), peak15 **+0.70 %** (thin), peak87 +8.24 %. On the same 355 coins, first print at 10 s is **+9.86 %** at 15 s / **+24.37 %** at 87 s. The rebuy lands at age p50 **29 s**, v0 p50 43.6, after the hill. 29 % of coins have crew peak15 above first-print peak15. | The rebuy is a lagging tag of the rare coins that still have a campaign. It is not E: leftover of that print is thin at 15 s, and picking those coins uses a later print. Do not freeze it as a door. | lg_crew.py |
| 5 D1.2 | Yesterday's users of SEQ6_BUY whose coins did not dump: does leftover of first print / nb buy reopen? | 49,196 SEQ6_BUY coins 08-25..09-20, 6,892 creators, 68 % have a yesterday row, yest_n p50 7. Every named split of first_age10 and nb_buy_age10 stays red: dump_rate < 0.5, dump_rate < 0.3 with n>=5, n>=20, mover_rate >= 0.2, slow-wall >= 5 % with n>=20, and the dump>=0.8 control. Slow-wall n>=20 is 368 fires, peak87 **-3.50 %**. | Occupancy of first-print is a class property. Creator rotation makes yesterday-user D1.2 the wrong split of this type. | lg_nb.py |
| 6 graph | On this type, when does an up-move start, and what does the path look like? | SEQ6_BUYV2 study_exact, 19,839 coins. **86.0 % dead**: vsol p50 30.2 at age 2, 10, 30, 60, 87 s, peak 30.3, prints p50 4. **10.8 % meh**: slow bump to 39, end 30. **3.2 % movers** (peak>=50): already **vsol 41.8 at 2 s**, 42.4 at 10 s, 45 at 87 s, peak 56.3 at age p50 **419 s**, end 30.5. First cross of 35 after age 10 s: 8.7 % of coins; **83.6 % false starts** (peak stays <50). False-start and mover start prints look the same (sol p50 0.98, mv p50 5.7 %, fresh ~84 %). 87 % of movers make a second hill after 60 s (age p50 105 s); even on those coins the hill-start leftover at 15 s is **-1.48 %**. SEQ6_BUY is the other shape: 67 % meh dump (v2 37 -> v30 31), 21 % movers already at 44 by 2 s then decay. | This type is not 8dtx's sleeping coin. Dead coins never wake. Movers are already running by 2 s, then grind for minutes. A start print does not tell false from real. | lg_shape.py, lg_hill2.py |
| 7 8dtx analog | Quiet >= 0.4 s, own step >= 1.90 %, fresh buyer, age >= 10 s, one open. Prove it in 5-10 s or abort. | SEQ6_BUYV2 first wake: n=3,636 (~661/day). Cost 0. Peak15 **-2.62 %**, peak87 **+0.80 %** (thin). Clock 5 s **-3.79 %**, 10 s **-3.74 %**, abort **-3.78 %**. Abort why: cut5 11 %, abort10 68 %, run 21 %. On the 3 % movers peak87 +16.8 % and still clock 5 s **-3.11 %**. Cheap (vbef<=45, prior high<=50) is the same occupancy book. SEQ6_BUY same E: peak87 **+5.02 %**, peak15 +0.37 % thin, abort **-6.76 %** (cut5 45 %); movers abort **-6.69 %** too. | 8dtx's entry plus 8dtx's fast abort is occupancy-red on this type. Follow-through is not a 5-10 s burst. Do not copy his fill, and do not copy that sentence onto this graph. | lg_shape.py |
| 8 census | Which exact-launch types sit near launch at 10 s and later make a real hill? | 71 types with >= 300 coins in 09-01..09-20 (406,278 coins). Sleep-then-wake = v10<35, peak>=50, age_peak>=10 s. Highest **count** is SEQ5_BUYV2 (956, **1.0 %**) and SEQ6_BUYV2 (691, **0.9 %**). Highest **rate** with size is 3ix Create_v2+ATA+BuyV2 (no CU): n=4,278, **3.1 %** (134 coins), dead 45 %, mover 19 %, v2 33.2, v10 32.3. Next rates are n<1,000. SEQ6_BUY sleep-wake 1.2 % with dump 36 %. No type is mostly sleep-then-wake. | Exact launch does not concentrate 8dtx's graph. The sleep-wake tail is 1-3 % of every large type. | lg_census.py |
| 9 SEQ3 analog | Same wake + abort on the best-rate type (Create_v2, ATA, BuyV2). | Study_exact 977 coins: dead 35 %, meh 40 % (v2 37 then dump), mover 25 % already **v2 40.7 / v10 43.7**, peak 64 at 148 s. 58 % asleep at 10 s; of those, **4.0 %** later reach 50 (23 coins). 91 % of movers were already up at 10 s. quiet_real_fresh n=468: peak15 **+0.81 %** thin, peak87 +9.84, abort **-3.94 %**. Wake on the asleep coins: peak15 **-3.29 %**, abort **-3.91 %**. Wake on already-up coins: peak15 +3.85, abort **-5.51 %**. | The leftover at 87 s is coins already running by 10 s, and the 5-10 s abort sells them. The asleep subset has no leftover. This type is not a sleeping-coin door. | lg_shape.py |

---

## 3. What the coins are, in human terms

**D is a type of launch, not a developer.** Ordered ix labels group coins born
the same way. CU limit/price and the creator wallet are knobs that bot can
change, so they are not the key.

**This 6ix (BuyV2) is a dust-spray template.** Opening buy p50 0.22 SOL. The
graph is three lines:

- **Dead (86 %).** A flat line at vsol 30.2 from 2 s through 87 s. Four prints.
  Nobody ever spends. There is no up-move to catch.
- **Meh (11 %).** A slow bump to ~39, then back to 30. Crossing vsol 35 after
  10 s is usually this: 84 % of those crosses never reach 50.
- **Mover (3 %).** Already at **42 by age 2 s**, still ~45 at 87 s, peak 56 at
  ~7 min, then back to 30. The hill is not a wake from sleep. It is an early
  push, a plateau, then a slow grind. 8dtx's "quiet coin just woken, prove it
  in 5-10 s" is the wrong shape: even on these coins a wake's 5 s clock is
  **-3.1 %**.

The start print of a cross-35 looks the same on meh and movers (1 SOL, +5.7 %,
fresh 84 %). Selection at that print cannot tell which line you are on.

**The other 6ix (Buy)** is a dump-factory graph: first slot 7.5 SOL / 4 wallets,
price up by 2 s, first crew sell at 1.65 s, back near 30 by 30 s. A 5-10 s
abort sells into the extract.

**No large type is a sleeping-coin universe.** 71 exact lists with >= 300 coins:
sleep-then-wake is 1 % of the sprays and **3.1 %** of the best-rate type (3ix
Create_v2+ATA+BuyV2, no CU). On that type, 91 % of movers are already at 44 by
10 s. A wake on the ones still asleep has leftover **-3.29 %**. 8dtx's door is
the coin at the fire, not the create list.

Roster wallets are a thermometer for a DELAY-legal event, not the parent.

---

## 3b. Second pass: the creator door, the ceiling, and three more entries

An independent run on the same key (CU 500,000 / 10,000 pin, `study-kernel/cvx_prints.parquet`,
12.47 M prints, 08-30..09-06) against a 25,000-coin pool control. It agrees with the chain above
on identity and adds four things.

**A creator door that holds forward.** The chain's step 5 asks whether creator history reopens
*leftover* and finds red. This asks a different question - does it predict **coin quality** - and
the answer is yes. Creators with >= 5 coins in the trailing 7 days, banded by how many reached 20
trades, read forward on the NEXT day's coins:

| trailing record | coins/day | next-day survive | next-day volume >= 50 SOL | days beating base |
| --- | ---: | ---: | ---: | ---: |
| >= 50 % survived | 235 | **74.7 %** | **19.6 %** | **30/30** |
| 20-50 % | 142 | 31.8 % | 4.1 % | 14/30 |
| < 20 % | 489 | 6.4 % | 0.8 % | 0/30 |
| group base | | 17.2 % | 4.2 % | |

Both readings are true and they do not conflict: **the creator's record selects better coins, and
better coins are still not an entry.** 119 creators carry the hi band in the tape week, the top
one 22.7 % and 29 of them 80 %, so it is ev 3.1's rotating client and must be refreshed daily.

**Volume is not a swing** (law 23). The same bands read on episodes give the hi band 8.18 % of
coins producing a playable +50 % swing against a 5.33 % pool control - a 1.5x lift, not the 4.7x
the volume table suggests. The door's real work is exclusion: the low band runs 0.66 %.

**The 24-point gap, which is the whole problem.** Firing at the true episode low against firing
once price has risen 10 % off that same low, playable lows (age >= 60 s), same exits, same seat:

| where we fire | pool | group hi band |
| --- | ---: | ---: |
| the true low (look-ahead, a ceiling) | **+14.73 %** 8/8 | **+9.56 %** 7/8 |
| 10 % above it (decision time) | **-9.06 %** 0/8 | -12.57 % 0/8 |

The coins are tradeable and the exit is adequate. Price confirmation costs 24 points, which
reproduces 1.1's -9.87 % of entry per slot from an independent build. Any entry on this cohort
must therefore contain **no price confirmation at all**.

**Three more entries, none passing.**

| entry | result |
| --- | --- |
| price location (10 % above the trailing 30 s low) | best cell **-3.10 %/trade, 0/8 days**. Re-ran a refuted family (E8) and got the refuted answer |
| buyer identity at a local low, crew membership causal (a wallet on 3+ EARLIER coins of the same creator) | 233,278 tickets, 30 cells of class by position, **not one positive**. Every cell within a point or two of the round-trip toll, so the gross edge of every class is about zero |
| rule 1's frenzy shape (public sell >= 1 SOL, new high <= 20 s, quick-flip seller, N recipes in 5 s) | a **monotone ladder** in both bands: -4.70 / -2.92 / -1.92 / -1.50 / **+0.88** / **+4.52** %/trade as each term is added, the door worth ~6 points at the tightest cut. **Fails its folds**: -14.20 % on 26 trades against +10.53 % on 81 |

**One durable exclusion.** The creator's own buy on its own coin is the most reliably losing print
measured here: **-21.97 %** at a local low, **-38.96 %** near one, on 2,385 tickets.

Parquets: `study-kernel/cvx_gA_door.parquet` (door labels), `cvx_gA_ep.parquet` (swings),
`cvx_gA_book.parquet`, `cvx_gA_who.parquet`, `cvx_gA_frenzy.parquet`. Build scripts ran from a
session scratchpad and are in no commit; the door is the only one worth promoting.

---

## 3c. Third pass: the 21-day tape, and whether the creator address is identity

The group here is the **ix sequence alone**. A dev running a launch bot retypes a CU preset in one
line and replaces a creator address for free, so neither is identity; the ordered ix list costs a
code change, so it is the one stable layer. Tape: `study-kernel/tape6ix/`, 09-01..09-21,
74,948 coins, 22,466 creator addresses, 2,713,148 prints. Scripts: `g2_pairs`, `g2_cluster2`,
`g2_succ`, `g2_cfg`, `g2_door`, `g2_frenzy` (scratchpad).

**Can several addresses be joined into one operator?** Not from this data. Crew co-occurrence
builds one 4,266-creator blob and its held-out check runs backwards (linked pairs agree on a CU
preset 67.2 % against a 73.9 % null). A 27-setting sweep on wallet loyalty never lifts that check
above 2 points. Linked creators run **concurrently**, not in sequence (3.7 % sequential against a
9.3 % null), which is the opposite of a handoff. A day-matched succession test, asking whether a
stopping creator's crew reappears on a fresh address, sits below its null at every threshold
(25.3 % against 33.3 % at Jaccard >= 0.2). The configured buy does not travel either: the amounts
are round SOL net of the 1 % pump fee (99,012,169 lamports is 0.1 SOL), each shared by 500 to
2,000 addresses.

`raw_txs` is empty, so **funder -> creator, the one link that would settle it, needs an RPC
budget** and stays open.

**The door replicates.** Rebuilt from scratch on the 21-day tape, the hi band reads 75.3 %
survive and 17.38 % reaching 50 SOL against a 19.5 % / 4.55 % base, beating the day's base on
**14/14** days. The 8-day read (74.7 % / 19.6 %, 30/30) is not a fluke.

**Rule 1's frenzy shape is a regime.** With 938 trades instead of 107 at the tightest cut, the
ladder still climbs (-5.71 -> -2.90 -> -0.74 -> +1.63 -> **+3.25** %/trade) and still splits:
**+6.43 % on 09-08..09-14, -0.92 % on 09-15..09-21**, 7/14 days. The base row is flat across the
same halves, so what decays is the selection. No outlier day and no outlier trade carries it.

---

## 4. Empty slots, and what would reopen them

| slot | now | what would fill it |
| --- | --- | --- |
| D | exact launch type | another type does not fill it: 71 types, none are a sleeping-coin universe. CU / creator are not the key. |
| E | 8dtx analog occupancy red on every type tried | a public unpriced print on a graph that actually sleeps, then wakes. Exact launch does not make that graph. |
| X | 8dtx 5-10 s abort red | his abort sells this graph's grind and dump. |
| P R S | empty / standing | after a DELAY-legal E |

Exact launch as D for an 8dtx-style sentence is closed. The sleep-wake tail is
1-3 % of every large type, and leftover of the wake on that tail is red.

---

## 5. Next

1. Do not keep searching ordered ix lists for an 8dtx sleeping-coin universe.
2. 8dtx's door is the coin's state at the fire (cheap, quiet, never pumped), not
   its create list. That work stays on the mid-tape wallet line.
3. CU and creator stay knobs, not identity.
4. Rule 1's frenzy shape is closed on this group: a regime that pays for five days
   and decays, not a rule.
5. The only unspent lead on identity is funding. `raw_txs` is empty, so joining
   creator addresses into one operator needs an RPC budget and explicit approval.
   Every on-chain link tried sits at or below its null.

---

## 6. Coverage and the unread list (derive 10.1)

Every inventory family, marked **tried** / **partly** / **not tried** / **no data** / **not his**.
A fact read alone, or a book priced under a placeholder exit, is **not tried**.

| family | mark | what was run | what is left |
| --- | --- | --- | --- |
| D1 creation fingerprint | **tried** | the sequence is a tool (41,013 creators); 71 types censused, none a sleeping-coin universe; the CREATOR's trailing record is a forward-holding quality door (3b) | the same door keyed on `initial_buy_lamports` bands (D1.1 "First-slot buy") |
| D2 creator's document (URI) | not tried | - | the whole family on this cohort |
| D3 this coin's life before the fire | **partly** | age, vsol at 2/10/30/60/87 s, peak age, cross-35 false-start rate | "Made a hill", "Flush recovered", "Floor held", "High peak, mid-curve" - the cycler facts, untried |
| D4 loss door | not tried | - | the whole family. The creator-buy exclusion (3b) is its nearest relative |
| D5 off-chain | no data | outside the data scope (derive 2.0) | - |
| E1 a listed ix structure acts | not tried | `build` read only as a frenzy count | which structures buy this type's coins |
| E2 silence, then a spend | **tried** | chain step 7, the 8dtx analog: peak15 -2.62 %, clock 5 s -3.79 %, abort -3.78 % | - |
| E3 an operator's plan is unfinished | **partly** | chain step 4b, crew rebuy at age >= 10 s: thin at 15 s, a lagging tag | `clip_vs_med` and `clip_step_up` for crew wallets, using the group's other coins for the habit |
| E4 a count crosses a line | **partly** | the frenzy recipe count as a standing cut (3b) | the count as the crossing itself |
| E5 after sellers | **tried, fails folds** | rule 1's shape: monotone ladder, +4.52 % at >= 15 recipes, folds -14.20 / +10.53 on 107 trades | a longer tape. This is the one live thread |
| E6 this print | **partly** | sell size >= 1 SOL, quick-flip seller, own step >= 1.90 %, fresh buyer | the buy side of the same read |
| E7 clock | not tried | - | the cohort's one real freedom: the door is known at birth, so a clock is not a race |
| P1 curve position | **partly** | reserve <= 100 SOL; cheap (vbef <= 45, prior high <= 50) read once | the bands fitted on this cohort rather than borrowed |
| P2 windowed tape metrics | not tried | - | the whole family |
| P3 skin in | not tried | - | the whole family. Check `is_mayhem_mode` first: it doubles supply and halves every supply-share reading |
| P4 ix makeup of the recent tape | **partly** | distinct recipes in 5 s | the tool mix and seed racers as permissions |
| P5 tape state already true | **partly** | new high within 20 s; prior high <= 50 | the rest of the family |
| X1 static | **tried** | rule 1's bracket, clock 5/10/30 s, trail 30 and 50 | fitted on this cohort rather than borrowed |
| X2 the tape stops | **partly** | `abort(5 s, 2 %)` and 8dtx's 5-10 s abort, both red | the abort grid, and the **curved trail**, never run here |
| X3 another actor acts | not tried | - | the crew's own selling as the exit signal |
| R re-entry | not tried | one open per coin | the whole family |
| S size | **partly** | 0.2 and 0.03 SOL both booked; 0.03 is slightly worse because the fixed cost is 1.5 % of it | - |
| 5.4 an earlier sign | **no data** | crew funding transfers would be the tell with real DELAY | `trades` stores no wallet-funding transfer. Out of scope until that changes |
| 5.4 a slower part of the same move | **partly** | chain step 6: 87 % of movers make a second hill after 60 s, but hill-start leftover at 15 s is -1.48 % | the second hill with a non-price tell |
| 5.4 the other side | **partly** | rule 1's shape buys the sell side | - |
| 5.4 another of his decisions | not tried | no operator read as a person at all | step 2 |

**Unread, in the order to run it:**

1. **A longer tape.** Rule 1's frenzy shape is the one live thread and 8 days gives it 107 trades.
   Postgres holds a rolling ~30 days. Every cut narrow enough to be interesting is too thin here,
   so this gates almost everything else.
2. **Steps 2 and 3, in the right order.** Take the largest hi-band creator and read it as a
   person: its launches, its crew's spend, its own exit. Both passes entered through numbers, and
   that is why every event tried was borrowed rather than derived.
3. **The cycler facts (D3)** on hi-band coins, which the group's own second-hill rate points at.
4. **E7 a clock**, the one event shape only a cohort can use, since the door is known at birth.
5. **The curved trail** on whatever event survives; never run on this cohort.
