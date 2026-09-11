# 2026-09-09: the sequencing race is the cost, and every print-anchored red is re-opened

The user rejected the day's conclusion with a specific objection: *these wallets pay the same fee
and have the same latency; a 1 % margin is already after costs; if I fire at the same decision
point I am racing them, not following them.* Every part of that is right, and checking it found
two bugs of mine and overturned the standing reading of all five solo nodes.

## The check that should have come first

Nobody had ever priced the roster's OWN decisions through our own kernel. Every node verdict in
this program compares a reconstructed rule against a roster number computed by a different
pipeline. If the two disagree on the same trades, none of the verdicts mean anything.

**Two bugs, both mine, found in the doing.**

`v[k]` is the reserve **after** print `k` - verified on 12.3M consecutive prints, where
`delta(reserve) / signed amount` is 1.00000 from p1 to p75. The first replay entered at
`v[their buy]` and exited at `v[their sell]`, which prices us from after-their-buy to
after-their-sell and therefore **charges us their entire round-trip displacement**: 3.8 to 6.4
points at their clip sizes. That is the whole reason it read -6.6 % on trades the roster reads
+1.55 % on. The correct reference is `v_before(k) = v[k] - signed_amount(k)`, exact.

The second replay scored only 1-buy-1-sell episodes. For a re-entry trader that subset is their
**worst** trades - 64hP reads +1.18 % on the roster and -12.12 % on it. An episode has to be built
from the position, which the tape gives exactly: on `K = vsol * vtok`, a buy hands over
`K/v_before - K/v[k]` tokens. An episode opens when position leaves zero and closes when it
returns. Built that way the numbers reconcile across 25 wallets - omegoM 0.76/0.94, 88887Q
1.52/1.70, 9999hu 1.33/1.41, 8dtx2t 2.65/2.67, ocBBRK 0.88/0.90 - and 1.07 % are bags, which are
reported rather than dropped the way `census.rb_ep2 ... AND NOT is_bag` drops them.

## The result

131,339 episodes, 25 wallets, our 0.2 SOL clip on their decisions:

| seat | per trade | days positive | SOL |
| --- | ---: | :---: | ---: |
| THEIRS - their money, their clip | +0.96 % | - | - |
| **RACE** - sequenced BEFORE their print, both legs | **+2.77 %** | **8/8** | +718.95 |
| **PEER** - same slot, order is the leader's coin flip | -0.03 % | 3/8 | -7.19 |
| FOLLOW - after their print, zero lag | -2.82 % | 0/8 | -733.33 |
| FOLLOW - after their print, +115 ms | -3.59 % | 0/8 | -933.37 |

The cost ladder that falls out of it:

| term | points per round trip |
| --- | ---: |
| **losing the sequencing race** | **-5.59** |
| pump.fun protocol fee | -2.47 |
| our own impact at 0.2 SOL | -0.76 |
| **the 115 ms** | **-0.77** |

**The term this program has spent two months on is the smallest of the four.** Being sequenced
after a print costs seven times what the latency costs, because the print being reacted to is
itself an order worth 1.16 % of the pool at the median episode, and its displacement is paid
twice.

And the roster's clip is not the best clip. Impact is `B/vsol`; they run 0.2-2.0 SOL where we run
0.2, so on their own decisions **our book beats theirs** - +2.77 % against +0.96 %.

## What it invalidates

**An event anchored on a PRINT is a FOLLOW model by construction.** The print has to exist before
anything can react to it, so a print-anchored rule always pays that print's impact and always
loses its slot. Every node study in evidence section 6 is anchored that way, including the
hot-tape work earlier the same day. A state that has been true for *seconds* is the only anchor
that can be a peer - and "the state changed at this print" is a print anchor wearing a state's
clothes, which is exactly what that hot-tape event was.

So the red verdicts on the five nodes are not verdicts. They are readings taken at the worst of
four seats. The ceiling at the reachable one:

| node | episodes | PEER ceiling | days positive |
| --- | ---: | ---: | :---: |
| deep-age big clip | 1,197 | **+2.62 %** | 6/8 |
| mid-tape one-shot | 14,790 | **+0.71 %** | 7/8 |
| hot-tape re-entry | 93,905 | -0.14 % | 3/8 |
| instant launch | 12,187 | -0.25 % | 4/8 |
| quiet deep-age | 7,849 | -0.16 % | 4/8 |

Two nodes have a positive ceiling at a seat we can reach, and the one with both volume and
consistency is **mid-tape one-shot** - where 8dtx and 3Xk2 live.

## The three claims retracted

- "Copying traders is structurally impossible because their margin is under our toll." The toll
  is not the binding term; the sequencing race is, and it is twice the toll.
- "All five solo nodes are closed at our seat." They were read at the FOLLOW seat. Two have a
  positive ceiling at the PEER seat.
- "Only a book whose winners are tens of percent can pay the entrance fee." That followed from
  the same mistake. It remains true that a few-percent edge is fragile; it is not true that it is
  arithmetically impossible.

What survives untouched: the curve physics, the no-absorption corollary, the client gate, the
reachability floor, and the two manufacturing traps closed earlier in the day.

Scripts: `study-kernel/cvx_replay3.py` (and `cvx_hottape_replay.py`, `cvx_replay2.py` kept as the
two wrong versions, each documenting its own bug). Evidence 1.4.
