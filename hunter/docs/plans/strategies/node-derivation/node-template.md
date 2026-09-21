# <Wallet>: its logic, and rule <n>

A wallet's case file starts as a copy of this one. [hot-tape-rule-1.md](hot-tape-rule-1.md) is a
filled rule; [mid-tape-8dtx2t-logic.md](mid-tape-8dtx2t-logic.md) is a filled portrait. The method
is [_!___derive.md](../_!___derive.md); its first page is the step list this file ticks. One
wallet, one file: two logics get two files. An empty slot stays written as empty, never dropped.
Scripts go in `node-derivation/<node>/`, one per step, each opening with its step and question;
they stay local unless a rule or a gate needs them re-run. Toolkit calls:
[toolkit/README.md](toolkit/README.md).

Tapes: the **study tape** (<from> to <to>, <n> prints, <days> days), where every threshold is read;
the **holdout tape** (<from> to <to>), where changes are only confirmed.

---

## 0. Checklist - read this first, every session

Tick in order. A skipped step is written as skipped, with the reason. Start a session here and at
section 6 (what is unread), never from memory.

| step | what | done? | chain rows |
| --- | --- | --- | --- |
| 1 Pick | he pays, he is a reader, RACE against FOLLOW (derive 1, 2, 4) | | |
| 2 Portrait | shape in numbers, 20 trades read, his logic in clauses (derive 3) | | |
| 3 His exit | read from his own sells, frozen, confirmed on the holdout (derive 8.0) | | |
| 4 Test each clause | the print he answers (5.1), which ones he takes (6.1 ladder) | | |
| 5 Our version | the rise still ahead at our fill (5.2, with its control); who lands inside our lag; the next event (5.4) | | |
| 6 Fit D, P, X to that E | together, starting from his own exit (derive 6.2, 7, 8.2, 9, 10) | | |
| 7 Coverage | section 6 of this file is current (derive 10.1) | | |
| 8 Prove | gates, audit, holdout once, replay, engine, paper, real at 0.03 SOL (derive 11, 12) | | |
| 9 Record | every result as three lines (derive 13) | | |

---

## 1. His logic (the portrait, derive 3)

> He buys **<what, when>** because he expects **<who spends next, and why>**; he skips
> **<what>** because **<reason>**; he leaves when **<what>** because then **<the reason is gone>**.

**Who he is:** <bot or person, reaction time, trades a day, clip, hold, one-shot or re-entry, how
many open at once>.

| clause | slot | his likely reason | the yes / no test (written before the run) | step | answer |
| --- | --- | --- | --- | --- | --- |
| | E | | | 5.1 | |
| | D / P | | | 6.1, 7, 9 | |
| | X | | | 8.0 | |

### The 20 trades

The 10 best by SOL and the 10 losers nearest his median loss, picked by that rule. One line each.

| # | coin, age, vsol | the minute before | the print before his buy (who, side, size, its own move, the quiet before it, his lag) | inside his hold | the print before his sell | result |
| --- | --- | --- | --- | --- | --- | --- |

What is the same in the best entries: <...>. Where the losers differ: <at the entry / only after>.
What he never buys: <...>.

---

## 2. His exit (derive 8.0)

| branch | plain rule | his likely reason | names his sell on (first crossing) |
| --- | --- | --- | --- |

| the frozen set against his own close | study | holdout (read once) |
| --- | ---: | ---: |
| his SOL / the set's SOL | | |
| his top 1 % / the set's | | |
| mean absolute SOL a trade between them | | |
| sells before him / within 300 ms / after him | | |

This set is the starting X of every book below, read from our fill.

---

## 3. Rule <n>

```
E  <the public print we fire on and its terms, in tape state>
P  <the permission terms>
X  <the exit: every branch resolves to a print index; filled at our lag>
D  <the door, or none>
R  <re-entry>
S  <the clip>
seat  both legs fill at the last print landed 83 / 115 ms after the decision print
```

Plain words: <one sentence, with the reason it pays>.

### The book

