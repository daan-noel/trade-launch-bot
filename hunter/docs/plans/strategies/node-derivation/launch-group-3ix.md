# Launch-group: 3ix Create_v2 + ATA CreateIdempotent + Buy

D is a creation instruction sequence with no CU instructions, and the subject is the crew behind
it: a handful of wallets buy the curve up in the creation slot, ride outside buyers, and sell in a
queue once the top is in, or dump within seconds when no outside buyer comes.

```
Create_v2, ATA CreateIdempotent, Buy
```

Tapes: lake tokens 09-01 .. 09-22 (9,700 coins on the sequence), lake trades 09-01 .. 09-21 on
those coins, curve prints only (the lake carries AMM prints for 50 of them). Study 09-01 .. 09-12,
holdout 09-13 .. 09-21 (the wide split; 09-13 .. 09-21 is already read once by R2, so the forward
test is days from 09-22 on). Fill: kernel, 115 ms, clip **0.03 SOL**.

Scripts (local, `launch-group-3ix/`): `g3_extract.py`, `g3_load.py` (per-coin arrays and the crew
tag), `g3_portrait.py`, `g3_crew.py`, `g3_book.py`, `g3_fall.py`, `g3_hazard.py`, `g3_x.py`,
`g3_facts.py`, `g3_band.py`, `g3_wallet_exit.py`, `g3_crew_exit.py`, `g3_csell.py`, `g3_c1.py`,
`g3_split.py`, `g3_entry.py`, `g3_door.py`, `g3_c1haz.py`, `g3_mult.py`, `g3_final.py`, `g3_abort.py`,
`g3_auc.py`, `g3_plain.py`, `g3_plainwho.py`, `g3_whale_door.py`, `g3_template.py`, `g3_firex.py`,
`g3_final2.py`, `g3_pg.py` (the forward tape), `g3_bigbuy.py`. Tables in
`data/g3_*.parquet`.

---

## 0. Checklist - read this first, every session

| step | state | where |
| --- | --- | --- |
| 1 Pick | **done** | the user's pick: 3ix:Buy, split by the creation-slot SOL (chain 1) |
| 2 Portrait | **done for the coins and the crew** | chain 1-3, 8 |
| 3 Their exit | **done: a queue, cheapest wallet first; the first sell marks the top; an abort at ~7 s when nobody comes** | chain 9-10, 14 |
| 4 Test each clause | partly | chain 11-13 |
| 5 Our version | **two candidates**; candidate 2 has no fires since its machine stopped | section 1 |
| 6 Fit D, P, X | **partly**: X read alone (chain 5-7, 10); E by age and outsider money (chain 12); D birth facts read alone (chain 13) | |
| 7 Coverage | section 3 | |
| 8 Prove | **forward read done on candidate 2**: 3 trades, the machine gone (chain 21); no replay audit | |
| 9 Record | this file | |

---

## 1. Sentence

### Candidate 2 - frozen before the forward read (chain 15-22): the machine it stands on stopped on 09-22

```
E  the first plain `ATA CreateIdempotent, Buy` >= 1 SOL outside the creation slot at age <= 5 s,
   while the crew has not sold >= 0.5 SOL
P  none
X  the first crew sell >= 0.5 SOL, out 115 ms later; vsol >= 110; 600 s
D  3ix:Buy, creation-slot buy SOL >= 23
R  one open per coin
S  0.03 SOL
seat  both legs fill at the last print landed 115 ms after the decision print
```

Plain words: the crew launches and waits for one big outside buyer; when it arrives we buy behind
it, and we leave the moment the first crew wallet sells.

