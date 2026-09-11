# Mid-tape one-shot: rule 1 and the member book

The working file for the mid-tape node, derived by [_!___derive.md](../_!___derive.md).
Members are split, never pooled. Event candidates are re-read under leftover existence
(derive 5.2) on each paying member. 5.8-5.10 clock and occupancy numbers are diagnostics.
`buy >= 1` reaction cost +9.2 % stays a 5.2 kill on that print class.

Numbers: [_!___evidence.md](../_!___evidence.md) 5.8, 5.9, 5.10. Code: [toolkit/](toolkit/README.md),
scripts in [mid-tape/](mid-tape/README.md).

Tapes: the **study tape** (2026-08-30 17:48 to 2026-09-06 12:00 UTC, 6.76 days), where every
threshold is read; the **holdout tape** (2026-09-06 12:00 to 2026-09-10), where changes
are only confirmed. Days after 09-10 are the clean test.

---

## 1. Rule 1

```
E  not frozen. Re-read leftover existence (derive 5.2) on each paying member's acted fires.
   9Uq8GV print neighbor buy >= 1: reaction cost +9.2 %, 5.2 kill on that class.
   Burst start / nb2 / sell >= 1: 5.8-5.10 clock and occupancy columns only; leftover test open.
P  none
X  none (9Uq8GV hold p50/p90 = 16.0 / 16.2 s is a clock prior, not phase 8)
D  none
R  unlimited, one position per coin
S  0.2 SOL
seat  both legs fill at the last print landed 115 ms after the decision print
```

Plain words: (empty until E is a public sentence).

### The book

(empty until a public sentence exists)

### Each slot, in one line

| slot | where it comes from | evidence |
| --- | --- | --- |
| E, the trigger | leftover existence open on burst start / nb2 / sell >= 1. buy >= 1 reaction cost +9.2 % is a 5.2 kill | 5.8, 5.9, 5.10 |
| E, the terms | vs random: nb2 = 5, priced buys5 / mv10. vs other buy >= 1: stalled dip (price). Neither spelling lifts | 5.9, 5.10 |
| P | | |
| X | his hold p50/p90 = 16.0 / 16.2 s (a clock, not yet booked as X) | 5.8 |
| D | none | |

---

## 2. How rule 1 was derived: the chain

| step | question | what it shows | what it does to the rule | script | ev |
| --- | --- | --- | --- | --- | --- |
| 1-2 | Frame | study tape, mid-tape roster marked NODE. 3Xk2Eu has 0 episodes | holdout unused | `mt_p15.py` | 5.8 |
| 3 | Shape | 9Uq8GV: 1,024 episodes / 849 mints, re-entry 17.1 %, age p50 159 s, hold 10.7/16.0/16.2 s, max open 2 | mostly one-shot; X prior is a 16 s clock; do not split first vs later yet | `mt_p15.py` | 5.8 |
| 4 | Who pays, which seat? | All six printers green at RACE, red at FOLLOW. 9Uq8GV RACE +2.21 % 8/8, FOLLOW -1.57 % 3/8. 8aaRWu RACE body red (tail). 9999hu / 88887Q are a different tell | keep 9Uq8GV; do not pool; do not copy the fill | `mt_p15.py` | 5.8 |
| 5.1 | What print does he react to? | buy >= 1 @ 75-100 ms lift 13.01; burst start @ 50-75 ms lift 8.14; sell >= 1 lift 0.32 (avoids) | E class is a public buy, not a sell | `mt_p15.py` | 5.8 |
| 5.2 | DELAY columns at 115 ms | buy >= 1: dt p50 68 ms, ahead 30 %, on-trig 0.00 %, behind -1.66 %. Burst start: dt 125 ms, ahead 52 %, on-trig +4.09 % (his coins), behind -1.77 %. Sell >= 1 is 14 % of episodes, not his logic | buy >= 1 is a 5.2 kill (price already moved). Burst start leftover test still open | `mt_p15.py` | 5.8 |
| 6.1 | Which of those buys does he take? | 75 ms window: 87 % of his buys. Stall 73 s vs 28 s, dd -37 % vs -22 %, quieter 10 s. Reaction cost **+9.2 % p50**. Acted -1.55 %, ignored +1.77 % on his coins. Burst start which is the same >= 1 SOL buy | do not follow the print he takes; its leftover is gone | `mt_p6.py`, `mt_p6_win.py` | 5.9 |
| 6.2 | Public sentence on every coin? | buy >= 1 + terms: -0.06 to -2.00, 0/8. Held buying-state: -0.81 to -1.54, 0/8. Control (sells) worse and red. Unpriced `buys10 <= 4.56`: +0.82 6/8, body -221, top 1 % 218 % | print trigger closed. Do not AND. Go back to 5.1 for a state | `mt_p6.py`, `mt_p6_unpriced.py` | 5.9 |
| 5.1b | Missed print classes? | pro / new_build peak at 0-25 ms, dt 16-24 ms, behind red. No unpriced lift cell with lag >= 115 ms | those classes are races. Kill | `mt_p5b.py` | 5.10 |
| 5.1c | What unpriced state is true at its buy? | vs same-coin random prints: nb2 5 vs 3. Stronger separators are priced (buys5, mv10). It fires into a live 2 s burst | E candidate is rising edge of nb2 / npro, not stall / dd | `mt_p5b.py` | 5.10 |
| 5.2b | DELAY columns of that rising edge | nb2 >= 4: cover 64 %, dt 308 ms, behind **-2.41 %**. npro5 >= 1: cover 43 %, dt 399 ms, behind **-2.70 %**. On-trig is its later buy | lag open; clock-behind and occupancy are 5.3 diagnostics, not the leftover veto | `mt_p5b.py`, `mt_p5b_nb2.py` | 5.10 |
| 5.3 / 6.2 | Every-fire occupancy, clock 15 | nb2 >= 4: full tape -1.86 % 0/8, its coins -0.38 % 2/8. nb2 >= 5: +0.03 % 3/8 body -48. npro same shape | occupancy red is 6.1, not a 5.2 kill. Leftover re-read on acted tickets | `mt_p5b.py`, `mt_p5b_nb2.py` | 5.10 |

