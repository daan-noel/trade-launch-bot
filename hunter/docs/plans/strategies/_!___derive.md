# Derive: the method, from one paying wallet to a shippable rule

How a target node, trader or wallet becomes a sentence; the gates every cell passes; how a
result is recorded. [_!___strategy.md](_!___strategy.md) is the basis and the laws;
[_!___inventory.md](_!___inventory.md) is the idea list used after the event is named;
[_!___evidence.md](_!___evidence.md) holds the numbers; [_!___workflow.md](_!___workflow.md)
is the open queue; [_!___terms.md](_!___terms.md) is every word; [_!___metrics.md](_!___metrics.md)
is what the engine measures. Code: [node-derivation/toolkit](node-derivation/toolkit/README.md). Worked
example: [node-derivation/hot-tape-rule-1.md](node-derivation/hot-tape-rule-1.md). A node's
working file starts from [node-derivation/node-template.md](node-derivation/node-template.md).
Two more this method leans on by name: the roster of members is
[solo-traders.md](solo-traders.md), and every number passes
[backtest-audit.md](backtest-audit.md) before it is reported as a result. Those five are the
whole of what derive needs outside the seven.

A wallet is an instrument. It names a decision. It is never a term, its coins are never a
door, and its own prints stay out of every public count. Volume manufacture is not an
instrument (section 1, 4.0).

Read top to bottom. One picture, one job.

```
WHAT
  leftover of a real up-move
  fire on a PUBLIC print, fill 115 ms later
  latency is not the edge

FLOW

  PICK one wallet
    harvest hold, total > 0, rare big losses, RACE pays
    a reader, not volume manufacture
        |
        | fail --> next wallet
        v
  FIND E
    5.1  he reacts to this public print
    5.2  leftover still there 115 ms later
        |
        | fail --> other class / earlier tell / next wallet
        | pass --> spell E until the class is his prints
        |          (tiny acted share: more E, not D)
        |          then D / X / P on that E only
        v
  SHIP
    gates --> holdout --> engine --> paper
```

```
DELAY  (why a 115 ms fill can still pay)

  unseen intent
       |
       v
  public print on the tape          FIRE here
       |
       |  115 ms, SOL has not all landed yet
       v
  our fill                          leftover must still be here
       |
       v
  remaining SOL lands               now it is in the PRICE
```

```
SLOTS  empty is a value. a red number closes this sentence, never a slot.

  D  door         which coins we watch          optional (none is valid)
  E  event        the public print we fire on   unpriced tell + DELAY
  P  permissions  already true when E prints    cuts losers
  X  exit         how leftover is eaten         from his close, this pool
  R  re-entry     tickets per coin, one open
  S  size         clip (standing 0.2 SOL)
```

```
WHERE  (section)

  0        target     leftover of a real up-move at 115 ms
  1-4      pick       one member, every-leg tape, shape, RACE pays, a reader
  5-6      find E     he reacts + leftover exists, then spell it
  7-10     rest       D optional, X, P, the ladder on that parent
  11-13    ship       gates, holdout, record
  14       refusals
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

The operating seat is **p50 115 ms**. Fastest observed own-fill is about 45 ms; sender ACK
is 8-10 ms (not a fill). A lag ladder is a diagnostic. It does not change the veto.

The seat is a measurement, not a constant. Before any "too slow" verdict, re-read it from real
fills: `strategy_positions` with `mode = 'real'`, `entry_time - target_time` on the ingest clock
(the tape's clock). The copy rules' real fills since 09-01 read p50 **83 ms** (evidence 1.1).
Every gate reports the measured p50 beside 115 ms.

D is not required. Empty D is a filling. A door earns its place only when it raises money
and keeps the bars. With a door the book is more concentrated; without one the event and
exit have to carry it.

---

## 1. Pick the instrument

A node label is a cluster of wallets, not a strategy. Six wallets under one name can be
two logics. The unit of derivation is **one member**. Measure every roster member on
every-leg study. Shape (one-shot / re-entry / mixed) comes from **his** episodes
(phase 3). E is a public print, not "first on this mint". Two E's only when 5.1 names
two prints.

```
  node --> each member on every-leg tape --> keep who pays at RACE --> FIND E
