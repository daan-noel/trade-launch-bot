# 2026-09-10 - The hot-tape node splits by member, and the exit verdict of the same day is withdrawn

## What was claimed that morning

[2026-09-10-the-exit-slot-is-empty-on-the-hot-tape-node.md](2026-09-10-the-exit-slot-is-empty-on-the-hot-tape-node.md)
reported the exit slot searched over 480 causal shapes and empty, and closed X as a standalone
lever on this node.

## Why that verdict does not stand

Three errors, in order of size.

**The grid was swept on an unselected pool.** All 95,135 episodes, D empty and P empty. Workflow
section 5 refuses exactly that: an exit swept on an unselected pool returns the shortest clock,
because the pool is mostly dying coins. The grid's own result - flat clocks beating ladders beating
trails, cap15 beating cap30 beating cap60, shorter beating longer everywhere - is that refusal's
signature. It measured the pool, not the exit.

**The convexity was measured at a horizon the node does not trade.** MFE p50 33 %, p90 181 % and
the +53 % perfect-exit ceiling were all read over 1800 s. The node's median hold is 19.9 s, and the
peak available inside that hold is +7.6 %.

**Six wallets were pooled that are not one shape.** `8fStGV` wins 68.3 % of its trades at a median
of +12.02 %; the other five carry a negative median and a 180 % top-1 % share.

Recorded as strategy 7.4 laws 26, 27 and 28.

## What the re-run found

Five runs on the same episodes and on the full 12.47 M print tape.

**The node is two nodes.** Booked at the RACE seat under a 15 s clock, three of the six members -
`8fStGV`, `AbQcLH`, `49uohd` - read +2.2 to +2.3 %/trade. Pooled they are 12,933 tickets, 1,913 a
day over 4,327 coins, +58.77 SOL, 8 of 8 days positive, worst day +0.01, +29.50 SOL with the top
1 % of tickets removed, and their biggest single coin carries 1.8 % of the net. That was the first
cell in the program to clear days, body and coin concentration together. It failed the tail gate
at 49.8 % against the 9-12 % calibration, and it was a ceiling rather than a rule, because the
oracle was a wallet list. The other three were noise: top 1 % at 494 % of net and a body of
-138.84 SOL.

The whole of it lived at the seat. The same decisions priced at our own fill read -0.66 %/trade on
1 of 8 days.

**The two groups act in different states.** The members that paid bought a pullback inside a live
up-move: the coin up 22.3 % over the last minute against 9.8 %, a new high 24 s ago against 63 s,
24.7 % below the peak against 32.0 %, 73 SOL through the pool in the last minute against 45, and
the last five seconds turned net sell. The others bought a deep dip on an older coin. Evidence 1.6
had reported this node buying deep below the peak because the second group outnumbered the first
eight to one.

**Every public reconstruction was red.** A swing pullback taken at the turn: -3.35 %/trade, 0 of 8
days, 357,896 fires. The up-move portrait as a standing condition: -3.58 %, 0 of 8, 207,688 fires.
The independent-machine count: -2.99 %, 0 of 7. Tightening the price state made the book
monotonically worse, and buying the turn beat buying the crossing print at every depth and every
exit.

**One term moved the book without being headroom.** Fewer than five distinct builds printing in
the last five seconds booked -4.08 %/trade over 85,416 fires; twelve or more with a de-concentrated
build mix booked -2.99 %. About 1.1 points, and it did not cross zero. It was the first time the
axis strategy 1.4 calls unpriced entered this node's event at all.

## The lookahead this run caught

The level rule's one green cell read +5.02 %/trade on 8 of 8 days behind the door of evidence 1.8,
reserve at age 60 s of 50 to 70. All of it was the 15.5 % of its fires that happened before age
60 s, where that door fact did not yet exist: 2,287 tickets, +251.42 SOL, +54.97 %/trade, 80.8 %
win rate. At age 60 s and later the same cell read -4.12 %, 1 of 8 days. A door fact dated later
than the fire is law 24 in the D slot, and the 80.8 % win rate was the tell.

