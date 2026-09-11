# Derive: from one paying wallet to a shippable rule

The playbook that turns a target node, trader, or wallet into a sentence. It is not bound
to any one member. [_!___strategy.md](_!___strategy.md) is the basis;
[_!___inventory.md](_!___inventory.md) is the idea list used after the event is named;
[_!___workflow.md](_!___workflow.md) is the gates, the campaign queue, and how a result
is recorded; [_!___evidence.md](_!___evidence.md) holds the numbers.
Code: [node-derivation/toolkit](node-derivation/toolkit/README.md). The hot-tape worked
example (toolkit calls on one node) is [node-derivation/method.md](node-derivation/method.md).
A node's working file starts from [node-derivation/node-template.md](node-derivation/node-template.md).

A wallet is an instrument. It names a decision. It is never a term, its coins are never a
door, and its own prints stay out of every public count.

```
  0.  TARGET      what counts as done
  1.  PICK        one member that pays; never the pooled node
  2.  FRAME       two tapes, the seat, the coordinate
  3.  SHAPE       one-shot, re-entry, or mixed — measured, not assumed
  4.  WHO PAYS    median vs tail; RACE vs FOLLOW
  5.  TRIGGER     the print (or state) it reacts to, and DELAY
  6.  EVENT       which of those it takes; spell it publicly
  7.  DOOR        optional; none is a valid filling
  8.  EXIT        its closing hazard on the selected pool
  9.  PERMISSION  the fires that die
 10.  INVENTORY   combine ideas on this parent only
 11.  HOLDOUT     confirms; never chooses
 12.  RE-DERIVE   every slot on the sentence's own pool
 13.  RECORD      evidence, inventory, working file
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
| leftover on **its coins** is green at `lag_115` (a ceiling, not a door) | leftover on its coins is red at our seat |
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
| 2.1 Two tapes | Study tape: every threshold is read here. Holdout: later lake days, same format (`lake_export.export`, an entry in `tapes.TAPES`). Overlap of the two tapes is checked row by row | Nothing is chosen on the holdout |
| 2.2 Instrument | The member's wallet id is marked NODE so every public fact excludes it | It never enters a term |
| 2.3 Seat | Both legs fill at the last print landed 115 ms after the decision print. Exact curve arithmetic, 125 bps + 0.000225 SOL a leg (`kernel.py`) | One kernel prices everything |
| 2.4 Coordinate | Every measurement writes D E P X R S plus seat, cost, universe | Empty is a value. A red number closes this sentence, never a slot |

Working file: copy [node-template.md](node-derivation/node-template.md) to
`<node>-rule-<n>.md`. Scripts live in `node-derivation/<node>/`, one per step, each
opening with its step and question.

---

## 3. Shape — measured, not assumed

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

## 5. Trigger and DELAY — before any conjunction

Four questions, in this order. Question 4 is the veto. If it fails, this trigger is dead
at our seat. Do not walk D×P×X on it.

```
  1  WHO    will spend SOL on this coin in the next seconds or minutes
  2  WHY    its business reason, not ours
  3  WHAT   on the tape (never the price) shows the intention now
  4  DELAY  why that SOL has not landed yet, and still has not landed 115 ms after we fire
