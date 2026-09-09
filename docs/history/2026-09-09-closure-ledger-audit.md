# 2026-09-09 - the closure ledger was audited, and most of it was closed for the wrong reason

Two months of strategy work had produced 44+ refuted lines and no shippable rule. This audit
re-read every closure in the strategy and evidence files against the measurements behind it, then
re-ran the measurements that the closures depended on. **The numbers were almost all correct. The
verdicts drawn from them were not.**

The reference files were rewritten on 2026-09-09 around what this entry records:
`hunter/docs/plans/strategies/_!___strategy.md` (the basis), `_!___evidence.md` (standing
measurements only, by coordinate) and a new `_!___workflow.md` (the method and the campaign
queue). Superseded figures that the rewrite dropped are preserved here.

## The root cause

Every study followed the same loop: pick an idea, build a parent around it, score it **alone**,
get a red number, record "X is closed", move on. That loop cannot find a conjunction, because an
unconcentrated pool is red by construction - so each term alone was guaranteed red and every
negative was pre-ordained.

The second half of the cause was bookkeeping. A result was recorded as a **verdict** rather than
as the **coordinate** it was measured at. "Mid-tape entry is closed" instead of "the sentence
(door = none, event = router buy >= 0.5 SOL, permissions = none or one at a time, exit = clock 30
/ trail50 c600, seat = 115 ms) is red". With the coordinate written down, the empty slots would
have been visible on the page and nobody could have closed a slot from one run.

## What was measured on 2026-09-09

All runs reproduce through `hunter/docs/plans/strategies/study-kernel/` (`audit_exits.py`,
`audit_conj.py`, `audit_conj2.py`, `audit_conj3.py`, `audit_tailbar.py`, `audit_final.py`,
`audit_bar.py`, `audit_exit2.py`). Acceptance: the harness reproduced the recorded parent book to
the cent (-377.80 SOL, -4.55 %/trade, 0/8 days against the recorded -378, -4.55 %, 0/8).

**1. The bar was wrong.** The convexity program judged everything against
`P(this swing >= 50 %) >= 50.7 %`. Each book's own realised break-even `L / (W + L)`, measured on
the same fires, is **46.4 %** under clock 30 and **29.0 %** under trail50 c600 - it moves 17
points with the exit alone. The deficit against the money label is 8-10 points, not the 18.8
recorded. The cell recorded as "4.6 points short" (Terminal + cu p75 + 2 builds) books
-0.40 %/trade, 4/8 days at 278 first-per-mint a day, with win 42.3 % against its own break-even
43.0 %: **0.7 points short**, above the ticket floor.

**2. The exit hypothesis was tested and refuted.** Re-reading section 3 under tail-preserving
exits makes the unselected parent *worse*, not better: -4.55 % (clock 30) becomes -10.74 %
(trail50 c600) and -10.09 % (tp100/tr50/c1200). `W` rises 23 -> 87 but `L` rises 20 -> 36 and the
win rate falls 36 % -> 20 %. The conclusion "no exit could have hidden a positive" stands; its
stated reason did not. The correct statement is that an exit cannot be judged on an unselected
pool at all.

**3. Exit choice is nonetheless worth 7.6 points on the same fires** (-10.74 to -3.11), which is
inside the range the docs attributed to selection alone. Fifteen exits were priced on both
parents. A static loss cap cuts `L` 60 % (31.6 -> 12.7) and `W` 75 % (84.7 -> 21.0): the winner
dies faster than the loser, so no static exit reaches the shape a professional runs (8dtx: 1.1 %
of trades worse than -20 %, 4.2 % above +50 %; the best static cap buys 7.7 % worse than -20 % and
loses the right tail).

**4. The conjunction search the docs never ran.** Every 2- and 3-term conjunction of 16
decision-time terms, both event families, both exit families, occupancy enforced:

| parent | combos | above the floor | positive | tail-robust | both halves | walk-forward best 10 |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| zigzag TURN keep | 650 | 522 | 8 / 17 | 0 | 1 / 0 | +23.4 -> -18.7 ; +74.9 -> -40.5 |
| machine print in the dip | 166 | 141 | 15 | 0 | 1 | +57.7 -> -19.4 |

A conjunction moves an unselected parent from -4.55 % a trade to about zero and stops there. It
buys "this coin is not decaying", which is worth the toll.

**5. The tail bar was calibrated.** "Carried by the top 1 %" had been used as a disqualifier with
no reference. On the prize book (trough entries on real >= 50 % episodes, tp100/tr50/c1200:
+26.99 %/trade, 8/8 days), subsampled 200 times at matched trade counts, the top 1 % of trades
carries **9-12 %** of net and 100 % of subsamples are positive. Every candidate found in the
scans carries 49-273 % there. The bar is real and now has a number.

**6. The metadata document does not add on a machine-print event.** A floor-free 4-term scan threw
up five cells containing the document that passed money, both halves and top-1 % removal; the
ablation killed it. `document` alone books -18.19 SOL (0/7); `terminal + creator_in + document`
+2.38 on 166 trades against `terminal + creator_in` **without** it at **+8.83 on 1,465**. It
removes 90 % of the tickets and keeps 27 % of the money.

## The verdicts that changed

