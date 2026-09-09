# 2026-09-08 - The roster's 77 wallets are ~50 programs, and two of them are provably one operator

Sub-classification of the N4 and N5 rosters
([_!___evidence.md](../../hunter/docs/plans/strategies/_!___evidence.md) 6.1) on a new axis:
**which tokens each wallet picks**, against a popularity-corrected null. The axis was chosen
because the one open edge is token-level, so co-selection asks the question the edge is about.

Source: `census.rb_ep2` (survives the `public.trades` retention cut, which on 09-08 holds only
09-01 .. 09-07). 77 wallets, 170,010 wallet x mint rows, 53,592 distinct mints, universe
91,213 mints over 736 wallets.

Statistic: `lift = obs / sum_t q_a(t) q_b(t)` with `q_a(t) = min(1, n_a k_t / S)`, `k_t` = how
many of the 736 wallets touched mint `t`. Degree correction is required - token popularity is
skewed 1 to 70, so a uniform null overstates lift for the large books.

---

## 1. Program A: 16 wallets, one hash-partitioned work queue

Eight N4 wallets and eight N5 wallets form a perfect bipartite matching on shared tokens, and
have **exactly zero** overlap within each side:

| N4 leg | N5 leg | shared | expected | lift |
| --- | --- | ---: | ---: | ---: |
| BS5ZZp | 3vnqjU | 455 | 33.8 | 13.5 |
| 3TB4Ew | 4nDJMt | 513 | 43.5 | 11.8 |
| BNqfkx | DjQ5U3 | 437 | 30.4 | 14.4 |
| 8GSDbE | FHKhfx | 416 | 32.1 | 13.0 |
| 5HnzXh | AvgzJY | 407 | 29.0 | 14.0 |
| ANQAWc | 6PU45i | 402 | 28.5 | 14.1 |
| 7qaQFE | 5MTFCN | 394 | 28.5 | 13.8 |
| d4uZpX | 8eNTaN | 388 | 28.5 | 13.6 |

All 56 off-matching cross pairs are 0. All 56 within-side pairs are 0, against expectations of
16 to 62 tokens each. All 16 wallets trade on all 8 days of the window at comparable volume, so
address rotation does not explain it.

**The shard key is `ord(mint_address[0]) % 8`: 13,587 of 13,587 mints, zero violations.** Each
of the 58 base58 leading characters maps to exactly one shard at 100 % purity. Shard sizes
1,608 to 1,990.

So: one operator, 16 addresses, the mint space hash-partitioned 8 ways, two wallets per shard -
an early leg (0.32 SOL, entry ~61 s, hold ~13 s) and a late leg (0.24 SOL, entry ~219 s, hold
~18 s). On shared tokens the early leg goes first 68-75 % of the time.

Book: 34,828 trades, 219.6 SOL, per-wallet return on spend 0.76 % to 3.62 %, grades A through D.

## 2. Program B: 2 wallets, a runtime queue

`6sViQ1` and `BwRUYh` (both N5, both 0.99 SOL, both hold 60.1 s, entry 365 / 356 s, vsol
57.2 / 57.4, 2.04 / 2.03 trades per token) share **0 of 3,385** mints against 85.9 expected. No
mint hash tested reproduces the split (best 52.2 %), so the partition is a runtime claim from a
shared queue, not a static shard. 6,766 trades, 137.8 SOL.

## 3. A swarm is not a shard

Twelve N4 wallets share one parameter set (0.77 SOL, entry ~300 s, hold 19.3-19.8 s, tape ~10,
silence 44-50 %) and **do** collide: zero of their 132 pairs is disjoint, median pair shares 44
tokens against 7.9 expected (lift 5.6). Of their 4,167-token union, 71 % is touched by exactly
one wallet and the maximum is 10 wallets on one token. That is a concurrency pool over one
candidate feed, not a partition. 6,279 trades, 114.0 SOL.

The three structures separate cleanly:

| same parameters | shared tokens | reading |
| --- | --- | --- |
| yes | zero, against a large expectation | one program, partitioned queue |
| yes | high | one program, concurrency pool |
| no | high | independent selectors - the only case that is evidence about a token |

## 4. What this corrects

- **`operator_id` in the roster undercounts by a lot.** It is a funding-batch heuristic; it gave
  Program A's 16 wallets 16 distinct ids across both nodes, and Program B's two wallets ids 13
  and 14 in a column that reports `operator_wallets = 1` for each.
- **N4 and N5 are not two answers for these wallets.** For Program A they are two legs of one
  position, split across addresses. The 95 % one-buy-one-sell scope rule - the rule that built
  the roster - is what made a two-leg machine read as 16 independent one-shot traders.
- **`grade` is inside the noise.** Program A's 16 wallets run identical code on a hash partition
  of one token stream and score 0.76 % to 3.62 % return on spend across grades A, B, C and D.
  The 12-wallet swarm spans 6.51 % to -1.24 % on ~520 trades each. Wallet-level margin
  differences below roughly 3 points at 2,000 trades, or 8 points at 500, are sampling noise.
- **Node medians are weighted by copies.** 30 of 77 roster rows are 3 programs.

## 4b. Actor A is a reader, not a volume maker

Its size invites the label. Measured on the coins it trades, 09-01 .. 09-03 (the days
`public.trades` still holds):

| | ACTOR A | the top solo readers |
| --- | ---: | ---: |
| share of prints on its own coins | **0.72 %** | 1.72 % |
| share of SOL | **0.53 %** | 3.15 % |
| median per coin | 1.26 % of prints, 0.75 % of SOL | 1.88 %, 2.51 % |
| other wallets on the coin, median | 115 | 125 |

Gross over the window: **+4.71 %** on 10,203 SOL deployed, 34,828 round trips, 13,565 coins
(+480.7 SOL gross, +219.6 SOL after 125 bps a leg).

A volume maker manufactures the motion: he owns a large share of his token's tape and round-trips
to about zero minus fees, because his pay is creator fees or off-chain. Actor A owns less of the
tape than the readers already named as such and keeps the difference, which is what a reader is.
It is a reader run industrially - a scanner over the whole mint space taking a thin margin an
enormous number of times. Script: [rb-actor-tape-share.py](../../hunter/docs/plans/strategies/rb-actor-tape-share.py).

## 5. What it does not claim

Nothing here is a rule or a candidate for one. It changes what a roster row means: the unit of
evidence is the program, not the address, and only the "different parameters, shared tokens"
edges are independent evidence that a token was worth picking. Those edges are the input to the
token-level question, and they have not been priced.

Outside the tight structures the co-selection graph percolates - above a lift of 4 it collapses
into one 49-wallet component. The roster shares one token pool; only the named structures are
partitions of it.
