# Workflow: how a rule is found, measured and closed

The gates, the campaign queue, and how a result is recorded.
[_!___strategy.md](_!___strategy.md) is the market and the basis;
[_!___derive.md](_!___derive.md) is how a parent sentence is derived from one paying
wallet;
[_!___inventory.md](_!___inventory.md) is the ideas used on that parent;
[_!___evidence.md](_!___evidence.md) is the numbers. Read this file for the coordinate,
the gates, and the queue. A result that skips a step is not a result.

The target is a **shippable harvester**: leftover of real up-moves, 115 ms both legs, net SOL,
most days positive, enough tickets, re-entry. Latency is not the edge. The edge is remaining
intent that is still unpriced at fill.

```
  0.  TARGET           what counts as done
  1.  IDENTITY         E = P.W - (1-P).L - toll; two selection axes
  2.  COORDINATE       six slots plus a frame; a result is an address
  3.  LOOP             parent from derive.md; inventory on that parent; where a red number routes
  4.  GATES            floor, money, tail, walk-forward, holdout
  5.  EXIT             derived from the sentence, never swept on junk
  6.  CLOSING          the only two ways a slot closes
  7.  RECORDING        the template every result is written in
  8.  SENTENCES        parent from derive.md; inventory on that parent
  9.  INVENTORY        [_!___inventory.md](_!___inventory.md) - one file, not copied here
  10. QUEUE            campaigns in order, with kill conditions
  11. REFUSALS         what is never done again
```

---

## 0. The target

A public trigger, filled at 115 ms on both legs, that harvests leftover of real up-moves.
Done, in order: frozen sentence, green on a disjoint holdout week, at or above the ticket
floor, tail-robust, engine-reconciled trade by trade, paper, small real.

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

Break-even is `p = L / (W + L)`. It is not a constant. It moves with entry position and with
the exit. Judge a rule against **its own realised break-even**, never against a fixed
percentage (`P(swing >= 50 %)` is leftover description, not the score).

Two axes live in the equation. Both belong in D and P, not in X:

```
   P-axis : which token goes UP
   L-axis : which token goes to -50 %
```

Cutting L is worth as much as lifting P. Bundle share, dev holding, fresh-wallet share,
sniper count and creator-sold are in hand. They score red against "does this spike" and they
**hurt** survival money on the documented-project book. On that book they also fail as an
L-door: the -50 % tail is 8 trades in 646, too small to select ([_!___evidence.md](_!___evidence.md)
3.6). The remaining L-coordinate is door-v3 MONEY (C0b: 235 of 1,667 trades at -50 %).

8dtx's coins do not raise detection (31.2 % vs 31.3 %). They cheapen losers (-8.1 % vs
-12.3 %). That is the thermometer that L-selection exists. His mint list is not a gate.

---

## 2. The coordinate

A rule is six slots plus a frame. Every measurement fills all six. Empty is a value.

```
  +---------------------------------------------------------------------------+
  |  D  door         which tokens we watch at all; token-level, latency-free   |
  |  E  event        the print we fire on; a condition on UNPRICED state       |
  |  P  permissions  state already true when the event prints                  |
  |  X  exit         the harvest shape, derived from the story                 |
  |  R  re-entry     tickets per token, one position open at a time            |
  |  S  size         the clip (standing: 0.2 SOL; physics min ~0.126)          |
  |  ---------------------------- frame --------------------------------------|
  |  seat      last print landed by fire + 115 ms, BOTH legs                   |
  |  cost      125 bps a leg + 0.000225 SOL a leg + own impact on vsol         |
  |  universe  full tape, the window, the fire count                           |
  +---------------------------------------------------------------------------+
```

**A red number at coordinate `c` means "sentence `c` is red". It never means "slot X is
closed".** A red sentence with `D` empty says only that this event has no edge without a door.

Standing clip: 0.2 SOL. Standing re-entry: unlimited, one position per token.

---

## 3. The loop

```
  [0] FRAME              fixed once: seat, cost, full tape, coordinate recording, gates
        |
        v
  [1] PARENT             one paying member, DELAY-legal event, then D / X / P
        |                ([_!___derive.md](_!___derive.md)). Inventory does not invent this.
        v
  [2] SCORE THE WHOLE    money only, on the conjunction, on the full tape
        |                both exit families side by side, every time
        |                rank among cells that look like a TYPE (section 4)
        v
  [3] ABLATE             drop one term at a time; keep it only if it RAISES total SOL
        |                if nothing lifts, do not cut
        v
  [4] INVENTORY          vary ONE slot at a time on this parent, from
        |                [_!___inventory.md](_!___inventory.md). a one-slot change is a
        |                new address. add an idea only in the named empty slot.
        v
  [5] GATES              floor, tail share, fit/hold  (section 4)
        |                fail -> back to [4] naming WHICH SLOT. never "closed"
        v
  [6] FREEZE -> HOLDOUT  a disjoint week, read once, never trimmed
        |                red = the SENTENCE is wrong -> derive.md phase 10, then [1] or [4]
        v
  [7] ENGINE -> PAPER -> SMALL REAL
```

An event is never scored alone. An unconcentrated pool is red by construction. If the
parent is still red after searching the inventory on its fires, the list is short in a
named slot: add one idea there from the member's contrast, and re-search that slot. Do
not replace E to invent a new parent. Do not stack ANDs on a frozen parent.

---

## 4. The gates

```
   all cells
      |  FLOOR      >= 50 first-per-mint trades a day, ON EACH DAY, not on average
      |             (a refusal, not a target - print the per-day list, never the mean)
      v
      |  MONEY      total net SOL > 0 on the whole sentence  (never a proxy label)
      v
      |  TAIL       top 1 % of trades <= ~20 % of net
      v
      |  CLIENT     positive with its single best CLIENT removed, and positive in
      |             >= 95 % of a bootstrap over clients
      v
      |  WALK-FWD   fit > 0 AND hold > 0 on disjoint halves
      v
   candidate -> FREEZE -> disjoint week -> engine reconcile -> paper -> small real
```

**Tail calibration.** A real convex book at this seat puts about 9-12 % of its net in its
top 1 % of trades. A cell holding 50-270 % of its net in its top 1 % is noise around zero.
Report the share, and the single largest token's share beside it. If one token carries the
result, that is the result.

**The floor is per day, and the mean will lie to you.** A cell whose tickets run
`16, 177, 210, 24, 28, 9` has a mean of 53.9 and clears fifty on **two of six days**. Worse, the
two big days are exactly the days its single carrying client was alive, so the mean **launders
the client concentration through the gate** - the two failures reinforce each other instead of
being caught. **Print the per-day ticket list beside every book.** A mean is never the floor.

**A stub is not a floor day.** The last-leg week is 6.76 days: 6.2 h, then six full UTC days,
then 12 h (`cvx.py` DAY0 = 2026-08-30 00:00 UTC, tape 17:48 that day through 2026-09-06 12:00).
A day with hours covered well under 24 is a truncated window. Quote tickets/hour × 24 beside
it; do not refuse the floor on the stub. C8's 47 tickets in 6.2 h is ~182/day.

**TYPE sits in front of ranking.** Money is the score of a cell that already looks like a
general type. Ranking by total SOL first, then applying the floor, selects the day-specific
cohort every time: that cohort is where the SOL is. A door is a general type only if its
**token-birth supply** tracks the tape's births on full UTC days. A cell is a general type
only if its per-day first tickets track that door, not a two-day slice of it. Report, beside
every book:

```
  per-day first tickets
  peak/trough of those tickets on FULL UTC days
  share of tickets in the two fattest days
  the same three numbers for the tape's births and for the door's births
```

On this tape, full-day births peak/trough at **2.1x** (24,145 / 11,704) and the two fattest
days hold 36 %. A cell at 26x / 81 % in two days is a client, even when its door's own supply
is only 5.7x. The walk's **reading** is the best SOL>0 cell that passes TYPE. If none, the
inventory on this parent has no general-type cell. The SOL leader of a two-day spike is a named client
book, not the next parent, and C7 (more days) does not turn it into a type.

**The client gate, and it outranks walk-forward.** Trades are not independent draws. Behind a
launch door they come in **clients**: one creation build launches many coins over a day or
two, and every trade on those coins shares a creator, a machine and a two-day window. The
sample size is the number of clients, not the number of trades.

Measured (evidence 3.1a): a one-week print tape holds **34 slow-wall builds, median life two
days**, and **one build carried 82 % of C2's net and 221 % of C0b's**. A 1,006-trade cell was
19 draws with one of them inside the fit half. So:

```
  CLIENT = the unit that generates correlated trades
           launch-door sentence  -> the creation BUILD (exact ordered creation ix_labels)
           trader-node sentence  -> the machine (ix structure + payer), never the wallet
           event-only sentence   -> the token

  report, always, beside the book
     trades and CLIENTS, side by side
     the top client's share of net
     leave-one-client-out: the book with each client removed in turn, worst case quoted
     a bootstrap over clients, 2000 draws: share positive, and p5
```

A cell that dies when its best client leaves is that client's book. A cell that survives is a
candidate - and if it clears leave-one-out but sits under 95 % on the bootstrap, the answer is
**more clients, not more terms**: a longer print tape, not another AND.

**Walk-forward.** A cell chosen in-sample is a candidate to freeze, never a result. Split on
clients where they exist; a day split behind a door whose client rotates every two days is
splitting on the wrong axis and will read as a refutation when it is an absence of power.

