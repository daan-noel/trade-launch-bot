# Hot-tape rule 1: cut the left tail with D x P, not the stop

Rule 1 ([hot-tape-rule-1.md](../plans/strategies/node-derivation/hot-tape-rule-1.md)) loses more
than 60 % on 3.4 % of its trades. The node's paying members almost never do. This plan finds what
separates those trades at the fire (P, D) or early in the hold (X3), and ships the result as a
clone of rule 1, never as a patch to it. Method: [_!___derive.md](../plans/strategies/_!___derive.md)
phases 7-9, 11, 12; laws: [_!___strategy.md](../plans/strategies/_!___strategy.md) 7.4.

## 0. What the tail is

Simulate, rule 1 (Bracket), lake 09-01..09-10, 0.2 SOL, `lag_115` (the same book as evidence 1.23):

| | trades | SOL |
| --- | ---: | ---: |
| the whole book | 1,051 | +9.41 |
| stop-outs, all worse than -60 % (fills -62..-75 %, p50 -63.4 %) | 36 | -4.75 |
| clock exits at -40..-60 % | 30 | -2.96 |
| **the left tail, worse than -40 %** | **66 (6.3 %)** | **-7.71** |

- 36 stop-outs sit on 36 different coins: not a client, not a re-entry chain (23 first entries,
  13 later ones; the stop rate is 3.3 % on a first entry, 3.6 % on a later one).
- They are younger at the fire: age p50 275 s against 409 s for the rest.
- They die inside the hold: p50 52 s, p10 17 s.

The exit cannot carry it (evidence 1.22, 1.24): stops at -60 %, -75 % and none book the same; a
-30 % stop and every cut under water fail the folds, because the winners' own trough is -5.8 %
at the median. A rug is an L-axis problem (derive 8), so the slots are P and D, with X3 only
where the dump comes in legs.

## 1. Target

A new sentence, rule 1's E unchanged, that at least halves the left tail (< -40 %) with total SOL
not lower and every ship bar held (derive 11), under both exits (Bracket, Room).

## 2. Steps

Steps 1 and 2 are read (case file steps L1-L3): the members avoid the tail by which fires they
take, never by an exit; the tail is a distribution wave led by the fire's top holders, not a
one-print rug; bundle and creator share read 0 on this pool; once under water, who sells carries
nothing beyond price. Step 4 therefore has nothing to read, and step 3 is the remaining slot, its
first candidates age (AUC 0.37), top-1 share (0.59) and fresh-60 s share (0.56).

Step 3 is read (case file step L4): no cut on age, top-1 share or fresh-60 s share passes the
keep rule under either exit, so the kill (section 3) holds on money. Age >= 250 s cuts the tail by
about 40 % at the same SOL on the study days; it stays a risk dial for the owner's call, and only the days
after 09-10 can certify a clone that carries it.

The rug door is read (case file steps L5-L7, evidence 1.28): public-app share of live supply >= 0.7
is learned on the market's one-slot rugs, bundled-buyer share <= 0.2857 on rule 1's tail, and the
pair, frozen, passes its bar on the holdout under both exits (tail 22 -> 12, SOL up).

Step 6 is built (case file L9): `m_holder_book` (`public_app_share`, `bundled_share`) over the daily
`build_breadth_day_stats` table (migration 0017), the past-only spelling the engine can read, and
simulate books its Python reference ticket for ticket on the holdout and on 09-11..09-12. The clone
rules `Flip-Catch - Bracket + Door` and `Flip-Catch - Room + Door` sit in the local DB, paper and
inactive. What remains: migration 0017 and the new bins on the server, the two rules on paper beside
rule 1 and rule 1b, and the clean days (the first two read, case file L10).

Every step reads `study_exact` (lake 09-01..09-06 12:00, every leg) and chooses there. The
holdout (`holdout_exact`) books a frozen change once. The days after 09-10 stay untouched.

### Step 1: how the members avoid it (a thermometer, never a term)