| | study tape | holdout (unseen) | ship bar |
| --- | ---: | ---: | ---: |
| tickets a day | | | - |
| %/trade | | | - |
| SOL, whole tape | | | - |
| SOL a day at the clip | | | - |
| days positive | | | >= 5/7, every holdout day |
| worst day | | | - |
| halves of the days | | | both > 0 |
| body (net without the top 1 %) | | | > 0 |
| top 1 % share of net | | | <= 15 % |
| biggest coin's share | | | <= 15 % |
| capped book (gains capped at the median take profit) | | | > 0 |
| exits on the graduation print | | | counted |
| the same book at 0.03 SOL (derive 12.12) | | | > 0 |

### Each slot, in one line

| slot | where it comes from | evidence |
| --- | --- | --- |
| E, the print | <the clause, the print class, his lag, lift, cover> | |
| E, the terms | <the 6.1 ladder: within-coin, both folds> | |
| X | <his own exit (section 2), then money on the selected pool> | |
| P | <the losers against the winners at the fire> | |
| D | <or none, with what was tried> | |

---

## 4. The chain

One row per step, in order, dead ends included. Every result is three lines in its last two
columns: red or green with the number; what is unread; the next idea.

| step | question (the clause it tests) | what it shows | what it does to the rule, and what is next | script | ev |
| --- | --- | --- | --- | --- | --- |
| 1 / 4.0-4.2 | Does he pay, at which seat, and is he a reader? | | | | |
| 2 / 3 | Who is he: shape, 20 trades, his logic | | | | |
| 3 / 8.0 | How does he leave? | | | | |
| 4 / 5.1 | What print does he answer, and at what lag? (WHO and history first, priced classes last) | | | | |
| 4 / 6.1 | Which of those prints does he take? (the ladder, never one fact alone) | | | | |
| 5 / 5.2 | Is the rise still ahead at our fill, against the random-print control? | | | | |
| 5 / 5.4 | Who lands inside our lag, and which next event does that name? | | | | |
| 6 / 6.2 | Does it hold as a public sentence on every coin, under his exit? | | | | |
| 6 / 7, 9, 8.2, 10 | Which coins, which fires die, which exit: the money ladder | | | | |
| 8 / 12.1 | Does it hold on unseen days? | | | | |
| 8 / 12.2-12.12 | Every slot re-derived on its own pool, then as the engine computes it, then real at 0.03 SOL | | | | |

---

## 5. Data and code

| where | what |
| --- | --- |
| `<node>/README.md` | the tracked scripts, and where the step scripts are kept |
| `node-derivation/data/<prefix>_*.parquet` | the candidate tables and outputs |

---

## 6. Coverage and the unread list (derive 10.1)

Created with every row **not tried**; updated in the same edit as each chain row. Marks: **tried**,
**partly**, **not tried**, **no data** (outside the data scope, or a sidecar that does not cover the
tape), **not his** (with the reason). A fact read alone, or under a placeholder exit, is **not
tried**.

| family | mark | what was run (chain row) | what is left |
| --- | --- | --- | --- |
| D1 creation fingerprint | not tried | | |
| D2 creator's document (URI) | not tried | | |
| D3 this coin's life before the fire | not tried | | |
| D4 loss door | not tried | | |
| D5 off-chain | no data | outside the data scope (derive 2.0) | - |
| E1 a listed ix structure acts | not tried | | |
| E2 silence, then a spend | not tried | | |
| E3 an operator's plan is unfinished | not tried | | |
| E4 a count crosses a line | not tried | | |
| E5 after sellers | not tried | | |
| E6 this print | not tried | | |
| E7 clock | not tried | | |
| P1 curve position | not tried | | |
| P2 windowed tape metrics | not tried | | |
| P3 skin in | not tried | | |
| P4 ix makeup of the recent tape | not tried | | |
| P5 tape state already true | not tried | | |
| X1 static | not tried | | |
| X2 the tape stops | not tried | | |
| X3 another actor acts | not tried | | |
| R re-entry | not tried | | |
| S size | not tried | | |
| 5.4 an earlier sign | not tried | | |
| 5.4 a slower part of the same move | not tried | | |
| 5.4 the other side | not tried | | |
| 5.4 another of his decisions | not tried | | |

**Unread, in the order to run it:** <the rows above that are not tried or partly, most promising
first, each with one line saying why>.

The next step itself is in [_!___workflow.md](../_!___workflow.md), the one queue, which points
here for the list.