### What was tried and did not make it

| idea | result | why it is out |
| --- | --- | --- |
| copy 9Uq8GV's fill | FOLLOW -1.57 % | the fill is in the price |
| follow buy >= 1 at 115 ms | 0.00 % on-trig, -1.66 % behind | DELAY; buying into a buy |
| the buys he takes (stalled dip) | reaction cost +9.2 %; acted -1.55 % on his coins | DELAY; leftover is already in the print |
| buy >= 1 + dd/stall/quiet, full tape | -2.00 % 0/8 | terms do not lift; price-path as event |
| held buying-state + same terms | -1.54 % 0/8 | DELAY of this leftover is still 0 |
| unpriced only (buys10, holders, recipes) | +0.82 then tail 218 %; holders -1.91 % 0/8 | body red; not a filling |
| pro / new_build as the print | dt 16-24 ms, behind -2.2 to -2.7 % | race; co-arrival, not DELAY |
| rising edge nb2 >= 4 / 5 | full tape -1.86 / -1.66 0/8; its coins -0.38 / +0.03 body -48 | every-fire clock 15 occupancy; leftover on acted tickets is unread |
| rising edge npro5 >= 1 / npro2 >= 1 | full tape -1.56 / -1.67 0/8; its coins -0.46 / -0.49 | same: occupancy column, not 5.2 |
| sell >= 1 as his event | lift 0.32, 14 % coverage | he avoids sells; the +20 % row is a rare corner |
| pool the seven members | two tells (buy vs sell), two ages | law 27 |
| 8aaRWu as instrument | RACE body -1.39, top 1 % 188 % | law 28, noise |
| 3Xk2Eu | 0 episodes on this tape | no instrument |

---

## 3. The member book

| member | RACE cap15 | FOLLOW cap15 | days R/F | its trigger (peak lift, lag) | status |
| --- | ---: | ---: | :---: | --- | --- |
| 9Uq8GV | +2.21 % | -1.57 % | 8/8 / 3/8 | 2 s recipe burst (nb2=5); print peak buy >= 1 | leftover re-read (5.2); buy >= 1 is a 5.2 kill |
| 8dtx2t | +3.16 % | -1.54 % | 7/7 / 0/7 | burst start, 7.49, 50-75 ms | leftover re-read; public burst START is C2 |
| 8aaRWu | +1.06 % | -0.67 % | 5/7 / 3/7 | burst start, 9.20, 75-100 ms | drop (tail) |
| 9999hu | +5.94 % | -2.60 % | 7/7 / 0/7 | sell >= 1, 9.12, 50-75 ms | instrument of [mid-tape-rule-2.md](mid-tape-rule-2.md); leftover live (5.11) |
| 88887Q | +5.73 % | -1.81 % | 7/7 / 0/7 | sell >= 1, 8.61, 50-75 ms | same tell as 9999hu; off that file |
| ApfmkS | +2.81 % | -1.46 % | 7/7 / 2/7 | flat (~1.3) | no print tell |
| 3Xk2Eu | - | - | - | - | no prints |

---

## 4. Data and code

| where | what |
| --- | --- |
| [mid-tape/README.md](mid-tape/README.md) | every script, by step |
| `node-derivation/data/mt_p15_shape.csv` | shape and seat per member |
| `node-derivation/data/mt_lift_*.csv` | excess-intensity lift tables |
| `node-derivation/data/mt_ep_*.parquet` | episodes with RACE / FOLLOW clocks |
| `node-derivation/data/mt_react_*.parquet` | 9Uq8GV reaction on buy >= 1 / buy >= 0.5 |
| `node-derivation/data/mt_p6_which_*.parquet` | buy >= 1 / burst start on its coins, with act |
| `node-derivation/data/mt_p6_all_*.parquet` | full-tape candidates (buy, sell, buying-state) |
| `node-derivation/data/mt_p6_book_*.csv` | 6.2 ledgers |
| `node-derivation/data/mt_p5b_rank.csv` | state contrast, its buys vs same-coin random prints |
| `node-derivation/data/mt_p5b_react.csv` | pro / new_build / npro / nb5 reaction |
| `node-derivation/data/mt_p5b_nb2_react.csv` | nb2 rising-edge reaction |
| `node-derivation/data/mt_p5b_book.csv` | npro public sentence and ceiling |
| `node-derivation/data/mt_p5b_nb2_book.csv` | nb2 public sentence and ceiling |

## Open

1. 9Uq8GV leftover existence (derive 5.2) on acted burst start / nb2 is unread. `buy >= 1` / pro / new_build stay 5.2-dead. Do not kill on a hold-matched clock or every-fire occupancy.
2. 9999hu is the node's instrument: [mid-tape-rule-2.md](mid-tape-rule-2.md), evidence 5.11.
3. Public burst START remains C2 and does not ship. Empty D stays allowed on a leftover-legal parent.
4. Holdout unused. Days after 09-10 are the clean test.
