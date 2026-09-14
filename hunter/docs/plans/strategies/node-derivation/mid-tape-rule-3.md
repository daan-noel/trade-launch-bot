# Mid-tape one-shot: rule 3 (instrument 8dtx2t)

The working file for **8dtx2t**, derived by [_!___derive.md](../_!___derive.md).
One member, never pooled. Phase 3-4 ranks every roster member on the every-leg study tape;
8dtx2t is the instrument. E is the public burst family; the wide classes and the exclusive
splits of that family are 5.2 kills at 115 ms.

Numbers: the chain rows below. Code: [toolkit/](toolkit/README.md), scripts in
[mid-tape/](mid-tape/README.md).

Tapes: the **study tape** is `study_exact` (every leg, 6.00 days). The last-leg study tape
does not carry 3Xk2Eu. The **holdout tape** is `holdout_exact` (2026-09-06 12:00 to 2026-09-10),
where changes are only confirmed. Days after 09-10 are the clean test.

---

## 1. Rule 3

```
E  public burst family: quiet restart (burst start AND NOT up>=3 %), continuation
   ((buy>=0.5 OR up>=3 %) AND NOT burst start), loud restart (burst start AND up>=3 %).
   He reacts at 50-75 ms (lift 5.74 / 5.81 / 9.72). Behind our 115 ms fill: cost 4.23 /
   4.48 / 6.11 %, peak leftover +0.08 / +1.92 / +1.07 %. 5.2 kill on cost. Not frozen.
P  none
X  none. Prior: hold p50 16.6 s; >20 % losses 0.8 % on his close; top 1 % is 56.4 % of
   his net, body still +11.67 (a cut, not a scalp). His harvest shape is not the 5.2 veto.
D  none
R  unlimited, one position per coin
S  0.2 SOL
seat  both legs fill at the last print landed 115 ms after the decision print
```

Plain words: he buys 50-75 ms after a public burst print. The rest of that burst lands
inside 115 ms, so a fill at our seat is behind the move.

### The book

(empty until a public sentence exists)

### Each slot, in one line

| slot | where it comes from | step |
| --- | --- | --- |
| E, the trigger | burst family @ 50-75 ms; exclusive quiet / continuation / loud all 5.2 kill on cost 4.23 / 4.48 / 6.11 %. Wide burst_start / buy>=0.5 / up>=3 % and rising-edge counts fail the same line. Sells he avoids | 5.1, 5.2 |
| E, the terms | | |
| P | | |
| X | hold p50 16.6 s; his >20 % losses 0.8 % (a prior, not phase 8) | 3 |
| D | none | |

---

## 2. The chain

