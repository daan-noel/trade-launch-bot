# Derive: the method, from one paying wallet to a shippable rule

The one method file: how a target node, trader or wallet becomes a sentence, the gates every
cell passes, and how a result is recorded. [_!___strategy.md](_!___strategy.md) is the basis and
the laws; [_!___inventory.md](_!___inventory.md) is the idea list used after the event is named;
[_!___evidence.md](_!___evidence.md) holds the numbers; [_!___workflow.md](_!___workflow.md) is
the open queue. Code: [node-derivation/toolkit](node-derivation/toolkit/README.md). The worked
example, every phase run on one node with its numbers, is
[node-derivation/hot-tape-rule-1.md](node-derivation/hot-tape-rule-1.md). A node's working file
starts from [node-derivation/node-template.md](node-derivation/node-template.md).

A wallet is an instrument. It names a decision. It is never a term, its coins are never a
door, and its own prints stay out of every public count.

```
  0.  TARGET      what counts as done
  1.  PICK        one member that pays; never the pooled node
  2.  FRAME       two tapes, the seat, the coordinate
  3.  SHAPE       one-shot, re-entry, or mixed - measured, not assumed
  4.  WHO PAYS    median vs tail; RACE vs FOLLOW
  5.  TRIGGER     the print (or state) it reacts to, and DELAY
  6.  EVENT       which of those it takes; spell it publicly
  7.  DOOR        optional; none is a valid filling
  8.  EXIT        its closing hazard on the selected pool
  9.  PERMISSION  the fires that die
 10.  INVENTORY   combine ideas on this parent only; route a red result
 11.  GATES       the keep rule, the gates on every cell, the ship bars
 12.  HOLDOUT     confirms, never chooses; then every slot re-derived
 13.  RECORD      each fact once, in its one place
 14.  REFUSALS    and the guards
```

Inventory combination is the search engine **on a parent**. It is not how the parent is
born. A 4-tuple of unused ideas is not a sentence until a paying member already harvests
that leftover.

---

## 0. The target

A public trigger, filled at 115 ms on both legs, that harvests leftover of real up-moves:
net SOL, most days positive, enough tickets, re-entry allowed. Latency is not the edge.
The edge is remaining intent that is still unpriced at fill.

Done, in order: frozen sentence, green on a disjoint holdout, ship bars, engine-reconciled
trade by trade, paper, small real.

D is not required. Empty D is a filling. A door earns its place only when it raises money
and keeps the bars. With a door the book is more concentrated; without one the event and
exit have to carry it.

---

## 1. Pick the instrument

A node label is a cluster of wallets, not a strategy. Six wallets under one name can be
two logics. The unit of derivation is **one member**.

```
  pick a node  ->  price each member at two seats  ->  keep who pays
                   (phase 4)                          at RACE or FOLLOW
```

How to choose among members (all of this is measured on the study tape, none is a name):

| prefer the member that | drop the member that |
| --- | --- |
| has a positive median, not only a tail | a negative median whose net is the top 1 % |
| names a public print or a state (excess intensity spikes, or is flat because it fires on a held state) | copies a fill we cannot reach (lag ~50 ms, ahead < 10 %) |
| leftover **exists** at `lag_115` on the fires it takes (phase 5.2) | peak leftover after our fill does not cover round-trip cost |
| DELAY of that leftover survives 115 ms | the tell and the SOL land in the same slot |

The node's largest book is not automatically the instrument. The largest book often does
not name a public print. The member whose tell we already copied unsuccessfully is not
the next instrument either: its public spelling is already a result. Prefer the member
that still has an unread decision point at our seat.

Roster and node lists: [solo-traders.md](solo-traders.md). `toolkit.tapes.roster(node)`
returns the wallets. `S.wallet(prefix)` is the member.

---

## 2. Frame

| step | what | rule |
| --- | --- | --- |
| 2.1 Two tapes | Study tape: every threshold is read here. Holdout: later lake days, same format (`lake_export.export`, an entry in `tapes.TAPES`), started a few days early as a warm-up. Overlap of the two tapes is checked row by row | Nothing is chosen on the holdout |
| 2.2 Instrument | The member's wallet id is marked NODE so every public fact excludes it | It never enters a term |
| 2.3 Seat | Both legs fill at the last print landed 115 ms after the decision print. Exact curve arithmetic, 125 bps + 0.000225 SOL a leg (`kernel.py`) | One kernel prices everything |
| 2.4 Coordinate | Every measurement writes the six slots of a rule (strategy section 6) plus the frame | Empty is a value. A red number closes this sentence, never a slot |

