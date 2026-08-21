# Round 7 (2026-08-20): the fresh-wallet rule forward, and the venue-state gate

Two items from the open queue, run together on the fresh day 2026-08-19:

1. forward validation of the fresh-wallet entry rule, the one rule that clears the fee bar;
2. the venue-state gate - the first candidate that selects *when to trade* instead of
   *which token*.

Both come back negative. Neither is a clean refutation of the kind round 6c produced, and
the distinction matters, so it is stated explicitly below.

## The engine had to be rebuilt before either number meant anything

The published rule numbers came from a numpy engine (`exit5.py` / `verify5.py`) that no
longer existed in this session. A first SQL re-implementation of the same written spec -
arm 8 / trail 4 / TP 15 / timeout 300, fill at the next print past the trigger, cost
`0.0253 + 4*sqrt(F/vsol)` - produced **+1.95%** against a published **+3.68%**, on a
selection whose token count matched to 5,980 of 5,982 and whose per-day `c_fresh1h`
cutoffs matched to three decimals. The selection was identical; the exit engine was not.

Two conventions carry the whole 1.7pp:

- **The mark is `px_sell_med` alone, and slots with no sell print are dropped from the
  path entirely** - not coalesced to the buy median. A buy-only slot is an up-tick nobody
  could have sold into; marking it raises the running max, moves the trail trigger onto a
  price that was never available, and books an exit fill at a buy-side price.
- Entry reference is `e_px * (1 + bsz/e_vsol)` and the exit multiplier is
  `(1 - bsz/e_vsol) * 0.9875/1.0125`, with the fixed leg cost as `1 + 2F/bsz` - the same
  quantity as `0.0253 + 4*sqrt(F/vsol)` at `bsz = sqrt(F*vsol)`, but applied
  multiplicatively rather than additively.

With those two fixed the replication is exact: **+3.67% per token, IS +3.32, OOS +4.21,
n = 5,975** against the published +3.68 / +3.34 / +4.21 / 5,982. Every number below comes
from that verified engine, run identically on both windows.

## 1. The fresh-wallet rule does not clear on the fresh day

The fresh day is rebuilt end to end at the pool level: `iv.wide9` reproduces the pool
definition exactly (5,659 rows / 3,013 mints on 08-19 against 5,629 rows / 2,892 mints per
day over the eight backtest days).

`c_fresh1h` is recomputed directly from `trades` joined to wallet first-seen, skipping the
`ws`/`wd`/`tk` chain, and validated against the stored column on a 400-mint sample:
**r = 1.000000, max absolute difference 0**. `iv.wfs` had to be backfilled first - it was
built when the tape ended 2026-08-18 21:03, so 39,139 wallets whose first-ever trade falls
after that were missing entirely and would have read as *not* fresh.

Selection uses the rule's own live recipe: the 80th percentile of the pool per age band
over the **last seven sealed days**, applied blind. Those cutoffs are 0.9351 (age < 25
slots) and 0.2905 (25-74 slots), matching the rule document's "about 0.93 and about 0.30".

| | tokens | net | CI95 | P(>0) | median | win |
| --- | --- | --- | --- | --- | --- | --- |
| pool, 08-19 | 2,931 | **-0.80%** | [-1.88, +0.35] | 8% | -4.53% | 31.8% |
| rule, blind cutoffs (live form) | 716 | **-0.34%** | [-2.43, +1.94] | 37% | -5.47% | 32.4% |
| rule, own-day ranking (backtest form) | 633 | +0.30% | [-2.00, +2.71] | 60% | -5.16% | 33.5% |
| pool, 8 days | 23,073 | +0.53% | | | | |
| rule, 8 days | 5,975 | +3.67% | | | | |

The placebo tells the story more sharply than the mean does. Against 200 random same-size
draws from the fresh-day pool the rule scores **z = 0.38**, beating 66% of draws. The
in-window figure is z = 9.46. On 08-19 the selection carries no measurable information.

Reading it against the venue removes the "bad day" defence. The **edge** - rule minus pool,
same day, same engine - is stable across the eight backtest days and collapses on the ninth:

| day | rule | pool | edge |
| --- | --- | --- | --- |
| 08-11 | +2.74 | +0.09 | +2.66 |
| 08-12 | +4.13 | +2.18 | +1.95 |
| 08-13 | +4.44 | +0.86 | +3.58 |
| 08-14 | +1.66 | +0.35 | +1.31 |
| 08-15 | +2.66 | -0.24 | +2.90 |
| 08-16 | +1.24 | -0.65 | +1.89 |
| 08-17 | +8.41 | +3.12 | +5.29 |
| 08-18 | +0.74 | -2.37 | +3.11 |
| **08-19** | **-0.34** | **-0.80** | **+0.46** |

Eight-day edge: mean +2.84, sd 1.24, minimum +1.31. The fresh day sits **1.92 sd below the
mean and below the eight-day minimum**. The own-day-ranked variant reaches +1.10, still
below that minimum.