| | study 09-01 .. 12 | holdout 09-13 .. 21 | whole tape | ship bar |
| --- | ---: | ---: | ---: | ---: |
| trades | | | 139, 7.7 a day | - |
| %/trade at 115 ms | +6.47 | +12.94 | +8.98 | - |
| days positive / worst day | 8/10 | 7/8 | **15/18** / -16.0 % (09-21) | >= 5/7, every holdout day |
| halves | | | +6.47 / +12.71 | both > 0 |
| SOL at 0.03 | | | +0.375 | - |
| win / median | | | 68 % / +7.41 % | - |
| top 1 % share / biggest coin | | | **0.10** / 0.10 | <= 0.15 |
| capped book (gains capped at the median winner) | | | +3.55 % | > 0 |
| coin bootstrap 90 % | | | +5.86 .. +12.25 | - |
| lag 50 / 200 / 300 / 400 ms | | | +9.09 / +8.65 / +7.41 / +6.66 | - |
| both legs at the slot end | | | +8.71 | - |

W = 5 s leads W = 10 s on the study days (+6.47 against +5.81); the plain buy's size floor (1, 2,
4 SOL) changes nothing. The holdout days were read once before by R2, so the forward read is days
from 09-22 on, once, with the sentence unchanged. **One machine**: 76 % of the fires are one wallet
behind the plain structure (the term is the structure, not the wallet; if it stops, fires stop).

### Candidate 1

```
E  first print at age >= 1 s
P  none
X  the first crew sell >= 0.5 SOL, out 115 ms later; vsol >= 110; 600 s
D  3ix:Buy, creation-slot buy SOL >= 23 (bands mid and big)
R  one open per coin
S  0.03 SOL
seat  both legs fill at the last print landed 115 ms after the decision print
```

The crew is every wallet whose creation-slot buy is `ATA Create, Buy` (optionally with `Transfer`,
Compute Budget dropped), plus the create signer. Outsider = not a creation-slot buyer.

Plain words: we buy the crew's fresh launch and leave the moment the first crew wallet sells,
because that sell is the top on most coins.

### The book (`g3_final.py`)

| | study 09-01 .. 12 | holdout 09-13 .. 21 | whole tape | ship bar |
| --- | ---: | ---: | ---: | ---: |
| trades | | | 495, 24 a day | - |
| %/trade at 115 ms | +4.03 | +9.08 | +5.99 | - |
| days positive / worst day | | | **13/21** / -18.8 % | >= 5/7 |
| halves | | | +4.37 / +7.89 | both > 0 |
| SOL at 0.03 | | | +0.889 | - |
| win / median | | | 59 % / +6.96 % | - |
| top 1 % share / biggest coin | | | **0.25** / 0.08 | <= 0.15 |
| coin bootstrap 90 % | | | +2.39 .. +9.47 | - |
| lag 50 / 83 / 200 / 300 ms | | | +6.02 / +6.04 / +5.63 / +5.48 | - |

Fails the day bar and the top 1 % bar. The ceiling on the same entries is +76 %/trade.

---

## 2. The chain

