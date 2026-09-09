# Workflow: how a rule is found, measured and closed

The method file. [_!___strategy.md](_!___strategy.md) carries the market and the basis;
[_!___evidence.md](_!___evidence.md) carries the numbers; this one carries **the procedure and
the current queue of work**. Read it at the start of every strategy session and follow it in
order. Nothing here is optional, and no result is admissible that skips a step.

```
  0. THE TARGET        what counts as done
  1. THE IDENTITY      the one equation, and the two selection problems inside it
  2. THE COORDINATE    six slots plus a frame; a result is an address, never a verdict
  3. THE LOOP          eight phases, and where a red number routes to
  4. THE GATES         floor, tail, walk-forward, holdout - with their calibration
  5. THE EXIT          derived from the story, never swept on an unselected pool
  6. CLOSING           the only two ways a slot may close
  7. RECORDING         the template every result is written in
  8. THE QUEUE         the campaigns, in order, with their kill conditions
  9. REFUSALS          the standing list of what is never done again
```

---

## 0. The target

**A shippable rule.** A public trigger, filled at 115 ms on both legs, that harvests the
leftover of real up-moves, books net SOL, ends most days positive, produces enough tickets to
be a book, and re-enters.

Latency is not the edge. The edge is **seeing remaining intent on the tape before that flow is
in the price.** Everything in this file is a tool for that one sentence.

Done means all of it, in order: a frozen sentence, green on a disjoint holdout week, at or
above the ticket floor, tail-robust, reconciled trade by trade against the engine, then paper,
then small real.

---

## 1. The identity, and the two selection problems

```
                    E  =  P . W  -  (1-P) . L  -  toll
                          ^   ^            ^         ^
                          |   |            |         |
            SELECTION ----+   |            |         +-- 3.2-4 % a round trip, fixed
            door x event      |            |
            x permission      |            +-- LOSER COST: entry position + exit
                              |
                              +-- WINNER PAYOFF: headroom (vsol) + exit
```

Break-even is `p = L / (W + L)`. It is **not a constant**: it moves with the entry position and
with the exit, measured from 29 % to 46 % on the same fires. Never judge a rule against a fixed
percentage; judge it against **its own realised break-even**.

Two different problems live in that equation, and only one has ever been worked:

```
   P-selection : which token goes UP        - every study so far, all red
   L-selection : which token goes to -50 %  - never attempted
```

Cutting `L` from 31.6 to 16.3 is worth exactly as much as lifting `P` from .20 to .29. The
terms that mark a collapse (bundle share, dev holding, fresh wallets, creator sold, create
structure) are in hand and have only ever been scored against "does this spike".

---

## 2. The coordinate

A rule is six slots plus a frame. Every measurement fixes all six, whether or not the study
says so, and the most common silent value is empty.

```
  +---------------------------------------------------------------------------+
  |  D  door         which tokens we watch at all; token-level, latency-free   |
  |  E  event        the print we fire on; a condition on UNPRICED state       |
  |  P  permissions  state already true when the event prints                  |
  |  X  exit         the harvest shape, derived from the story                 |
  |  R  re-entry     tickets per token, and how many open at once              |
  |  S  size         the clip                                                  |
  |  ---------------------------- frame --------------------------------------|
  |  seat      last print landed by fire + 115 ms, BOTH legs                   |
  |  cost      125 bps a leg + 0.000225 SOL a leg + own impact on vsol         |
  |  universe  full tape, the window, the fire count                           |
  +---------------------------------------------------------------------------+
```

**A red number at coordinate `c` means "sentence `c` is red". It never means "slot X is
closed".** The empty slot is the tell: a red sentence with `D` empty says only that this event
has no edge without a door.

---

## 3. The loop

```
  [0] FRAME              fixed once: seat, cost, full tape, coordinate recording, gates
        |
        v
  [1] NODE + STORY       pick one node from the solo 26; say the sentence out loud:
        |                "whose money is still coming, and what on the tape says so"
        v
  [2] FILL ALL SIX       never a bare event. every term carries one causal clause.
        |                the EXIT is derived HERE, from the story's failure mode
        v
  [3] SCORE THE WHOLE    money only, on the conjunction, on the full tape
        |                both exit families side by side, every time
        v
  [4] ABLATE             drop one term at a time; keep it only if it RAISES total SOL
        |                if nothing lifts, do not cut
        v
  [5] NEIGHBOURHOOD      vary ONE slot at a time around the sentence
        |                this is where the conjunction search lives
        v
  [6] GATES              floor, tail share, fit/hold  (section 4)
        |                fail -> back to [2] naming WHICH SLOT. never "closed"
        v
  [7] FREEZE -> HOLDOUT  a disjoint week, read once, never trimmed
        |                red = the STORY is wrong -> back to [1]
        v
  [8] ENGINE -> PAPER -> SMALL REAL
```

