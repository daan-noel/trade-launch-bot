# Workflow: the open queue

What is open, in order, each with its next step and what kills it. The method, its gates and
how a result is recorded are [_!___derive.md](_!___derive.md); the basis and the laws are
[_!___strategy.md](_!___strategy.md); the numbers are [_!___evidence.md](_!___evidence.md); the
ideas are [_!___inventory.md](_!___inventory.md). A closed line keeps one row in the evidence
ledger (section 7) and, when a mechanism closed it, one row in strategy 8.1. This file changes
only when the open list changes.

---

## 0. The nodes

The instrument set is the 26 solo traders ([solo-traders.md](solo-traders.md)), five decision
nodes. A node is picked here, then derived by [_!___derive.md](_!___derive.md).

| node | status at our seat | where |
| --- | --- | --- |
| hot-tape re-entry | **rule 1 passes every ship bar**; the clean test is next | section 1 |
| mid-tape one-shot | **open**: 9999hu's sell >= 1 is a 5.2 kill; another class or a state next | section 2 |
| deep-age big clip | red: a public size print is not the tell (response equals the base); the remainder is which coin, not which print | evidence 7, agreement in section 4 |
| quiet deep-age | red: the token-silence burst it follows lasts ~80 ms, so a 115 ms fill is after it | evidence 7 |
| instant launch | dead at this seat: consumed inside ~2 slots | strategy 8.1 (age-0 launch ramps) |

Count machines, not wallets: 41 of 77 roster addresses are 7 machines (evidence 5.1).

---

## 1. Rule 1 and rule 1b (hot-tape): the clean test

Rule 1 (Flip-Catch - Bracket) passes every ship bar on the every-leg holdout, +4.44 %/trade,
100 a day, 5/5, top 1 % 9.8 %, and simulate books it ticket for ticket (evidence 1.22, 1.23).
Rule 1b (Flip-Catch - Room, the wall target) is its entry with the exit the holdout chose, so
only new days certify it (1.24, 1.25); it misses the study tail bar (top 1 % 15.5 %), so the clean
days must pass every bar, the tail included. Working file:
[node-derivation/hot-tape-rule-1.md](node-derivation/hot-tape-rule-1.md).

- **Next:** book the untouched days after 09-10, rule 1 and rule 1b side by side, with the same
  code; then paper at the 115 ms seat (live paper books `worst_case` fills, not this seat); then
  small real.
- **Kills it:** a ship bar failing on the clean days.
- **The loss-door clone joins the clean test** (evidence 1.28): rule 1 and rule 1b with
  `m_holder_book.public_app_share >= 70` and `bundled_share <= 28.57`, holdout +5.61 / +6.84
  %/trade, trades worse than -40 % 22 -> 11 / 10, simulate equal to Python ticket for ticket. The
  rules `Flip-Catch - Bracket + Door` / `- Room + Door` are in the DB, paper, inactive; they run
  once the server has migration 0017 and the new bins
  ([left-tail plan](../../roadmap/hot-tape-rule-1-left-tail-plan.md)). 09-11..09-12 are read (case
  file L10); the clean test goes on day by day, all four side by side.
- **More trades a day need a second event, not a looser rule 1** (evidence 7, rule 1c). The open candidate is
  49uohd's capitulation dip-buy: +3.50 %/trade 8/8 on its picks at our seat under a clock (a 5.3
  column), red in every public spelling (1.16). Its sell >= 1 class is a 5.2 kill behind its buy
  (cost 2.69 %, 1.27): run 5.2 on the dip-buy picks as 1.16 defines them before its descriptors
  (fall, burst, busy tape, seller at a loss, size) are fitted as one within-coin score. AbQcLH's
  burst start is a 5.2 kill (cost 4.55 %).

---

## 2. Mid-tape one-shot: instrument 9999hu