```

Pick does **not** require leftover at 115 ms. That is phase 5.2, after his reaction is named.
A member that fails 5.2 is closed at our seat; it is not "never an instrument".

| keep | not a drop |
| --- | --- |
| harvest-scale hold (seconds to tens of seconds) | 1-2 s scalp as the whole book |
| his **total** > 0, enough tickets | negative median when total is green and >20% losses are rare (a **cut**) |
| RACE pays most days, body > 0 | FOLLOW red on a copy of his fill |
| unread public tell | largest book with no named print yet |
| rare closes at <= -20% of clip | top 1% share of **his** net (his harvest shape, not the 5.2 veto) |

Drop before FIND E: a reader, not volume manufacture. Two measurements, both required; 4.0
spells them.

The node's largest book is not automatically the instrument. Prefer the member that still
has an unread decision point at our seat. A member whose public tell is already a 5.2 kill
is not the next instrument of that same family.

Roster and node lists: [solo-traders.md](solo-traders.md). `toolkit.tapes.roster(node)`
returns the wallets. `S.wallet(prefix)` is the member.

---

## 2. Frame

| step | what | rule |
| --- | --- | --- |
| 2.1 Two tapes | Study: `study_exact` (every leg). Every threshold is read here. Last-leg `study` drops wallets whose prints are not last-leg. Holdout: later lake days, same grain (`lake_export.export`, an entry in `tapes.TAPES`), started a few hours early as a warm-up so each coin already has history. `study_exact`'s prints run to 09-06 24:00; `holdout_exact` contains the same 09-06 12:00..24:00 prints, but its fires start at 09-06 12:00. A cut fitted on a fire after that instant is chosen on the holdout. Count study fires only before 09-06 12:00. Loading `study_exact` does not cut them; the script must (`tapes.TAPES` `t_min` is None on study, set on holdout). The toolkit's `T.day` counts from `cvx.DAY0` (08-30), so day 2 is 09-01 | Nothing is chosen on the holdout |
| 2.2 Instrument | The member's wallet id is marked NODE so every public fact excludes it | It never enters a term. Two logics get two working files |
| 2.3 Fire and fill | Fire = the **public** print, or the rising-edge print of a held state. Never his buy. Never buy-minus-L. Fill = last print landed by fire + 115 ms, both legs. Exact curve arithmetic, 125 bps + 0.000225 SOL a leg (`kernel.py`) | One kernel prices everything. A lag ladder does not change the veto |
| 2.4 Coordinate | Every measurement writes the six slots of a rule (strategy section 6) plus the frame | Empty is a value. A red number closes this sentence, never a slot |

```
  D E P X R S  -- see SLOTS at the top. empty is a value.
  frame         seat = last print landed by fire + 115 ms, both legs
                cost = 125 bps + 0.000225 SOL a leg + own impact on vsol
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

E is a public print **at a moment** (this structure steps up, this restart). 5.1 runs on
all of his decisions, re-entries included. Re-entry is slot **R** (one open, fire again
when the same tell prints). "First on this mint" is not a term of E.

Read the shape so the other slots know what they carry. Split first-on-mint vs later into
two E's **only when 5.1 names a different print** on later buys. Same class, one sentence.
Do not average two reactions.

| shape | what it means for the sentence |
| --- | --- |
| mostly one episode per mint | X harvests that hill; D may concentrate; E is still the print; R may stay one-per-coin |
| many re-entries on the same mint | E aims at the moment inside a live move; X eats a slice; D is often empty; R is open |
| mixed rates | report first-share and re-entry rate. One E unless 5.1 splits |

The shape also says which slots have to carry the book. On a re-entry scalper, E and X
usually carry more than D and P. On a one-shot, E still has to be DELAY-legal; D can
concentrate, and is still allowed to stay none.

---

## 4. Who pays

Two seats, two questions. Do not mix them.

```
                    his buy print
                         |
       RACE              |              FOLLOW
  fill just BEFORE ------+------ fill 115 ms AFTER
  does HIS decision pay?        copy of HIS fill
  (keep if this is green)       (red is expected; not a drop)
```

FOLLOW red on a copy of his fill is expected. It is not a drop of the wallet. "His fill is
unreachable" is not "his decision is unreachable".

| # | question | how | decide |
| --- | --- | --- | --- |
| 4.0 | Is this a reader? | Tape share of prints and SOL on coins he prints. Lake `ix_labels`: share of prints with `InitUserVolumeAccumulator`, bundled `TransferChecked`, `CreateCoinAndBuy` | Drop volume manufacture before FIND E, even when RACE pays. Small tape share is not enough if the ix book is a hopper (evidence 5.1, ApfmkS) |
| 4.1 | Does this member's own book pay? | Each member's episodes through our kernel: n, days, total SOL, median, top 1% share of **his** net, share of closes at <= -20% of clip, hold p50 | Keep: total > 0, enough tickets, harvest hold, rare >20% losses. A negative median with green total and rare large losses is a **cut** (his X), not a drop. Top 1% of his net is his harvest shape. Law 28 (ship tail) is top 1% of **our** book in section 11, not a pick veto |
| 4.2 | At which seat? | `seat.seat_book(S, E, caps=(15,))`: RACE and FOLLOW | Keep members positive at RACE on most days with a positive body. Positive only at RACE: the event must be reached **before** his print (a state, not a follow of his buy). Positive at FOLLOW: copying his fill still pays; that is rare and still not the event (the event is a public print, not his buy) |

Do not pool members (law 27). Derive from the ones that pay. A node that splits is two
nodes; write two working files.

---

## 5. Trigger and DELAY - leftover existence, not an exit P&L

Four questions, in this order. Question 4 is the veto. If it fails, this trigger is dead
at our seat. Do not walk D / P / X on it.