**Artifact detectors, beside every book:** zero-lag column (the ratio is the artifact);
gap-to-next-print (money under 50 ms is an artifact); share of entries with no print in the
hold; trail fire rate (a trail firing on 18 % of trades is a clock).

---

## 5. The exit is derived, not swept

```
  story failure mode              ->  exit shape
  --------------------------------------------------------------------------
  "the up-move never starts"      ->  cut fast and small, on STATE, not a clock
  "the up-move ends"              ->  ride, give back a fixed fraction of the peak
  "the coin rugs"                 ->  a DOOR / L-axis problem, never an exit problem

  families, always read side by side:
     scalper control    clock 45
     loss-capped ride   unarmed trail 20-40, cap 600-1800
     tail preserving    tp100 / trail50 / cap 1200
     state-conditional  cut on: the pusher sells | new-buyer arrival stalls |
                                flow reverses before price does | the burst dies
                                (only AFTER the entry sentence already clears the gates)
```

A static abort (time x percent) gives up winners as fast as it saves losses (104 cells, zero
positive). Do not rerun that grid. A state-conditional cut is a different object and is open.

An exit swept on an unselected pool returns the shortest clock, because that pool is mostly
dying tokens. Settle the exit **after** the gates, on the selected set. A 45 s clock is a
scalper control, never the primary read on a harvester story (median playable move is 87 s).

---

## 6. The only two ways a slot closes

1. **A measured mechanism** that holds at every coordinate. Copying a wallet's fill. The
   wave's first buy as a trigger (landing first is look-ahead, landing second is already
   late). Silence freezing price. Windowed flow on a curve (it is the price path). Waiting
   later in a move for confirmation (the deficit is invariant). A zigzag turn as the event
   (leftover description; T6). Unique-new-buyer acceleration as an event (dump-factory
   detects the same). A racer bundle after silence (confirmation, not the event).
2. **A conjunction search** showing that no filling of the other slots rescues it, at the
   floor, under both exit families, walked forward.

Anything else is "sentence `c` is red", and the line stays open with its empty slot named.

---

## 7. How a result is written

Every entry in [_!___evidence.md](_!___evidence.md) uses this shape.

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

`be` is the cell's own realised break-even `L / (W + L)`. `top1 %` is the share of net in
the top 1 % of trades, against the 9-12 % calibration.

---

## 8. How a new sentence is found

The generator of a **parent** is one paying member: [_!___derive.md](_!___derive.md).
[_!___inventory.md](_!___inventory.md) is the idea list used **on that parent** (derive
phase 10). DELAY is the veto on the trigger, before any conjunction, not a spoken seed
that produces a 4-tuple.

A parent answers four questions. Three of them are easy; the fourth is what makes a
sentence possible at all.

```
  1  WHO    will spend SOL on this coin in the next seconds or minutes
  2  WHY    - his own business reason, not ours
  3  WHAT   on the tape (never the price) shows the intention now
  4  DELAY  why that SOL has not landed yet, and still has not landed 115 ms after we fire
```

**Question 4 is the scarce one.** If the tell and the SOL arrive together there is no trade at
any seat, and that is how nearly every dead line here died: the swarm lands in the trigger's own
slot. So enumerate delay first - it is the binding constraint, not the tell.

| delay mechanism | magnitude | used here |
| --- | --- | --- |
| a human reacting to a feed, a call, a stream | seconds to minutes | **no** - the largest one in the market |
| a committed budget spent on a schedule | seconds to minutes | **C9 (S3)** - cadence of the beat is the wrong spelling |
| one operator executing in legs | seconds to minutes | **C9 (S1) first cluster; C11 late-leg; C12 this-coin second burst** |
| a mechanical re-buy below its own exit | seconds | **C10 (S4)** - red here (6.15) |
| the tape goes still after a fall | seconds | **C9 (after-flush)** |
| live rotation: a buy on another coin, then a buy here | ~1 slot | **C13** - DELAY = 0 (6.18). Live-gap p50 is 1.0 slot |
| first buy after a run of sells | tell 1 slot, leftover 81 ms | **C14** - no SOL>0 TYPE-pass (6.19). Next-print p50 81 ms |
| first operator after only creator and seed racers | same slot, leftover 2 ms | **C15** - DELAY = 0 (6.20). Launch co-arrival; age p50 0 s |
| first run of an operator structure (coin already has others) | remaining spend 8.8 s on 22.5 %, next-print 56 ms | **C16** - TYPE-pass +2.27 fails floor, body, hold, client (6.21) |
| an off-chain event with an on-chain footprint | seconds to minutes | **no** - not stored at decision time |
| a structural obligation (the wall, a fee tier, migration) | minutes | **out** - the target is not migration |
| habit: the same operator repeats himself across coins | minutes to days | **no** - that is a door (S9), not an event |
| co-arrival inside one slot | ~0 | yes, repeatedly - and it is why those lines are dead |

Terms are replaceable. Holdout kills the sentence.

A red frozen sentence is not patched. Ablation names the **dead clause**. The next sentence
is the next unused filling of the named empty slot on the same parent (inventory), or a
new idea in that slot from the member's contrast ([_!___derive.md](_!___derive.md)
phase 10). DELAY = 0 on the trigger goes back to a new member or a new trigger, not a new
inventory E. Neighbourhood is not a new idea.

```
  frozen sentence RED
        |
        v
  [A] name the dead clause     who was wrong / the tell was wrong /
        |                      the remainder does not survive 115 ms
        v
  [B] do not patch             no trim, no extra AND
        |
        v
  [C] next filling             same parent, named slot; or a new trigger / member
        |
        v
  [D] score the whole conjunction
```

**Which clause is dead is readable from the shape of the failure.** Only one of these six kills
a story; the other five hand you the next one.

| the failure | the clause that died | the next story |
| --- | --- | --- |
| nobody arrived at all | WHO | same tell, a different actor class |
| they arrived, but before or after our window | DELAY | right actor, wrong moment - move the event, keep the story |
| the tell was already in the price | WHAT | find an earlier footprint of the same intention (the commitment, not the execution) |
| the tell and the money are simultaneous | DELAY = 0 | this actor is unusable at any seat. **This is the only failure that kills a story** |
| positive on some coins, negative overall | the DOOR is missing | keep the story, add a coin selector |
| winners fine, losers catastrophic | the L-DOOR or the EXIT is missing | keep the story, work the loser cost |
| positive but under the ticket floor | too narrow | widen the ACTOR class, never add permissions |
| tickets spike on two days and vanish on the rest | the DOOR is a client, not a type | do not rank this SOL leader as the reading; a type tracks tape births (4.9) |

### G1 - the open nodes (a reader is an instrument)

Each of the 26 independent readers answers the one question differently. A node is a story.
Pick a node open at our seat. Split members first. Locate **one member's** decision print
(his own reaction time before his fill). Read what was true there that is not price. His
response rate **names** the event. His mint list is never the universe. His own prints
stay out. The playbook: [_!___derive.md](_!___derive.md). Toolkit mapping and the hot-tape
worked example: [node-derivation/method.md](node-derivation/method.md),
[node-derivation/toolkit](node-derivation/toolkit/README.md),
[node-derivation/hot-tape-rule-1.md](node-derivation/hot-tape-rule-1.md).

| node | story in one line | seat |
| --- | --- | --- |
| mid-tape one-shot | an up-move is starting | **open, split (5.8).** Instrument **9Uq8GV**: buy >= 1 at 75-100 ms lift 13.01; FOLLOW of that print is not reachable; RACE +2.21 % 8/8. Working file [mid-tape-rule-1.md](node-derivation/mid-tape-rule-1.md). 9999hu / 88887Q name sell >= 1 (a second parent). 8dtx2t names burst start; public spelling is C2. Inventory 4-tuple walks C8-C16 do not invent this parent (6.13-6.21) |
| quiet deep-age | the pusher of an old quiet coin restarted | **red here** (evidence 6.5). Named event is token-silence burst start; the burst lasts ~80 ms so a 115 ms fill is after it. Slow-wall × silence is **-91.95** (6.13) |
| deep-age big clip | few coins deserve a real size; this is one | **red here** (evidence 6.7). A public size print is not the tell (response = base). He starts the burst half the time |
| hot-tape re-entry | a pullback inside a live up-move, taken again and again on the same coin | **OPEN, and it is two nodes** (evidence 1.11). Three of the six book **+2.2 to +2.3 %/trade, 8/8 days, body positive, biggest coin 1.8 %** at the RACE seat under a 15 s clock; the pooled node hides them. The ceiling needs a STATE anchor - at our own fill the same decisions are -0.66 %. Public events tried: swing pullback, the up-move portrait, the independent-machine count - all red. E is the frenzy-absorbed sell (1.12); with D earlier-frenzy-sells-failed and the member's own exit tp10 sl25 t60 it books **+1.22 %/trade 5/7, body +1.23** on the full tape and fails the tail (1.13). With P holders >= 368 x age >= 158 s instead of the door: **+2.07 %/trade, 141 a day, 7/7, body +3.26, top 1 % 17.6 %** (1.14), in-sample |
| 2+ of 26 agree, age > 400 s | two independent readers picked the same living coin | a door those two deep-age nodes share; unmeasured forward |
| instant launch | snipers and retail at birth | **dead** at this seat (consumed inside ~2 slots) |

