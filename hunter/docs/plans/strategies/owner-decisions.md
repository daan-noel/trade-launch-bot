# Owner decisions: the method that finds a group's rule

**What this file is.** The workflow for any launch group. Trades are already split into owner
and outsider ([_!___owner-split.md](_!___owner-split.md)). This file finds **why and when the owner
acts**, then builds the rule from those decisions: buy when the next act is "keeps working the
coin", stay while it rides, sell just before it dumps.

A correction to the workflow is written here, in the session it is accepted. A group's regions,
levels, lead times, and booked stages go in that group's case file, written when a run produces
them.

The picture and sections 1-7 are the workflow. Cuts for a run come from the fit week.

---

## The picture

```
owner-split: each trade is owner or outsider
        |
        v
fit week = the latest 7 days of the group's prints
        |
        v
every print while the owner still holds
        |
        +-- label = the owner's NEXT act
        +-- attach = past readings only, at several spans
        |
        v
a region = a small set of readings where one next act
           becomes common, and the other acts do not
        |
        +-- same readings, same next act, any age  --> one logic
        +-- one next act, two different pasts      --> two logics
        |
        v
inside the fit week the cut recurs on most days
        |
        +-- lead long enough to fill  --> a stage
        +-- lead too short to fill    --> the logic stands;
                                         the sell is the dump print
        |
        v
the stage books more than ignoring it, on the fit week
        |
        v
entry (keeps working) / stay (ride, hold, migrate) / exit (a sell)
        |
older weeks sit beside the result:
  same region still splits?  --> the logic held; write that week's level
  a different reading splits --> a logic change; write it down
neither one moves this week's level
```

---

## 1. The owner

Each coin is a small business for the owner.

- **It spends** its own SOL to push the price, and its time.
- **It earns** outsiders' SOL. Its profit is what outsiders pay for its tokens.
- **Its risk** is outsiders holding a bag that can sell first.

It keeps working the coin while it expects outsiders to pay more than the next push costs, and
it dumps when that stops. A name in section 6 is applied **after** a region splits. The figures
inside those names are examples of a reading.

Its income is that profit. Its own trades are most of the coin's volume, so creator fees are
mostly fees it pays itself.

## 2. A decision

A **decision** is a branch in the next act, read from the past only.

The **next act** is whatever the owner does next, while it still holds:

| next act | what the tape shows |
| --- | --- |
| buys again | the owner buys and the price is pushed |
| sits | a gap with no owner trade while the coin goes on |
| sells part | an owner sell that leaves most of the bag |
| sells most, one fall | an owner-majority [fall](_!___owner-split.md) in one drop |
| sells in steps | an owner-majority fall made of several drops |
| pumps again | after a fall, the owner buys and the price climbs off the bottom |
| migrate | the coin completes the curve |
| moves on | the owner launches or starts pushing another coin |

A **logic** is one region of past readings where that next act becomes common. The list of
logics is however many regions survive section 4. Age does not make a new logic. Two ages with
the same readings and the same next act are one logic. One dump with two different pasts is two
logics.

A fall is the one defined in [_!___owner-split.md](_!___owner-split.md). That definition tells the acts
apart. It is not a decision window.

## 3. Readings, spans, levels

Every reading below is attached. The search keeps the ones that separate the next act.

| reading | what it is |
| --- | --- |
| owner profit | tokens marked to market, minus what the owner paid |
| bag vs pool | the owner's bag against the SOL in the pool |
| since last owner buy | time since the owner's last buy |
| since last owner sell | time since the owner's last sell |
| outsider buys | outsider SOL, buyer count, largest buy |
| outsider sells | outsider sell SOL |
| price vs peak | price against the high so far |
| flatness | how tight the price has sat, over a span |
| pool | SOL in the pool |
| age | time since creation |
| other coin | the owner launched, or is pushing, another coin |

Each flow, each flatness, and each "since" is stored at several spans at once: one slot, 1 s,
3 s, 10 s, 30 s, since the last owner act, since creation, and the change from a short span to
a longer one. The span a region uses is the span that separates its next act. A level (a SOL
amount, a percent, a pool size, a clock) is the cut where that separation is sharpest on the
fit week.

A figure named in a session is an example of a reading until the fit week shows that it splits.

