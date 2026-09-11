# <Node>: rule <n> and the member book

A new node's working file starts as a copy of this one; [hot-tape-rule-1.md](hot-tape-rule-1.md)
is the filled example. Fill each section as the phase of [method.md](method.md) that produces it
runs; an empty slot stays written as empty, never dropped. Scripts go in
`node-derivation/<node>/`, one per step, each opening with its step and question, and the
step-by-step index in `<node>/README.md`.

Numbers: [_!___evidence.md](../_!___evidence.md) <sections>. Code: [toolkit/](toolkit/README.md).

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

One row per step, in order, dead ends included.

| step | question | what it shows | what it does to the rule | script | ev |
| --- | --- | --- | --- | --- | --- |
| A2 | Which members pay, and at which seat? | | | | |
| B1 | What print does each react to, and at what lag? | | | | |
| B2 | Where does our fill land against it? | | | | |
| B3 | Which of those prints does it act on? | | | | |
| B4 | Does it hold as a public sentence on every coin? | | | | |
| C1-C3 | Which coins? | | | | |
| D1-D2 | How does it exit, on the selected pool? | | | | |
| E1 | Which fires die? | | | | |
| F1 | Does it hold on unseen days? | | | | |
| G0-G8 | Every slot re-derived on the sentence's own pool | | | | |

### What was tried and did not make it

| idea | result | why it is out |
| --- | --- | --- |

---

## 3. The member book

| member | RACE cap15 | days | its trigger (peak lift, lag) | its exit | status |
| --- | ---: | :---: | --- | --- | --- |

---

## 4. Data and code

| where | what |
| --- | --- |
| `<node>/README.md` | every script, by step, with its evidence section |
| `node-derivation/data/<prefix>_*.parquet` | the candidate tables and outputs |

## Open

1. <what is left, and the clean test on the days after the holdout>