`8dtx` sits in mid-tape one-shot. Stop mining 8dtx. All seven members are measured (6.9).
The two largest books do not name burst START. `9Uq8GV` does, and leftover on their coins
is real; public D besides slow-wall is red. `8aaRWu` leftover is a short keep-pop off
slow-wall and they do not name the event. `ApfmkS` starts the burst and names nothing.
Quiet deep-age is red at the same seat (6.5): the public print they follow is a
token-silence burst start, and the burst is over before a 115 ms fill. Deep-age big clip is
red too (6.7): a public size print is not D9Uite's tell. The on-tape remainder of that story
is C6 (agreement on the coin). Count machines, not wallets: 41 of 77 roster addresses are 7
machines.

### G2 - phase transitions (the dev's plan)

```
  launch -> scramble -> (abandon | campaign) -> attention -> (push | extract) -> wall -> fees
```

Money sits at the change of phase. Each transition is a different "who decided what":

| transition | story seed |
| --- | --- |
| scramble ends, not abandoned | this launch client builds coins that keep living (slow-wall door: a screen) |
| silence breaks | someone just chose to spend again (campaign-break v0 is red at lag_115, 6.6. Quiet deep-age is red at lag_115, 6.5) |
| attention arrives | strangers are looking (needs feed/replies/live **stored at decision time**) |
| push toward the wall | remaining campaign budget is still firing |
| extract starts | L-axis: do not enter |

The machine census is how the decision is seen: which fingerprint fired at the stall.

### G3 - the dead clause names the next story

| dead clause | next story it produces |
| --- | --- |
| metadata marks coins that spike (30.7 % vs 31.3 %) | metadata marks coins that **live** (documented-project) |
| zigzag turn is the event | event = first print where unpriced state holds |
| Terminal names remaining spend (dropping it raises SOL; 8dtx responds 0.84 %) | the machine class is wrong; the dip-after-hill frame can be reused |
| any router buy >= 0.5 is remaining spend starting (holdout red) | the door is wrong, or the event is too wide (any buy vs first-of-burst) |
| holder book predicts life (it hurts survival money) | those facts may predict **collapse** (L-door) |
| 8dtx coins spike more (detection identical, losers cheaper) | hunt decision-time facts that cheapen -50 % trades |
| static abort (winners die as fast as losses are saved) | rugs are do-not-enter, not cut-after-entry |
| quiet-deep-age token-silence burst lasts ~80 ms | DELAY. Leftover of that burst is not at 115 ms. Next unused G1 node: deep-age big clip |
| campaign-break v0 leftover is not at 115 ms (TP40 -0.60 SOL, 18.6 first/day) | DELAY. The +2-slot holdout does not transfer. Do not widen the machine class |
| deep-age big clip does not follow a public size print (response = base) | WHAT. The tell is which coin, not which print. C6 |
| 9999hu / 88887Q do not name burst START (lift 1.02 / 1.14); leftover on their age>=15 coins is ~0 at lag_115 | WHAT. Burst START is 8dtx's tell. Create cgroup include is the market (6.9). Next unused member on this node: 9Uq8GV |
| 9Uq8GV names buy>=1 / burst START and leftover on their coins is +4.12 %/trade; public keep / not-dump / not-3ix / cgroup are red; slow-wall reproduces C2 | the DOOR is missing. Keep the story. Slow-wall is already measured (6.4) |
| 8aaRWu does not name burst START (lift 1.33); leftover +1.72 %/trade clock 45 on keep and off slow-wall; 7ix:Buy is two launch builds | WHAT is weak. Keep leftover off slow-wall does not make keep a door (already red as public D). Loosening slow-wall is the wrong direction for this book |
| ApfmkS names nothing (all lifts ~1); self-start 45 %; followed leftover +4.90 % unnamed | WHAT. Same shape as deep-age big clip. He starts the burst. Public print is not the tell. All seven members of the node are measured |
| mid-tape ep1 vs ep2: four unpriced facts are the market; n_pro grows with age; windowed machines fall; ep1 does not separate return coins; D x burst START all red, closest -0.92 %/trade | the DOOR is still missing. Those four facts are not it. Keep the story. Do not AND them onto burst START again |
| mid-tape holder book (tokens remaining) last_pro_hold lift 3.50 then -3.57 %/trade as D on burst START; ep1 book does not name return coins | WHAT. "Pushers still hold" as balances is not D on this event. Stop adding holder-book terms here |
| four-slot walk: documented × silence × creator × trail40 +47.54 is keep+ep50 lookahead; causal keep launch **-1,007**; age >= 60 s **-0.82** | WHAT. The door file is not the spelled door. Do not AND onto documented × silence. Do not reuse `cvx_meta` as D. C9 is 6.14 |
| C9: after-flush × n_pro>=8 × creator+av+cu × trail40 +8.73, hold +2.03, LOO +5.93, boot 98.5 %, top client 32 %; tickets 21/72/170/143/58/35/39; peak/trough 8.1x, top2 58 %; tail 84 %, body +1.41; S1 is slow-wall; S3 is launch; S4 under is_pro is 2 machines | DELAY / WHAT. TYPE fails. Client gate clears; floor and tail do not. Do not AND onto this SOL leader. S4 without is_pro is C10 |
| C10: slow-wall × rebuy × creator+av × trail40 +4.47, hold -1.13, LOO +0.57, boot 81.8 %, top client 87 %; tickets 16/179/208/33/32/7; peak/trough 26x, top2 81 %; tail 181 %, body -3.63; 30 machines, med_nw 958 | DELAY / WHAT. TYPE fails: two-day client inside slow-wall (door supply is 5.7x). Do not AND onto this parent. Do not restore nw<=50. Tape-only mid-tape E in the inventory is scored |
| H7: the hot-tape node splits by MEMBER - three of six are +2.2 to +2.3 %/trade 8/8 at RACE cap15 with positive bodies, three are noise; pooled it is +0.50 % with a 216 % tail | the DOOR / WHO. The node label was wrong: two strategies share one name. Keep the story, keep only the members that pay, and derive the event from THEM |
| H7: the price path is red in three constructions (swing pullback -3.35 %, the up-move portrait -3.58 %, both 0/8) and TIGHTENING the state makes it worse | WHAT. Every hot-tape feature to date sits in strategy 1.4's already-in-the-price column. Do not build another price-path event on this node |
| H7: the independent-machine count is monotone in money (-4.08 % under five builds, -2.99 % at twelve-plus with a de-concentrated mix) and does not cross zero | WHAT, partly right. The unpriced axis carries about 1.1 points as a threshold. The unused form is the machine vector FITTED, not cut |
| H7: a door fact dated at age 60 s, fired on at age 20 s, reads +54.97 %/trade and 80.8 % win | the check, not the story. Every door fact carries a date; a fire before it is a lookahead |
| H8: the trigger is a SELL for the member that pays and a BUY for the member that loses, at the same speed; the frenzy-absorbed sell is +1.30 % on its coins and -3.96 % on the rest | the DOOR. E is found; keep it frozen as the event and search D. Size, busyness and prior-frenzy facts only buy the coin list's share |
| H9: on the coins the member trades, E books +4.46 % 7/7 BEFORE its first buy there and -0.68 % after; earlier-frenzy-sells-failed x its own exit tp10 sl25 t60 is +1.22 % 5/7 with a 39.7 % tail and a fading second half | the DOOR, re-aimed. The coin list was the member's future arrival, not a coin property. Keep E, the door fact and the exit; the next door term predicts the FIRST arrival of a buyer like the member |
| H10: the stop-outs are young, thin coins - age, holders, reserve and held share separate them, heat and composition do not; holders >= 368 x age >= 158 s makes the sentence +2.07 % 7/7 with the door redundant | the check, not the story. In-sample on seven days: the next result is the HOLDOUT on the days after 09-06 12:00, before any new term |
| H11: the frozen sentence on 4.5 unseen days (09-06 12:00 to 09-10): +1.90 %/trade, 132 a day, 5/5, body +1.73, top 1 % 23.2 %; the door of 1.13 fails (-0.53 %) | none - it holds. What is left is the tail and the ticket count: a second event, not a new term on this one |
| H12: the second operator's burst-start leg reacts in 47 ms (we land ahead 5 %); its dip-buy leg is reachable (+3.50 % 8/8 on its picks) and red in every public spelling (-4.6 % full, -1.7 % established) | A: DELAY about zero - unusable at our seat. B: WHAT - no single tape fact carries its selection; the unused form is the descriptors fitted as one score |
| H13: rule 1's exit re-read on its own pool: the member sells hard at +15..+20 %, not +10 %; take profit +15 %, stop -25 %, 90 s books +2.73 % study / +2.53 % holdout, tail 13.9 % / 17.7 % | the check - the exit was read on the wrong pool (law 26). Next: the tail, then size |
| H14: rule 1's top 1 % is upside gaps (take profits filled +33..+61 %); capped at the median take profit the book stays positive with a 10.3 / 12.6 % tail. 0.5 SOL clips book 1.36 / 1.16 SOL a day, every day positive | none - the tail gate is passed in its purpose; the cost is the stop-outs. Next: a term that removes stop-outs, then the arrival door |
| H15: nothing at rule 1's fire separates its stop-outs (best 0.055, not stable across tapes); a sell-reactive buyer count does not predict the member's arrival (0.60 / 0.45); the surviving cuts are a bigger frenzy - higher margin, smaller book | D and P are done for this seat. Next: paper |
| H16: every slot of rule 1 re-derived on its own pool by walk-forward: "bought >= 2 SOL in 2 s" drops out; 16-17 % of the old trades (46-48 % of the SOL) closed on the graduation print, and the reserve <= 100 SOL removes them; stop -40 % holds out of sample, clock 240 s fails. Updated rule 1: +4.39 % study / +3.44 % holdout, 5/5, top 1 % 9.8 / 13.1 %, 1.58 / 1.07 SOL a day at 0.35 SOL | none - every ship bar passes on both tapes. Next: paper, then the days after 09-10 as the clean test |
| C11: slow-wall × late-leg × creator+av × derived +6.93, hold +0.34, LOO +2.15, boot 99.5 %, top client 69 %; tickets 2/78/92/11/3/2; peak/trough 46x, top2 90 %; tail 23 %, body +5.33 | DELAY / WHAT. TYPE fails. S1's unused moment. Body pays; floor 2/6; plus is C2's door on two days. Do not AND onto this parent. Do not move S1's event again. TYPE reading is 4.10 |
| C12: n_pro>=8 × second-attempt × creator × trail40 +3.12, hold -7.61, LOO -7.18, boot 57.4 %, top client 330 %; tickets 53/171/344/377/203/142/102/2; peak/trough 3.7x, top2 54 %; tail 935 %, body -26.04 | DELAY / WHAT. TYPE passes. Floor passes on full days. Body, tail, hold, client gate fail. SOL leader is slow-wall +17.78, not the reading. Do not AND onto that parent |
| C13: slow-wall × arrive × creator+av × trail40 +16.46, d50 2/6, top1 73.6 %, body +4.35; fire tickets 0/18/180/211/36/30/8/0, peak/trough 26.4x, top2 81 % | DELAY = 0. Live-gap p50 is 1.0 slot. No SOL>0 TYPE-pass. Wide TYPE cells are -112 to -437. Leave exit is red. Do not AND onto the slow-wall leader. This rotation is not an event at 115 ms |
| C14: slow-wall × sellrun × creator+av × shipped +10.40, d50 2/6, top1 90.2 %, body +1.02; fire tickets 0/23/159/189/35/25/5/0, peak/trough 37.8x, top2 80 % | DELAY of leftover is 81 ms (gap p50), 41.9 % <50 ms. No SOL>0 TYPE-pass. Closest TYPE cells -5.24 to -9. Wide TYPE cells -194 to -289. low X red. Do not AND onto the slow-wall leader. Do not walk another after-sellers spelling next |
| C15: none × firstop × creator × clock45 +0.01, 7 trades, d50 0/3; full-tape none × none × clock45 **-50.75**, 1,360 trades, 0/8 | DELAY = 0. Age p50 0 s; prior-gap p50 0 slots (89.1 % same slot); next-print p50 2 ms. Launch co-arrival, not mid-tape. follow X red. Do not AND. Do not walk first-outside-buyer (same launch shape) |
| C16: slow-wall × firstrun × cu_hi × derived +2.27, d50 4/6, top1 219 %, body -2.71, hold -2.93, LOO -1.32, boot 65 %; tickets 40/101/120/75/92/17, peak/trough 7.06x against door 4.7x | DELAY / WHAT. TYPE passes at the 1.5x bar. Floor, body, hold, client fail. SOL leader n_pro>=8 +9.14 TYPE-fails 6.9x / 62 %. follow red. Do not AND onto either parent. Do not walk operator-cross-K (a count cut on this event) |