```
  D  door         which tokens we watch at all; token-level, latency-free
  E  event        the print we fire on; a condition on UNPRICED state
  P  permissions  state already true when the event prints
  X  exit         the harvest shape, derived from the story
  R  re-entry     tickets per token, one position open at a time
  S  size         the clip (standing: 0.2 SOL; physics min ~0.126)
  frame           seat (last print landed by fire + 115 ms, both legs) . cost (125 bps +
                  0.000225 SOL a leg + own impact on vsol) . universe (full tape, window, fires)
```

A red number at coordinate `c` means "sentence `c` is red". A red sentence with `D` empty says
only that this event has no edge without a door.

Working file: copy [node-template.md](node-derivation/node-template.md) to
`<node>-rule-<n>.md`. Scripts live in `node-derivation/<node>/`, one per step, each
opening with its step and question (section 13 says which are tracked).

---

## 3. Shape - measured, not assumed

A node name ("one-shot", "re-entry") is a prior. The member's own episodes decide.

On its round trips (`seat.episodes`: first buy while flat to the sell that takes it to
<= 2 % of its peak), report:

```
  episodes per mint          (share of mints with 2+ episodes)
  re-entry rate              (share of episodes that are not the first on that mint)
  hold p10 / p50 / p90
  max positions open at once
  clip p50, entry reserve p50, age p50
```

Read the shape, then split if mixed:

| shape | what it means for the sentence |
| --- | --- |
| mostly one episode per mint | E is the start tell; X harvests that hill; D concentrates which hills if a door earns it |
| many re-entries on the same mint | E aims at the moment inside a live move; X eats a slice; D is often empty |
| mixed | first-on-mint and later-on-mint are **two sentences**. Derive them separately. Do not average |

The shape also says which slots have to carry the book. On a re-entry scalper, E and X
usually carry more than D and P. On a one-shot, E still has to be DELAY-legal; D can
concentrate, and is still allowed to stay none.

---

## 4. Who pays

| # | question | how | decide |
| --- | --- | --- | --- |
| 4.1 | Does this member's own book pay, or is it a tail? | Each member's episodes through our kernel: win rate, median, top 1 % share | A negative median whose net is the top 1 % is noise (law 28). Search which of **its** trades win; do not imitate the median |
| 4.2 | At which seat? | `seat.seat_book(S, E, caps=(15,))`: RACE (sequenced before its print) and FOLLOW (our fill 115 ms after) | Keep members positive at RACE on most days with a positive body. Positive only at RACE: the event must be reached **before** its print (a state, not a follow). Positive at FOLLOW: a print trigger can work at 115 ms |

Do not pool members (law 27). Derive from the ones that pay. A node that splits is two
nodes; write two working files.

---

## 5. Trigger and DELAY - leftover existence, not an exit P&L

Four questions, in this order. Question 4 is the veto. If it fails, this trigger is dead
at our seat. Do not walk DxPxX on it.

```
  1  WHO    will spend SOL on this coin in the next seconds or minutes
  2  WHY    its business reason, not ours
  3  WHAT   on the tape (never the price) shows the intention now
  4  DELAY  why that SOL has not landed yet, and still has not landed 115 ms after we fire
```

Question 4 is the scarce one: if the tell and the SOL arrive together there is no trade at any
seat, and that is how nearly every dead line died - the swarm lands in the trigger's own slot.
Enumerate the delay first (strategy 1.6 lists the delays this market has).

A P&L number is always E+X. A hold-matched clock, a copy of its close, and every-fire
occupancy are exit or selection choices. None of them is the 5.2 veto. Its close is not
the best X for our late fill; a median-hold clock is one X among many. Grade an event
candidate by whether leftover **exists** at our seat, and whether this is its decision.

