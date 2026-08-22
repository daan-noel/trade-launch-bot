# 2026-08-22 — the impulse-inception island is a same-slot fill artifact

The island rule of [`plans/strategies/impulse-inception-island.md`](../plans/strategies/impulse-inception-island.md)
does not survive execution. Its entire measured edge sits in the interval between the
trigger print and the next print, and on the trades that carry the money that interval is
**half a millisecond wide, inside the same slot**. Both the authorable rule and the
instruction-structure ("ix cut") variant that was going to be built on top of it are dead.

## The measurement

Same decision points, same rule, same exit, same leave-one-day-out blacklists. Only the
fill instant moves. Fill convention is the kernel's: you get the print *after* you arrive.

| target lag | SOL | expectancy | days + | median realised fill |
| --- | ---: | ---: | ---: | ---: |
| next print | **+138.22** | +0.001254 | 7/7 | 0.137 s |
| +50 ms | −22.30 | −0.000202 | 1/7 | 0.347 s |
| +100 ms | −50.98 | −0.000461 | 1/7 | 0.458 s |
| +200 ms | −73.21 | −0.000663 | 0/7 | 0.682 s |
| +400 ms | −90.08 | −0.000821 | 0/7 | 0.984 s |

The base rule without the ix cuts behaves the same way, only worse (+114.33 → −54.21 at
+50 ms), so this is a property of the island, not of the cuts.

Re-running the search **from scratch** at a +100 ms fill finds nothing: not one single
threshold on `net_flow(0.4)`, `rise(3)`, `liquidity` or the creation-bundle axis is
positive, in either direction. The whole decision-point universe is −0.0016 SOL/trade
there, which is the round-trip cost. Fading the impulse instead of buying it is −847 SOL.

## Where the money actually is

Splitting the rule's own 110,195 trades by the gap from the trigger print to the next
print — the width of the window in which we would have to arrive:

| gap | n | SOL | expectancy |
| --- | ---: | ---: | ---: |
| 0–10 ms | 30,877 | **+131.26** | +0.004251 |
| 10–25 ms | 3,050 | +7.40 | +0.002425 |
| 25–50 ms | 6,771 | +8.54 | +0.001261 |
| 50–100 ms | 9,515 | +7.61 | +0.000800 |
| 100–400 ms | 24,622 | +6.85 | +0.000278 |
| > 400 ms | 35,305 | −24.22 | −0.000686 |

**95% of the book is trades whose next print arrives within 10 ms.** Those pairs are
**100% same-slot** (median 0.49 ms apart on the feed clock, verified over 604,297 pairs on
2026-08-17). `block_time` in the lake is the ingest clock, so a sub-millisecond gap means
the two trades reached us in one delivery — the second had already executed on chain
before the first was visible to any consumer. No send path puts a transaction between
them.

## The rule this produces

**Price the fill before ranking anything.** A first-in-window backtest answers "what would
this be worth if we were the next transaction", which is a different question from "what is
this worth". The two only agree where the tape is slow, and a rule search maximising the
first will walk straight to the region where they disagree most — the densest bursts —
because that is where the unearned fill is worth the most.

Concretely, for any future entry search on this corpus: report the gap-to-next-print
distribution of the selected trades alongside the PnL. If the money concentrates below
~50 ms, the rule is an artifact and the number is not real.

## What survives

The instruction-structure work is the one part that holds up, and it should be reused
rather than rediscovered. Blacklisting creation / trigger-print / impulse-driver ix
structures is worth +23.89 SOL and +46% expectancy leave-one-day-out, 7/7 days, stable
across folds (11 of 15 `ix_top_buy` ids appear in all 7), and robust across three
different exits. It is a real discriminator — it was simply improving a rule that does not
exist at an achievable fill. See
[`2026-08-22-ix-structure-cuts.md`](2026-08-22-ix-structure-cuts.md).

Two method corrections also came out of it and are recorded there: the original cut sets
were selected using the days they were graded on, and `ix_launch` ("the biggest trade in
the token's first slot") conflated the launcher with the first sniper and is fully
subsumed by the creation transaction's own `ix_labels`.
