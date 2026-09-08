# 2026-09-07 - the N4 node at our seat: closed; the no-initial-buy door: one machine, two days

Study of the mid-tape generalist node (N4 in
[roster-habit-nodes.md](../../hunter/docs/plans/strategies/roster-habit-nodes.md)) as three
daily-profitable wallets run it (3Xk2, 8dtx, 6J1T), on one week of full tape (2026-09-01 ..
09-07, 11.76 M curve prints, 115,646 tokens) with three disjoint holdout weeks (08-11 .. 08-31).
Everything priced by one kernel: last print landed by decision + 115 ms on both legs, exact curve
arithmetic, 125 bps + 0.000225 SOL a leg, one position per token. The kernel passed two acceptance
tests before any rule was read: it reproduces the earlier exit-fill refutation table to the cent,
and random fires on the full tape lose on every admissible exit (the zero-lag column shows a
2.6-point reactive-exit artifact even on random fires).

## What the three wallets are

All three are durable-nonce racer builds (one private build each, 3 wallets per build). Every
entry lands inside a burst that a router (terminal) buy of about 0.5-1 SOL started, 130 ms after
that print (median), 1-3 prints behind it, beside other fast bots (a Lighthouse-asserting build
with 70 wallets, a 643-wallet seed builder). Their response to wallet-free events on the full tape
names nothing cleanly: any buy >= 1 SOL 2.3x base, a racer buy >= 0.5 SOL 3.2x, the seed-builder
buy 5.2x; silence-breaks, multi-build slots, dips and sell flushes all at or below 1x. They hold
17-50 s, exit at a median 0.92-0.98 of entry, and are carried by the tail: 27-39 % of trades win.

## The node at our seat

On their own picks (hindsight selection, so an upper bound), fired one print upstream and filled at
115 ms both legs:

| decision print | lands after them | clock 20 | clock 45 | tp25 c120 | trail30 c600 |
| --- | ---: | ---: | ---: | ---: | ---: |
| burst start (their trigger) | 42-46 % | +2.6 .. +6.2 % | +3.5 .. +6.7 % | +1.8 .. +4.8 % | +2.7 .. +3.8 % |
| the print before them | 77-90 % | -0.6 .. -2.4 % | +0.3 .. -3.8 % | -1.4 .. -3.2 % | -0.4 .. -4.4 % |
| their own print | 100 % | -1.8 .. -3.9 % | -0.8 .. -4.2 % | -2.4 .. -3.3 % | -1.6 .. -5.5 % |

The money exists only in the half of cases where the 115 ms fill lands inside the swarm; behind it,
every exit is negative on tokens chosen with hindsight. The node's edge is the swarm's own impact.

## The story on the full tape

Event: a router buy >= 0.5 / 1 / 2 SOL on a token 10-600 s old at reserve 33-55; every instance.
At 115 ms both legs every exit is negative on every size: -1.5 .. -13 % a trade, 0 of 7 days,
34,097 fires at >= 1 SOL. Ceiling column (0 ms): +0.4 .. +4.6 % on the trails - the artifact size.
Permissions one at a time (creator not sold, dip, active tape, silence-break): none turns it
positive; all four together -1.3 %.

## The one positive cell, and its holdout

Tokens whose creation transaction carries no buy (`initial_buy_lamports IS NULL`, ~4 % of
launches) and whose creator never traded: router buy >= 0.5 SOL, clock 45 -> +12.58 SOL,
+3.1 % a trade, 2,029 fires on 741 tokens, 5/7 days. Frozen (door, event, phase, permission, two
declared exits) before the holdout was read. Holdout, same sentence:

| week | fires | clock 45 | days | trail30 c600 | days |
| --- | ---: | ---: | --- | ---: | --- |
| 09-01 .. 09-07 (fit) | 2,029 | +12.58 SOL (+3.1 %) | 5/7 | +10.99 (+3.3 %) | 5/7 |
| 08-25 .. 08-31 | 459 | -1.18 (-1.3 %) | 3/7 | -4.40 (-5.7 %) | 3/7 |
| 08-18 .. 08-24 | 354 | -6.67 (-9.4 %) | 0/7 | -8.46 (-15.0 %) | 0/7 |
| 08-11 .. 08-17 | 234 | -0.66 (-1.4 %) | 3/7 | +1.26 (+3.2 %) | 4/7 |

The fitting week was one launch machine: build `fb455027` (Create_v2 through a fee-sharing
program, 294 creators) launched 921 tokens on 09-01 and 09-02 and none on any other day of the 30;
it carried 1,194 of the 2,029 fires and most of the SOL. The sentence is refuted as written and
is not trimmed.

## What stands

* The creator-sold blacklist. After any buy >= 0.5 SOL mid-tape, tokens whose creator has already
  sold lose 3.9-4.6 % (clock 20) and 6.0-7.7 % (clock 45) a trade, 0 of 7 days positive in each of
  four weeks, 36,000-89,000 fires a week; creator-holds tokens lose 0.5-3.4 %. The ranking
  sold < holds < never-bought holds every week. It is a permission, never an edge.
* The kernel and its two acceptance tests, in
  [study-kernel/](../../hunter/docs/plans/strategies/study-kernel/).
* The three wallets remain thermometers for the swarm, not for a decision open to us.

Scripts and outputs: `study-kernel/` (kernel, tape loader, tests, the frozen sentence, the holdout
runner); the week's census (`wk_census.py`) and the probes live with the session scratchpad.