**Correct E is two yeses.** (1) His decision: he pays at RACE (phase 4) and reacts to this
public, unpriced tell (5.1: a lift spike of one class, covering at least 10 % of his
decisions). (2) Our leftover: after a 115 ms fill on the fires he takes, behind his own buy,
the path still has enough convexity inside his hold to pay round-trip cost (5.2). (1) without
(2) is unreachable DELAY. (2) without (1) is noise: 5.2 alone passes a member that loses at
every seat (evidence 1.27), so 5.2 never runs before (1).

| # | question | how | decide |
| --- | --- | --- | --- |
| 5.1 | What does it react to? | `trigger.excess_intensity(S, {m: [w]})`: its buys against same-coin controls; every public print in the 5 s before, by (class, lag); `trigger.peak(lift, cls)` | A spike of one class at one lag band is the trigger, and that lag is the reaction time. Flat everywhere: it fires on a **state** that has been true for seconds, not on a print. A class covering under 10 % of its decisions (acted tickets over its episodes, `seat.leftover`) is a corner, not its logic: 8fStGV's trigger covers 93 %, the burst start it avoids 5.8 % (evidence 1.27) |
| 5.2 | Does leftover exist at our seat? | `seat.leftover(S, w, E, trigger)`, then `seat.leftover_summary`. Acted ticket: the latest print of the class <= 300 ms before each of his decisions (or the rising-edge print of the state). Our fill: `kernel.fill_idx`, 115 ms after that print. Read the **behind** row: the tickets where our fill lands after his buy, so his own fill is already in the price (on an ahead ticket his buy counts as our leftover: a copied fill). Reaction cost = % price move from the print to our fill. Peak leftover = the best net % of an exit decided inside his hold p50, filled 115 ms later, both legs' cost paid (break-even is +3.1..+3.9 % in price). Missed = our fill at or after his closing sell | **Kill** when any of, on the behind row: median reaction cost >= 2 %; median peak leftover <= 0; missed >= 50 %. On all acted tickets (the behind row has ahead 0 by construction): lag p50 <= 50 ms with ahead < 10 % (the tell and the SOL land together). Calibrated on the hot tape (evidence 1.27): rule 1's trigger reads cost 1.41 %, peak +7.46 %; AbQcLH's burst start costs 4.55 % and is killed; so is 9999hu's sell >= 1 at 8.97 %, whose peak behind its buy is still +12.6 %: a large move leaves a volatile path, and its acted book rests on its tail. A better X cannot save a kill. Try another class, or a state |
| 5.3 | Diagnostics, not a veto | `seat.reaction` (dt, ahead, behind). Horizon scan at 1, 2, 5, 15, 30, 60 s clipped to hold p90, and at his sell time, all from the `lag_115` fill. His-exit column: our entry at 115 ms, his sell at RACE (no exit lag). Occupancy column: every fire of the class on its coins, hold-matched clock. From `leftover_summary`: the acted and ignored rows, the peak at hold p10 and p90, the within-coin peak excess over ignored, the share that reaches break-even before the mirror loss | Report every column. **Do not kill** on a red clock, a red copy of his exit, or a red every-fire occupancy. Those mix X or which-fires into E. The within-coin peak excess and the break-even-first share are never the veto: the first reads -1.93 on rule 1's trigger (its fills pay the rebound the ignored prints do not), the second passes every anchor (evidence 1.27). Law 23: peak leftover never ships |

**DELAY is leftover of the move, not gap to the next print.** Next-print p50 of 50-80 ms
is burst density (the hill is still printing). It is an artifact detector for a one-print
pop (section 11), not a veto of an 87 s episode. The veto is: the tell and the SOL we wanted
arrive together, so a 115 ms fill has nothing left.

A waiting time is not a reaction time. "Seconds since the last big buy" measures how often
big buys happen. Latency is measured from the trigger print, by excess intensity.

Do not score a DxPxX grid on a trigger that 5.2 killed. A red 5.3 occupancy book with
green 5.2 leftover is a 6.1 problem (which fires), not a dead E.

**5.2 is a veto, not a verdict.** Its lines separate the known triggers only in this order:
phase 4 drops sssssw (it passes 5.2: cost 1.18 %, peak +1.01 %), and 5.1's coverage drops the
class 8fStGV avoids. A trigger that passes 5.2 stays open until a finished search closes it:
6.1 terms, then phases 8 and 9 on that pool, both exit families, walked forward (section 14).