**Mid-tape one-shot (recorded as "closed at our seat as 8dtx, 3Xk2 and the racer four run it").**
The node study's own table prices the burst-start print at 115 ms on both legs and books
**+2.6 to +6.7 % a trade on all four exit families**. The full-tape version of the same event with
no door books -1.5 to -13 %. The seat, the event and the exit are exonerated by the measurement;
the door was never filled. Three errors produced the closure: the door slot stayed empty and the
blame went to the seat; a seven-wallet node was closed on its two racer members, who hold the two
smallest books in it while its two largest have never been studied; and those members do not even
share an exit (trades losing more than 20 % run 2.6 % for 8dtx and 27.7 % for 3Xk2). Recorded now
as **open, blocked on the door**.

**Everything closed from a single gate.** Machine census and exact `ix_labels`, holder-book terms,
the launch-build door "as a trade", telegram-first, the metadata document as a convexity term:
each was measured with a red number at one coordinate with slots empty. The conjunction scans now
supply the missing evidence for the two event families they cover, and the closures that remain
are recorded with the empty slot named.

**Telegram-first** had been closed as "33.6 % against 51 %", comparing a slice's detection to the
parent's break-even. Its own break-even is 41.4 % against 34.6 % have - the smallest deficit
measured anywhere in the file.

**"The deficit is invariant at about 20 points wherever you stand on the move"** was generalised
from five rows of one trigger family at one exit. Within a burst the gradient is real and steep
(+6.7 % at the burst start against -5.5 % at the trader's own print); the invariance claim is
withdrawn.

**Axiom push** was closed at slot +1 - a worse seat than ours - with a take-profit on a convex
book, which the same documents forbid.

**Campaign-break v0** passed a disjoint holdout and engine reconciliation at +2 slots and is
recorded as "re-price at lag_115 before sizing". What later carried a 115 ms number was a *wider
family* (camp / camp + no-extraction / + returning + live + creator-in) on a differently-named
machine, and the node was then treated as closed. The frozen sentence itself has never been
priced at our seat.

## Figures the rewrite dropped, preserved here

The evidence file was rewritten to carry only standing measurements. These superseded rows are
kept for the record.

**The detection master table** (all against a break-even of 50.7 % taken from the clock-30 parent,
which is the error): every zigzag turn 31.3 %; first print off the low 25.4 %; the 155-structure
`ix_labels` dictionary 31.0 %; holder state 29.7-34.4 %; Terminal + CU p75 + 2 builds 46.1 %; the
metadata document 30.7 % against a 48.5 % bar; unique-new-buyer acceleration 26.2 % against 66.9 %;
campaign restart 24.7-29.4 % against 63.5 %; a 1,200-wallet oracle 31.0 % against 49.6 %, and the
coins it skips 32.4 % against 74.8 %; 8dtx's own mints 31.2 % against 43.2 %; episode-1 wallet
resume 31.7 % against 61.6 %; telegram-first 33.6 % against 41.4 %; pump.fun `complete` (a
look-ahead) 42.1 %.

**The constant-deficit table**, which the invariance claim rested on: TURN clock 30 break-even
50.7 % against 31.9 % have (18.8); TURN + project 48.5 against 32.0 (16.5); RESTART 63.5 against
42.4 (21.1); RESTART pusher 65.4 against 46.4 (19.0); RESTART pusher already up 82.1 against 62.1
(20.0).

**The honest books on the zigzag parent**: clock 15 -3.64 %, 0/8, -351 SOL; clock 30 -4.55 %, 0/8,
-378; trail15 c120 -4.57 %, 0/8, -554; trail30 c300 -6.06 %, 0/8, -429; arm10 t20 c300 -6.81 %,
0/8, -444. Oracle counterparts on the same turns: +9.03 % to +17.91 %, 7-8/8 days.

**Section 3.10's own sentence** (Terminal + creator in + crowd gone, trail40 c600): 324 trades,
38.0 first/day, +4.57 SOL, +7.05 %, 4/8 days, top 1 % = 105 % of net, one coin 46 %, -0.21 without
the top 1 %. It did not freeze and did not ship, correctly.

**Smaller corrections found in the same pass**: the episode census subtracted across two different
censuses (21,711 episodes >= 100 % at a 20 % retracement, then 21,217 from the 15 % zigzag
decomposition used as though it were the same number); section 2.2 quoted a 3.2 % required hit
rate for a door book while section 4.1 derived 14.5 % for the same book at the same exit; the node
table carried two rows for one node (quiet deep-age and campaign rider); the 155-structure
dictionary was listed as an independent axis when 85.3 % of parent turns match it by construction;
and the phrase "the racer four" named a set of three that already contained the two wallets listed
beside it.

## What changed in the discipline

- Every result is recorded as a **six-slot coordinate** (door, event, permissions, exit,
  re-entry, size) plus its seat. No result may be stated without naming which slots were empty.
- A line enters the closed ledger **only** with a measured mechanism. Three qualify: copying a
  wallet's fill, the wave's first buy, and silence freezing price. A red number produces "this
  sentence is red".
- **Money is the only score**, judged against the cell's own realised break-even, under both exit
  families, with the ticket floor, the calibrated tail share and a walk-forward split reported on
  every line.
- The instrument set is the **26 solo traders, node by node**. A node is never closed on a subset
  of its members, and "his fill is unreachable" is never written as "his decision is unreachable".