| step | question | what it shows | what it does to the rule | script | ev |
| --- | --- | --- | --- | --- | --- |
| 3-4 | Shape and who pays, every member, every-leg study | All seven have episodes. FOLLOW red on all (a copy of the fill, not a drop). RACE total green on all. His book at 0.2 SOL: 8dtx2t median -1.65 %, total +26.78, **>20 % losses 0.8 %**, hold 5.7/16.6/91.2 s, 424/day, RACE +16.34 6/6. 88887Q more RACE SOL (+52.94) but >20 % losses 9.3 % and hold p10 2.4 s. 3Xk2Eu 998 episodes, hold p50 50.0 s, >20 % losses 22.0 %. 9Uq8GV hold 9.7/16.0/16.2 s (a clock). A negative median with a positive total and few large losses is a cut, not a tail | Freeze **8dtx2t**: harvest hold, enough tickets, RACE pays, the tightest loss cut. Others stay candidates. Do not pool | `mt_p34.py` | - |
| 5.1 | What print does it react to? | Every-leg study, buys vs same-coin controls. burst_start @ 25-50 ms lift **7.24**; buy>=0.5 7.01; up>=3 % 6.93; buy>=1 5.87. Sells 0.13-0.88 (avoids). Peak lag 25-50 ms on every buy-side class. Fire is that public print, not its buy | E class is a public burst start (a size buy after silence). Reaction is faster than 115 ms | `mt_p51.py` | - |
| 5.2 | Leftover on that print at 115 ms | Acted tickets: latest class print <= 300 ms before its decision; fill 115 ms after that print. Behind row (its buy already in the price): burst_start cover 70.5 %, cost **5.20 %** (98.6 % >= 2), peak +0.86 %. buy>=0.5 cost 4.46 %; up>=3 % 4.55 %; buy>=1 5.34 %. Ahead tickets are the copied fill (burst_start ahead 37.5 %). PASS none | DELAY: the tell and the SOL land in the burst. No D/P/X on this trigger | `mt_p51.py` | - |
| 5.1c / 5.2 | Held state, then leftover on its rising edge | vs same-coin public prints: unpriced top nb2 rank 0.67 HIGH (p50 3). Stronger priced separator is buys5. Rising edge nb2>=3 / nbig2>=1 / nb5>=4 / nbig5>=1: behind cost 5.14-6.70 %, cover 21-32 %. PASS none | The unpriced state is already inside the burst. The crossing is the same 5.2 kill | `mt_p51.py` | - |
| 5.1 / 5.2 exclusive | Same family, split so the trigger print is not the 3 % step | Quiet restart / continuation / loud restart: lift 5.74 / 5.81 / 9.72 @ 50-75 ms, cover 25.7 / 41.9 / 44.8 %. Behind cost **4.23 / 4.48 / 6.11 %** (97.1 / 95.1 / 99.3 % >= 2), peak +0.08 / +1.92 / +1.07 %. Quiet trigger itself moves +2.27 %; the extra cost is the next prints inside 115 ms. His book on those cells: tot +6.98 / +10.54 / +14.45, >20 % losses 0.5 / 1.3 / 0.7 %, top 1 % 38 / 67 / 59 % of that cell's net (his X, not the veto). PASS none | DELAY: even the exclusive quiet print is followed by the burst before our fill. No D/P/X. No tighter AND on this class (97 % of behind quiet tickets already cost >= 2). Instrument closed at this seat for this family | `mt_p51_excl.py` | - |
| lag ladder | Cost / leftover at this bot's fastest fill vs 115 ms | Fastest ACK is 8-10 ms (not a fill). Fastest observed own-fill ~45 ms. Standing seat p50 115 ms. At 50 ms, acted median cost 0.00 / 1.78 / 2.43 % and peak +5.32 / +5.34 / +5.81 % — the 0 is "no later print has landed yet", not a reachable next-print seat. Behind 8dtx2t still cost 4.01 / 4.24 / 5.27 %. At 115 ms behind cost 4.23 / 4.48 / 6.11 % | A faster ACK does not change the 5.2 kill. The 50 ms 0-cost median is first-in-window, which sequencing does not buy | `mt_p51_lag.py` | 1.1 |

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

---

## 3. The member book

Every-leg study (`study_exact`, 6.00 days). His columns are our 0.2 SOL clip through its
entry reserve and its close. RACE / FOLLOW are a 15 s clock at those seats. `loss>20` is the
share of tickets at <= -20 % of clip.

| member | ep/day | hold p10/p50/p90 | age p50 | his med | his tot | his >20% | RACE tot | RACE 15s >20% | FOLLOW tot | status |
| --- | ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 8dtx2t | 424 | 5.7 / 16.6 / 91.2 | 157 s | -1.65 % | +26.78 | **0.8 %** | +16.34 6/6 | 5.5 % | -7.22 0/6 | **instrument** |
| ApfmkS | 66 | 2.8 / 7.1 / 33.8 | 204 s | +4.31 % | +3.78 | 6.3 % | +1.97 6/6 | 6.2 % | -1.75 1/6 | candidate (small n) |
| 9Uq8GV | 136 | 9.7 / 16.0 / 16.2 | 173 s | -0.82 % | +4.31 | 7.7 % | +4.22 6/6 | 9.8 % | -1.93 2/6 | candidate; hold is a 16 s clock |
| 88887Q | 765 | 2.4 / 22.6 / 78.9 | 42 s | +3.26 % | +64.56 | 9.3 % | +52.94 6/6 | 9.1 % | -16.31 0/6 | candidate (larger book, looser cut) |
| 9999hu | 591 | 4.1 / 25.3 / 73.5 | 18 s | +0.17 % | +47.99 | 17.9 % | +42.19 6/6 | 16.6 % | -19.59 0/6 | candidate (younger band, loose cut) |
| 8aaRWu | 117 | 2.8 / 25.6 / 109.3 | 89 s | -0.09 % | +3.39 | 19.8 % | +0.80 3/6 | 11.9 % | -1.53 2/6 | candidate (RACE body -1.32, top 1 % 264 %) |
| 3Xk2Eu | 166 | 12.3 / 50.0 / 213.6 | 51 s | -4.28 % | +13.95 | 22.0 % | +8.35 6/6 | 11.5 % | -7.10 1/6 | candidate (long hold, does not cut) |

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

## Open

The open items are in [_!___workflow.md](../_!___workflow.md) section 2, the one queue.