```
  1 WHO     who spends SOL next
  2 WHY     their business reason
  3 WHAT    the public print that shows it now
  4 DELAY   that SOL still unpriced 115 ms after we fire     <-- veto
```

Question 4 is the scarce one: if the tell and the SOL arrive together there is no trade at any
seat. Enumerate the delay first (strategy 1.6 lists the delays this market has).

A P&L number is always E+X. A hold-matched clock, a copy of his close, and every-fire
occupancy are exit or selection choices. None of them is the 5.2 veto. Grade an event
candidate by whether leftover **exists** at our seat, and whether this is his decision.

**Correct E is two yeses, in this order.**

```
  5.1  HE reacts to this public print          YES
  5.2  leftover still there 115 ms later       YES
           both --> E
           only 5.1 --> unreachable DELAY
           only 5.2 --> noise (do not run 5.2 first)
```

1. **5.1 his decision.** He pays at RACE (phase 4) and reacts to this public, unpriced tell:
   a lift spike of one class (or a held state), covering **>= 10%** of his decisions.
2. **5.2 our leftover.** After a 115 ms fill on the fires **he** takes, **behind his own
   buy**, the path still has enough convexity inside **his** hold to pay round-trip cost.

A better X cannot save a 5.2 kill.

Worked pattern (rule 1): public **SELL >= 1** at 25-200 ms, he avoids burst starts; behind
cost 1.41%, peak leftover +7.46%; then 6.1 spells which of those sells (recent buyer,
crowded tape, new high).

### Print classes (5.1 scan)

A class is yes/no on **this print**. 5.1 is the fire clock. 6.1 then spells **which** of
those prints he takes. Do not dump inventory 4-tuples into 5.1.

```
SCAN this print  (yes / no). Prefer 1, then 2. 3 is a screen, often DELAY = 0.

  1  WHO printed           tool  nonce  operator  direct     unpriced -- scan
  2  this printer's past   structure_burst  seller_recent  seller_loss
                           clip_step_up  struct_first_here
                           alone_in_slot  two_struct_slot  buy_after_sells
  3  priced screen         buy/sell x size  this-print up/down %
                           coin-gap burst_start  pro  new_build

NOT a class: price path, windowed flow, named wallet lists, copy of his buy
```

Every class above is defined once, with the code that computes it, in
[_!___terms.md](_!___terms.md) under "Print classes a study fires on". Read it before scanning:
several are easy to take loosely and each has an exact test.

The two that are confused most often: `burst_start` is the **coin** quiet ~400 ms, then any buy at
all, while `structure_burst` names **whose** silence broke - one ix structure's. A named list of
structures he follows is a thermometer: if it spikes, the public class is the **type** of those
structures, never the list.

If a priced class and an identity class both spike, **identity is the candidate**. Score
every spike that covers >= 10%, not only the loudest priced one. The loudest is often
DELAY = 0.

Held state (only if 5.1 is flat, or as a second pass): unpriced counts crossing K (distinct
recipes / wallets / operator structures). Fire = the rising-edge print. Windowed flow
(`buys5`) is the price path, not a state.

### 5.1 then 5.2

```
  pick a class
    5.1  he reacts?  (lift >= 2 and cover >= 10%)
      no   -> next class.  all flat -> held STATE, then 5.2
      yes  -> 5.2 leftover for us?  behind him, fill 115 ms later
                                    peak leftover > 0   missed < 50%
                                    (cost is diagnostic: how much of the hill is spent
                                     before our fill. It is not a kill if leftover remains)
        no   -> If 5.2 kills (below)
        yes  -> SPELL E (6) until the class is his prints; then D / X / P
```

Fire = that public print (or the rising-edge print). **Not** his buy. **Not** buy minus L.

```
BEHIND  (5.2 reads this row only)

  time -->   public print     his buy      our fill         later peak
             FIRE             already      we pay           leftover
                              in price     (behind him)

AHEAD = we fill before his buy. That copies his fill. Not the veto.
```

