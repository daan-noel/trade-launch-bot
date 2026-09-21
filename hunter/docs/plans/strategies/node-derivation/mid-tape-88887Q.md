# 88887Q (mid-tape one-shot): its logic, and where the method stands

One wallet, one case file (derive 13). The node's largest book. It answers a public SELL, which is the one direction where a late fill pays.

The method is the nine steps on the first page of [../_!___derive.md](../_!___derive.md).
Its **chain rows** - every step ever run on it, dead ends included - are the rows naming
`88887Q` in the node chain, [mid-tape-rule-3.md](mid-tape-rule-3.md) section 2; the inventory
reaches them as `mt3 <row>`. Tapes, seat and frame: the head of that file. New work on this
wallet writes its rows here.

---

## 0. Checklist - read this first, every session

| step | state | where |
| --- | --- | --- |
| 1 Pick | done | row `3-4`: RACE +52.94 on 6/6 days, +64.56 SOL his own, hold p50 22.6 s |
| 2 Portrait | **not done** | no portrait exists |
| 3 His exit | **not done** | no exit read |
| 4 Test each clause | started | row `5.1 88887Q`: he answers a seller at a loss (lift 5.27), a recent seller (4.16), a sell >= 1 SOL (8.49). Which of those sells he takes is unread |
| 5 Our version | partly | row `5.2 88887Q`: behind his buy the cost is 6.55-7.53 % and the peak leftover +3.13 to +6.45 %, with **no control** beside it |
| 6 Fit D, P, X | no | - |
| 7 Coverage | done | section 5 of this file |
| 8 Prove | no | - |
| 9 Record | done | the chain rows |

---

## 1. His logic (derive 3)

> **Not written.** He buys into public selling; why he expects the buyers back is unread.

Where: unread - step 2 is the next step.

---

## 2. His own exit (derive 8.0)

**Not read.** Hold p10 is 2.4 s against a p50 of 22.6 s, so part of his book is a fast cut and part is a hold: read the shape before assuming either.

Where: unread - step 3.

---

## 3. His sentence

none. His 5.1 and 5.2 rows are [mid-tape-rule-3.md](mid-tape-rule-3.md) section 2.

---

## 4. Coverage and the unread list (derive 10.1)

Every inventory family, marked **tried** / **partly** / **not tried** / **no data** /
**not his**. Two rules decide a mark, and both bite here: a fact read **alone** is not
tried (derive 6.1), and a book priced under a **placeholder exit** is not tried (derive
8.0). Where a mark is uncertain it reads **not tried**, which keeps the idea alive; the
expensive error is marking something tried that was never properly read.

| family | mark | what was run | what is left |
| --- | --- | --- | --- |
| D1 creation fingerprint | not tried | - | the whole family |
| D2 creator's document (metadata URI) | not tried | - | the whole family |
| D3 this coin's life before the fire | not tried | - | the whole family |
| D4 loss door | not tried | - | the whole family |
| D5 off-chain | no data | outside the data scope (derive 2.0) | - |
| E1 a listed ix structure acts | not tried | - | the whole family |
| E2 silence, then a spend | not tried | - | the whole family |
| E3 an operator's plan is unfinished | not tried | - | the whole family |
| E4 a count crosses a line | not tried | - | the whole family |
| E5 after sellers | partly | his 5.1 class: a seller at a loss, a recent seller, a sell >= 1 SOL, all at lift 3.45-8.49 | which of those sells he takes (6.1), and the same under his own exit |
| E6 this print | partly | sell >= 0.5 and down >= 2 % are in the 5.1 scan at 7.24-7.25 | read inside a ladder |
| E7 clock | not tried | - | the whole family |
| P1 curve position | not tried | - | the whole family |
| P2 windowed tape metrics | partly | a held state was read: `nw5` rank 0.65, `nb2` 0.60, rising nb2 >= 4 / npro2 >= 1 cover 11-12 % | the family inside a ladder |
| P3 skin in | not tried | - | the whole family |
| P4 ix makeup of the recent tape | not tried | - | the whole family |
| P5 tape state already true | not tried | - | the whole family |
| X1 static | not tried | - | the whole family |
| X2 the tape stops | not tried | - | the whole family |
| X3 another actor acts | not tried | - | the whole family |
| R re-entry | not tried | - | the whole family |
| S size | not tried | - | the whole family |
| 5.4 an earlier sign | not tried | - | the whole family |
| 5.4 a slower part of the same move | not tried | - | the whole family |
| 5.4 the other side | not tried | - | the whole family |
| 5.4 another of his decisions | not tried | - | the whole family |

**Unread, in the order to run it:**

1. **Step 2, the portrait.** He is the node's largest book and nothing says what he means.
2. **Step 3, his own exit**, before any entry book.
3. **6.1, which sells he takes.** His class covers 24-84 % of his decisions and nothing narrows it.
4. **The random-print control** beside his leftover pass: his reaction cost is 6.55-7.53 %, the highest of the node.

The next step itself is [_!___workflow.md](../_!___workflow.md) section 2, which points
here for the list.