---

## 6. Event - which triggers, then spell it publicly

Run 6.1 before treating a class-wide red book as a dead E. Occupancy takes the first fire
on the coin; the fire he takes can be a later one.

| # | question | how | decide |
| --- | --- | --- | --- |
| 6.1 | Which of those prints does it take? | On **its coins**, every print of the trigger class (`candidates.build`); labelled by whether it acted (`contrast.label_acted` in its reaction window); `contrast.strat_rank(acted, ignored_same_coins, facts)` | Facts far from 0.50 with a mechanism become the event's terms. Acted medians are the first thresholds. A conjunction at high lift covering a tiny share of its buys is a rare corner, not its logic. If leftover exists only on the fires he takes, the class is too wide: add terms to E, do not add D |
| 6.2 | Does it hold as a public sentence? | Every term in public tape state, on **every coin** (`candidates.build` + `book.fires`). Add terms one at a time. Run the opposite-side control (same terms, other side). Temporary public X = a hold-matched clock (a prior, not phase 8) | Each term must lift the book monotonically. The control must be worse. Own prints stay out. A red 6.2 with green 5.2 is not "E is wrong": it is this spelling plus this prior X on every coin. Next is 6.1 terms or phase 8 on the acted pool, not a random new E. The prior X is a yardstick: it ranks the terms (each must lift the book under the same X) and its sign decides nothing. Rule 1's event reads -0.68 % under a 15 s clock and carries +4.5 % once P and X are filled (evidence 1.12, 1.22). A term chosen under the prior X is re-read under the real X in phase 12: rule 1's "bought >= 2 SOL in 2 s" was costing money there |

Report coverage and reaction cost beside every lift. An event that costs over about 2 %
to react to (price move from the state a watcher held to our 115 ms fill) is unreachable
whatever its lift. That cost is a 5.2 kill, not a 6.2 comment.

Every fact is built from prints before index k. A feature window that contains the
member's own print is a lookahead.

The event is now frozen for the next phases. A slot is held fixed only to search the
next one on its fires, and is reopened in phase 12.

---

## 7. Door - optional

Search D on the frozen event's fires. Default is none.

| # | question | how | decide |
| --- | --- | --- | --- |
| 7.1 | Is the gap a coin property, or its future arrival? | Event fires on coins it trades vs every other coin, and on its coins split at its **first** buy there | A gap **before** its first buy is a coin property (a door candidate). A gap only after is its future arrival: D has to predict that arrival, and the coin list is not it |
| 7.2 | How much of the book is incoming demand? | `candidates.build(..., actor=w)` gives `act_in` | Diagnostic only. The member is 2-5 % of a real move's SOL; it detects the move, it does not ignite it |
| 7.3 | Search D | Coin facts from prints **before** the fire. Inventory D ideas are the candidate list. AUC of fires that pay against the rest; money by quintile; the best two stacked. Keep rule (section 11) | Keep a door only if it survives the holdout and keeps the ship bars. None is valid. A door that raises SOL and breaks a bar is not taken |

A fact dated at age 60 s cannot serve a fire at age 20 s. The member's mint list is never
D. Create-cgroup include of its coins is usually the market (concentration near 1): measure
it, do not assume it is a door.

Inventory D ideas live in [_!___inventory.md](_!___inventory.md). A new D idea, if the
list is short, comes from 7.1's contrast (what is true on coins it picks **before** it
arrives, and is rare on the rest) - not from a new event.

---

## 8. Exit - on the selected pool, at its hold

Phase 5 does not choose X. A specific exit is how the leftover is eaten, not how its
existence is decided. Do not sweep X on an unselected pool. That pool is mostly dying
coins and returns the shortest clock (law 26).