**Derive 5.2 kills this trigger** (evidence 1.27): behind its buy the reaction cost is 8.97 %, 93 %
of tickets over 2 % (88887Q 7.01 %). A read over all acted tickets counts the ones ahead of its buy
(cost 0), where its own buy is our leftover: a copied fill. Peak leftover behind its buy is still +12.6 % at
its hold p50, the volatility of a 9 % move, and the book below agrees with the kill. Public
occupancy of that class at its fire time is red. The working exit on the acted pool
is take profit +15 %, stop -40 %, 70 s: +1.61 % 7/7, body +4.47, and it fails the tail (top 1 %
43 %, capped book red). P stays none: the keep rule takes no cut on stop-outs vs take-profits.
The line's ledger row: evidence 7 (mid-tape 9999hu). Working file:
[node-derivation/mid-tape-rule-2.md](node-derivation/mid-tape-rule-2.md).

- **Next:** another class or a state for 9999hu (derive 5.2: no X, P or D saves a kill); no D
  search on this parent.
- **Do not:** freeze age <= 16 s occupancy (it is the launch first-sell, fire age p50 2 s); copy
  its close (a 20-30 s give-back); AND nb2 onto sell >= 1 occupancy; copy rule 1's holders x age
  (under the floor on a 15 s fire).
- **9Uq8GV:** its `buy >= 1` is a 5.2 kill (reaction cost +9.2 %); leftover behind its buy on its
  burst start and on the nb2 rising edge is unread - run 5.2 on those acted tickets
  ([mid-tape-rule-1.md](node-derivation/mid-tape-rule-1.md)). 88887Q is the same tell as 9999hu
  and stays off; 8dtx2t's public burst START is C2 (section 3) and does not ship on its own.

---

## 3. Thirty days of prints (C7): a calendar item

Every door-behind sentence waits on clients, not terms: a week of prints holds 34 slow-wall
builds, a door cell draws on about 20, and one carries each book; thirty days holds about 150
(evidence 3.1a). The lake's sealed days start 2026-09-01 and Postgres keeps a rolling 30, so the
tape grows one day a day and reaches thirty consecutive lake days around 2026-09-30. Then the frozen slow-wall burst-start cell re-runs unchanged; the
sentence, what it fails today (tail, per-day floor) and the recheck step by step are
[frozen-sentence-recheck.md](../../roadmap/frozen-sentence-recheck.md).

- **What must not slip:** the lake is the only durable copy and its export runs by hand; PG drops a
  day at 30 days. Check that `hunter/lake-data/trades/dt=*` holds yesterday at the start of a session.
- **Do not** run another AND on the frozen cell while waiting.

---

## 4. Lines open with a named empty slot

The number stands; the verdict does not. Each line names what it needs.

| line | what stands | what it needs |
| --- | --- | --- |
| L-selection (which coin goes to -50 %) | bundle share < 0.20 cuts the rate 14.1 % -> 2.3 % on door-v3 MONEY and lifts the book +5.39 -> +10.43 SOL inside a fixed reserve band; red alone on the full tape (evidence 3.7) | a sentence that clears the client gate (section 3) |
| agreement among the solo 26 | 0.37 lift on the -50 % rate against a 0.82 random null, out of sample, stacks with bundle share (evidence 6.8) | an ix-structure or tape-state twin; as named wallets it is refused as a term |
| state-conditional exits | a loss-only cut takes L 31.5 -> 10.1 while W holds, break-even 23.4 % -> 9.0 %, and triples the tickets (evidence 4.7) | a sentence whose added round trips pay their toll |
| the independent-machine count (hot-tape) | a money gradient, -4.08 % -> -2.99 %, that does not cross zero (evidence 1.11) | the machine vector fitted, not cut |
| documented-project rule | holdouts +1.46 % and +0.64 %/trade, 4/7 each, under 1 SE from zero (evidence 3.3) | paper |
| burst START on the mid-tape traders' coins | +2.6 .. +6.7 %/trade on every exit there, red on the full tape (evidence 5.4) | a public door; every public door tried is red (6.4; evidence 7, mid-tape rows) |
| machine census, exact `ix_labels` | the best structures reach the toll in money alone, on a price-path parent (evidence 5.7) | a door and an exit, on a parent that is not the price path |
| metadata document as a convexity term | flat on two event families | the burst-start event |
| telegram-first | the smallest deficit measured; still red on money | more measurement |
| Axiom push | priced at slot +1 with a take profit on a convex book | our seat and a harvester exit |
| off-chain attention (feed rank, replies, livestream) | not stored at decision time | a data job: store it live; a delayed fetch is not it |
