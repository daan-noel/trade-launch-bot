# 2026-09-08 - the edge is token-level, it is not the swarm, and it is not yet reproducible

Follow-on to [2026-09-08-convexity-is-reachable-selection-is-the-gap](2026-09-08-convexity-is-reachable-selection-is-the-gap.md).
Same week (09-01 .. 09-07, 11.76 M prints, 115,646 tokens), same kernel, both legs at 115 ms, re-entry
unlimited, tickets rather than tokens, funnel printed before every number.

## The finding that changes the shape of the problem

Using the three N4 wallets purely as an oracle - never as a rule term - and pricing **every** candidate print
on a token rather than the ones they took:

| population (candidate = router buy >= 0.5 SOL, age 10-900 s, reserve 33-60) | 45 s clock | 30 % trail, 600 s |
| --- | ---: | ---: |
| tokens they never touch, all fires | -6.43 %, 0/7 days | -9.62 %, 0/7 |
| tokens they touch, all fires | +1.04 %, 6/7 | +2.96 %, 7/7 |
| ... the prints on those tokens they SKIPPED | +0.91 %, 6/7 | +2.92 %, 7/7 |
| ... the prints they TOOK | +2.68 %, 6/7 | +3.49 %, 5/7 |

Their moment-picking is worth about two points; **their token-picking is worth about ten**. Once the token is
right, any candidate print works and re-entry is free.

## It is not their price impact

[reader-token-profit-is-swarm-arrival] warns that "positive on the reader's tokens" is usually the window
containing their own arrival. Splitting on that, on a 45 s clock:

| fires on their tokens | result |
| --- | ---: |
| trade still open when they land | +8.04 % a trade, 7/7 days |
| trade opens AND CLOSES before they land | +2.30 %, 6/7 days, n=4,428 on 1,632 tokens |
| ... fired 60-300 s ahead of them | +2.97 %, 7/7 days |
| ... fired 300 s+ ahead of them | +2.77 %, 6/7 days |
| fires after they have bought | -1.56 %, 1/7 days |

Riding their arrival is the known artifact and it is worth 8 points. What remains after removing it is still
positive on 6 of 7 days: the token was already good minutes before any of them appeared.

## What does not reproduce it

* **The holder book**, reconstructed from our own prints - holders, top-1 and top-5 share, HHI, bundle share,
  never-sold share, fresh-wallet share, dev share, supply sold back. Every field, every octile, every exit
  negative; the best single cut is -2.3 %. It does not separate their picks from their skips either.
* **Token-level supervised selection.** 32 decision-time features at the token's first candidate print -
  creation machinery, first-minute tape, holder book, metadata, plus a new **meta-cluster** feature counting
  how many tokens launched in the prior 24 h share this one's symbol or lead name word. Fitted on days 0-3
  the top decile books +6.06 % a trade on 4 of 4 days; on days 4-6, which the model never saw, the same top
  decile books -3.63 % on 0 of 3. It learns the week, not the market.
* **Professional convergence.** Counting distinct *private* buy builds (<= 50 wallets, >= 200 prints - one
  operator's own tool rather than a retail terminal) already in the token gives a clean monotone gradient,
  -7.4 % at none through -2.7 % at nine or more, and never crosses zero.
* **Coverage.** The full age x reserve grid, 10 s to 24 h and reserve 33 to the wall, 49 cells x 6 exits:
  no cell with 300+ fires is positive. Money improves toward old-and-mid-reserve (-1.0 to -2.5 %, close to
  the toll) and is worst on young shallow tokens.

## Where this leaves the search

A token-level property exists that is worth roughly ten points a trade against the pool, is not the swarm,
and is visible to at least three independent operators minutes before they act. Roughly forty decision-time
features drawn from the price tape, the machine census, the holder book and token metadata do not contain it.

The two untried axes, in order of promise:

1. **The metadata document itself.** `tokens.meta` stores the URI; nothing fetches it. That JSON carries the
   description and the twitter / telegram / website links, which is exactly the retail-attention filter every
   terminal shows a human. Static IPFS content, no Helius budget, and it is the one panel field still missing.
2. **A wider oracle.** Three wallets give 2,455 positive tokens a week. The daily-profitable population is
   about 600 wallets; labelling with all of them yields a far larger positive set and makes the token-level
   classification learnable, with the same before-arrival honesty split to keep the swarm out.

Scripts: `study-kernel/` - `holderbook.py`, `analyze_hb.py`, `tokenpick.py`, `convergence.py`, `grid.py`.