### When the generator is empty

Stop inventing tape conjunctions when all of these hold:

- every **open** node has one frozen sentence at `lag_115`, and it is red
- the dead clauses all say the remainder is not on the tape at decision time
- off-chain fields, stored live, have also been scored

Until then, a red result is a named false clause. Next filling is the named empty slot on
the same parent, or a new trigger / member ([_!___derive.md](_!___derive.md)).

---

## 9. Inventory

Ideas live in [_!___inventory.md](_!___inventory.md). This file does not copy them.

---

## 10. The queue

Each campaign is a full sentence with a kill condition. C0-C3, C5 and G1 are measured; what is
left of the queue is below them.

**Where the queue stands.** Every launch-door sentence now fails on the same thing and it is
not a term: the door's ticket supply is a rotating client, a week of prints holds about 20
independent builds, and one of them carries the book (section 4, evidence 3.1a). Two cells
are frozen and waiting on more clients, not more ideas. C6 does not wait on that.

```
  C0  TWO JOBS THAT UNBLOCK THE BEST UNTESTED CELLS         [do these first; neither is a story]
      C0a  slow-wall door labels on the last-leg week tape.                    [done]
           sidecar study-kernel/cvx_swdoor.parquet from launch_build_day_stats
           (previous UTC day, not aa.door3's running total). 127,833 tape tokens,
           119,234 with previous-day stats. Slow-wall n>=20 sw>=5 % not bundler:
           7,616 tokens, 34 hashes / 22 cgroups, 6.0 % of tokens and 17.3 % of
           prints (~1,130/day). Shipped 8 % cut: 3,906 tokens. C2 unblocked.
      C0b  run door-v3 MONEY at lag_115 through the three gates.               [measured]
           +5.38 SOL, 3/6 days, 247 first/day (floor ok). top1 358 % of net,
           without top 1 % **-13.85**. fit +7.30 hold **-1.93**. creator-in is
           load-bearing. 235 / 1,667 trades at -50 %. does not ship.
           Reproduced independently by cvx_c0b.py to one trade (+5.39, 1,666).
           CLIENT GATE, run after: 23 builds, dropping the best one is **-6.50 SOL**,
           bootstrap positive in 61.7 %. It is one operator's book (3.1a).
           It delivered two things instead of a book:
             L-AXIS ANSWERED - bundle share < 0.20 cuts the -50 % rate 14.1 % -> 2.3 %
             and lifts +5.39 -> +10.43 SOL, surviving the reserve-band control that
             kills every other term. Evidence 3.7. First term in the program to cut
             the loser cost and raise the money at once. That cell clears
             leave-one-build-out (+1.42) on the shipped exit but not on trail40.
             CLIENT CORRECTION - evidence 3.1a, new gate in section 4.

  C1  L-DOOR ON A BOOK THAT IS ALREADY PLUS                 [measured on documented-project]
      start   documented-project (holdout +1.46 % and +0.64 %/trade, 4/7 days each)
      result  parent +1.21 SOL, 4/7, 23.5 first/day (UNDER the floor), 8/646 trades at -50 %.
              creator-in +3.36 SOL (permission, L-rate unchanged). bundle/fresh/snipe/dev
              fail the hold half.
      what it kills, exactly:  this BOOK has no left tail to cut. Its door enters at
              age >= 300 s and reserve 50-85, past the window where coins collapse, so the
              L-rate is 1.2 % here against roughly 15 % on door-v3 MONEY. C1 did not test
              whether the -50 % tail is predictable; it tested a population that barely has
              one. **The L-axis stays open** and its coordinate is C0b's fires.
      next    both done. The L-axis is ANSWERED on C0b's fires (3.7): C1's kill was
              about a book with no tail, and where the tail exists it is predictable.
              The trap C1 and C0b share is now named: below reserve 42.43 a -50 % is
              arithmetically impossible, so an L-rate compared across reserves measures
              headroom. Read every L-term inside a reserve band.

  C2  DOOR x BURST START                                    [measured]
      result  full-tape event -1151 SOL, 0/8. documented + P: -1.27 SOL, 45/day.
              keep+ep + P + crowd gone: +9.24 SOL, 4/8, 75/day, top1 79 %, hold -0.35.
              does not ship.
      slow-wall re-run                                                     [measured]
              slow-wall 5 % + E + P, trail40 c600: 1,006 trades, +15.51 SOL,
              +7.71 %/trade, 68.7 first/day MEAN but over fifty on only 2 of 6 days
              (4.8), l50 2.5 %, 4/6 days - against -5.34 %/trade
              behind the door with no permission and -3.33 behind the best public door.
              The strongest cell the program has produced, and every term load-bearing.
              Still does not ship: 82.1 % of the net is ONE creation build alive two days.
              It CLEARS leave-one-build-out (+2.77) - the first cell ever to - and lands
              at 89.8 % on the build bootstrap, not 95 %. Tail fails (top 1 % 78.3 %),
              though the body pays about +1.7 %/trade. Clock 45 is +0.81: the harvester
              is the derived exit, the scalper is the control.
      next    C7. It is short of CLIENTS, not terms. Quiet deep-age (C5) is measured and
              red on the doors on this tape (6.5); it does not wait.

  C3  CAMPAIGN-BREAK V0 AT LAG_115                                          [measured]
      the door is ixh 29d9aacbcbb4d1a8a8ad0975968a7626 (the seed fingerprint), 42,178
      prints on this tape. It is not a build_core hash.
      result  frozen TP40 c600 **-0.60 SOL**, 4/8 days, **18.6 first/day** (under the
              floor), fit +2.37 hold **-2.98**. trail40 on the same fires +4.18 / 6/8,
              hold -0.01, still 18.5/day. clock 45 -4.52. slot+2 on the frozen exit
              is **-0.89**: the recorded +2-slot holdout does not appear on this tape.
              Concentration-species widening is separately red (-15.57 SOL, 0/8).
      next    nothing on this story. Do not widen the machine class.

  C4  EXIT LAB                                                              [measured]
      run on the first entry that clears the client gate: slow-wall + burst START + P
      + A>=2 (6.4, 6.8). D/E/P untouched, only X moved.
      ANSWER: yes. The arrivals-stall cut takes L from 31.5 to 10.1 while W holds at
      101.9 (from 103.4), break-even 23.4 % -> 9.0 %. The static loss cap could not:
      it cuts L 60 % and W 75 %. Three cells land under the L <= 16.3 the basis names.
      but it does NOT raise SOL: cutting early frees the position, 867 trades become
      1,940, and 1,073 extra round trips at 3.2-4 % is ~7.7 SOL of toll against ~4.3
      SOL earned back. A better distribution is not more money once the toll is per
      ticket.
      THE LOSS-ONLY CONDITION IS THE WHOLE THING. The same cuts allowed to fire above
      the fill: -14.40, -15.77, -19.80 SOL, with W collapsing 100 -> 8-14. A cut that
      can fire while the position is ahead is not a cut, it is a clock.
      X IS SETTLED for this sentence: the shipped armed trail (arm +21 / trail 36 /
      unarmed stop -43.75 / cap 1200, the reserve 10/20/-25 squared). On the sentence
      with the wallet term REMOVED it books +13.75 SOL, 986 trades, 5/6 days, top
      client 64 %, LOO +4.93, bootstrap 95.9 % - the first cell to clear that bar.
      It gives up 1.76 SOL to the incumbent trail and buys the client gate.
      Evidence 4.7, and the audit that corrected the numbers is 4.8.

  C5  QUIET DEEP-AGE                                        [measured]
      result  reconstructed E is first buy >= 0.5 after >= 10 token-silent slots.
              keep+P -32.46 SOL, 0/8, 200/day. document -3.63. keep+ep -9.46.
              returning-pro restart keep+P -1.88 SOL, 1/8, 37/day.
              DELAY: they follow an ~80 ms burst; 115 ms is after it.
              does not ship. slow-wall × silence is **-91.95** (6.13).
      next    nothing on this event at mid-tape age. The documented × silence plus is age < 60 s, a different sentence.

  G1  DEEP-AGE BIG CLIP                                     [measured]
      reconstruct D9Uite. E is a public non-racer buy >= 1.0 SOL on a live mid-life tape.
      result  keep+P **-13.19 SOL, 1/8, 125 first/day**. Response to that event equals
              the 0.05 % base. He starts the burst 53.6 % of the time; prev size-buy
              12.5 %. documented and slow-wall neighbourhoods are under the floor and
              tail-broken. Zero-lag is already -1.91.
      next    C6. The story that remains is which coin, not which print. Do not AND
              onto size-buy.

  C6  THE SOLO 26 AS A DOOR AND AS AGREEMENT                                [measured]
      (1) the creation-seq door is REFUTED. The 143 creation sequences the 26 trade
          cover 95.3 % of coins and 97.3 % of prints; concentration 1.02. As a door on
          burst start it books -23.07 SOL over 3,654 trades, bootstrap 13.4 %. It is
          the market, not a filter, and 95.2 % of slow-wall coins are already in it.
          Usable only as a weak EXCLUDE (it removes 4.7 % of coins).
      (2) AGREEMENT is the strongest L-term measured, and it is a gate on the LOSS,
          never on the entry (firing after they land is -10.4 %/trade).
          Inside the band where -50 % is reachable: base 36.05 %, A>=1 13.73 %,
          A>=2 10.79 %, A>=3 6.82 %. Monotone in every band.
          controls  matched-random null 0.77-0.82 (not 1.00 - "anyone bought it" is
                    already worth 20 %). The ten roster wallets that FAILED the profit
                    cut: 0.91-0.98, nothing. The 26: 0.37-0.40.
          transfer  the roster was fitted on 08-27..09-03. On 09-04 onward - outside
                    that window - the lift is 0.37 and 0.18, unchanged. Not circular.
          it STACKS with bundle < 0.20: together 8.77 % L-rate and +3.86 %/trade
          against -14.62 % for the whole reachable population.
      on the best cell  slow-wall + P + A>=2: +13.03 SOL (from +15.51), 6/6 days,
          worst day +0.03, hold +0.36 (from -0.28), l50 1.7 %, 53.9 first/day MEAN
          - the per-day list is 10/145/175/18/11/5, over fifty on 2 of 6 (floor
          ok), bootstrap 91.1 %. Requiring two NODES is better still but falls under
          the floor at 48.1/day.
      what it does NOT fix: the client. Top build still carries 80.6 % of the net.
      A>=2 DOES NOT JOIN ANY SENTENCE. It is the readers' mint list used as a gate
      and a factor on wallet identity, refused by the super-root CLAUDE.md, by
      strategy 7.4 and 9, and by section 11 here. Removing it from the frozen cell
      raised the book +10.98 -> +13.75 and the bootstrap 94.5 -> 95.9 %.
      next    what agreement needs is an ix-STRUCTURE or tape-state twin: the same
              "several independent machines are already in" written without naming a
              wallet. Until that exists it stays a thermometer. Evidence 6.8, 4.8. This is the on-tape remainder of G1.

  C8  FOUR-SLOT INVENTORY WALK                              [measured]
      7 D × 7 E × 8 P × 4 X, doors one-at-a-time, 1,528 occupancy cells (6.13).
      result  SOL leader documented × token-silence × creator × trail40 books **+47.54 SOL**
              and is not a legal door. `cvx_meta` is the keep+ep50 shortlist (18,583 mints);
              missing = not documented. Causal twin (keep × silence × creator, age < 60 s):
              **n 44,548, -1,007.34 SOL**. 818 / 1,622 leader trades are first print of the
              token (`k == 0`, vacuous 10-slot gap) and carry +46.08. Age >= 60 s **-0.82**.
              Slow-wall × silence **-91.95**.
      next    do not implement. Do not AND onto documented × silence. Do not reuse
              `cvx_meta.parquet` as D until metadata covers the tape. Short slot for a
              mid-tape harvester is still E. C9 is the four new E.

  C9  FOUR NEW EVENTS, FOUR-SLOT WALK                       [measured]
      S1 staged-leg, S3 unfinished-budget, S4 price under own exit, S12 after-flush.
      7 D x 4 E x 8 P x 4 X, age>=60 ranking slice (6.14).
      result  SOL leader: n_pro>=8 × after-flush × creator+av+cu × trail40
              **+8.73 SOL, 5/7, hold +2.03, LOO +5.93, boot 98.5 %, top client 32 %**.
              Tickets 21/72/170/143/58/35/39. Peak/trough 8.1x, top2 58 % - tighter than
              n_pro>=8 supply (3.3x on full days). Floor 4/7. Tail 84 %, body +1.41.
              TYPE fails; this is not the walk's reading. Client gate clears. Does not ship.
              S1: slow-wall only, first/day 24. S3: launch (age p50 0). S4: 2 machines.
              Even-floor SOL>0 cells exist (D=none × flush × creator+av+cu, +3.61) and
              fail the tail (body red).
      next    do not AND onto the after-flush SOL leader. It is not a type. S4 without
              is_pro is C10. More days (C7) do not turn an 8x two-day slice into a type.

  C10 S4 WITHOUT THE PROFESSIONAL-BUILD CUT                 [measured]
      Sell-then-buy (n_sell>=10, rebuy frac>=0.25), no n>=200 / nw<=50.
      7 D x 1 E x 8 P x 4 X, age>=60 ranking slice (6.15).
      result  SOL leader: slow-wall × rebuy × creator+av × trail40
              **+4.47 SOL, 3/6, hold -1.13, LOO +0.57, boot 81.8 %, top client 87 %**.
              Tickets 16/179/208/33/32/7. Peak/trough 26x, top2 81 % against slow-wall
              births at 5.7x. TYPE fails: the conjunction is a two-day client inside the
              door. Floor 2/6. Tail 181 %, body -3.63. 30 machines. Derived X **-7.17**.
              Dropping the door **-82.70**. No SOL>0 cell on this walk clears the floor
              on every day it prints. Does not ship.
      next    do not AND onto this parent. Do not restore nw<=50 (that is the emptying cut).
              Tape-only mid-tape E in the inventory (S1, S3, S4, S12) is scored.
              C11 is S1's unused moment (late-leg). This SOL leader is not a type.

  C11 LATE-LEG OF A STAGED-LEG MACHINE                      [measured]
      First >= 0.5 buy of cluster 2+ (C9 S1 is cluster 1). Same 44 machines.
      7 D x 1 E x 8 P x 4 X, age>=60 ranking slice (6.16).
      result  SOL leader: slow-wall × late × creator+av × derived
              **+6.93 SOL, 5/6, hold +0.34, LOO +2.15, boot 99.5 %, top client 69 %**.
              Tickets 2/78/92/11/3/2. Peak/trough 46x, top2 90 %. TYPE fails. Derived
              raises vs trail40. Dropping the door: body -0.06. Floor 2/6. Tail 23 %,
              body +5.33. No SOL>0 even-floor cell. Does not ship.
      next    do not AND onto this parent. Do not move S1's event again.
              Short slot remains E. C12 is this-coin second burst (not the staged class).
              The SOL leader is C2's door on two days, not a type. C7 does not fix it.

  C12 SECOND-ATTEMPT BURST, THIS COIN ONLY                  [measured]
      First >= 0.5 non-racer buy of any ix structure's second burst on this coin.
      One fire per (coin, structure). Not C8 restart, not C11. documented dropped
      (law 30). 6 D x 1 E x 8 P x 4 X, age>=60, TYPE then SOL (6.17).
      result  TYPE reading: n_pro>=8 × second × creator × trail40
              **+3.12 SOL, 4/8, hold -7.61, LOO -7.18, boot 57.4 %, top client 330 %**.
              Tickets 53/171/344/377/203/142/102/2. Peak/trough 3.7x, top2 54 %
              against the door 3.3x / 45 %. Floor passes on full days. Tail 935 %,
              body -26.04. k==0 is 0. Funnel printed. Does not ship.
              SOL leader slow-wall × creator+av × trail40 +17.78, d50 2/6, is C2's
              door, not the reading.
      next    do not AND onto the slow-wall SOL leader. Do not restore the staged
              class. Short slot remains E. C13 is live rotation (arrives from
              another coin).

  C13 ARRIVES FROM ANOTHER COIN                             [measured]
      First >= 0.5 non-racer buy here by an ix structure whose latest print is
      a buy on a different coin, slot gap < 10. One fire per arrival.
      Not C12, not C11, not the S9 door. documented dropped (law 30).
      6 D x 1 E x 8 P x 5 X, age>=60, TYPE then SOL (6.18).
      result  READING: none. No SOL>0 TYPE-pass.
              SOL leader slow-wall × creator+av × trail40 **+16.46**, d50 2/6,
              top1 73.6 %, body +4.35, peak/trough 26.4x, top2 81 %. C2's door.
              Closest TYPE-shaped cell n_pro>=8 × creator × trail40 **-4.64**.
              Live-gap p50 **1.0 slot**. k==0 1.9 %. leave X red. Does not ship.
      next    do not AND onto the slow-wall SOL leader. DELAY = 0 kills this
              event at 115 ms. Do not walk clip-step-up as the next E (the tell
              is the extra SOL). Short slot remains E. Unused delay family:
              first buy after a run of sells. group-live-now stays a door.
              attention-now is a data job.

  C14 FIRST BUY AFTER A RUN OF SELLS                        [measured]
      First >= 0.5 non-racer buy that breaks >= 3 consecutive sells on
      this coin. One fire per sell-run. Not after-flush, not K=1, not C13.
      documented dropped (law 30). 6 D x 1 E x 8 P x 5 X, age>=60, TYPE
      then SOL (6.19).
      result  READING: none. No SOL>0 TYPE-pass.
              SOL leader slow-wall × creator+av × shipped **+10.40**,
              d50 2/6, top1 90.2 %, body +1.02, peak/trough 37.8x, top2 80 %.
              C2's door. Closest TYPE-shaped cell slow-wall × cu_hi ×
              derived **-5.24**, floor on every full day. Wide TYPE cells
              **-194 to -289**. Sell-gap p50 1.0 slot. Next-print p50
              **81 ms**, gap<50 41.9 %. k==0 0. low X red as TYPE-pass.
              Does not ship.
      next    do not AND onto the slow-wall SOL leader. Leftover of the
              bounce is 81 ms; do not walk another after-sellers spelling
              as the next E. Do not walk clip-step-up. Short slot remains E.
              Unused delay family: first operator structure after only
              creator and seed racers. group-live-now stays a door.
              attention-now is a data job.

  C15 FIRST OPERATOR AFTER ONLY CREATOR AND SEED RACERS     [measured]
      First >= 0.5 non-creator, non-seed operator-structure buy on a coin
      whose prior prints are only the creator and seed racers. One fire
      per coin. documented dropped (law 30). group-live-now is a door
      (sibling print in last 10 slots at birth). 7 D x 1 E x 8 P x 5 X,
      age>=60, TYPE then SOL (6.20).
      result  READING: none. No SOL>0 TYPE-pass (age>=60 has 8 fires).
              SOL leader none × creator × clock45 **+0.01**, 7 trades,
              d50 0/3. Full-tape none × none × clock45 **-50.75**,
              1,360 trades, 0/8. Age p50 **0 s**; 0.6 % age>=60.
              Prior-gap p50 **0 slots** (89.1 % same slot). Next-print
              p50 **2 ms**, gap<50 74 %. k==0 0. follow X red.
              group-live-now supply TYPE-passes (2.5x / 41 %) and does
              not save the event. Does not ship.
      next    do not AND. DELAY = 0 kills this spelling at 115 ms: it is
              launch co-arrival. Do not walk first-outside-buyer (same
              launch shape). Do not walk clip-step-up. Short slot remains
              E. Unused delay family: first run of an operator structure
              on this coin (E1.2; the coin already has others).
              group-live-now stays a door. attention-now is a data job.

  C16 FIRST RUN OF AN OPERATOR STRUCTURE                    [measured]
      First >= 0.5 buy of a non-creator, non-seed operator structure that
      has never printed here, after at least one prior non-creator
      non-seed print. One fire per (coin, structure). Not C15, not C9 S1.
      documented dropped (law 30). group-live-now is a door. 7 D x 1 E x
      8 P x 5 X, age>=60, TYPE then SOL (6.21).
      result  TYPE reading: slow-wall × cu_hi × derived **+2.27**, 3/6,
              hold -2.93, LOO -1.32, boot 65.4 %, top client 158 %.
              Tickets 40/101/120/75/92/17. Peak/trough 7.06x against
              door 4.7x, top2 50 %. Floor 4/6. Tail 219 %, body -2.71.
              k==0 is 0. Funnel printed. Does not ship.
              SOL leader n_pro>=8 × creator+av+cu × derived +9.14, d50
              2/7, TYPE fails 6.9x / 62 %, not the reading.
      next    do not AND onto the slow-wall TYPE reading or the n_pro
              SOL leader. Do not walk operator-cross-K (a count cut on
              this event). Do not walk clip-step-up. Short slot remains E.
              Unused delay family: silent coin, then K buys of one
              operator structure (E1.2). group-live-now stays a door.
              attention-now is a data job.
```

