# Backtest audit: the checks every number passes before it is a result

A study number is a claim until it passes every check below. It is not quoted as a result - in a
reply, a case file, the evidence file or the workflow - and no engine work starts on it, until
each line is answered. A line that cannot be answered is written beside the number as open.

Each line is a logic mistake that has already produced a positive study book that was false. The
rule's reasoning lives where the link points ([_!___derive.md](_!___derive.md),
[_!___strategy.md](_!___strategy.md) 7.4 and 9); this file is the list that walks them, in order.
Rows marked **run** are code: their output is pasted beside the number.

---

## 1. Verification: the replay (run)

| # | check | how | the case that set it |
| --- | --- | --- | --- |
| V1 | An independent replay rebuilds the book | Code that imports nothing from the study (no `toolkit/`, no `kernel.py`, no candidate table) reads the lake: every coin born in the window, every leg, every wallet. It first reproduces the study book to the ticket (same trigger print, SOL equal), then removes the study's filters and spellings one at a time, each with its own row. Template: [mt_d8_replay.py](node-derivation/mid-tape/mt_d8_replay.py) (`slim`, `cands`, `repro`, `book`); the hot-tape pair `r1_exact.py` / `r1_exact_check.py` | Rule 3b: the study book (+5.00 %/trade, 6/6) is reproduced to the ticket, and one removed filter turns it into -3.10 % 0/6 (evidence 7) |
| V2 | A second code that shares an idea cannot catch that idea | Each term's input is read against an independent exact field of the lake (derive 12.11) | Rule 1's holder count is float dust in two agreeing replays (evidence 1.21, 1.22) |
| V3 | Every window, every day | Study (in sample, labelled so), holdout, the days after it: each its own row. The per-day list beside every book, never a mean alone (strategy law 19) | Rule 3b red on study, holdout and new days alike; a per-day floor averaged over a week passes on two days of six |
| V4 | The data is complete | Each lake day's row count equals Postgres, and no minute of the fire window is empty; a short day is refused, not booked | 09-02 holds 41 empty minutes; a missing print reads as silence, and silence freezes price at the entry |

## 2. Universe: nothing known after the fire

| # | check | how | the case that set it |
| --- | --- | --- | --- |
| U1 | No coin kept or dropped on its future | No floor on a coin's print count, life, peak reserve or graduation in any table, scan, leftover or hazard (`toolkit/candidates.py`, `seat.py`, `hazard.py`, `trigger.py` carry none). Split the book by the coin's full life to see what such a floor would hide | Rule 3b: 40.4 % of fires sit on coins that end with < 60 prints, at -14.2 .. -14.9 %/trade; a >= 60 prints / >= 60 s floor removes them (derive 14) |
| U2 | A candidate table is not the universe | The book reads every print of the event on every coin; the query that builds the universe is part of the rule (strategy 7.4, laws 1 and 15) | strategy 9 |
| U3 | A missing sidecar row is not a fail | A left-join that fills missing with False is a filter: print sidecar rows, tape tokens and their overlap first (law 30) | +47.54 SOL with the sidecar, -1,034.87 without it |
| U4 | The event is not vacuous on a coin's first print | Report the share of fires at local index 0 (law 31) | 818 of 1,622 fires at `k == 0` carry +46.08 of +47.54 |
| U5 | Study fires stop at the holdout start | The script cuts fires at 09-06 12:00 itself (`study_exact` holds prints to 09-06 24:00); the toolkit's `T.day` counts from 08-30, so day 2 is 09-01. Print the first and last fire time (derive 2.1) | A day cut on `T.day` read as 09-01 does nothing, and thresholds fit on holdout fires |
| U6 | No ticket on a coin born before the tape | Its holder book and history miss its early life | evidence 1.21 |
| U7 | No fire the engine cannot see | A fire after the engine's dead verdict (real SOL < 30 and no trade >= 0.1 SOL for 300 s, `deadness.rs`) is dropped | evidence 1.23: two reference-only tickets on retired coins |

## 3. Fill, clock and cost