**The live form is the worse form.** `c_fresh1h` drifted in the older age band - its 80th
percentile moves from 0.291 over the sealed week to 0.602 on 08-19, while the young band
barely moves (0.935 to 0.968). The stale cutoff therefore selects roughly 40% of the older
band instead of 20%, and costs about 0.64pp (blind -0.34 against own-day +0.30). The
nightly-refresh recipe in the rule document is exactly what produces this; the drift is
faster than a seven-day window tracks.

**This is not a clean refutation, and it is not a confirmation.** One day of a rule that
wins 32% of its trades and books 6.4pp of its mean in the top 10% of tokens has a CI of
about +/-2.2pp; -0.34% is inside the eight-day CI of [+2.01, +5.54] the same way it is
inside zero. What can be said is exact: on the only genuinely later day available the rule
does not clear the fee bar, its selection is not distinguishable from a random draw of the
same size, and its venue-controlled edge is the worst of nine days. Nothing here supports
capital. The rule needs more genuinely later days, and the honest cost of getting them is
that they arrive one per day.

Intraday the failure is not uniform: the edge holds in the 06-11 UTC block (+3.07) and is
absent or negative in the other three. See the venue-state section, which reaches the same
place from the other direction.

## 2. The venue-state gate is refuted at its root

The hypothesis: every study so far selects tokens, and the trade reduces to `P(arm)` whose
break-even is 73.5% against 65-73% delivered. Per-token features move `P(arm)` but degrade
the payoff in lockstep. A variable that is **not a property of the token at all** might move
`P(arm)` without touching the payoff - and being structural rather than identity, it cannot
rotate away the way round 6c's pattern books did.

Six trailing venue-state variables are built at one-minute resolution over all nine days
from the same slot tables the entries come from (`iv.vm` / `iv.vs`), each a strictly
trailing five-minute window with no lookahead: live mints, new launches, venue buy SOL,
venue net-flow share, mean buy size, and hour of day. They are joined to two pools - the
105,255-entry D->L->I chain superset and the 26,001-token fresh-wallet pool.

**`P(arm)` is invariant to venue state.** That is the whole result.

| pool | corr(venue var, armed), worst of six | arm rate range across all buckets |
| --- | --- | --- |
| chain superset | 0.013 | 71.4% - 72.6% |
| fresh-wallet pool | 0.013 | 38.4% - 40.8% |

The mechanism the idea depended on does not exist. Consistently, chain net is flat too:
all 30 single-variable buckets land between -1.56% and -1.94% against a -1.74% pool, and
an exhaustive two-way search returns **0 of 329 cells (n >= 300) positive across IS, OOS
and the fresh day** - the same count round 6e returned for the ix-by-chain search.

The fresh-wallet pool does show spread, and it is noise: every best-in-sample cell flips
out of sample (venue-buy q1 +1.58 / -0.39 / -1.50, live-mints q1 +1.56 / -0.13 / -1.20,
new-launches q1 +1.02 / +0.21 / -1.95).

**One lead survives, and it is a lead only.** On the rule selection, the 00-05 UTC block
runs +5.02% against +3.12% for the rule as a whole and is positive on **9 of 9 days,
including the fresh day** (+0.62 where the rule books -0.34):

| cell | n | net | IS | OOS | FRESH | n on 08-19 |
| --- | --- | --- | --- | --- | --- | --- |
| the rule | 6,500 | +3.12 | +3.15 | +4.17 | -0.34 | 716 |
| hour 00-05 UTC | 1,458 | +5.02 | +6.26 | +4.55 | +0.62 | 181 |
| venue-buy q1 | 1,300 | +4.61 | +5.60 | +4.28 | +1.14 | 128 |
| both | 547 | +7.36 | +7.61 | +7.23 | +6.69 | 42 |

Against 500 random same-size subsets of the rule the night cell scores z = +2.27 over the
nine days but only **z = +0.45 on the fresh day itself** (n = 181). It is one cell chosen
from thirty, its fresh-day sample is a quarter of an already-underpowered day, and the two
conditions are only weakly the same thing (corr +0.24, 38% overlap). It says the rule's
fresh-day failure is concentrated in the busy hours. It does not say the quiet hours pay.

## What these two results have in common

Both attack `P(arm)`, from opposite ends, and both find the same wall. The chain arm rate
is 72% regardless of how busy the venue is; the fresh-wallet rule arm rate is 40%
regardless. Nine rounds of features - level, flow, stock, identity, breadth, concentration,
instruction shape, and now venue regime - move net by at most about 1pp against a 3.45%
bar, and the one rule that beat the bar in-window does not repeat on the first day it has
not seen.

Scratch built here, safe to drop: `iv.p9`, `es9`, `cf9`, `wide9`, `q8`/`o8`, `q9`/`o9`,
`fs9`, `pf8`/`pg8`/`pg8b`/`rw8`/`rw8b`, `pf9`/`pg9`/`rw9`, `vsm`/`vchk`, `cut9`, `tb9`,
`vm`/`vs`/`va`/`vb`. `iv.wfs` is *modified*, not scratch - 39,139 rows backfilled, and the
backfill is correct for every consumer since first-seen is a minimum.