```
  C7  THIRTY DAYS OF PRINTS                                 [the binding constraint now]
      C7 adds CLIENTS of a type that already prints every day. It does not turn a
      two-day cohort into a type (C9/C10/C11 SOL leaders fail TYPE on this week).
      A week is 16-23 builds with one of them carrying the book; thirty days is
      about 150. Nothing behind a launch door can pass the client gate until this
      tape exists. Rank those extra days under TYPE, not by SOL alone.

      IT CANNOT BE EXPORTED. The history does not exist anywhere (checked 09-09):
        aa.pxf     08-30 17:48 .. 09-06 12:00   the study week, and it IS the tape
        PG trades  09-01 .. now, 9 chunks       retention policy says 30 days, but
                                                everything before 09-01 was purged
        the lake   dt=2026-09-01 .. 09-08       8 sealed days, ~3.9 GB
      So C7 is a CALENDAR item, not a job: the tape grows one day per day and reaches
      thirty days around 2026-09-30. Re-run the two frozen cells then, unchanged.

      WHAT MUST NOT SLIP. The lake is the only durable store and its export is run by
      hand (09-07 and 09-08 were both sealed late, on 09-09). PG drops a day at 30 days.
      Every day not exported before that is gone forever, and a lost day is a lost
      client. Check `hunter/lake-data/trades/dt=*` has yesterday before doing anything
      else in a session.

      it is an export, not a story. Do not run another AND while waiting for it.
      the two cells it decides, frozen exactly as written today:
        slow-wall 5 % + E + P, SHIPPED exit                  (6.4, 4.7, 4.8)
          NO wallet term - A>=2 is refused (section 11), and dropping it made the
          cell BETTER: +13.75 SOL, 986 trades, 5/6 days, top client 64 %,
          LOO +4.93, bootstrap 95.9 % - the first cell to clear that bar.
          STILL FAILS the tail (top1 91.5 %) and the PER-DAY floor: tickets
          16/177/210/24/28/9, over fifty on 2 of 6 days.
        door-v3 MONEY + bundle < 0.20, shipped exit               (3.7, 7.0)
          also under the per-day floor: 13/346/296/35/26/8, 2 of 6.
```

