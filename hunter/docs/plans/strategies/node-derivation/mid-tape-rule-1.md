# Mid-tape one-shot: rule 1 and the member book

The working file for the mid-tape node, derived by [_!___derive.md](../_!___derive.md).
Instrument: `9Uq8GV`. Other members are split, never pooled.

Numbers: [_!___evidence.md](../_!___evidence.md) 5.8. Code: [toolkit/](toolkit/README.md),
scripts in [mid-tape/](mid-tape/README.md).

Tapes: the **study tape** (2026-08-30 17:48 to 2026-09-06 12:00 UTC, 6.76 days), where every
threshold is read; the **holdout tape** (2026-09-06 12:00 to 2026-09-10), where changes
are only confirmed. Days after 09-10 are the clean test.

---

## 1. Rule 1

```
E  not frozen. 9Uq8GV names a public buy >= 1 SOL at 75-100 ms (lift 13.01).
   FOLLOW of that print is not reachable (0.00 % on-trig, -1.66 % behind).
   Next: which of those buys he takes, spelled as a state that has held.
P  none
X  none (his own hold is a 16 s clock: p50 16.0, p90 16.2)
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
| E, the trigger | 9Uq8GV, public buy >= 1 SOL, 75-100 ms, lift 13.01. FOLLOW not reachable. Burst start is the 6.9 ceiling on his coins | 5.8 |
| E, the terms | (phase 6, not yet) | |
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
| 5.2 | DELAY at 115 ms? | buy >= 1: dt p50 68 ms, ahead 30 %, on-trig 0.00 %, behind -1.66 %. Burst start: dt 125 ms, ahead 52 %, on-trig +4.09 % (his coins), behind -1.77 %. Sell >= 1 is 14 % of episodes, not his logic | FOLLOW of the named buy is not reachable (price moves away). Next is a state that has held, from which buys he takes | `mt_p15.py` | 5.8 |

### What was tried and did not make it

| idea | result | why it is out |
| --- | --- | --- |
| copy 9Uq8GV's fill | FOLLOW -1.57 % | the fill is in the price |
| follow buy >= 1 at 115 ms | 0.00 % on-trig, -1.66 % behind | DELAY; buying into a buy |
| sell >= 1 as his event | lift 0.32, 14 % coverage | he avoids sells; the +20 % row is a rare corner |
| pool the seven members | two tells (buy vs sell), two ages | law 27 |
| 8aaRWu as instrument | RACE body -1.39, top 1 % 188 % | law 28, noise |
| 3Xk2Eu | 0 episodes on this tape | no instrument |

---

## 3. The member book

| member | RACE cap15 | FOLLOW cap15 | days R/F | its trigger (peak lift, lag) | status |
| --- | ---: | ---: | :---: | --- | --- |
| 9Uq8GV | +2.21 % | -1.57 % | 8/8 / 3/8 | buy >= 1, 13.01, 75-100 ms | **instrument** |
| 8dtx2t | +3.16 % | -1.54 % | 7/7 / 0/7 | burst start, 7.49, 50-75 ms | same tell family; public spelling is C2 |
| 8aaRWu | +1.06 % | -0.67 % | 5/7 / 3/7 | burst start, 9.20, 75-100 ms | drop (tail) |
| 9999hu | +5.94 % | -2.60 % | 7/7 / 0/7 | sell >= 1, 9.12, 50-75 ms | second instrument, not this parent |
| 88887Q | +5.73 % | -1.81 % | 7/7 / 0/7 | sell >= 1, 8.61, 50-75 ms | with 9999hu |
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

## Open

1. Phase 6 on 9Uq8GV: which buy >= 1 (and which burst starts) he takes vs ignores on the
   same coin; spell those terms as a state that has held, then as a public sentence.
2. 9999hu / 88887Q (sell >= 1) are a second parent, not this one.
3. Holdout unused. Days after 09-10 are the clean test.