| # | question | how | decide |
| --- | --- | --- | --- |
| 8.1 | How does it close? | `trigger.excess_intensity(..., cases="close", controls="hold")` for a print trigger; `hazard.closing_hazard(S, w, pool)` on the pool the sentence selects: chance its next print is the close, by profit x time held, at **fine** bins | A hazard jump at a profit band is a take profit; at a loss band a stop; a flat band waiting for time is a clock. Coarse bins mis-place the stop (-20 % coarse, -25..-40 % fine on rule 1's member) |
| 8.2 | Book families against that bracket | `exits.X(kind=...)` for bracket, scale, sellbuy, trail, ride, fade, dump | The bracket is the default until a family beats it on money **and** bars. Read at the actor's own hold, not at a 1800 s horizon on a 20 s node |

The exit comes from the story's failure mode, and both families are always read side by side:

```
  story failure mode              ->  exit shape
  "the up-move never starts"      ->  cut fast and small, on STATE, not a clock
  "the up-move ends"              ->  ride, give back a fixed fraction of the peak
  "the coin rugs"                 ->  a DOOR / L-axis problem, never an exit problem

  scalper control    clock 45
  loss-capped ride   unarmed trail 20-40, cap 600-1800
  tail preserving    tp100 / trail50 / cap 1200
  state-conditional  cut on: the pusher sells | new-buyer arrival stalls | flow reverses
                     before price does | the burst dies - loss-only, and only on a sentence
                     that already clears the gates
```

A static abort (time x percent) gives up winners as fast as it saves losses (104 cells, zero
positive; evidence 4.4): it is not rerun. A cut that can fire while the position is ahead is a
clock (evidence 4.7). A 45 s clock is a scalper control, never the primary read on a harvester
story (the median playable move is 87 s).

Every exit branch resolves to a print index; the smallest index wins (law 24). A multi-leg
exit is priced leg by leg (law 25). Count exits on the graduation print; a sentence that
needs them is priced at a point the tape cannot see.

---

## 9. Permission - the losers

| # | question | how | decide |
| --- | --- | --- | --- |
| 9.1 | What is true at the fire on stop-outs vs take-profits? | Facts at the fire; AUC; a one-sided cut by the keep rule, applied **before** occupancy so a refused fire frees the coin for a later one | P is the cut that removes stop-outs without shrinking the book below the bars. Inventory P ideas are the candidate list |

P does not explain why the move happens. It puts the fire where a move can pay and where
a loss is bounded (age, holders, room under the wall).

---

## 10. Inventory search - on this parent only, and where a red result routes

The parent is the sentence after phases 5-9, including empty slots.

[_!___inventory.md](_!___inventory.md) is the idea list. Use it here, not at phase 0.

```
  parent exists (E named, DELAY legal)
        |
        v
  search D, P, X from the inventory on those fires
  one slot at a time, keep rule, both exit families
        |
        v
  red  ->  name the empty slot
           add one idea THERE, from the member's contrast
           (acted vs ignored, winners vs losers, coins before arrival vs rest)
           re-search that slot; do not replace E
        |
        v
  DELAY of this leftover died  ->  new trigger (phase 5), not a new inventory E row
```

A one-slot change is a new address. Ablate: drop one term at a time; keep it only if it
raises total SOL. Neighbourhood is this search. It is not a new parent.

Do not invent a parent by walking unused DxExPxX 4-tuples. That scores leftover of a
named print, not leftover of the member's up-move.

When adding an idea: put it in the inventory under the family that shares its mechanism,
then score it on this parent's fires.

**A red frozen sentence is not patched.** Ablation names the dead clause; do not trim it and
do not add an AND. The next sentence is the next filling of the named empty slot on the same
parent, or a new trigger or member. The shape of the failure names the clause, and only one of
these kills a story:

| the failure | the clause that died | the next story |
| --- | --- | --- |
| nobody arrived at all | WHO | same tell, a different actor class |
| they arrived, but before or after our window | DELAY | right actor, wrong moment - move the event, keep the story |
| the tell was already in the price | WHAT | an earlier footprint of the same intention (the commitment, not the execution) |
| the tell and the money are simultaneous | DELAY = 0 | this actor is unusable at any seat. **The only failure that kills a story** |
| positive on some coins, negative overall | the DOOR is missing | keep the story, add a coin selector |
| winners fine, losers catastrophic | the L-DOOR or the EXIT is missing | keep the story, work the loser cost |
| positive but under the ticket floor | too narrow | widen the ACTOR class, never add permissions |
| tickets spike on two days and vanish on the rest | the DOOR is a client, not a type | a named client book, not the reading (section 11, TYPE) |

Stop inventing tape conjunctions only when every open node has one frozen sentence at
`lag_115` and it is red, the dead clauses all say the remainder is not on the tape at decision
time, and off-chain fields stored live have also been scored.

---

## 11. Keep rule, gates and ship bars

Every choice in phases 7-12 is made the same way.

**Walk-forward.** Split the current sentence's fire **days** into two halves. Each half
(fit) picks the value with the most SOL among those whose every fit day is positive; the
other half (test) scores it against the current value there. Take a change only when both
folds move the same way and both beat the current value on their test half. The value is
the mean of the two picks, snapped to the grid, a tie to the side nearer the current
value (`walkforward.thresholds`, `converge`, `new_terms`, `axes`). Where clients exist
(launch-door sentences), split on **clients**, not on days that a two-day build straddles.

**Chance.** A gain on a test half smaller than the SD of the SOL a random share of the
tickets carries (`walkforward.cut_noise`) is chance, whatever the folds say. On rule 1 a random
5 % of the tickets carries 0.15-0.18 SOL a half.

**The gates every cell passes**, in order, before it is a candidate:

```
  FLOOR     >= 50 first-per-mint trades ON EACH DAY - a refusal, not a target
  MONEY     total net SOL > 0 on the whole sentence, never a proxy label
  TAIL      top 1 % of trades <= 20 % of net to stay a candidate, <= 15 % to ship
  CLIENT    positive with its single best client removed, and in >= 95 % of a client bootstrap
  WALK-FWD  the keep rule above
  candidate -> freeze -> disjoint holdout -> engine reconcile -> paper -> small real
```

- **The floor is per day, and a mean lies.** Tickets of `16, 177, 210, 24, 28, 9` average 53.9
  and clear fifty on two days of six; the two big days are the days one client was alive, so
  the mean launders the client concentration through the gate (law 19). Print the per-day
  list. A stub UTC day (hours well under 24) is not a floor day: quote tickets/hour x 24.
- **Tail calibration.** A real convex book at this seat puts 9-12 % of its net in its top 1 %
  (evidence 2.3); a cell at 50-270 % is noise around zero. Report the biggest coin's share
  beside it; if one coin carries the result, that is the result.
- **The client gate outranks walk-forward.** Trades behind a launch door come in clients: one
  creation build launches many coins over a day or two, and a week holds about 20 of them
  (evidence 3.1a). The client is the creation build for a launch-door sentence, the machine
  (ix structure + payer, never the wallet) for a trader-node sentence, the token for an
  event-only sentence. Report trades and clients side by side, the top client's share, the
  book with each client removed (worst quoted), and a 2,000-draw bootstrap over clients. A
  cell that dies when its best client leaves is that client's book; one that clears it but
  sits under 95 % needs more clients, not more terms.
- **TYPE sits in front of ranking.** Ranking by total SOL first selects the day-specific cohort
  every time (law 29). Beside every book: per-day first tickets, their peak/trough on full UTC
  days, the share in the two fattest days, and the same three for the tape's births and the
  door's births. Tape births run 2.1x peak/trough with 36 % in two days; a cell at 26x / 81 %
  is a client even when its door's supply is 5.7x. The reading is the best SOL > 0 cell that
  passes TYPE; the SOL leader of a two-day spike is a named client book.
- **Artifact detectors, beside every book:** the zero-lag column (the ratio is the artifact);
  gap-to-next-print (money under 50 ms is an artifact); the share of entries with no print in
  the hold; the trail fire rate (a trail firing on 18 % of trades is a clock).

**The ledger** (`book.ledger`) on every result, never a mean alone:

| bar | the ship line |
| --- | --- |
| days positive | >= 5/7 on the study tape, every day on the holdout |
| the two halves of the days | both > 0 |
| body (net without the top 1 % tickets) | > 0 |
| top 1 % share of net | <= 15 % (a candidate stays at <= 20 %) |
| biggest coin's share of net | <= 15 % |
| capped book (every gain capped at the median take profit) | > 0, and its top 1 % share inside the bar |
| exits on the graduation print | counted; the SOL must not depend on them |
| the seat | > 0 at a 200 ms fill on both legs, with the days bar held; 115 / 200 / 300 / 500 ms reported beside it. Our real decision-to-fill is p50 115 ms, p90 228 ms, and on a burst the fill lands behind the swarm (evidence 1.1) |
| per-day first tickets | the list, every day over the floor |
| client | positive with its best client removed, and in >= 95 % of a client bootstrap. The client is the gate's: the creation build behind a launch door, the machine for a trader node, the coin for an event-only sentence (rule 1's coin-resampled 95 % interval: +2.19..+6.58 %) |