Phase 3 is the rule that most work has broken: **an event is never scored alone.** An
unconcentrated pool is red by construction, so a single term's red number carries no
information about the term.

---

## 4. The gates

```
   all cells
      |  FLOOR      >= 50 first-per-mint trades a day        (a refusal, not a target)
      v
      |  MONEY      total net SOL > 0 on the whole sentence  (never a proxy label)
      v
      |  TAIL       top 1 % of trades <= ~20 % of net
      v
      |  WALK-FWD   fit > 0 AND hold > 0 on disjoint halves
      v
   candidate -> FREEZE -> disjoint week -> engine reconcile -> paper -> small real
```

**Tail calibration.** A real convex book at this seat puts **9-12 %** of its net in its top 1 %
of trades, and stays positive in essentially every subsample; that is measured on the trough
book (`+26.99 %/trade, 8/8 days`) at matched trade counts. A cell holding 50-270 % of its net
in its top 1 % is not a thin version of that book, it is noise around zero. Report the share,
not the adjective. The single largest token's share sits beside it: if one token carries the
result, that is the result.

**Walk-forward calibration.** Choosing the best cells on one half of a week and scoring them on
the other half collapses them (measured: +74.9 SOL becomes -40.5). Any cell chosen in-sample is
a candidate to freeze, never a result.

**Artifact detectors, reported beside every book:** the zero-lag column (the ratio is the size
of the reactive-exit artifact), gap-to-next-print (money concentrated under 50 ms is an
artifact), the share of entries with no print during the hold, and the trail fire rate (a trail
firing on 18 % of trades is a clock wearing a trail's name).

---

## 5. The exit is derived, not swept

```
  story failure mode              ->  exit shape
  --------------------------------------------------------------------------
  "the up-move never starts"      ->  cut fast and small, on STATE, not a clock
  "the up-move ends"              ->  ride, give back a fixed fraction of the peak
  "the coin rugs"                 ->  a DOOR problem, never an exit problem

  families, always read side by side:
     scalper control    clock 45
     loss-capped ride   unarmed trail 20-25, cap 600-1800
     tail preserving    tp100 / trail50 / cap 1200
     state-conditional  cut on: the pusher sells | new-buyer arrival stalls |
                                flow reverses before price does | the burst dies
```

Three measured facts force this:

- **No profitable node in the roster exits on a clock.** Hold `p90 / p50` runs 3.0x to 5.3x in
  every one of the five nodes; the hold is reactive.
- **Exit choice moves the same fires by up to 7.6 points**, which is inside the range that gets
  attributed to selection. A book judged under one exit family is not judged.
- **A static loss cap trades the winner for the loser at a losing rate**: tightening the stop
  cuts `L` by 60 % and `W` by 75 %. Only a state-conditional cut can move `E`, because it has to
  tell a drawdown inside an up-move from the end of one.

An exit swept on an unselected pool always returns the shortest clock, because that pool is
mostly dying tokens. The exit is settled **after** the gates, on the selected set.

---

## 6. The only two ways a slot closes

1. **A measured mechanism** that holds at every coordinate. Three qualify today: copying a
   wallet's fill (truncate the path one slot before they land and every "before" cell goes
   negative); the wave's first buy (landing first is a look-ahead and landing second is already
   negative, so a tape-observable trigger cannot precede the print that triggers it); silence
   freezing price (the curve is the counterparty, so an exit needs no print).
2. **A conjunction search** showing that no filling of the other slots rescues it, at the floor,
   under both exit families, walked forward.

Anything else produces "sentence `c` is red", and the line stays open with its empty slot named.

---

## 7. How a result is written

Every entry in [_!___evidence.md](_!___evidence.md) uses this shape. No exceptions.