The owner tag is part of the reading. On the prints that feed a region, owner flow has to be
owner. Owner buys counted as outsider are a different region, and the region is rebuilt once
the tag is corrected.

## 4. How a logic is found

Run this on the fit week (section 5).

1. **One row per print** while the owner still holds. The row's label is the next act. The row's
   readings use only prints already in the past.
2. **Search the readings and the spans** for a small set where one next act is common and the
   others are not. Each such set is a candidate region.
3. **Merge and split.** Same readings and the same next act are one logic, at any age. One act
   with two pasts is two logics. A region that matches no name in section 6 is still a logic.
4. **The cut recurs across days.** The same readings separate the next act on most days of the
   fit week, and the level stays in a tight range across those days. A cut that appears on one
   day is noise. The owner moves a level from week to week, not from day to day. The shape of a
   logic (which readings separate the act) moves rarely; the level moves easily.
5. **Measure the lead.** The lead is the time from the readings turning true until the act. It
   is whatever the tape shows for that logic. A lead of about 0.1 s or more leaves room for the
   fill. A shorter lead is a real logic, and the order lands on the act itself: the sell is the
   dump print (section 7).
6. **Book it.** Fills are 0.115 s after the decision, 0.03 SOL, unless the group's case file
   sets another size. A region becomes a stage when acting on it books more than ignoring it,
   on the fit week. A region that reads the owner and books no better than ignoring it is not a
   stage.
7. **Read the misses.** Where the readings hold and the owner takes another act, record that
   act. That is where a stage gives money back.

## 5. The fit week

The **fit week** is the latest 7 days of the group's prints. It is the owner's current market.
Logics and levels are fit on that week only.

Each older week answers two questions, and nothing else:

| question | what is written down |
| --- | --- |
| Does the same region still split the next act? | the logic held |
| If it held, where did the level sit? | that week's level, so the drift is visible |
| Does a different reading split the act? | a logic change |

An older week does not move this week's level. A region that fails to split on an older week
stays in the rule when it splits on the fit week. A region that splits only on an older week
stays out of the rule. Weeks are not averaged.

## 6. Names

A name is written on a region after it separates. These are the names already in use. A new
region with no fitting name keeps its readings as its name.

| name | the reading it points at |
| --- | --- |
| "I have made enough." | owner profit reaches the level this region uses |
| "Buyers are here - sell to them now." | outsider buys over the region's span, with profit already near its level |
| "Take the small crowd." | a small outsider crowd, profit short of its level |
| "Real buyers carry it." | outsiders keep buying after profit reached its level; the next act is ride or migrate |
| "Hold the price and wait." | the price sits flat while the owner keeps trading, then a sell into the next buyers |
| "My next coin needs me." | a launch or a push on another coin, then a sell here |
| "Early holders are dangerous." | outsiders hold or sell a bag the owner does not want under it |
| "Sell while the pool can take my bag." | bag vs pool, the sell lands while the pool can still absorb it |
| "All at once, or in steps." | buyers stop, one fall; buyers trickle, sells in steps |
| "Is pumping still worth it?" | outsider SOL per SOL the owner pushes, and that ratio falling |
| "The holders are gone." | after a fall, outsiders who held through it have sold; a new pump is safe |
| "Use what worked." | a reused name, or the hours the owner chooses to launch |
| "I am working a session." | several launches close together, against a lone coin |

Any SOL amount, percent, span, or age written next to a name elsewhere is an example. The fit
week sets the level.

## 7. From logics to the rule

One stage per logic that survives section 4.

| stage | when it acts | the next act it reads |
| --- | --- | --- |
| entry | the readings say the owner keeps working the coin, and the lead covers the fill | buys again |
| stay | the readings say ride, sit, or migrate | buys again, sits, migrate |
| exit | the readings say a sell, including while the position is down | sells part, sells most, sells in steps, moves on |
| dump print | the owner's dump instruction prints | the act itself, when no earlier exit landed |
| migrate print | the curve completes and no post-migration tape exists | the backtest closes at this print |

After the curve completes, the live rule has its own stage. That stage is fit the same way once
post-migration prints exist: a hold and a trail are readings, and the fit week sets their
levels. Until those prints exist, the backtest closes at the migration print.

A pool size, a clock, or a profit percent is a stage only when it is the level of a region that
survives section 4.

The number of stages is the number of regions that survive. Every line of the rule names the
region it reads.
