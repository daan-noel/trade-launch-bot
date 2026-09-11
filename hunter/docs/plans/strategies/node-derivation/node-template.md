# <Node>: rule <n> and the member book

A new node's case file starts as a copy of this one; [hot-tape-rule-1.md](hot-tape-rule-1.md)
is the filled example. Fill each section as the phase of [_!___derive.md](../_!___derive.md)
that produces it runs; an empty slot stays written as empty, never dropped. Scripts go in
`node-derivation/<node>/`, one per step, each opening with its step and question; they stay
local unless a rule or a gate needs them re-run. Toolkit calls for each phase:
[toolkit/README.md](toolkit/README.md).

This file is the record of every step (derive section 13). Numbers a rule, a law or an open line
stands on: [_!___evidence.md](../_!___evidence.md) <sections>.

Tapes: the **study tape** (<from> to <to>, <n> prints, <days> days), where every threshold is read;
the **holdout tape** (<from> to <to>), where changes are only confirmed.

---

## 1. Rule <n>

```
E  <the trigger print and its terms, in public tape state>
P  <the permission terms>
X  <the exit: every branch resolves to a print index; fill 115 ms later>
D  <the door, or none>
R  <re-entry>
S  <the clip>
seat  both legs fill at the last print landed 115 ms after the decision print
```

Plain words: <one sentence>.

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

### Each slot, in one line

| slot | where it comes from | evidence |
| --- | --- | --- |
| E, the trigger | <member, print class, lag, lift> | |
| E, the terms | <within-coin contrast; then money on the sentence's own pool> | |
| P | <the losers against the winners at the fire> | |
| X | <the member's closing hazard on the selected pool; then money> | |
| D | <or none, with what was tried> | |

---

## 2. How rule <n> was derived: the chain

One row per step, in order, dead ends included. The phase is [_!___derive.md](../_!___derive.md)'s.

| step | question | what it shows | what it does to the rule | script | ev |
| --- | --- | --- | --- | --- | --- |
| 4 | Which members pay, and at which seat? | | | | |
| 5.1 | What print does each react to, and at what lag? | | | | |
| 5.2 | Does leftover exist at our 115 ms fill on the fires it takes? | | | | |
| 6.1 | Which of those prints does it act on? | | | | |
| 6.2 | Does it hold as a public sentence on every coin? | | | | |
| 7 | Which coins? | | | | |
| 8 | How does it exit, on the selected pool? | | | | |
| 9 | Which fires die? | | | | |
| 12.1 | Does it hold on unseen days? | | | | |
| 12.2-12.11 | Every slot re-derived on the sentence's own pool, then as the engine computes it | | | | |

---

## 3. The member book

| member | RACE cap15 | days | its trigger (peak lift, lag) | its exit | status |
| --- | ---: | :---: | --- | --- | --- |

---

## 4. Data and code

| where | what |
| --- | --- |
| `<node>/README.md` | the tracked scripts, and where the step scripts are kept |
| `node-derivation/data/<prefix>_*.parquet` | the candidate tables and outputs |

## Open

1. <what is left, and the clean test on the days after the holdout>