| # | question | how | decide |
| --- | --- | --- | --- |
| 5.1 | What does it react to? | `trigger.excess_intensity(S, {m: [w]})`: its buys against same-coin controls; every public print in the 5 s before, by (class, lag); `trigger.peak(lift, cls)`. Scan the families above, not only size / side / this-print % / coin-gap | A spike of one class at one lag band is the trigger, and that lag is the reaction time. The class is where E fires; which of its prints he takes is a conjunction, learned in 6.1. Identity beats a priced class when both spike. Flat everywhere: it fires on a **state**. A class covering under 10% of its decisions is a corner, not its logic: 8fStGV's trigger covers 93%, the burst start it avoids 5.8% (evidence 1.27) |
| 5.2 | Does leftover exist at our seat? | `seat.leftover(S, w, E, trigger)`, then `seat.leftover_summary`. Acted ticket: the latest print of the class <= 300 ms before each of his decisions (or the rising-edge print of the state). Our fill: `kernel.fill_idx`, 115 ms after **that** print. Read the **behind** row only: our fill after his buy, so his print is already in the price. Ahead tickets copy his fill; they are not the veto. Reaction cost = % price move from the print to our fill (how much of the hill is already spent). Peak leftover = the best net % of an exit decided inside his hold p50, filled 115 ms later, both legs paid (break-even is +3.1..+3.9% in price). Missed = our fill at or after his closing sell | **Kill** on the behind row when median peak leftover <= 0 or missed >= 50%. Cost is **not** a kill: a 7 % entry move with peak leftover +6 % is still a hill we can harvest. On all acted tickets: lag p50 <= 50 ms **and** ahead < 10% (tell and SOL land together, a race). Calibrated on the hot tape (evidence 1.27): rule 1's trigger reads peak +7.46 %; AbQcLH is a race on its picks (lag 47 ms, ahead 5 %, evidence 1.16); on its leftover table it reads 12 % ahead and passes thin (+1.69 %). A better X cannot save a peak leftover <= 0. A pass under `seat.THIN_PEAK` (+2 %) is flagged **thin**: a flag, never a kill, and the line is not yet calibrated (workflow). `seat.veto(L, n)` is the one reader of these lines: a script calls it, never re-spells them |
| 5.3 | Diagnostics, not a veto | `seat.reaction` (dt, ahead, behind). Horizon scan at 1, 2, 5, 15, 30, 60 s clipped to hold p90, and at his sell time, all from the `lag_115` fill. His-exit column: our entry at 115 ms, his sell at RACE (no exit lag). Occupancy column: every fire of the class on its coins, hold-matched clock. From `leftover_summary`: the acted and ignored rows, the peak at hold p10 and p90, the within-coin peak excess over ignored, the share that reaches break-even before the mirror loss | Report every column. **Do not kill** on a red clock, a red copy of his exit, or a red every-fire occupancy. Those mix X or which-fires into E. His median / top-1% / >20% losses are his X, not this veto. Occupancy red + leftover green -> **6.1** (which fires), then **7.1** if occupancy stays red, not a dead E. Law 23: peak leftover never ships |

A 0-cost median at 50 ms on **acted** tickets means no later print has landed yet. That is
not a purchasable fill. It does not change the seat.

**DELAY is leftover of the move, not gap to the next print.** Next-print p50 of 50-80 ms
is burst density (the hill is still printing). It is an artifact detector for a one-print
pop (section 11), not a veto of an 87 s episode.

A waiting time is not a reaction time. "Seconds since the last big buy" measures how often
big buys happen. Latency is measured from the trigger print, by excess intensity.

### If 5.2 kills

```
  most behind tickets already peak leftover <= 0 ?
      YES --> this family is dead. do not AND. do not walk D/P/X.
      NO  --> quieter subset? exclusive split, then 5.1, then 5.2

  this print is the execution (SOL already landing) ?
      YES --> earlier footprint of the SAME intent (WHO / history)
              leftover after our fill can still pass (peak leftover > 0)

  no class and no state passes 5.2 ?
      --> next wallet. do not invent E from inventory.
```

Overlapping spikes (`burst_start` / `buy>=0.5` / `up>=3%`) are one **family**, not three
events. The same for overlapping identity spikes (`structure_burst` / `clip_step_up` /
`buy_after_sells` / `two_struct_slot` / `tool`): one family. Overlap first, exclusive
split second, then 6.1 on that parent. One family is one term: do not AND two views of the
same mechanism as the event. Terms of different mechanisms are ANDed, in 6.1's ladder. Do
not drop the quieter class because another has higher lift. Split exclusive only when the typical
print **is** the move:

```
  quiet restart     burst start AND NOT up>=3%
  continuation      (size OR up>=3%) AND NOT burst start
  loud restart      burst start AND up>=3%
```

`up>=3%` is this print's own step: `((v[k] / v[k-1]) ** 2 - 1) * 100`. It is not an
accumulated window.

**5.2 is a veto, not a verdict.** A trigger that passes 5.2 stays open until a finished
search closes it: the 6.1 ladder, then the money ladder over D, P and X on that pool
(section 10), both exit families, walked forward (section 14). Occupancy red does not skip D. Phase 4 drops sssssw
(it passes 5.2: cost 1.18%, peak +1.01%), and 5.1's coverage drops the class 8fStGV avoids.

---

## 6. Event - which triggers, then spell it publicly

Run 6.1 before treating a class-wide red book as a dead E. Occupancy takes the first fire
on the coin; the fire he takes can be a later one.

