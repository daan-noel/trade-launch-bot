# 9Uq8GV (mid-tape one-shot): its logic, and where the method stands

One wallet, one case file (derive 13). The one member of the node whose exit is a plain clock, confirmed on money.

The method is the nine steps on the first page of [../_!___derive.md](../_!___derive.md).
Its **chain rows** - every step ever run on it, dead ends included - are the rows naming
`9Uq8GV` in the node chain, [mid-tape-rule-3.md](mid-tape-rule-3.md) section 2; the inventory
reaches them as `mt3 <row>`. Tapes, seat and frame: the head of that file. New work on this
wallet writes its rows here.

---

## 0. Checklist - read this first, every session

| step | state | where |
| --- | --- | --- |
| 1 Pick | done | row `3-4`: RACE +4.22 on 6/6 days, hold 9.7 / 16.0 / 16.2 s |
| 2 Portrait | **not done** | the shape is read (section 5 below); the 20 trades and the logic are not |
| 3 His exit | **read** | row `exit read 9Uq8GV` below: a clock at about 15-16 s, the first exit on this node read with `toolkit.own_exit` |
| 4 Test each clause | started | he answers a buy >= 1 SOL (lift 4.75); which of those buys is unread |
| 5 Our version | partly | leftover on his coins is +4.12 %/trade at clock 45, with **no control** |
| 6 Fit D, P, X | partly | public doors besides slow-wall are red, and all of it predates his exit being read |
| 7 Coverage | done | section 5 of this file |
| 8 Prove | no | - |
| 9 Record | done | the chain row below, and the node chain |

---

## 1. His logic (derive 3)

> **Partly written.** His exit is a hard clock at about 16 s, so he is not a harvester: he buys a size print and gives the coin a fixed 16 seconds to work. Why he expects a rise in those 16 seconds is unread.

Where: section 4 below; the 20 trades are unread.

---

## 2. His own exit (derive 8.0)

**Read: a clock at about 15-16 s.** On 801 closed study positions his hold runs p10 9.7 s, p50 16.0 s, p90 16.2 s, so p90 / p50 is **1.0**. Every trail and every abort is refuted on him: the curved trail sits 0.023 SOL a trade from his close and correlates 0.53, the 10 s abort 0.044 and 0.22, while a **15 s clock sits 0.0061 SOL a trade from his close and correlates 0.96**, the same on the fit days and the test days. At 12-14 s he sits through the crossing on 84-86 % of positions; at 17 s and beyond he is already out on 99 %.

Where: row `exit read 9Uq8GV` below.

---

## 3. His sentence

none.

---

## 4. The chain, this wallet's own rows

| step | question | what it shows | what it does to the rule, and what is next | script | ev |
| --- | --- | --- | --- | --- | --- |
| exit read 9Uq8GV | Is his exit static, and which rule sells where he sells? | 801 closed positions on `study_exact`, fires cut at 09-06 12:00, `toolkit.own_exit`. **Coverage on first crossing** over every branch of `own_exit.families`: no trail names more than 8.6 % of his positions and each is sat through on 43-53 %; the clock at his median hold names 9.9 % and is sat through on **1.6 %**. The clock grid is a step: 12 / 14 s sit-through 86.3 / 84.4 %, 15 s 82.9 %, **17 / 18 / 20 s 0.7 / 0.6 / 0.6 %** (he is already out on 99 %). **Money against his own close** (bought at his fill, zero latency, 0.2 SOL, same kernel): his own 4.33 SOL; clock 15 s 3.50, diff 0.0061 a trade (fit 0.0062, test 0.0060), corr 0.959; clock 16 s 0.61, diff 0.0068, corr 0.981, selling after him on 60 %; clock 14 s 3.62, diff 0.0081; curved trail p 0.3 19 % OR abort 0.56, diff 0.0231, corr 0.526; abort alone 6.95, diff 0.0439, corr 0.222; keep 0.5 armed +10 % -2.17, diff 0.0455 | **His exit is a clock at about 15-16 s**, and it is the only static exit on this node. The trail families are refuted on him. Two consequences: a book on him is priced under that clock, not under a trail; and "hold is reactive, not a timer, in every node" is a pooled reading - his p90 / p50 is 1.0 (law 27). The clock at 16 s earns less than at 15 s only because it sells just after him and pays the extra move | `toolkit.own_exit` (no step script; re-run from the module) | - |

---

## 5. Coverage and the unread list (derive 10.1)

Every inventory family, marked **tried** / **partly** / **not tried** / **no data** /
**not his**. Two rules decide a mark, and both bite here: a fact read **alone** is not
tried (derive 6.1), and a book priced under a **placeholder exit** is not tried (derive
8.0). Where a mark is uncertain it reads **not tried**, which keeps the idea alive; the
expensive error is marking something tried that was never properly read.

| family | mark | what was run | what is left |
| --- | --- | --- | --- |
| D1 creation fingerprint | not tried | - | the whole family |
| D2 creator's document (metadata URI) | not tried | - | the whole family |
| D3 this coin's life before the fire | partly | leftover on his coins is +4.12 %/trade at clock 45; public doors besides slow-wall are red | the same under his own exit rather than a 45 s clock |
| D4 loss door | not tried | - | the whole family |
| D5 off-chain | no data | outside the data scope (derive 2.0) | - |
| E1 a listed ix structure acts | not tried | - | the whole family |
| E2 silence, then a spend | not tried | - | the whole family |
| E3 an operator's plan is unfinished | not tried | - | the whole family |
| E4 a count crosses a line | not tried | - | the whole family |
| E5 after sellers | not tried | - | the whole family |
| E6 this print | partly | a buy >= 1 SOL, lift 4.75 in the 5.1 scan | which of those buys he takes (6.1) |
| E7 clock | not tried | - | the whole family |
| P1 curve position | not tried | - | the whole family |
| P2 windowed tape metrics | not tried | - | the whole family |
| P3 skin in | not tried | - | the whole family |
| P4 ix makeup of the recent tape | not tried | - | the whole family |
| P5 tape state already true | not tried | - | the whole family |
| X1 static | tried | row `exit read 9Uq8GV`: his exit IS static, a clock at about 15-16 s. Every trail, abort and keep-a-share branch is refuted against his own close | - |
| X2 the tape stops | not tried | - | the whole family |
| X3 another actor acts | not tried | - | the whole family |
| R re-entry | not tried | - | the whole family |
| S size | not tried | - | the whole family |
| 5.4 an earlier sign | not tried | - | the whole family |
| 5.4 a slower part of the same move | not tried | - | the whole family |
| 5.4 the other side | not tried | - | the whole family |
| 5.4 another of his decisions | not tried | - | the whole family |

**Unread, in the order to run it:**

1. **Step 2, the 20 trades.** His exit is known, so the question is narrow: what makes him expect a rise inside 16 seconds?
2. **6.1, which buys >= 1 SOL he takes.**
3. **The random-print control** beside his leftover pass.
4. Every door book on him predates his exit being read, so each is re-read under the clock.

The next step itself is [_!___workflow.md](../_!___workflow.md) section 2, which points
here for the list.