```
### <name>
D  <door, or "none">
E  <event>
P  <permissions, or "none">
X  <exits read, all of them>
R  <re-entry>   S  <clip>   seat <fill>   universe <window, fires, tokens>

| cell | n | first/day | SOL | %/trade | days+ | worst | win | be | top1 % | maxtok % | fit | hold |

verdict: <what this sentence does>          empty slots: <which>
```

`be` is the cell's own realised break-even `L / (W + L)`. `top1 %` is the share of net in the
top 1 % of trades, against the 9-12 % calibration.

---

## 8. The queue

Each campaign is a full sentence with a kill condition. Run them in this order.

```
  C1  REPAIR THE BOOK THAT ALREADY WORKS                    [lowest risk, closest to done]
      start   door rule v3 MONEY: +4.80 SOL, 4/7 days at lag_115, engine-reconciled,
              ~286 tickets a day - and 300+ trades worse than -50 % against 55 above +200 %
      D  slow-wall launch door + an L-DOOR built from the rug terms          <- new half
      E  as shipped
      P  creator has not sold
      X  loss-capped ride + a state-conditional cut                          <- new half
      target  remove the left tail without removing the right one
      kills it  the -50 % tail is unpredictable at decision time; then L-selection is dead
                and that is itself a large finding

  C2  MULTIPLY THE TWO POSITIVE ANCHORS                                       [the flagship]
      story  this launch build makes coins that keep living; this coin is past its rug
             window and still alive; a router buy with size just started a burst on it;
             the creator has not sold and the last hill's crowd has left
      D  slow-wall launch door   (variants: metadata document, >= 8 professional builds)
      E  burst START - the first print of a burst opened by a router or terminal buy
         of >= 0.5 SOL on a mid-life token
      P  age 60-900 s . vsol 33-81 (headroom) . creator not sold . crowd hold < 50 %
         . not a racer print . not spraying
      X  both families, derived from the story
      R  unlimited, one position per token    S  0.2 SOL    seat lag_115
      known  the event with no door on the full tape: -1.5 .. -13 %/trade
             the same event on the tokens these traders pick: +2.6 .. +6.7 %, every exit
      the run  does a shippable door recover that token population?
      kills it  no door recovers it, on any of the three door variants

  C3  FINISH THE ONE RULE THAT PASSED A HOLDOUT                              [one run]
      re-price campaign-break v0 AS ITS FROZEN SENTENCE at lag_115.
      What carries a 115 ms number today is a wider family, on a differently-named machine.

  C4  THE EXIT LAB                                     [runs on C1/C2's best entry]
      hold D/E/P fixed at the best sentence and vary ONLY X across the four families.
      question  can a state-conditional cut hold W >= 80 while L falls toward 16?

  C5  THE THREE UNSTUDIED OPEN NODES
      deep-age big clip first: win 44 %, median trade -1.23 %, top 1 % = 32 % of net
      against 112-181 % everywhere else - the only node whose ordinary trades pay for
      themselves. Then quiet deep-age, then hot-tape re-entry.

  C6  AGREEMENT AS A DOOR
      two or more of the solo 26, independently, in the same coin, entry age > 400 s.
      Co-selection lift rises from 0.17 under 30 s to 3.50 over 400 s and is unmeasured
      forward.
```

```
  sequencing
  ----------
   C1 --+                              C1 and C2 share the door work and the exit lab.
        +--> C4 (exit lab)             C3 is a single run that settles a standing claim.
   C2 --+                              C5 and C6 open new ground once the method has
   C3 (parallel, one run)              produced one green sentence.
   C5, C6 after
```

---

## 9. Standing refusals

- **No result without its coordinate**, and without naming which slots are empty.
- **No proxy label.** Money is the score. Detection rates, graduation rates and swing labels
  describe a sentence; they never rank one.
- **No fixed bar.** The bar is the cell's own realised break-even.
- **No slot closed from a red number.** Only a measured mechanism or a conjunction search.
- **No verdict under one exit family.** Both, always, side by side.
- **No exit swept on an unselected pool**, and no selector judged by an exit chosen that way.
- **No node closed on a subset of its members**, and "his fill is unreachable" is never written
  as "his decision is unreachable".
- **No trimming a frozen sentence** because a prefix holds out better. That is fitting the
  holdout; write a new sentence and burn the week.
- **No candidate table read as a universe.** The raw tape is the universe and every condition
  is a column, so a filter cannot hide in the query that builds the table. Print the funnel.
- **The reader's own prints stay out of the pool**, and his response rate names an event and
  gates nothing.