| step | question | what it shows | what it does to the rule | script |
| --- | --- | --- | --- | --- |
| 1 bands | What is the group? | 9,700 coins, 22 days. The creation slot splits it: **A** creator buys 85 SOL, 5,040 coins, 97 % complete the curve at birth; **B** slot >= 60 SOL, 2,238, 67 % at birth (96 % overall); **C** slot 20-60 SOL, 694 (32 a day), median 119 prints, peak vsol 85, life 305 s, 26 % complete; **D** slot < 20, 1,728, peak 42. CU carries no instruction, so no CU split. | A and B are instant launches (dead at this seat, strategy 8.1); C is the subject, D beside it. | g3_extract.py, g3_portrait.py |
| 2 portrait C | How does a C coin run? | vsol 63 at 1 s, 71 at 30 s, peak 85 at age p50 41 s, back to 30 by 300 s. Creation-slot buyers p50 6 wallets, 32 SOL; outsiders put in p50 29 SOL; the creation-slot buyers have sold half their bag at p50 88 s. | A rise on outside money after a bought launch, then a return to 30. | g3_portrait.py |
| 3 the crew | Who are the creation-slot buyers? | 90 % of creation-slot SOL comes through one structure, `ATA Create, Buy` (+ `Transfer`): 4 wallets plus the creator, 30.8 SOL. The others in the slot are snipers (BuyExactSolIn, nonce accounts) with 0.35 SOL. The crew buys once (1.7 % of its buy SOL after birth) and sells through a bare `Sell` (88 %). | Crew = the creation-slot `ATA Create, Buy` wallets plus the creator; snipers are outsiders to the crew and out of the outsider count. Visible live by structure. | g3_load.py |
| 4 ceiling and clocks | What do a perfect exit and simple exits earn? | Entry at age >= 1 s, 609 coins: ceiling **+75.9 %/trade**, median +48.9 %, 21/21 days. Hold -24.2 %. Clock 20 s +2.7 %, 13/21. Creation-slot buyers' first wave seen +3.4 %. | Almost all the value is in the exit. | g3_book.py |
| 5 the fall | Who drives the fall from the peak? | At the peak the creation-slot buyers have sold p50 4 % of their bag. The fall is half theirs, half outsiders', bimodal per coin (share p25 0.16, p50 0.76). The largest one-slot fall is vsol -21 at the median. | The loss is a dump at the peak, as on 7ix. | g3_fall.py |
| 6 hazard | What precedes a 10-SOL fall within 5 s, once a second? | Every fact lifts about 2x at most; dead coins at vsol 30 fill the samples. | A per-second crash hazard does not name the exit on this group. | g3_hazard.py |
| 7 single exits | Which exit, read alone? | On all C: creation-slot buyer sells >= 0.5 SOL +3.7 %; no outsider buy 2-20 s -0.8 .. -22 %; trail 10 +2.5 %, trail 20-40 lose; creation-slot profit target 5-25 SOL -0.5 .. -7.8 %. Waiting exits lose on study days: the dump lands before a slow signal. | No exit read alone keeps more than 4 points of the ceiling. | g3_x.py |
| 8 which coins | Which birth facts split the money? | By creation-slot SOL: < 23 SOL loses on every exit (trail 10 -6 / -14 study / holdout); 23-35 SOL (mid, 371 coins) ceiling +79 %, median +63 %; >= 35 SOL (big, 124) completes the curve 56 % but a ride to vsol 110 books -1.5 %. vsol at entry < 54 loses on both periods. | D = creation-slot SOL >= 23. | g3_facts.py, g3_band.py |
| 9 their exit | When does each crew wallet sell? | First crew seller at age p50 25 s, **at the coin's running peak on 87 %**, at 3.4x its own buy, 30 % of its bag. The next ones follow ~16 s apart at 2.2 / 1.6 / 1.2 / 0.7x, each selling its whole bag. The cheapest buyer sells first on 65 % (chance 26 %), sell order follows buy order (spearman 0.6). No wallet sells at a fixed multiple (first seller IQR 1.8-4.6x) or a fixed vsol (54-81). | The crew's first sell marks the top; the rest of the queue follows. Our X = that first sell. | g3_crew_exit.py, g3_mult.py |
| 10 crew-sell exit | Sell 115 ms after the first crew sell >= s SOL? | Mid + big, s = 0.5: section 1. By band: mid +5.4 %, big +7.7 %, small -5.6 %. Sizes 0.01-4 SOL differ by < 2 points. | X = the first crew sell >= 0.5 SOL. | g3_csell.py, g3_final.py |
| 11 two kinds | What are the losers? | Mid coins: **66 % outsiders came** (price +52 % before the crew's first sell at 30 s, outsider SOL 18.7 before it) book **+31 / +28 %** study / holdout; **34 % nobody came** (outsider SOL 1.6) - the crew dumps at p50 **6.6 s** and the trade books **-40 / -42 %**. By 2 / 3 / 5 s the crew has sold on 14 / 23 / 39 % of the nobody coins and on 0-1 % of the came coins; outsider SOL splits only from 5-6 s (4.1 vs 1.2 at 6 s). | The crew aborts when nobody comes; the -40 % class is the whole problem. | g3_c1.py, g3_split.py |
| 12 later entry | Enter at age A only while the crew holds and outsider SOL since birth >= L? | A 1-10 s x L 0-5 SOL: every cell below A1 L0 (+7.03 %); A6 L3 +0.21 %, A10 any L negative. The outsiders of 2-5 s come on both kinds of coin, and each second of waiting pays for the rise. | E stays at 1 s. Waiting for actors is price confirmation here too. | g3_entry.py |
| 13 birth facts | Which creation facts split came / nobody and money, on both periods? | Crew wallets, crew SOL, size spread, largest crew buy, tip share, creator SOL, sniper SOL and count, creation-slot prints, the group's last five C coins, group launches in 1 h, C launches in 10 min, hour, name reuse: none holds a sign on both periods (came rates 53-80 % across fifths, money mixed). | No D term beyond the creation-slot SOL band. | g3_door.py |
| 14 first-sell hazard | What precedes the crew's first sell (next 2 s), at every print? | Age, vsol / v_cs, crew profit and its share of birth SOL, outsider SOL / crew SOL, outsider buys 3 / 10 s, since the last outsider buy, outsider buyers, drawdown, our gain: lifts 0.2-3.5 that flip between study and holdout. | The first sell is not anticipated by any fact read; the exit reacts to it and gives away ~21 points (+52 % before, +31 % after) on the came coins. | g3_c1haz.py |
| 15 abort sign | Which fact at 1 / 2 / 3 s marks the nobody coins before the crew dumps? | Early buyers split by tool (plain, app, snipe, arbitrage, the crew's structure after birth), outsider sells, the crew's and the creator's CU price and tip, slot gap, vsol: only the plain `ATA CreateIdempotent, Buy` SOL by 3 s holds on both periods (AUC 0.354 / 0.378 for nobody); the rest 0.45-0.58. | The plain big buyer is the tell. | g3_abort.py |
| 16 the plain buyer | Who is it? | One machine: 213 coins, 27 wallets, the top one 76.5 %, top five 87 %. Size p50 8.89 SOL (a fixed clip), CU price 950,000 (the crew 10,000), no tip, slot +11, age p50 3.6 s. It sells after the crew's first sell on 88 %, p50 60 s later, at 0.92x its own buy. | The crew's outside money is one buyer; the crowd follows it and the crew sells into both. | g3_plainwho.py |
| 17 as a split, a cut, a trigger | Book it | Coins with a plain buy >= 1 SOL by 4-5 s book +36 %/trade from the 1 s entry (17/18 days) and those without -3 .. -6 %: a label, known after the entry. Cut at 2-5 s when none came: -2.1 .. +1.7 %. Fire on the plain buy: W 3 +8.55 % 15/17, W 5 +8.98 % 15/18, W 10 +6.93 % 17/21. | E = the plain buy itself. | g3_plain.py |
| 18 its choice at birth | Which birth facts predict the plain buy? | Creator buy 0.494 SOL (the 0.5 SOL template) 58 / 58 % study / holdout against 4-25 %; 5 crew wallets with one tipped 49 / 46 % against <= 28 %; a C launch in the last 10 min 45-56 % against 25 %; name reused 51 / 47 % against 23 / 26 %. The template as a door at 1 s: +7.83 %, 11/19 days, holdout +4.89 against the undoored +9.08. | A door that raises the plain-buy rate 43 -> 59 % does not pass the day bar; not in the sentence. | g3_whale_door.py, g3_template.py |
| 19 exits after the plain buy | Which X on the plain-buy entry? | W 5: ceiling +71.1 %; first crew sell +8.98 %; 2nd / 3rd crew seller -1.7 / -17.0 %; the plain buyer's own sell -17.5 %; trail 10 / 20 / 30 +6.0 / +0.4 / -16.8 %; first crew sell or trail 10-30 +6.7 .. +7.5 %. W 10 the same order. | X stays the first crew sell. | g3_firex.py |
| 20 candidate 2 | The whole sentence with the ship-bar lines | Section 1, candidate 2. | Frozen for the forward read. | g3_final2.py |
| 21 forward read | Candidate 2 unchanged on 09-22 .. 09-25 12:00 (Postgres; parity with the lake on 09-21: 381 / 381 coins equal, the 09-21 ticket identical) | 89 band-C coins, **3 trades**, all on 09-24, +66 %. The 950,000-CU plain buyer has no buy on any band-C coin from 09-22 on (09-05 .. 09-21: 9-61 % of the coins a day); the plain buys of 09-24 are another wallet, the big early buys of 09-23 are Axiom. | Not confirmed, not refuted: the machine the E stands on stopped. The forward days 09-22 .. 09-25 12:00 are spent on candidate 2. | g3_pg.py, g3_final2.py |
| 22 any big buyer | The same sentence with E = the first outsider buy >= P SOL of any tool at age <= W s (P 1-5, W 3-10), with and without the 950,000-CU buyer's coins | With it: +3.4 .. +8.2 %, 13-16 of 19-21 days. Without it: -12.3 .. +3.1 %, 0-10 positive days on every cell. | The edge is that one buyer's fill, not early outside money: the crowd follows it and no other tool. The node has no live E while it is away. | g3_bigbuy.py |

---

## 3. Coverage and the unread list

| family | mark | what was run | what is left |
| --- | --- | --- | --- |
| D create fingerprint + birth band | **tried** | four bands (chain 1), creation-slot SOL >= 23 (chain 8) | - |
| D creator's opening buy | **tried** | band A (85 SOL, instant completion); creator SOL inside C (chain 13) | - |
| D birth noise (snipers, prints in the slot) | **tried** | chain 13 | - |
| D crew shape (wallets, SOL, spread, tip) | **tried** | chain 13 | the crew's CU price and tip level as a number |
| D group history / flood / reuse | **tried** | last five C coins, launches per hour, C in 10 min, name reuse (chain 13) | windows other than 10 min / 1 h |
| D document (URI) | not tried | - | the whole family |
| E age | **tried** | 1-10 s (chain 12) | - |
| E outside money arrives | **tried** | as a gate with the crew holding (chain 12); the plain buyer's fill as the trigger (chain 17, candidate 2); any tool's big buy as the trigger (chain 22) | what the crowd reads after that buyer's fill |
| P crew still holds | **tried** | chain 12 | - |
| P tape state at 5 / 10 / 30 s | not tried | - | nested windows on the came / nobody split |
| X crew's first sell | **tried**, in the sentence | chain 10 | the queue: sell when the cheapest crew wallet sells, hold through a pricier one |
| X anticipate the first sell | **tried**, red | chain 14 | the crew's per-wallet cost against the live price with a finer time basis; the Sell structure's CU / tip on the crew's first sell |
| X clock, trail, buys stop, crew profit target | **tried** | chain 4, 7 | the curved trail |
| X immediate cut when nobody comes | not tried | - | an early cut in the first 3-6 s on a fact that separates the nobody coins |
| R re-entry after the first crew sell | not tried | - | a second entry after the queue's dump on coins still bought |
| S | standing | 0.03 SOL | - |

**Unread, in order:** (1) watch for the 950,000-CU plain buyer's return on new days (its fires are
the only E found; candidate 2 re-reads unchanged on days after 09-25 12:00); (2) what the crowd
follows after that buyer's fill, spelled without the buyer (who buys in the next seconds, and what
tells them); (3) the queue exit and the curved trail on the came coins; (4) band B's non-instant
coins and D under the same crew tag.

The creation-slot SOL cut (23) is read on 09-01 .. 09-21 as a fifth boundary; the forward test is
days from 09-22 on.