```
  sequencing
  ----------
   C0a C0b C1 C2 C3 C4 C5 C6 C8 C9 C10 C11 C12 C13 C14 C15 C16 G1 all measured.
   C8: the SOL leader is illegal (cvx_meta = keep+ep50; silence `k == 0` is vacuous).
       Causal keep launch is -1,007. Mid-tape age is red. Short slot for that target is E.
   C9: S1 / S3 / S4 / S12 against standing D, P, X. After-flush × n_pro>=8 SOL leader
       clears the client gate at age>=60 and fails TYPE (8.1x, top2 58 %), floor and tail.
       S4 under is_pro is 2 machines.
   C10: S4 as sell-then-buy, 30 machines. SOL leader is slow-wall, TYPE fails (26x, top2
       81 %), top client 87 %, body -3.63, hold -1.13. Tape-only mid-tape E in the inventory
       is scored. Short slot remains E.
   C11: S1's unused moment (late-leg). Body pays (+5.33, tail 23 %), TYPE fails (46x,
       top2 90 %). Plus is C2's door on two days. TYPE reading +5.14 body +0.75 (4.10).
       Do not move S1's event again.
   C12: this-coin second burst. TYPE reading n_pro>=8 × creator × trail40 +3.12, floor
       passes, body -26, tail 935 %, hold -7.61, top client 330 %. SOL leader is
       slow-wall, not the reading. k==0 is 0. Short slot remains E.
   C13: arrives from another coin. READING none. Live-gap p50 1.0 slot (DELAY = 0).
       SOL leader slow-wall × creator+av × trail40 +16.46, d50 2/6, TYPE fails
       (26.4x, top2 81 %). Wide TYPE cells -112 to -437. leave X red. k==0 1.9 %.
       Short slot remains E. Do not walk clip-step-up next.
   C14: first buy after a run of sells. READING none. Next-print p50 81 ms.
       SOL leader slow-wall × creator+av × shipped +10.40, d50 2/6, TYPE fails
       (37.8x, top2 80 %). Closest TYPE cells -5.24 to -9. Wide TYPE -194 to
       -289. k==0 0. low X red. Short slot remains E. Do not walk another
       after-sellers spelling next.
   C15: first operator after only creator and seed racers. READING none.
       DELAY = 0 (prior-gap p50 0 slots, next-print p50 2 ms). Age p50 0 s.
       SOL leader +0.01 on 7 trades. Full-tape -50.75. k==0 0. follow X red.
       Short slot remains E. Do not walk first-outside-buyer.
   C16: first run of an operator structure. TYPE reading slow-wall × cu_hi
       × derived +2.27, floor 4/6, body -2.71, hold -2.93, client 158 %.
       SOL leader n_pro>=8 +9.14 TYPE-fails. Next-print p50 56 ms; remaining
       spend 8.8 s on 22.5 %. k==0 0. follow red. Short slot remains E.
       Do not walk operator-cross-K.
   C4 settled X on the one cell that clears the client gate: the shipped armed trail.
   C7 is a CALENDAR item, not a job - the history to export does not exist, so the
   tape reaches thirty days around 2026-09-30. Until then the sentence stays frozen.
   C7 adds clients of a type that already prints every day; it does not fix TYPE.
   AUDITED (4.8, 4.9): seat, universe, door label, timestamp grain, concurrency and
   pricing all pass. Three method failures: a per-day floor reported as a mean, a
   wallet-identity term in the sentence, and ranking by SOL on a two-day cohort.
   All three corrected above.
   C3 measured: the frozen fingerprint is on this tape and red at lag_115 (floor, money, hold).
   C6 measured: the seq door is refuted, agreement is an L-term and joins the cells.
   R1 RECONCILIATION (evidence 1.4) OVERTURNS THE NODE VERDICTS. Priced on the roster's own
   131,339 episodes with our clip: losing the SEQUENCING race -5.59 pp, the fee -2.47, our
   impact -0.76, the 115 ms only -0.77. An event anchored on a PRINT is a FOLLOW model by
   construction, and every node study below carries that anchor, so their reds are readings
   at the worst of four seats. PEER-seat oracle ceilings: deep-age big clip +2.62 % 6/8,
   mid-tape one-shot +0.71 % 7/8 on 14,790 episodes, the other three about zero.
   H2/H3 hot-tape worked at the user's direction (evidence 1.5, 1.6). GATE PASSED: the six
   share a public trigger (agreement lift 6-9x within 2 s) and are SLOW (0.4-2.2 s), so a
   115 ms rule lands ahead of them 68-85 % of the time. The event was then measured
   WITHIN THE MINT - their buys against non-buy prints on the same coin, which holds the
   door and every off-chain factor constant. Mint-LOCAL features beat absolute ones almost
   everywhere. Story: LULL then WAKE.
   THE BIND: the wake half discriminates (within-coin lift 4.5) but has 0.216 s of dwell and
   costs +5.98 % to react to at 115 ms; the lull half has 24-52 s of dwell and a -1.0 % fill
   but predicts nothing. Both book red. And the conjunction covers only 1.2 % of their buys,
   so their real trigger is NOT this and is still unfound.
   H4 THE MODEL IS BUILT AND THE ANSWER IS THE DOOR (evidence 1.7). A within-mint model on
   the whole feature vector reads AUC 0.7212 on HELD-OUT COINS using only coin-relative
   features - their moment is learnable out of sample, a first for this program. It books
   -4.24 to -10.62 %/trade over five thresholds x seven exits, 0/8 days, with a reaction
   cost of only -0.5 to +1.2 %, so the fill is CHEAP and the moment is still worthless.
   Splitting its fires: on a coin the node touches -2.63 %, on a coin it never touches
   -14.21 %. ELEVEN AND A HALF POINTS SIT IN WHICH COIN, NOT WHICH SECOND.
   H5 THE NODE CLOSES, WITH ITS MECHANISM NAMED (evidence 1.8). The door census names EARLY
   LIVELINESS, not creation structure: reserve at age 60 s of 50-70 is 9 % of coins and 34 %
   of their trades (conc 3.83), under 32 is 49 % of coins and 7 % of trades (conc 0.14).
   No public door pays - best -6.29 %, 0/8 days. Because the coin was never the thing:
   arrivals (one of the six buying INSIDE our hold, 30.1 % of fires) pay +2.37 %/trade 7/8
   days; touch-but-no-arrival -8.02 %; never-appear -12.71 %. Break-even needs 81.6 %.
   Seven abort-on-no-response exits x five thresholds cannot close it.
   THE NODE IS INCOMING DEMAND AND THE SIX ARE THE DEMAND. Being early, cheap and right about
   the moment is not an edge if nobody joins the move.
   WHAT CARRIES FORWARD AS METHOD, not opinion: the within-mint control (kills off-chain
   confounding by construction); the coin-LOCAL feature basis (one ruler cannot serve 11,765
   coins); and the REACTION COST beside every event - over ~2 % is unreachable at any lift.
   H1 hot-tape re-entry MEASURED and red (6.10) AT THE FOLLOW SEAT, and NOT on cost: the cell captures a
   +0.29 % price move where the node's 1.10 % NET margin implies +3.65 %, so at a zero fee
   it still books -0.63 %. Absorption and deceleration refuted and inverted; buy-flow
   acceleration survives a node-blind control; red under eighteen exits and about sixty
   pre-registered cuts. All five solo nodes are now read.
   THE ERROR THAT RUN NEARLY SHIPPED: comparing a roster margin (NET of 125 bps) to a study
   cell's GROSS move - a 2.5 pp flatter, and it turns "we did not find their edge" into
   "the toll ate it", two findings with opposite next steps. Strategy 7.4 law 22.
   Two things are NOT closed there: a size small against the flush, and an E that fires
   seconds AFTER the flush ends. Neither runs without a reason.
   H6 THREE RETRACTIONS, THEN THE EXIT SEARCHED AND EMPTY (evidence 1.9, 1.10). Retracted at
   the user's direction: a red D+E with P empty and X a guessed grid closes nothing; the
   "they are slow, we beat them 68-85 %" of H2/H3 measured a WAITING time, not a reaction
   time, and is withdrawn; and they do not ignite - their share of the move from our fill to
   the peak is p50 0.020, mean 0.052, so their arrival is a MARKER that a real move is
   running.
   X IS NOW SEARCHED, NOT GUESSED, on the convexity they themselves enter (95,135
   position-tracked episodes). The convexity is real - MFE p50 33 %, p90 181 %, perfect exit
   +53 to +58 %/trade 8/8 - and no exit reaches it: 480 causal shapes, three-rung ladders and
   five flow tells give -2.35 %/trade 0/8 behind their print and +0.21 % 5/8 (-0.89 % without
   its top 1 %) sequenced ahead of it. Flat clocks beat ladders beat trails beat flow tells,
   and shorter beats longer everywhere.
   WHY: a quarter of the entries that eventually double are 31 % under water first, so no
   protective trail survives the winners; and the median future MINIMUM is -40 to -60 % in
   every state bucket, so holding always gives it back. The move DOES have momentum (up 25 %
   at 20 s implies median future max +107 %) and the momentum does not cover the toll.
   Their own exit is not the edge either: capture ratio 0.07, own book +0.70 % on spend.
   THE LOOKAHEAD THAT RUN NEARLY SHIPPED: +8.30 %/trade, 8/8 days, LOO +599 SOL, every
   robustness column green - from a dead cut evaluated as "no rung EVER fired" instead of
   "no rung fired before this deadline". Index order fixes it to -2.38 %. Strategy 7.4 laws
   23, 24, 25.
   WHAT IS OPEN: P (public terms only - reserve under 35, age under 30 s are the two slices
   positive at the race seat) and E re-derived from an IDENTIFIED trigger print, because the
   race seat exists only for someone who decides when they decide.
   H7 THE 1.10 VERDICT IS WITHDRAWN AND THE NODE RE-OPENS (evidence 1.11). The 480-shape grid
   ran with D and P EMPTY, which workflow 5 refuses outright, and its own result - shorter beats
   longer everywhere - is that refusal's signature. Two scope errors ride with it: the geometry
   runs to 1800 s on a node that holds 19.9 s (the peak inside their own hold is +7.6 %, not
   +33 %), and the six wallets are pooled. Strategy 7.4 laws 26-28.
   THE SPLIT IS THE FINDING. Three of the six - 8fStGV, AbQcLH, 49uohd - book +2.2 to +2.3 %/trade
   at the RACE seat under a 15 s clock: pooled 12,933 tickets, 1,913 a day, 4,327 coins, +58.77
   SOL, 8/8 days, worst day +0.01, body +29.50 SOL with the top 1 % removed, biggest coin 1.8 %.
   That is the first cell in the program to clear days, body and coin concentration at once. It
   fails the tail (49.8 % against 9-12 %) and it is a CEILING, not a rule - the oracle is a wallet
   list. The other three are noise (top 1 % 494 %, body -138.84).
   IT LIVES AT THE SEAT. The same decisions at our own fill read -0.66 %, 1/8. A rule anchored on
   a PRINT cannot reach it; only a state true for seconds can.
   THE TWO ANIMALS. The ones that pay buy a pullback INSIDE a live up-move: coin up +22.3 % over
   the last minute against +9.8 %, a new high 24 s ago against 63 s, only -24.7 % below the peak
   against -32.0 %, 73 SOL through the pool in the last minute against 45, and the last five
   seconds turned net SELL. The others buy a deep dip on an old coin. 1.6's `dd_peak` reading is
   the second group outnumbering the first eight to one.
   WHAT IS RED, and it is the price path: swing pullback -3.35 %, the up-move portrait -3.58 %,
   both 0/8 on the full tape, and tightening the state is monotonically WORSE. Buy the turn, never
   the knife, at every depth and every exit.
   WHAT IS OPEN: the independent-machine count (the only non-headroom money gradient found here:
   -4.08 % under five builds against -2.99 % at twelve-plus, 1.1 points, no zero crossing) as a
   FITTED vector rather than a threshold; the own-exit level as a full sentence; and still their
   trigger print.
   THE LOOKAHEAD THIS RUN CAUGHT: a door fact dated at age 60 s, fired on at age 20 s, reads
   +54.97 %/trade at an 80.8 % win rate. Causal, the same cell is -4.12 %, 1/8.
   H8 THE TRIGGER IS FOUND AND THE EVENT SLOT IS FILLED (evidence 1.12). Measured as excess
   intensity against the coin's own print rate - a reaction, not a waiting time - per wallet:
   8fStGV buys 25-200 ms after a public SELL >= 1 SOL (lift 8.1) and avoids burst starts;
   sssssw buys 75-100 ms after a BUY / +3 % print (lift 9.2) and avoids sells. Speed does not
   separate the members that pay; the SIDE does (strategy 1.5's direction law).
   8fStGV picks 2.2 % of the sells on its coins, and within the coin the ones it picks land in a
   frenzy: 15 distinct builds in 5 s against 7, 3.9 SOL bought in 2 s against 0.4, a new high
   5 s ago against 80 s, a seller who bought 21 s ago against 51 s. At our 115 ms we land behind
   it two times in three and still book +0.21 % on the sells it picks.
   THE PUBLIC EVENT: sell >= 1 + 15 builds in 5 s + 2 SOL in 2 s + new high <= 20 s + seller
   bought <= 30 s ago, cap15. Full tape -0.68 % 1/7 (each term monotone up from -4.26 %); on
   8fStGV's coins +1.30 % 5/7, body +1.20; everywhere else -3.96 %. D IS THE EMPTY SLOT.
   Public coin facts (SOL in so far, prior frenzies, peak reserve, prints, wallets) separate its
   coins at AUC 0.66-0.71 and move the book to -0.27 % at best, 38 a day - they buy the coin
   list's share, not its money. And on its coins the event pays +6.04 % when 8fStGV buys inside
   our hold, -0.62 % when it does not, +0.68 % (311 tickets) when none of the six does.
   NEXT: freeze E and search D the way 1.8 searched it - but on this event's fires, and with the
   coin's own history of frenzies ABSORBED (sell inside a frenzy followed by a new high) rather
   than frenzies merely SEEN, which is the unpriced version of "this coin's frenzies finish".
   H9 THE DOOR AND THE EXIT, DERIVED (evidence 1.13).
   CORRECTION: the coin list is the member's FUTURE arrival. On its coins E books +4.46 % 7/7
   before its first buy there and -0.68 % after it, which is the full-tape number.
   D: thirty public facts, node wallets dropped, walk-forward on days. Absorbed history has the
   wrong sign - the event pays where this coin's earlier frenzy-sells FAILED to make a new high
   (at most a third did: folds +0.45 % and +2.40 %, reserve rank correlation -0.00). The
   seller's profit passes too and is the price in disguise.
   X: the member's closing sells trip 25-50 ms after a public BUY >= 1 / +3 % print (lift 5.8),
   at +14.4 % since entry; the hazard is a bracket - take profit +10 %, stop at -25..-40 %,
   time from 40 s, nothing past 80 s. It buys the sell and sells the buy.
   THE SENTENCE: E x earlier-frenzy-sells-failed x tp10 sl25 t60 = +1.22 %/trade, 124 a day,
   5/7, body +1.23, biggest coin 11.1 %, top 1 % 39.7 %; days 0-3 +1.66 %, days 4-6 +0.25 %.
   Does not ship.
   NEXT: a door term for the FIRST arrival of a buyer like the member (the +4.46 % half), and
   the same method on AbQcLH / 49uohd, who share one 15-wallet trade-ix set - a recipe door for
   their own event.
   H10 THE PERMISSION (evidence 1.14). The stop-outs are young, thin coins: age, public holders,
   reserve and the held share separate them; heat, composition and the seller do not. Folds pick
   holders >= 368 (368 / 369) and age >= 123 / 193 s. Frozen at holders >= 368, age >= 158 s:
   E x P x bracket = +2.07 %/trade, 141 a day, 7/7, worst day +0.22, body +3.26, biggest coin
   4.6 %, top 1 % 17.6 %, days 0-3 +2.23 % / days 4-6 +1.77 %, stop-outs 11.7 %. A plateau:
   every cell at holders >= 200 and age >= 158 s is 7/7 at +1.5 to +2.7 % but one. The door is
   redundant once P is in.
   NEXT: the holdout - export the tape after 09-06 12:00 and book the frozen sentence on it,
   unchanged. Then exits scaled by headroom or frenzy size on this set, then the arrival door.
   H11 THE HOLDOUT (evidence 1.15). Lake days converted to the study tape's exact format (every
   shared row identical; build_core = md5 of the ix labels joined by "|" without ATA creates,
   account closes and memos). Fires from 09-06 12:00, 4.50 days, same booking code:
   E x holders >= 368 x age >= 158 s x bracket = +1.90 %/trade, 132 a day, 5/5 days, worst day
   +0.17, body +1.73, biggest coin 7.0 %, top 1 % 23.2 %, stop-outs 10.3 %. In-sample +2.07 %.
   The door of 1.13 fails out of sample (-0.53 %, 2/5). THE FIRST SENTENCE TO HOLD OUT.
   NEXT: the tail (23.2 % against 15 %) and the book size (~0.5 SOL a day): a second event from
   AbQcLH / 49uohd on the same method, and exits scaled by headroom, then paper.
   H12 THE SECOND EVENT (evidence 1.16). A - AbQcLH buys a public burst start (a ~1 SOL buy
   after a dip) at a 47 ms median; we land ahead 5 %; -1.36 %/trade. DELAY about zero at our seat.
   B - 49uohd buys a public sell >= 1 at a 260 ms median; we land ahead 96.8 %; +3.50 %/trade
   8/8 on its picks. Within the coin: a capitulation cascade (fall, busy, in-burst, seller at a
   loss). Spelled publicly: -4.26 % to -5.04 %, not monotone; with established coin -1.72 %.
   Operator recipe set present: rank 0.50. A public big dip-buy just before: rank 0.49.
   NEXT: B's descriptors fitted as one within-coin score on half the days; if red, move to the
   headroom exits and paper for sentence one.
```