| # | check | how | the case that set it |
| --- | --- | --- | --- |
| F1 | The fill is a print that landed by the fire + lag | Both legs: the last print landed by fire + lag, never the next print after it, never before the fire print; the engine's LagMs rule (the fire's slot or the next observed slot <= 3 slots on) beside it (derive 2.3) | The next print as the fill is +8 to +12 pp of look-ahead (strategy 9) |
| F2 | The seat is measured, and the fill model is checked on real fills | Re-read the seat from `strategy_positions` real fills; check the model against the state our real buys met ([mt_d8_fillcheck.py](node-derivation/mid-tape/mt_d8_fillcheck.py), **run**). Book at 50 / 83 / 115 / 200 / 300 ms and at the real lag mix | The seat reads 83 ms, not 115; the model prices the state our buy met in 98.4 % (evidence 1.1) |
| F3 | Split every book by age at the fire | No fire inside the first second (the fill model cannot see the pre-sent sniper queue); the mid-tape frame is age >= 10 s (derive 9, 14) | Age < 0.5 s holds 72 % of a pool's SOL at a 0.0 % pick share; rule 3b's fires at 1-10 s (47 %) book -3.15 / -5.28 %/trade |
| F4 | A clock exits at its deadline | The state landed by entry + clock + lag (or the engine's 200 ms tick), not the last print before the deadline plus lag | Rule 3b's study exit reads the earlier state (a small effect; the replay reads both) |
| F5 | Stops and graduation | A stop fills lag after the print that trips it, not at its level; exits on the graduation print are counted and the SOL must not rest on them | A stop fills about 3.5 % past its level (strategy 9) |
| F6 | Silence freezes price | A silent hold exits at the last print, never at -100 % | strategy 8.1 |
| F7 | Full cost, on the priced reserve | 125 bps a leg, the fixed cost of each leg, own impact on `vsol` (never the real reserve), both legs; the engine kernel on spot beside the curve kernel | Impact on the real reserve reads 4.62 pp a trade instead of 0.66 |

## 4. Terms: spelled as a live engine computes them

| # | check | how | the case that set it |
| --- | --- | --- | --- |
| T1 | No wallet in a term | "Public" that leaves out the roster wallets is an identity term; book the every-wallet spelling beside it (law 20) | Repeated on rule 1 and rule 3b |
| T2 | Holders are counted, not floated | A reserve-sized float bag never returns to zero; spell distinct buyers (creator out) or the exact `token_amount` book | Repeated on rule 1 and rule 3b: 647 float "holders" against 96 exact on the same coins |
| T3 | Every fact from prints landed by the fire print | Nothing after the fire print is a term, a filter or a label; a window that holds the member's own print is look-ahead (derive 6; strategy 7.4, law 2) | 44.5 of 47 points of an apparent selection edge land after the decision point (strategy 7.4) |
| T4 | 6.1's ignored rows are every class print while it is flat | A table cut at its first buy makes its pick the coin's last candidate: age, holders and SOL bought then rank about 1.0 (derive 6.1) | 8dtx2t's first 6.1 read |
| T5 | The build recipe as the engine keys it | Missing labels are no recipe (`flow_ix::build_hash`) | - |
| T6 | Each term exists in the engine at the same basis | Or the metric system is extended first (strategy 7.4, law 14; derive 12.11) | Rule 1's recipes-with-the-print and entry fill (evidence 1.22) |

## 5. Method: the refusals already broken once

| # | refusal | where the rule lives |
| --- | --- | --- |
| M1 | "Every fire on every coin is red" under a prior clock is not a verdict while D, P and X are unread | derive 6.2, 10, 14 |
| M2 | A gap before its first buy is the 7.1 door test, not hindsight | derive 7.1 |
| M3 | The 5.1 scan covers WHO and history, not only priced classes; a scan older than its class list is re-run | derive 5.1, 14 |
| M4 | First entry and re-entry are one E when 5.1 names the same print; re-entry is R | derive 3, 14 |
| M5 | Age < 10 s is not the mid-tape prize | derive 9, 14 |
| M6 | 5.2 reaction cost is a diagnostic; the kill is peak leftover <= 0 or missed >= 50 % | derive 5.2 |
| M7 | No "too slow for our seat" before the seat is re-measured and every class scanned | derive 0, 14 |

## 6. What goes beside every number

The replay's match line (V1), the windows and the per-day list (V3), the seat ladder and the real
lag mix (F2), the age split (F3), the life split (U1), top 1 % share, biggest coin, and a coin
bootstrap interval. A number without them is reported as a study read, never as a result.