## What changed

- Evidence 1.11 records the re-run; 1.10's verdict is retracted at the head of that section.
- Strategy 7.4 gains law 26 (read an exit at the actor's own hold, on a selected pool), law 27
  (pooling a node's members averages away the one that is a different animal) and law 28
  (imitating an actor is bounded by his median trade, never by his net).
- The node moves from "red at lag_115" to open in strategy 5.2 and 8.2, and the workflow queue
  carries H7 with the three open threads: the machine axis as a fitted vector, the own-exit level
  as a full sentence, and their trigger print.

## The trigger, found later the same day

A reaction measurement by excess intensity against each coin's own print rate replaced the
retracted waiting-time statistic. The member that paid most cleanly, `8fStGV`, bought 25-200 ms
after a public sell of 1 SOL or more and avoided burst starts; the member that lost, `sssssw`,
bought 75-100 ms after a large buy and avoided sells. Speed did not separate them; the side did.

The sells `8fStGV` picked landed inside a buying frenzy: fifteen distinct builds printing in the
last five seconds, 3.9 SOL bought in the last two, a new high five seconds earlier, and a seller who
had bought 21 seconds before. Spelled as a public event and held 15 s, that booked +1.30 %/trade on
5 of 7 days on the coins `8fStGV` traded and -3.96 % on every other coin, so the event slot was
filled and the door was left as the empty slot. Evidence 1.12.

## The door and the exit, later still

The five points "in which coin" were mostly the member's future arrival. The coin list was built
from the whole tape; split at the member's first buy on each coin, the frozen event booked +4.46 %
on 7 of 7 days before it and -0.68 % after it, which is the full-tape number.

Thirty unpriced door facts were walked forward on days. The absorption idea that motivated the run
came out with the wrong sign: the event paid where at most a third of the coin's earlier
frenzy-sells had made a new high (+1.13 %/trade, 5 of 7 days, both folds positive, uncorrelated
with reserve).

The member's own exit, read by the same excess-intensity histogram on its closing sells, was the
mirror of its entry: it sold 25-50 ms after a public buy of 1 SOL or more, at a median +14.4 %.
Its hazard was a bracket - take profit +10 %, stop at -25 to -40 %, time from 40 s. A first read
at coarse bins put the stop at -20 %, which cost 0.66 points as a hard cut.

Door and exit together booked +1.22 %/trade, 124 a day, 5 of 7 days, body +1.23 SOL, biggest coin
11.1 % - the first positive causal sentence on the node. It failed the tail at 39.7 % and faded to
+0.25 % over the last three days. Evidence 1.13.

## The permission, last

The losers were not an exit problem: twelve tape-driven exits all booked below the static bracket,
because a frenzy's price swings 5-10 % inside a few prints. Read at the fire, the trades that
ended at the stop were on young, thin coins. Age >= 158 s and >= 368 public wallets holding, each
cut chosen on held-out days, took the sentence to +2.07 %/trade, 141 a day, 7 of 7 days, body
+3.26 SOL, top 1 % 17.6 %, and made the earlier-frenzy door redundant. Seven days in-sample; the
tape after 09-06 12:00 was left as the holdout. Evidence 1.14.

## The holdout

The lake's days after the study tape were converted to its exact format, and the frozen sentence
was booked on 4.5 unseen days (09-06 12:00 to 09-10) with the same code: +1.90 %/trade, 132 a
day, 5 of 5 days positive, body +1.73 SOL, top 1 % 23.2 %. The earlier-frenzy door, which had
looked like the door slot's filling, failed out of sample at -0.53 %. Evidence 1.15.

## The second event, not found

The second paying operator's burst-start leg reacted in 47 ms, ahead of a 115 ms fill 95 % of
the time. Its dip-buy leg was reachable - our seat on its picks booked +3.50 % on 8 of 8 days -
but every public spelling of what it picked (a capitulation cascade, the operator's recipes in
the coin, a big dip-buy just before) was red. Evidence 1.16.