Off-chain attention stored at decision time is not a campaign. It is a **data job** that
re-opens one G2 story. Do not delay C1/C2 for it. Do not pretend a delayed fetch is that
job.

---

## 11. Standing refusals

- **No result without its coordinate**, and without naming which slots are empty.
- **No proxy label.** Money is the score. Detection, graduation and swing labels describe a
  sentence; they never rank one.
- **No fixed bar.** The bar is the cell's own realised break-even.
- **No slot closed from a red number.** Only a measured mechanism or a conjunction search.
- **No verdict under one exit family.** Both, always, side by side.
- **No exit swept on an unselected pool**, and no selector judged by an exit chosen that way.
- **No static abort grid.** 104 cells are enough. State-conditional cuts only, and only on a
  sentence that already clears the gates.
- **No node closed on a subset of its members.** "His fill is unreachable" is never "his
  decision is unreachable".
- **No trimming a frozen sentence** because a prefix holds out better. Write a new sentence
  and burn the week.
- **No candidate table read as a universe.** The raw tape is the universe. Print the funnel.
- **No sidecar left-join with missing = False.** The file's construction filter is a rule term.
  Print sidecar rows vs tape tokens vs overlap before scoring the door. `cvx_meta.parquet` is
  the keep+ep50 shortlist; it is not documented-project on the tape (strategy 7.4 law 30, evidence
  6.13).
