# Axiom push node: refuted (census-wide and at 8dtx)

Question: does an Axiom operator returning to a mint ("Axiom appears several
times, then again") mark a restarted push we can ride, and is that what the
roster reader 8dtx trades. A push = all Axiom buys landing in one slot on one
mint. Window 08-30 17:48 .. 09-04, tables `census.axiom_builds`, `axiom_push`
(1,024,921 pushes / 36,650 mints), `push_path` (20k stratified sample),
`push_money`, `dtx_buys` / `dtx_pnl`. Seat: entry at the first print of push
slot + 1 (~100 ms reaction); fills per
[campaign-break-money.md](campaign-break-money.md).

## Axiom is a brand; two of its builds are machines

The mass builds (`cd05b71f`, `ccbcefe3`) are the terminal crowd: 34k / 15k
mints, 25-35 buys per mint, 30% multi-tx slots, break silences at 5-7% of
pushes. Two builds are volume machines: `8be2f7b0` (73k buys, 1,167 mints,
63 per mint, fixed 0.69 SOL, wallet rotated: 13,085 wallets for 29,359 buys,
0.2% of pushes break a silence - it lives inside busy tape) and `f93f023b`
(same shape, 236 mints). `payer_id` equals `wallet_id` on every Axiom build,
so the payer axis carries nothing here.

## Push recurrence raises the up-move and the drawdown faster

Median path from the slot+1 entry (price terms, 600 s):

| stratum | n | med up | med down | P(up > 8%) |
| --- | --- | --- | --- | --- |
| silence-breaking push | 4,958 | +11% | -22% | 55% |
| crowd, 1 tx | 5,000 | +31% | -49% | 75% |
| crowd, >= 3 tx | 4,993 | +32% | -54% | 76% |
| machine `8be2f7b0` | 4,990 | +40% | -75% | 81% |

Push index (the "appears again" count) lifts P(up > 8%) on silence-breaking
pushes from 30% (first push) to 63% (>= 32nd) while the median drawdown goes
-8% to -27%; the machine's build index does the same (+43% / -61% at index 1 to
+33% / -83% at >= 32). No cell of index x curve band x tx count has median up
above median down. Windowed flow before the push (gross and net, 30 s / 60 s)
scales up and down together: gross 60 s < 1 SOL gives +2% / -6%, >= 30 SOL
gives +36% / -61%, and the net sign leaves the ratio unchanged. Flow measures
volatility, not direction.

## Pre-registered money (one pass, no tuning)

0.05 SOL, TP +40% price resolved on prints else 600 s clock, e_vsol in [45,85):

| cell | trades | mints | net SOL | win | days red |
| --- | --- | --- | --- | --- | --- |
| A: silence-breaking push, index >= 4 | 22,980 | 3,223 | -109.83 | 41.4% | 5 of 5 |
| B: machine `8be2f7b0`, build index >= 4 | 6,206 | 735 | -42.67 | 56.1% | 5 of 5 |

Cell B hits the TP on 56% of trades and still loses: the failures are dumps.
Money worsens with the index. The node is closed at our seat; no holdout is
owed for a red derivation.

## 8dtx trade profile against Axiom and flow (descriptive, his own fills)

2,374 buys in the window, 820 wins / 1,554 losses by his own sells (+172.7 /
-74.4 SOL, +98.3 net gross-of-fee), median clip 0.67 SOL, builds `c1e49c00` /
`634f5fad` (AdvanceNonce + CU price first). Tables `census.dtx_state`
(entry state), `dtx_post` (first 30 s after), `dtx_base` (6,825 same-mint,
same-band moments he did not buy).

**Entry state does not separate his wins from his losses.** Win and loss
quartiles coincide on every Axiom-history metric (pushes on the mint before
him 14 vs 13 median; Axiom buys in his slot before him 0.38 vs 0.36 mean;
Axiom buys in the 5 / 10 / 30 / 60 s before: 0/0/1/3 median both; Axiom SOL
60 s 0.88 vs 1.11; seconds since last Axiom buy 3.8 vs 4.2) and on every flow
metric (gross 30 s 9.7 vs 10.9; net 30 s 0.77 vs 0.95; gross 60 s 19.0 vs 20.6;
net 60 s 3.0 vs 2.5; buy SOL 10 s 2.9 vs 3.3; buyers 60 s 26 vs 27; others
before him in slot 1 vs 1; vsol 41.3 vs 41.2). Win rate per bucket sits at
28-41% with no ordering on any of them.

**What he selects (his entries vs moments he skipped on the same mints, same
35-50 band):** not Axiom (Axiom buys 10 s before: mean 2.9 vs 4.2; 60 s: 17 vs
16). He enters on a calmer, more balanced tape than the mint's typical moment
(net 30 s median 0.87 vs 2.98 SOL; net 60 s 2.68 vs 5.29; gross 30 s 10.5 vs
13.8; max single buy 2.0 vs 2.0) and lands inside a burst slot (others before
him in-slot mean 1.47 vs 0.83).

**The outcome is decided after he is in.** Winners see net +1.16 SOL in the
first 5 s and +2.15 in 10 s (max price +14.3% within 10 s); losers see net
0.00 / -0.13 (min -3.75%). Axiom buys in the 10 s after him: winners median 1,
losers 0 - Axiom is part of the follow-through crowd, not the trigger.

**His P&L is his hold rule.** Sells under 15 s: 1,116 trades, 5-13% win,
-51.3 SOL. Sells over 60 s: 375 trades, 78-81% win, +121.8 SOL. He cuts a
non-follow-through in seconds and rides a follow-through for minutes.

## Consequence

"Axiom appears again" is attention recurrence, not one operator's plan: it
predicts activity in both directions. The reader we measure does not key on
it before entry; it appears after his entry as part of the crowd that pays him. The Axiom brand is not a decision-node signature; a decision node
needs a build that fires alone, returns to few mints, and precedes a
median-positive path - the shape `29d9aacb` has and no Axiom build has.