```

| # | question | how | decide |
| --- | --- | --- | --- |
| 5.1 | What does it react to? | `trigger.excess_intensity(S, {m: [w]})`: its buys against same-coin controls; every public print in the 5 s before, by (class, lag); `trigger.peak(lift, cls)` | A spike of one class at one lag band is the trigger, and that lag is the reaction time. Flat everywhere: it fires on a **state** that has been true for seconds, not on a print |
| 5.2 | Is that leftover at our seat? | `seat.reaction(S, E, trigger_fn)`: its lag from the trigger, the share where our fill on that trigger lands ahead of its buy, the book at our seat, ahead and behind | Reachable when lag p50 is well over 115 ms, **or** we land ahead often enough that the book is not red when we land behind. Lag near 50 ms with under 10 % ahead is DELAY about zero: a race, not an event. Kill this trigger. Try another class, or a state |
| 5.3 | Ceiling on its coins | Hindsight D = the coins it trades. Fire the trigger at `lag_115` on those coins, own prints out | Green ceiling: leftover of this tell exists. Red ceiling: this tell is not its leftover; go back to 5.1. The ceiling is not money in a slot (law 23) and is not a door |

**DELAY is leftover of the move, not gap to the next print.** Next-print p50 of 50-80 ms
is burst density (the hill is still printing). It is an artifact detector for a one-print
pop (`workflow` section 4), not a veto of an 87 s episode. The veto is: the tell and the
SOL we wanted arrive together, so a 115 ms fill has nothing left.

A waiting time is not a reaction time. "Seconds since the last big buy" measures how often
big buys happen. Latency is measured from the trigger print, by excess intensity.

Do not score a D×P×X grid on a trigger that 5.2 killed.

---

## 6. Event — which triggers, then spell it publicly

| # | question | how | decide |
| --- | --- | --- | --- |
| 6.1 | Which of those prints does it take? | On **its coins**, every print of the trigger class (`candidates.build`); labelled by whether it acted (`contrast.label_acted` in its reaction window); `contrast.strat_rank(acted, ignored_same_coins, facts)` | Facts far from 0.50 with a mechanism become the event's terms. Acted medians are the first thresholds. A conjunction at high lift covering a tiny share of its buys is a rare corner, not its logic |
| 6.2 | Does it hold as a public sentence? | Every term in public tape state, on **every coin** (`candidates.build` + `book.fires`). Add terms one at a time. Run the opposite-side control (same terms, other side) | Each term must lift the book monotonically. The control must be worse. Own prints stay out |

Report coverage and reaction cost beside every lift. An event that costs over about 2 %
to react to (price move from the state a watcher held to our 115 ms fill) is unreachable
whatever its lift.

Every fact is built from prints before index k. A feature window that contains the
member's own print is a lookahead.

The event is now frozen for the next phases. A slot is held fixed only to search the
next one on its fires, and is reopened in phase 12.

---

## 7. Door — optional

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
arrives, and is rare on the rest) — not from a new event.

---

## 8. Exit — on the selected pool, at its hold

Do not sweep X on an unselected pool. That pool is mostly dying coins and returns the
shortest clock (law 26).

| # | question | how | decide |
| --- | --- | --- | --- |
| 8.1 | How does it close? | `trigger.excess_intensity(..., cases="close", controls="hold")` for a print trigger; `hazard.closing_hazard(S, w, pool)` on the pool the sentence selects: chance its next print is the close, by profit x time held, at **fine** bins | A hazard jump at a profit band is a take profit; at a loss band a stop; a flat band waiting for time is a clock. Coarse bins mis-place the stop |
| 8.2 | Book families against that bracket | `exits.X(kind=...)` for bracket, scale, sellbuy, trail, ride, fade, dump | The bracket is the default until a family beats it on money **and** bars. Read at the actor's own hold, not at a 1800 s horizon on a 20 s node |

Every exit branch resolves to a print index; the smallest index wins (law 24). A multi-leg
exit is priced leg by leg (law 25). Count exits on the graduation print; a sentence that
needs them is priced at a point the tape cannot see.

---

## 9. Permission — the losers

| # | question | how | decide |
| --- | --- | --- | --- |
| 9.1 | What is true at the fire on stop-outs vs take-profits? | Facts at the fire; AUC; a one-sided cut by the keep rule, applied **before** occupancy so a refused fire frees the coin for a later one | P is the cut that removes stop-outs without shrinking the book below the bars. Inventory P ideas are the candidate list |

P does not explain why the move happens. It puts the fire where a move can pay and where
a loss is bounded (age, holders, room under the wall).

---

## 10. Inventory search — on this parent only

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

Do not invent a parent by walking unused D×E×P×X 4-tuples. That scores leftover of a
named print, not leftover of the member's up-move.

When adding an idea: put it in the inventory under the family that shares its mechanism,
then score it on this parent's fires. Status and evidence follow [_!___workflow.md](_!___workflow.md)
section 7.

---

## 11. Keep rule and ship bars

Every choice in phases 7-12 is made the same way.

**Walk-forward.** Split the current sentence's fire **days** into two halves. Each half
(fit) picks the value with the most SOL among those whose every fit day is positive; the
other half (test) scores it against the current value there. Take a change only when both
folds move the same way and both beat the current value on their test half. The value is
the mean of the two picks, snapped to the grid, a tie to the side nearer the current
value (`walkforward.thresholds`, `converge`, `new_terms`, `axes`).

Where clients exist (launch-door sentences), split on **clients**, not on days that a
two-day build straddles.

**Chance.** A gain on a test half smaller than the SD of the SOL a random share of the
tickets carries (`walkforward.cut_noise`) is chance, whatever the folds say.

**The ledger** (`book.ledger`) on every result, never a mean alone:

| bar | the ship line |
| --- | --- |
| days positive | >= 5/7 on the study tape, every day on the holdout |
| the two halves of the days | both > 0 |
| body (net without the top 1 % tickets) | > 0 |
| top 1 % share of net | <= 15 % |
| biggest coin's share of net | <= 15 % |
| capped book (every gain capped at the median take profit) | > 0, and its top 1 % share inside the bar |
| exits on the graduation print | counted; the SOL must not depend on them |
| per-day first tickets | print the list; a mean is never the floor ([_!___workflow.md](_!___workflow.md) section 4) |
| client | positive with its best client removed, and in >= 95 % of a client bootstrap, when the sentence is a launch-door book |

A change that raises SOL and breaks a bar is not taken. A change that raises SOL only in
the capped-away gaps is not taken.

TYPE (tickets track the door's births, not a two-day spike) sits in front of ranking when
several cells are compared. It does not send you to a new event. A cell that already
clears the client gate is not killed only because campaign activity is uneven across days.

---

## 12. Holdout, then re-derive

| # | question | how | decide |
| --- | --- | --- | --- |
| 12.1 | Does the frozen sentence hold on unseen days? | The same booking code on the holdout tape, nothing re-fitted, the full ledger | Every bar. The holdout confirms; it never chooses. Each change is booked **once** on the holdout, one at a time in the order taken; a change that fails there is dropped |
| 12.2 | The room | Split the sentence's trades by the member's pick (`act`) and its arrival (`act_in`) | If its picks pay no more than its skips, copying it has no room left: read every threshold off money |
| 12.3 | Candidate table | `candidates.build` under floors loose enough for every loosening; `book.save` | It must reproduce the current book exactly (`book.fires` against the recorded ledger) before any number off it is trusted |
| 12.4 | Thresholds by money | `walkforward.converge(C, days, base, grid, veto=bars)` | Keep rule and bars. A term on the grid edge means the table floor is too tight — rebuild looser |
| 12.5 | Structural checks | `graduation.grad_flag`, the capped book, a lookahead read of any term that looks too good | A book that rests on an unpriceable exit gets a term that keeps the exit priceable |
| 12.6 | New terms | `walkforward.new_terms`: each fact cut at 12 quantiles from both sides; `cut_noise` beside it | Taken only above chance. Inventory ideas not yet in the sentence enter here |
| 12.7 | Exit on the new pool | `exits.outcomes` then `walkforward.axes`, one axis at a time | Keep rule, then the holdout |
| 12.8 | Re-entry | `book.occupy` (cool, max per coin), book of the n-th entry and of entries after a stop | Taken only when the trades it touches beat chance |
| 12.9 | Size | `book.reprice` at every clip, flat and as a share of the reserve | The largest clip that keeps every bar on both tapes |
| 12.10 | Engine target | A print-by-print replay that shares no code with the candidate table. First with the members left out (must match tickets). Then with every wallet counted, every leg (`lake_export --all-legs`), across a lag sweep | Two codes that agree to the ticket rule out a booking bug. The every-wallet, every-leg book is the number the engine must reproduce |

The next unseen lake days after the holdout is the clean test.

---

## 13. Record

| result | goes in |
| --- | --- |
| each step's numbers | a numbered section of [_!___evidence.md](_!___evidence.md) |
| each step's coordinate and next action | a row of [_!___workflow.md](_!___workflow.md) |
| each idea tried | its slot in [_!___inventory.md](_!___inventory.md), status + evidence |
| the sentence, its book, its chain, the members | the node's working file, from [node-template.md](node-derivation/node-template.md) |
| the scripts | `node-derivation/<node>/`, one per step |

Evidence shape: [_!___workflow.md](_!___workflow.md) section 7. Write all six slots every
time. Empty is a value.

---

## 14. Refusals

- **No parent from an inventory 4-tuple.** Combine ideas on a DELAY-legal event named by
  a member. Do not walk unused D×E×P×X to invent the story.
- **No DELAY read as next-print gap** when the leftover is a hill. Next-print gap is an
  artifact detector for a one-print pop.
- **No conjunction walk on a trigger 5.2 killed.**
- **No pooling members.** Split first. Two logics get two working files.
- **No mint list as a door**, and no factor built on wallet identity.
- **No copying a fill.** Response rate names the event; the fill is already in the price.
- **No wallet in a term.** Every term is public tape state or ix structure.
- **No fact dated after the fire.**
- **No exit swept on an unselected pool**, and no selector judged by an exit chosen that way.
- **No slot closed from a red number.** Only a measured mechanism (DELAY = 0 at any seat,
  copying a fill, price-path as event) or a finished search of that slot on a parent.
- **No replacing E because D is empty.** Empty D is allowed. A missing door keeps E and
  searches D, or ships with D = none.
- **No mean as a per-day floor.** Print the per-day list.
- **No sidecar left-join with missing = False** as a door (law 30).
- **No event true at local index 0 by construction** (law 31). Report that share.
- **No Helius spend** without asking.