A change that raises SOL and breaks a bar is not taken. A change that raises SOL only in
the capped-away gaps is not taken. A cell that already clears the client gate is not killed
only because campaign activity is uneven across days.

---

## 12. Holdout, then re-derive

| # | question | how | decide |
| --- | --- | --- | --- |
| 12.1 | Does the frozen sentence hold on unseen days? | The same booking code on the holdout tape, nothing re-fitted, the full ledger | Every bar. The holdout confirms; it never chooses. Each change is booked **once** on the holdout, one at a time in the order taken; a change that fails there is dropped. A holdout read to choose between survivors is spent for that choice: the change is certified only by later unseen days (rule 1b's exit, evidence 1.24) |
| 12.2 | The room | Split the sentence's trades by the member's pick (`act`) and its arrival (`act_in`) | If its picks pay no more than its skips, copying it has no room left: read every threshold off money |
| 12.3 | Candidate table | `candidates.build` under floors loose enough for every loosening; `book.save` | It must reproduce the current book exactly (`book.fires` against the recorded ledger) before any number off it is trusted |
| 12.4 | Thresholds by money | `walkforward.converge(C, days, base, grid, veto=bars)` | Keep rule and bars. A term on the grid edge means the table floor is too tight - rebuild looser. A looser threshold is judged on the trades it adds, net of the sentence's own trades it displaces through occupancy: on rule 1 every term fails its first step under both exits, so more trades need a second event, not a looser one (evidence 1.26) |
| 12.5 | Structural checks | `graduation.grad_flag`, the capped book, a lookahead read of any term that looks too good | A book that rests on an unpriceable exit gets a term that keeps the exit priceable |
| 12.6 | New terms | `walkforward.new_terms`: each fact cut at 12 quantiles from both sides; `cut_noise` beside it | Taken only above chance. Inventory ideas not yet in the sentence enter here |
| 12.7 | Exit on the new pool | `exits.outcomes` then `walkforward.axes`, one axis at a time | Keep rule, then the holdout |
| 12.8 | Re-entry | `book.occupy` (cool, max per coin), book of the n-th entry and of entries after a stop | Taken only when the trades it touches beat chance |
| 12.9 | Size | `book.reprice` at every clip, flat and as a share of the reserve | The largest clip that keeps every bar on both tapes |
| 12.10 | Engine target | A print-by-print replay that shares no code with the candidate table. First with the members left out (must match tickets). Then with every wallet counted, every leg (`lake_export --all-legs`), across a lag sweep | Two codes that agree to the ticket rule out a booking bug. The every-wallet, every-leg book is the number the engine must reproduce: the same trigger, fill and exit print, reason and SOL on every ticket, the only misses allowed being tickets on coins the engine retires as dead (evidence 1.23) |
| 12.11 | Every term as the engine computes it | Each term's input against an independent exact field of the lake; then a replay spelling every term, fill and clock the engine's way, each line citing the engine code it mirrors; the rule re-derived on a study tape at the engine's grain; a second code sharing nothing rebuilds the tickets | A second code sharing an idea cannot catch the idea: rule 1's holder count was float dust in two agreeing replays. The grain and the fill move the answer; derive at the ones the engine runs (evidence 1.21, 1.22) |

The next unseen lake days after the holdout is the clean test.

---

## 13. Record - each fact once, in its one place

| result | goes in |
| --- | --- |
| each step: its question, what it shows, what it does to the sentence | one row of the node's case-file chain ([node-template.md](node-derivation/node-template.md)). This is the record of every step, dead ends included |
| a number a rule, a law or an open line stands on | a numbered section of [_!___evidence.md](_!___evidence.md), in the shape below. A closed line keeps one row in its ledger (section 7), not a section |
| each idea tried | its slot in [_!___inventory.md](_!___inventory.md): status + one reference |
| what is open next | [_!___workflow.md](_!___workflow.md), only when the open list changes |
| the scripts | `node-derivation/<node>/`, local scratch. A script is tracked only when a rule or a gate needs it re-run: name it in the root `.gitignore` |

Evidence shape. Write all six slots every time; empty is a value:

```
### <name>
D  <door, or "none">
E  <event>
P  <permissions, or "none">
X  <exits read, all of them>
R  <re-entry>   S  <clip>   seat <fill>   universe <window, fires, tokens>

| cell | n | first/day | tickets/day | peak/trough | top2 | SOL | %/trade | days+ | worst | win | be | top1 % | maxtok % | fit | hold |

verdict: <what this sentence does>          empty slots: <which>
```

`be` is the cell's own realised break-even `L / (W + L)`; `top1 %` is the share of net in the
top 1 % of trades, against the 9-12 % calibration.

---

## 14. Refusals, and the guards

The measurement laws are [_!___strategy.md](_!___strategy.md) 7.4 and its closed mistakes are
section 9; these are the method's own.

- **No parent from an inventory 4-tuple.** Combine ideas on a DELAY-legal event named by
  a member. Do not walk unused DxExPxX to invent the story.
- **No DELAY read as next-print gap** when the leftover is a hill. Next-print gap is an
  artifact detector for a one-print pop.
- **No conjunction walk on a trigger 5.2 killed**, and no 5.2 read before phase 4 and 5.1:
  the veto alone passes noise (evidence 1.27).
- **No hold-matched clock, copy of his close, or every-fire occupancy as the 5.2 veto**, and no
  every-fire red read as a dead E when acted leftover is green: that is 6.1.
- **No ahead ticket in the 5.2 veto.** Our leftover on it includes his own buy: a copied fill.
- **No pooling members.** Split first. Two logics get two working files.
- **No mint list as a door.** A member's coin list is a diagnostic split, made at its first
  buy on the coin before any gap is read.
- **No wallet in a term, agreement included.** Every term is public tape state or ix structure;
  a measurement that needs named wallets stays a thermometer (law 20).
- **No fact dated after the fire.**
- **No exit swept on an unselected pool** or past the actor's own hold.
- **No slot closed from a red number.** A slot closes only on a measured mechanism that holds
  at every coordinate (strategy 8.1), or a finished search of that slot on a parent, at the
  floor, under both exit families, walked forward. Anything else is "sentence `c` is red",
  with its empty slot named.
- **No replacing E because D is empty.** Empty D is allowed. A missing door keeps E and
  searches D, or ships with D = none.
- **No extra AND as a new sentence, and no trimming a frozen sentence** because a prefix holds
  out better: write a new sentence and burn the week.
- **No mixing doors in one sentence.** Slow-wall, documented-project and demonstrated-episode
  are different stories, one at a time.
- **No parking the harvester** because survival is the only holdout-positive book. Survival is a
  door; the target in section 0 is still leftover of real up-moves.

The guards:

- **An absurd win rate is a lookahead until shown otherwise** - 80 % on a scalper, or a cell
  green on every column at once.
- **A ceiling is not money in a slot** (law 23). A perfect-exit or perfect-door number bounds
  the slot; it does not say the slot can reach it.
- **A gradient is not a filling.** A term monotone in money that does not cross zero stays a
  candidate for a fitted vector, not a threshold AND.
- **Features in the coin's own units.** A fact against the coin's own prior history beats one
  ruler across thousands of coins almost everywhere (evidence 1.5); a live rule computes it
  from the coin's tape with no stratum and no lookahead.
- **Every flow fact has a node-blind twin** with the members' SOL removed, so a term can be asked
  whether it is public tape or a proxy for the members.
- **`v[k]` is the reserve AFTER print k.** Pricing an entry at `v[their buy]` charges THEIR
  displacement; the reserve they met is `v[k] - signed amount`. A state fire anchors at the
  print the decision sees (`k-1`): anchoring at `k` fills later, and on a falling tape a later
  fill is a cheaper buy.
