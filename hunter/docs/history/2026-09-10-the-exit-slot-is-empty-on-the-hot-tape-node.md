# 2026-09-10 - The exit slot is empty on the hot-tape node, and one lookahead nearly hid it

> **SUPERSEDED THE SAME DAY.** The search below ran with D and P empty, which workflow section 5 refuses, and it read the excursion over 1800 s on a node that holds 19.9 s. Its verdict on X is withdrawn. See [2026-09-10-the-hot-tape-node-splits-by-member.md](2026-09-10-the-hot-tape-node-splits-by-member.md) and evidence 1.11.

## What was claimed

On 2026-09-09 the hot-tape node's fires were booked under a cap-60 clock at -6.67 %/trade, and
selling each fire at its own 120 s peak was measured at +19.26 %. That gap was written up as
"twenty-six points sit in the exit slot", and the exit was named the largest unsearched thing on
the node.

## What the search found

The exit slot was searched properly against the convexity the six wallets themselves enter -
95,135 position-tracked episodes, our 0.2 SOL clip, both legs priced through the kernel.

The convexity is real and large. From our fill 115 ms behind their print, MFE is 33.19 % at the
median and 180.61 % at p90 over 1800 s, and a perfect exit books +53 to +58 %/trade on 8/8 days.

No exit reaches it. 480 causal shapes (first rung 5-30 %, rung fraction, runner trail 20-40 %,
six dead cuts, two trail modes), three-rung ladders, and five flow tells, fitted on half the
episodes and read on the other half:

- behind their print, the best shape is a flat 15 s clock at **-2.35 %/trade, 0/8 days**;
- sequenced ahead of their print - a seat that is not reachable without solving the entry - the
  best shape is **+0.21 %/trade, 5/8 days, and -0.89 % with its top 1 % of tickets removed**;
- shorter beats longer everywhere, ladders lose to flat clocks, and flow tells lose to both.

The reason is in the geometry. A quarter of the entries that eventually double are under water by
31 % before they get there, so no trail tight enough to protect capital survives the winners; and
the median future minimum is -40 to -60 % in every state bucket, so holding for the peak always
gives it back. The move has real momentum - at 20 s in, a position up 25 % has median future max
+107 % - and the momentum is not worth the toll.

Their own exit is not a model either: capture ratio 0.07 of the peak already available when they
left, within 10 % of the window peak in 0.6 % of episodes, own book +0.70 % on spend.

## The lookahead

The first version of the grid returned **+8.30 %/trade, 8/8 days, top-1 % concentration 27 %,
leave-one-day-out +599 SOL** on a held-out half. Every robustness column was green and all of it
was an artifact: the dead cut was evaluated as "no rung ever fired in the window" rather than "no
rung fired before this deadline", so a 30 s cut kept precisely the tickets that were going to
reach the rung at any point in the next ten minutes. Decided in index order, the same shape reads
-2.38 %.

The tell was in the grid's own ranking: every top row shared `cap30`, and the *higher* the rung
the better the result - a rung is supposed to cost money to reach, not select the survivors.

## What changed

- Evidence 1.10 records the search; the "twenty-six points" sentence in 1.9 is retracted in place.
- Strategy 7.4 gains law 23 (a perfect-exit ceiling is not money in a slot), law 24 (every exit
  branch resolves to an index, smallest wins), law 25 (a multi-leg exit is priced leg by leg).
- The open slots on this node are now P and E. The only slices that survive at the race seat -
  reserve 32-35, age under 30 s, several independent programs already acting - are P candidates,
  and they are the slices with the *least* available convexity, which is the reachability
  anti-correlation showing up on the exit side.