- **No vacuous base case on an event.** `i == 0` does not satisfy ">= 10 empty slots". Report the
  share of fires at local index 0 (strategy 7.4 law 31).
- **The reader's own prints stay out.** His response rate names an event and gates nothing.
- **No wallet identity in a sentence, and that includes agreement.** "Two of these 26 named
  wallets already bought this coin" is the readers' mint list used as a gate. It is refused by
  the super-root `CLAUDE.md`, by [_!___strategy.md](_!___strategy.md) 7.4 and 9, and here. The
  agreement MEASUREMENT is a finding and a thermometer (evidence 6.8); it is never a term. If a
  term cannot be spelled in ix structure or in tape state, it is not a term.
- **No gate reported as a mean.** A per-day refusal is checked per day and printed per day.
- **No SOL-leader captioned as the walk's reading when its tickets fail TYPE.** A two-day
  spike is a named client book. Rank among cells whose tickets track the tape; if none, the
  inventory is short, and that is the result. A stub UTC day is not a floor day.
- **No extra AND as a new sentence.** Neighbourhood is section 3 step [4]. A new parent is
  [_!___derive.md](_!___derive.md). A new filling on a parent is one idea in the named slot.
- **No parent from an unused D×E×P×X walk.** The parent comes from one paying member
  ([_!___derive.md](_!___derive.md)). Inventory search varies one slot on that parent.
  Do not caption a one-slot swap as a new story, and do not replace E to invent one.
- **No parking the harvester** because survival is the only holdout-plus book. Survival is a
  door. The target in section 0 is still leftover of real up-moves.
- **No mixing three doors into one sentence.** Slow-wall, documented-project, and
  demonstrated-episode are different stories; they are C2 variants, one at a time.
- **No Helius spend** without asking.