| # | question | how | decide |
| --- | --- | --- | --- |
| 6.1 | Which of those prints does it take? | On **its coins**, every print of the trigger class (`candidates.build`); labelled by whether it acted (`contrast.label_acted` in its reaction window); `contrast.strat_rank(acted, ignored_same_coins, facts)`. The ignored rows are every class print on its coins while it is flat, before and after its buys: a table cut at its first buy makes its pick the coin's last candidate, and every fact that grows with time (age, holders, SOL bought) then ranks about 1.0. Then the **E ladder**: section 10's loop on the acted label. Each step adds the one class or fact (a quintile cut, one side) that most raises the acted share among the prints that pass the terms so far, fit on one half of the days, scored on the other; chance is the same ladder with acted shuffled across whole coins. Candidates are facts of this print and its recent tape; age, holders and reserve (and public SOL bought, the same quantity) are P (section 9), never E candidates; so is a fact that ranks with one of them at abs(rho) >= 0.9 on the table (the class's own print count so far is holders at another grain). A fact that reads a change reads it against the coin's own tape, never against another row of this table: the table drops the prints made while he holds, so the previous row is his exit as often as the coin's last event (1 % of the rows, 33 % of the acted ones) | A depth counts when its acted-share gain on the test half beats chance in both folds (section 10), both folds hold the same terms (family and side), and the conjunction still covers >= 10 % of his buys. No fixed term count. Each term carries one causal sentence (strategy 7.4); one family is one term. A fact far from 0.50 alone is a first step, not E; a fact near 0.50 alone can still be a later step. A conjunction at high lift covering a tiny share of its buys is a rare corner, not its logic. **Class too wide (this fork wins):** leftover exists only on the fires he takes, or acted is a tiny share of class prints (8aaRWu: **0.3 %**). Add terms to E about **this printer / this print's past**, not another loudness cut (`mvk`, `nstruct`). Do not freeze E. Do not run 7.1. Occupancy of that wide class is ballast, not a missing door |
| 6.2 | Does it hold as a public sentence? | Every term in public tape state, on **every coin** (`candidates.build` + `book.fires`). Book the 6.1 conjunction whole, with its ladder prefixes beside it. Run the opposite-side control (same terms, other side). Temporary public X = a hold-matched clock (a prior, not phase 8) | Money is read on the whole conjunction; a prefix may be red (strategy 7.4). The control must be worse. Own prints stay out. A red 6.2 with green 5.2 is not "E is wrong": leftover still exists. **If he still takes a tiny share of the class, next is more 6.1, not 7.1.** 7.1 only after 6.1 has named a spelling that covers a real share of his buys and occupancy is still red: then the door is missing (section 10). Oracle: occupancy **on its coins** still red means D cannot fix this class (8aaRWu framed its-coins **-1.96 %** 0/6). Do not jump to phase 8 on the acted pool. Not a random new E. The prior X is a yardstick: every prefix is booked under the same X, and its sign decides nothing. Rule 1's event reads -0.68 % under a 15 s clock and carries +4.5 % once P and X are filled (hot-tape case step 25, evidence 1.22). A term chosen under the prior X is re-read under the real X in phase 12: rule 1's "bought >= 2 SOL in 2 s" costs money there (evidence 1.20) |

Report coverage, reaction cost and peak leftover beside every lift. Cost is how much of
the hill is spent before our fill. Unreachable is peak leftover <= 0 after that fill, not
a 2 % cost line. That leftover read is a 5.2 kill, not a 6.2 comment.

Every fact is built from prints before index k. A feature window that contains the
member's own print is a lookahead.

The event is frozen for the next phases **once 6.1 has named which prints** (acted share is
his logic, not a corner). A loudness pass that leaves a tiny acted share is not a freeze. A
slot is held fixed only to search the next one on its fires, and is reopened in phase 12.

---

## 7. Door - optional

Search D on the frozen event's fires. Default is none.

| # | question | how | decide |
| --- | --- | --- | --- |
| 7.1 | Is the gap a coin property, or its future arrival? | Event fires on coins it trades vs every other coin, and on its coins split at its **first** buy there. Run only on a **spelled** E (6.1 named which prints) | A gap **before** its first buy is a coin property (a door candidate) **only when occupancy on its coins is the green side** (a door that selected those coins would pay). Occupancy on its coins still red means D cannot fix this class: later fires on those same coins are red, so the green before-slice is which print (E), not which coin. A gap only after is its future arrival: the coin list is not D. Occupancy red on every coin and green on the fires he takes is this test (section 10) after E is spelled. The split is read on a table with no coin floor ([backtest-audit.md](backtest-audit.md) U1): on the old floor 8dtx2t's first-position split read +6.03 % before its first buy and does not count |
| 7.2 | How much of the book is incoming demand? | `candidates.build(..., actor=w)` gives `act_in` | Diagnostic only. The member is 2-5 % of a real move's SOL; it detects the move, it does not ignite it |
| 7.3 | Door candidates | Coin facts from prints **before** the fire: 7.1's contrast (true on its coins before its first buy, rare on the rest) and inventory D ideas. AUC and money by quintile are diagnostics, never the pick | The candidates enter the money ladder (section 10) with P and X. Keep a door only if it survives the holdout and keeps the ship bars. None is valid. A door that raises SOL and breaks a bar is not taken |

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
| 8.2 | Book families against that bracket | `exits.X(kind=...)` for bracket, scale, sellbuy, trail, ride, fade, dump | The bracket is the default until a family beats it on money **and** bars. Read at the actor's own hold, not at a 1800 s horizon on a 20 s node. The families are the X candidates of the money ladder (section 10): an exit swap is a step like a term, so X is re-read on every selected pool |

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

The shape of the event's own payoff picks between the families: a payoff that is small and
frequent wants a fixed target, one that is rare and large wants a trail, and the wrong family
spends the edge that is there.

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
| 9.1 | What is true at the fire on stop-outs vs take-profits? | Facts at the fire; AUC as a diagnostic. A P term cuts **before** occupancy, so a refused fire frees the coin for a later one | The facts that separate, and inventory P ideas, are the P candidates of the money ladder (section 10). P removes stop-outs without shrinking the book below the bars |

P does not explain why the move happens. It puts the fire where a move can pay and where
a loss is bounded (age, holders, room under the wall). The mid-tape harvester frame is
**age >= 10 s** at the fire: sniper-eating rugs die inside 10 s. That is P / frame, not a
term of E. Age < 1 s is a different cut (the fill-model hole, section 14).

---

## 10. The ladder - the one search, on this parent only, and where a red result routes

The parent is the sentence after phases 5-9, including empty slots.

[_!___inventory.md](_!___inventory.md) is the idea list. Use it here, not at phase 0.

```
  E named AND spelled (acted share is his logic; the 6.1 ladder)
       |
       v
  one money ladder over D, P and X     each step: one term from any slot, or an exit swap
       |
       | no step beats chance --> name the slot the failure shape names (table
       |                          below), add one idea THERE, retry (do not replace E)
       |
       | leftover of this E died --> new trigger (5), not a new inventory E
       v
  gates (section 11), on the finished sentence only
```

**The ladder.** One loop builds a conjunction; E runs it on the acted label (6.1), D, P and X
run it on money.

1. **Candidates.** One fact cut at a quintile, one side, or one exit swap. D from 7.1's
   contrast and inventory D; P from 9.1 and inventory P; X from the 8.2 families. An inventory
   idea joins only with one causal sentence (strategy 7.4). A new idea is one new candidate and
   one inventory row.
2. **Step.** Add each candidate to each of the 5 best sentences so far, on the fires that pass
   their terms; book with occupancy on the fit days; keep the 5 best by the worst fit day's
   %/trade on the body (the top 1 % of tickets out) with the per-day floor held. Plain %/trade
   crowds the beam with tail exits that pay on one half of the days only; the worst day and
   the body are the days and TAIL bars' own reads. The parent may be red at every step: a single term's number on an
   unconcentrated pool carries nothing (strategy 7.4). Keeping 5, not 1, lets a term that pays
   only beside a partner wait for it.
3. **Walk-forward.** The ladder is built on one half of the days and scored on the other, both
   folds. A depth counts in a fold when its best sentence's gain over the root on the test half
   beats the gain at that depth of the same ladder run on y shuffled across whole coins (a row
   shuffle breaks the coin clusters and is too easy). The best sentence at a depth need not
   hold the previous depth's terms (the beam), so a depth that fails does not stop the ladder:
   its term may be waiting for a partner. The finished sentence is the deepest depth that
   counts in both folds with both folds holding the same terms, compared by family and side:
   one idea read at two grains is one term (`walkforward.walk_ladder`). When the folds name two
   facts of one family, the fold with the larger test gain gives the fact and its cut.
4. **Stop.** The next term leaves a day under the floor, or a depth cap. There is no fixed
   term count: the floor, the chance ladder and the folds' agreement set the depth.
5. **Cost.** A step books 5 x the candidates, so depth adds cost; it does not multiply it. A
   full grid is never run.

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
| he takes a tiny share of the class; occupancy of the class is red | E is not spelled | keep leftover, add terms to E (this printer), do not add D |
| positive on some coins, negative overall | the DOOR is missing | keep the **spelled** story, add a coin selector |
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

**A ladder step is not this pick.** On a red parent no single term turns every fit day positive
(8dtx2t's pool: 0 of 336 cuts), so this rule would take nothing and prove nothing. While a
sentence is built (6.1, section 10), a step is taken on its lift over the current pool, against
the chance ladder. Every day positive, SOL > 0 and the gates below judge the finished sentence.

**The gates every cell passes**, in order, before it is a candidate:

```
  FLOOR     >= 50 first-per-mint trades ON EACH DAY - a refusal, not a target
  MONEY     total net SOL > 0 on the whole sentence, never a proxy label
  TAIL      top 1 % of trades <= 20 % of net to stay a candidate, <= 15 % to ship
  CLIENT    positive with its single best client removed, and in >= 95 % of a client bootstrap
  WALK-FWD  the keep rule above
  candidate -> freeze -> disjoint holdout -> engine reconcile -> paper -> small real
```

- **The floor is per day, and a mean lies** (strategy law 19). Print the per-day list. A stub
  UTC day (hours well under 24) is not a floor day: quote tickets/hour x 24.
- **Tail calibration.** A real convex book at this seat puts 9-12 % of its net in its top 1 %
  (evidence 2.3); a cell at 50-270 % is noise around zero. Report the biggest coin's share
  beside it; if one coin carries the result, that is the result.
- **The client gate outranks walk-forward.** Trades come in clients, not independent draws
  (strategy law 18, evidence 3.1a). The client is the creation build for a launch-door sentence, the machine
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
| 12.4 | Thresholds by money | `walkforward.converge(C, days, base, grid, veto=bars)` | Keep rule and bars. A term on the grid edge means the table floor is too tight - rebuild looser. A looser threshold is judged on the trades it adds, net of the sentence's own trades it displaces through occupancy: on rule 1 every term fails its first step under both exits, so more trades need a second event, not a looser one (evidence 7, rule 1c) |
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
| each idea tried, new or re-read | its row in [_!___inventory.md](_!___inventory.md), in the same edit that writes the step's chain row: name, idea, meaning, why it matters, a worked example, status + one reference, to that file's row contract. Every D/E/P/X term of a booked sentence gets a row, red ones included |
| a word or a code name an idea needs | its line in [_!___terms.md](_!___terms.md), in the same edit as the idea. A row never explains a word twice |
| an off-chain fact that cannot be recomputed later (feed rank, replies, livestream, the document behind the metadata URI) | stored with the ticket at the fire, because a later fetch reads a different state, or nothing at all on a dead coin. The rule that uses it is an inventory row; the capture is this line |
| a verdict (kill, PASS, red, next step) | wherever it is recorded, it names the line that decided it and the number: "5.2 PASS, peak +12.61 %", never "a 5.2 kill" alone |
| a change to a method line (a veto, a gate, a routing) | the same edit lists every recorded verdict the old line decided and re-reads it from the numbers already stored, or queues the re-read in the workflow; a line in section 5.2 changes `seat.veto` in the same commit |
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
section 9; these are the method's own. Every number passes
[backtest-audit.md](backtest-audit.md) before it is reported as a result: the checklist that walks
these refusals, the laws and the replay.

```
  copy his fill and call it E
  fire at buy-minus-L
  use last-leg study as the book
  drop a cutting harvester as a "tail"
  use his median / top-1% / >20% loss as the 5.2 veto
  run 5.2 before 5.1
  put ahead tickets in the veto
  kill E on occupancy / clock / copy of his close
  walk D/P/X on a 5.2 kill
  AND-filter a family where almost every behind ticket already has peak leftover <= 0
  kill leftover existence on reaction cost when peak leftover still pays
  invent E from unused inventory 4-tuples
  treat 50 ms 0-cost acted median as take-profit, or change the seat from a lag ladder
  call a member "too fast for our seat" before re-measuring the seat and scanning every class
  read "every fire on every coin is red" as a verdict while D, P, X are unread
  freeze E / run 7.1 while acted is a tiny share of the class
  treat a before-first-buy green slice as D when occupancy on those coins is still red
  skip D / jump to phase 8 because occupancy is red
  call its coins' before-first-buy gap hindsight (it is the 7.1 door test, on a spelled E)
  split first-on-mint vs re-entry into two E's when they share the 5.1 class
  fit study fires after the holdout start
  hunt leftover inside age < 10 s as the mid-tape prize
  pool members
  freeze D/E/P/X at random
  score or pick a term alone, in E or in D/P/X -- a term is read inside the ladder
  close a slot with the every-fit-day-positive pick on a red parent
  close the node because FOLLOW of his buy is red
  guess conjunctions instead of naming HIS reaction first
  scan only size / side / % / coin-gap -- WHO printed is the scan
  pick a volume-accumulator / bundled-transfer wallet as the instrument
```

- **No parent from an inventory 4-tuple.** Combine ideas on a DELAY-legal event named by
  a member. Do not walk unused DxExPxX to invent the story.
- **No DELAY read as next-print gap** when the leftover is a hill. Next-print gap is an
  artifact detector for a one-print pop.
- **No leftover kill on reaction cost when peak leftover still pays.** Cost is how much of
  the hill is spent before our fill. Peak leftover is what remains after it. A 7 % cost with
  +6 % leftover is a harvest candidate (derive 5.2).
- **No conjunction walk on a trigger 5.2 killed**, and no 5.2 read before phase 4 and 5.1:
  the veto alone passes noise (evidence 1.27).
- **No freeze of E, and no 7.1, while acted is a tiny share of the class.** Leftover only on
  the fires he takes is "class too wide": more 6.1 (this printer), not D. Occupancy red on
  that wide class is expected ballast (8aaRWu 0.3 % acted; its-coins occupancy still **-1.96 %**).
- **No hold-matched clock, copy of his close, or every-fire occupancy as the 5.2 veto**, and no
  every-fire red read as a dead E when acted leftover is green: that is 6.1. 7.1 only after
  6.1 has named which prints.
- **No skip D after a red 6.2 on a spelled E.** Occupancy red and leftover green, with acted
  share already his logic, is "the door is missing" (section 10). Next is 7.1, then 8, then 9.
  Phase 8 on the acted pool with D unread is the skip. Phase 8 on a wide-class red occupancy
  is law 26.
- **No before-first-buy green slice as D when occupancy on those coins is still red.** Later
  fires on the same coins are the bulk; a door that selected them still loses. That slice is
  which print (E), not which coin.
- **No two E's from first vs later** when 5.1 is the same print. E is this print. Re-entry is R
  (section 3).
- **No study fire after the holdout start.** `study_exact` still contains 09-06 12:00..24:00;
  those prints are the holdout's warm-up. Count study fires only before 09-06 12:00 (section 2.1).
- **No mid-tape prize inside 10 s.** Sniper-eating rugs die there. Age >= 10 s is the harvester
  frame (P), not a term of E. Age < 1 s is the fill-model hole (below).
- **No ahead ticket in the 5.2 veto.** Our leftover on it includes his own buy: a copied fill.
- **No pooling members.** Split first. Two logics get two working files.
- **No mint list as a door.** A member's coin list is a diagnostic split, made at its first
  buy on the coin before any gap is read.
- **No wallet in a term, agreement included.** Every term is public tape state or ix structure;
  a measurement that needs named wallets stays a thermometer (law 20).
- **No fact dated after the fire**, and no coin kept or dropped on its future (a floor on its
  print count or life is dated after the fire; [backtest-audit.md](backtest-audit.md) U1).
- **No exit swept on an unselected pool** or past the actor's own hold.
- **No slot closed from a red number.** A slot closes only on a measured mechanism that holds
  at every coordinate (strategy 8.1), or a finished ladder (section 10) on a parent, at the
  floor, with both exit families as candidates, walked forward. Anything else is "sentence `c` is red",
  with its empty slot named.
- **No replacing E because D is empty.** Empty D is allowed. A missing door keeps E and
  searches D, or ships with D = none.
- **No extra AND as a new sentence, and no trimming a frozen sentence** because a prefix holds
  out better: write a new sentence and burn the week.
- **No mixing doors in one sentence.** Slow-wall, documented-project and demonstrated-episode
  are different stories, one at a time.
- **No parking the harvester** because survival is the only holdout-positive book. Survival is a
  door; the target in section 0 is still leftover of real up-moves.
- **No last-leg study as the book.** Thresholds are read on `study_exact` (every leg).
- **No FOLLOW of his buy as a drop.** FOLLOW red on a copy of his fill is expected.
- **No volume-accumulator / bundled-transfer wallet as the instrument.** RACE green and
  `t >= 2` do not make a reader. Measure tape share and the ix book (section 1, 4.0) before
  FIND E.
- **No 50 ms 0-cost acted median as take-profit.** No later print has landed yet; that is not
  a purchasable fill, and it does not change the 115 ms veto.

The guards:

- **An absurd win rate is a lookahead until shown otherwise** - 80 % on a scalper, or a cell
  green on every column at once.
- **No fire inside a coin's first second.** Strategy 8.1's age-0 launch ramps are consumed in
  about two slots, and the kernel's fill (the last print landed by fire + lag) cannot see the
  pre-sent sniper queue there. A book that lives there is the fill model's blind spot: split every
  book by age at the fire before trusting it. 8dtx2t's first-position pool held 72 % of its SOL at
  age < 0.5 s, at a 0.0 % pick share of its own.
- **Age < 10 s is not the mid-tape prize.** Sniper-eating rugs die inside 10 s. The harvester
  frame is age >= 10 s at the fire (section 9). 8aaRWu's own age p50 is 89 s.
- **A 5.1 scan older than its class list is re-run.** 8dtx2t's priced-only scan named
  burst_start; the scan with WHO and history named clip_step_up (identity beats priced).
- **A ceiling is not money in a slot** (law 23). A perfect-exit or perfect-door number bounds
  the slot; it does not say the slot can reach it.
- **A gradient is not a filling.** A term monotone in money that does not cross zero stays a
  ladder candidate (section 10), not a threshold AND on its own.
- **Features in the coin's own units.** A fact against the coin's own prior history beats one
  ruler across thousands of coins almost everywhere (evidence 1.5); a live rule computes it
  from the coin's tape with no stratum and no lookahead.
- **Every flow fact has a node-blind twin** with the members' SOL removed, so a term can be asked
  whether it is public tape or a proxy for the members.
- **`v[k]` is the reserve AFTER print k.** Pricing an entry at `v[their buy]` charges THEIR
  displacement; the reserve they met is `v[k] - signed amount`. A state fire anchors at the
  print the decision sees (`k-1`): anchoring at `k` fills later, and on a falling tape a later
  fill is a cheaper buy.