- 1a. The loss distribution of the paying members' own episodes (hot-tape 8fStGV, AbQcLH,
  49uohd; mid-tape 9999hu and the node's other members) at their seat AND at ours under their own
  exit: the share worse than -40 % and -60 %. If their decisions also go to -60 % at our seat, the
  gap is their seat and their exit reaction; if not, it is selection.
- 1b. On rule 1's 66 tail tickets against the rest: did a member buy the coin within 300 ms of the
  trigger; did it hold the coin and sell inside our hold, and at which print relative to the
  drop. G0 (evidence 1.20) read the member's picks on money only, never on the tail.

Decides: selection (steps 3) or an early exit reaction (step 4), or both.

### Step 2: the anatomy of each loss

For each tail ticket, every sell from the fire to our exit, from an exact token book (`vtok`
deltas per leg, integer; never a reserve-derived bag, G10): who sold (creator, creation-slot
wallets, the top 10 holders at the fire, wallets that bought in the last 60 s, the trigger
seller), how many prints, the largest single print's price drop, time from the fire to the first
-10 % print.

- The drop is one or two prints (one bag dumped): no exit fills ahead of it at 115 ms; only P or
  D (who holds that bag at the fire) can cut it.
- The drop comes in legs over seconds: X3 is live (step 4).

The book is built from trades only; a wallet-to-wallet transfer is invisible to it, and the
share of tail tickets whose seller has no buy on the tape is reported beside the result.

### Step 3: P and D at the fire (derive 9, then 7)

Facts from prints before the trigger, tail tickets against the rest, AUC, money by quintile
inside a reserve band (law 21):

| candidate | inventory | note |
| --- | --- | --- |
| bundle share of live supply | D4, keep (ev 3.7) | never read on rule 1's pool |
| creator share of live supply | D4, open | |
| creator has sold | P3, keep | inverts on the L axis (ev 3.7): read, not assumed |
| top-1 and top-10 holder share | new | 1.14: top-10 share 0.56 on dying frenzies |
| overhang: held supply at >= +50 % unrealized | new | the bag that can dump |
| share of the last 60 s's buy SOL still held | 1.14 (0.42) | |
| one wallet's share of the frenzy's buy SOL | new | a manufactured frenzy |
| age | P1, in the rule | the tail is younger; a kept threshold is re-read under the tail label |
| whatever step 2 names | | |

A one-sided cut is applied before occupancy (a refused fire frees the coin), chosen by the keep
rule with the chance floor (`walkforward.cut_noise`), both exits. Taken only when total SOL rises
and every bar holds; a cut that raises SOL only by removing the tail's winners is not taken.

### Step 4: X3, an actor sells (only if step 2 finds legs)

Exit when a holder with >= X % of supply at the fire (or the creator, or the creation-slot
cohort) sells >= Y % of its bag while we hold. Every branch resolves to a print index (law 24);
the exit fills at `LagMs(115)`, away from us on a falling tape (fill / trigger 0.964-0.977).
Walk-forward, both exits. "Creator sells" alone is red on rule 1 (1.24); the holder-weighted
form is new.

### Step 5: freeze, holdout, clean days

The kept terms, frozen, booked once on `holdout_exact`; then the days after 09-10, rule 1,
rule 1b and the clone side by side, the same code.

### Step 6: engine, then the clone

The finding sets the metric: the kept term becomes one metric in the group whose subject and
basis it shares (a holder-concentration group on an exact per-wallet token book if none exists),
opened only when a loaded rule reads it, with its one definition in the registry. Parity against
the Python tickets (derive 12.10-12.11), then new DB rules cloned from `Flip-Catch - Bracket` and
`Flip-Catch - Room` with the term added, on paper beside rule 1.

## 3. Kill

No candidate in steps 3-4 passes the walk-forward above chance under both exits: the left tail
is rule 1's price at this seat. The result is one row in the case file's chain, no clone.

## 4. Cost

Local lake tapes only; no Helius call. The exact token book is per coin, built for rule 1's
fire coins (about 700), not the whole tape.
