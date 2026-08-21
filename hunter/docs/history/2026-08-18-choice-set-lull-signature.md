# What the four scalpers watch: the "lull" signature, and why it is not an edge (2026-08-18)

Prompted by a challenge that the wallet programme kept "doing the same wrong method" and
needed to derive the *logic under the signal* rather than copy landed transactions. The
answer: a new instrument (choice-set AUC), one validated signal family (deceleration), and
a sharp negative -- the family is a loss-avoider, not a return generator.

## The instrument

For every 10-second bucket, reconstruct the **full set of tokens alive at that moment**
(~20-25 in the matched cohort) and measure where the wallet's pick ranked among that set on
~30 features. The mean of a percentile *is* an AUC: 0.50 = indifferent.

Measured **twice**, and the two disagree, which is the point:

- **vs a random live token** -- all four look like momentum chasers (vol AUC 0.74-0.92).
- **vs a random BUY** (flow-weighted by each token's buy count) -- they invert into
  contrarians. This is the correct control, because everyone crowds into active tokens.

Nothing in the design assumes a fill model, fee, latency or exit rule, which is what the
old copy-the-trade method could not avoid.

Anchor is the **last complete 10s bucket before entry**, so no look-ahead. Minute
resolution was tried first and rejected: it matched only 28.9% of `3Xk2`'s picks because
61% of his buys land on tokens younger than 60s. At 10s coverage is 63/80/94/89%.

## What they watch (flow-weighted AUC, in-sample / out-of-sample)

| | buy accel | sell decel | pos in range | prior ret | pool size | buy conc |
| --- | --- | --- | --- | --- | --- | --- |
| `3Xk2` | .32/.32 | .44/.41 | .38/.40 | .40/.39 | **.18/.19** | .58/.60 |
| `8dtx` | .29/.31 | .43/.41 | .28/.32 | .29/.33 | **.15/.14** | .60/.67 |
| `64hP` | .30/.31 | .42/.42 | .31/.32 | .33/.34 | .38/.40 | .46/.52 |
| `omego` | .36/.37 | .45/.39 | .33/.39 | .37/.41 | .57/.58 | .61/.47 |

**All four share one core: they buy into a LULL.** Buying decelerating, selling
decelerating, price near the bottom of its trailing 30s range, price not yet moved. Every
cell holds out-of-sample. The racing pair (`3Xk2`, `8dtx`) add **tiny pools** and
**concentrated buying**; the slow pair sit closer to the average buyer on everything.

All four also **avoid retail-router flow** (retail-share AUC .25-.33) and **favour
nonce-bot presence** (.54-.73) -- they fish where other bots are, not where retail is.
Router class comes from `ix_labels` (Axiom 39%, Pump.Fun direct 29%, GMGN, Photon/Terminal,
Bloom, plus custom bot programs), a participant axis never used before.

## Sell deceleration is REAL, universe-wide, and monotone

`sell_decel` = (sell lamports in last 10s / 10) / (sell lamports in prior 30s / 30).

| decile | 1 (dried up) | 5 | 8 | 10 (surging) |
| --- | --- | --- | --- | --- |
| f30 | **+1.27%** | +0.03% | -4.63% | **-8.20%** |
| f60 | +0.88% | -0.98% | -6.12% | -9.34% |

Monotone across all ten deciles. Among young active tokens it moves forward return from
**-16.0%/-14.7% to -4.5%/-1.7%** (IS/OOS) -- an 11-13pp improvement.

Buy deceleration is **U-shaped and weaker**, so the sharper signal is the sell side.

## But the signature does NOT generate return -- it is a SILENCE filter

Stacking `sell_decel<0.5` + `pos_in_range<0.35` + `vsol<45` + `buy_conc>0.5`:

| conditions | n (OOS) | f60 | median | MFE60 | % up |
| --- | --- | --- | --- | --- | --- |
| 0 | 40,887 | -8.95% | -5.78% | 15.97% | 40.3% |
| 2 | 63,928 | -1.99% | -0.57% | 9.78% | 42.7% |
| 4 | 22,817 | **-0.93%** | -0.40% | **5.17%** | **21.3%** |

Return improves toward zero but **never turns positive**, and the upside dies at the same
rate: MFE collapses 16% -> 5%, share-up 40% -> 21%. It converts a -9% lottery into a -1%
flat line. This is the same mechanism as the price-action refutation -- a silent token
keeps its price. **A trailing exit on this filter earns nothing: there is no excursion to
trail.** The best sub-cell flips sign IS (-0.51%) to OOS (+1.03%) and is noise.

**Age is a red herring.** Their picks are young (154s vs 515s for look-alikes) but young is
the *worst* band universe-wide (<60s: -3.84% OOS; >480s: +1.18%). Age is a correlate of
their strategy, not a driver.

## `3Xk2` has a real selection edge that NONE of these features explain

Forward return from the decision bucket vs the flow-weighted "average buy at that same
moment":

| | median f60 | buyer median | edge | % up |
| --- | --- | --- | --- | --- |
| `3Xk2` | **+4.56 / +11.04** | -8.50 / -6.59 | **+13.1 / +17.6pp** | 55/62% |
| `8dtx` | -2.64 / +3.92 | -7.28 / -8.02 | +4.6 / +11.9pp | 46/54% |
| `omego` | +0.95 / +2.60 | -7.62 / -7.99 | +8.6 / +10.6pp | 52/54% |
| `64hP` | **-13.28 / -11.72** | -9.50 / -9.10 | **-3.8 / -2.6pp** | 35/38% |

Not tail-driven: median, mean-excluding-top-1% and win rate all move together.

Restricted to **look-alike buckets** (same pool size, same sell-deceleration, buying
present), his picks still return **+18.02% mean / +10.20% median vs -1.64% / -2.07%**.
So his information is **not in price or volume at all**.

Excluded as the source: pool size, age, activity level, buy and sell deceleration, position
in range, buy concentration, router mix, nonce share, volatility, churn.

## `64hP` has no selection edge -- three independent lines agree

- Selection is **negative** (-2.6 to -3.8pp vs the matched buyer baseline).
- **No racing infrastructure**: `3Xk2`/`8dtx` buy under a durable nonce (`8dtx` on both
  legs); `64hP` and `omego` use none.
- Completed round trips are ~flat: net of fee, **+0.30%** first entries / **+1.86%**
  re-entries, with the entire -2.50% book coming from the ~4% of positions never exited.

Related: **AMM is not a missing venue** -- 4 AMM trades across all four wallets over the
whole window, so the curve-only book is the real book, and no fee tier rescues `64hP`
(+0.63% gross on 73,700 SOL needs a sub-31 bps/leg fee).

## The reconciliation: real information, unreachable execution

Over the 4 study days `3Xk2`'s **realized** PnL is +0.11% on seen picks and -0.69% on
unseen, i.e. flat -- while those same picks rose a median 10-18% over the following 60s.
The selection information is real and large; he does not capture it. This confirms the
+9.87% one-slot entry slippage from the other direction.

`8dtx` is the mirror: his **sub-10s** entries (invisible to a 10s grid) return **+4.02%**
against +0.23% for the rest. His edge lives in the window this instrument cannot see.


## Multivariate: his FOOTPRINT is a losing profile (this is why clones fail)

A `HistGradientBoostingClassifier` on 40+ features (levels, derivatives, cross-sectional
ranks, flow-weighted ranks) predicts "is this a `3Xk2` bucket" at **OOS AUC 0.862**
(`8dtx` 0.894). His *behaviour* is highly predictable. But the buckets it scores highest
are far worse than random:

| OOS slice | n | mean f60 | median | contains his picks |
| --- | --- | --- | --- | --- |
| top 0.01% | 41 | **-25.17%** | -20.80% | 0.00% |
| top 1% | 4,014 | -12.64% | -19.94% | 1.20% |
| all | 401,365 | -2.67% | -0.07% | 0.11% |

And within the model's own top slice his real picks diverge completely from the look-alikes
it cannot distinguish them from:

| OOS stratum | his picks | look-alikes |
| --- | --- | --- |
| top 1% | **+9.03%** (med +7.97%, 56% up) | **-12.91%** (med -20.09%, 30% up) |
| top 5% | +12.75% | -11.55% |
| top 10% | +15.26% | -10.53% |

**Any rule fitted to his observable footprint selects the losing look-alikes.** That is the
mechanism behind every failed clone, and it predicts future ones will fail the same way.

## The value is REAL and CONVEX, measured from his own fill

| | decision->fill slip | ret from FILL | median from fill | MFE from fill |
| --- | --- | --- | --- | --- |
| `3Xk2` IS/OOS | +4.96 / +7.28% | +4.24 / +10.85% | **+0.28 / +2.21%** | **+36.79 / +38.55%** |
| `8dtx` IS/OOS | -3.79 / +0.73% | +4.20 / +8.13% | -1.55 / -0.72% | +33.08 / +34.42% |

Half the move is gone before he fills, but MFE of 33-39% remains. **Median ~0** -- purely
convex, which is why his book is +2.62%, why a TP destroys him, and why the wide trail is
load-bearing. Note the universe-wide lull filter kills MFE (16% -> 5%); **his picks do
not** (MFE 37%). Excursion is the thing his selection actually buys.

## Identity: a REAL conditioning variable, but NOT tradeable

Roster = 155 wallets that bought the same mint within 60s **before** his entry, selected on
days 1-12 with lift >= 3 against `64hP`'s entries as the matched control population.
Evaluated on days 13-18:

| | no roster | roster present |
| --- | --- | --- |
| his OOS trades | -0.28% (n=1289) | **+4.68%** (n=1455, +50.7 SOL) |

**Survives both controls.** Stratified by precursor-wallet count it separates in every band
(1-5: +23.40 vs +2.52; 6-15: +4.03 vs +2.20; 16-40: +6.73 vs +0.50; >40: +1.13 vs -5.93),
so it is not "more buyers were there". A **placebo roster** of 155 wallets matched on
universe buy count but not selected on him shows nothing (+0.88 present vs +2.91 absent).

**But it is not a strategy.** Buying every bucket a roster wallet buys returns **-7.11%**
(median -7.11%, 26% up) vs -3.94% for neither -- worse than random. And inside the model's
top 1% it only lifts look-alikes from -13.71% to **-2.77%**: an 11pp interaction that still
does not clear zero, let alone the 2.47% fee.

**Conclusion: the roster conditions HIS edge; it does not reproduce it** -- because the
roster wallets are co-racers of the same slot-level impulse identified in the next section,
not leaders to follow.


## THE TRIGGER IS A SLOT-LEVEL BUY IMPULSE -- the 10s grid smeared it away

Everything above concludes "not in on-chain data **at 10s resolution**". That
qualification carried the whole answer. Re-measured per SLOT, anchored on the entry slot:

**Buy SOL per entry, by slot offset before the entry** (18 clean days, other wallets only):

| offset | -12..-3 (flat baseline) | -2 | **-1** |
| --- | --- | --- | --- |
| `3Xk2` buy | ~0.33-0.38 | 0.394 | **0.736** |
| `3Xk2` sell | ~0.24-0.31 | 0.338 | 0.309 (flat) |
| `8dtx` buy | ~0.12-0.15 | 0.151 | **0.610** |
| `8dtx` sell | ~0.18-0.22 | 0.218 | 0.206 (flat) |

A sharp, isolated, **single-slot spike in BUY volume only** -- 1.9x for `3Xk2`, 4.0x for
`8dtx` -- in the slot immediately before they fire. Sell flow does not move. The ten
preceding slots are flat. **They react to a large buy landing in slot S-1 and fire to land
in slot S.**

A 10s bucket spans ~25 slots, so a single-slot 2-4x impulse becomes a 5-10% bump in the
bucket aggregate -- below noise. **The feature grid averaged the signal away.**

Confirmed from the other side. Buy flow AFTER the entry, per slot:

| offset | same slot | +1..2 | +3..5 | +6..12 | +13..37 | +38..75 | +76..150 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `3Xk2` buys/slot | **6.97** | 1.48 | 0.72 | 0.56 | 0.42 | 0.32 | 0.23 |
| pct nonce-bot | **61.0%** | 37.0 | 23.0 | 20.0 | 16.9 | 15.5 | 14.9 |
| pct retail-UI | 26.5% | 28.5 | 50.8 | 59.8 | 67.8 | 67.8 | 66.1 |

A ~30x same-slot spike that decays monotonically, **61% nonce-bots** at larger size (0.52
vs 0.41 SOL), with retail arriving only later. That is a **crowd of latency bots all firing
on one visible trigger**, not one wallet with private information -- which is why the
identity roster conditions his PnL without being tradeable: those wallets produce and race
the same impulse.

This reconciles every earlier result: the GBM saw nothing because the trigger is one slot
wide; sell-deceleration matters because a buy impulse into a book with no selling is what
moves price; latency is decisive (+9.87% at one slot) because the whole game is landing in
slot S, and this bot lands at S+1.


### ...but the impulse is NOT tradeable at any reachable latency

Universe-wide, 2 days (IS 08-02 / OOS 08-15), 658,483 slot-rows. Impulse = slot buy volume
over the trailing 30-slot mean; return measured from the NEXT slot's close, i.e. what a
reactor actually gets:

| impulse | n (OOS) | r30 | r60 | MFE60 | % up |
| --- | --- | --- | --- | --- | --- |
| <2x | 334,107 | -2.85% | -5.24% | 26.75% | 42.5% |
| 2-4x | 31,018 | -1.75% | -4.21% | 26.81% | 45.0% |
| 4-8x | 13,547 | -1.23% | -2.93% | 25.03% | 44.3% |
| 8-16x | 5,622 | -0.44% | -2.16% | 23.76% | 43.7% |
| >=16x | 3,149 | -0.16% | -0.80% | 22.42% | 41.9% |

Monotone and informative, but **never positive**, and the medians flip sign IS/OOS.
Adding the wallets' own conditions does not rescue it:

| signature | n (OOS) | **r1 (1-3 slots)** | r30 | r60 |
| --- | --- | --- | --- | --- |
| impulse>=4x & sell quiet | 18,889 | **+1.00%** | -1.00% | -2.60% |
| impulse>=4x & sell rising | 3,429 | +1.32% | -0.25% | -1.51% |
| no impulse | 365,125 | +0.42% | -2.75% | -5.15% |

There IS a real, IS/OOS-stable **~+1% pop within 1-3 slots** (+0.89 / +1.00), which decays
negative by +30 slots. Against the 2.47% round-trip fee it does not clear. **The entire
value of the trigger is consumed inside the slot it fires in.**

**Therefore: copying `3Xk2`/`8dtx` is CLOSED, and now for a mechanical reason rather than an
empirical one.** The trigger is visible, cheap and on-chain; the payoff lives in one slot;
this bot lands at S+1 at p50. Consistent with `3Xk2`'s own +9.87% one-slot entry slippage
and with his realized PnL being flat (+0.11% / -0.69%) over the study days while the move he
chases carries 37% MFE.

**One mechanism covers both wallets and both age regimes.** `8dtx`'s sub-20s entries -- the
ones worth +4.02% against +0.23% for the rest -- show the SAME slot-1 buy impulse (0.285 ->
0.815 SOL, 2.9x) with the same flat sell side, as his older-token entries (0.134 -> 0.584,
4.4x). They are not a second signal; they are the same trigger on younger tokens, where
baseline flow is 3-4x heavier and the payoff is larger.

**Consequence for the programme:** the signal is on-chain and cheap to compute, but it is
*reactive at slot granularity*. Any feature grid coarser than one slot cannot see it, and
any execution path slower than same-slot cannot trade it. Off-chain sources (social,
trending boards) are NOT needed to explain these two wallets.

## Refuted here

- **Identity roster as a followable signal.** Real as a conditioner (above), refuted as a
  strategy: -7.11% standalone. An early 4-day pass showed lifts of 100-400x purely because
  the control was unmatched -- against a universe base rate they collapse to 1.5-23.
- **Buy deceleration** as the generalisation of sell deceleration: U-shaped, weaker.
- **Age** as a driver.

## Data

PG schema `cs` on 4 clean days (IS 08-01/08-02, OOS 08-14/08-15): `f1` (mint,10s,wallet),
`fg`/`ff` (features), `fr`/`rw` (cross-sectional ranks), `fwd` (forward returns), `fpick`
(23,910 anchored picks), plus the 1-minute variants `s2`/`gr`/`gf`/`rk2`. Drop when done.

**Standing gate added:** quote every cross-sectional AUC **flow-weighted** as well as raw.
The raw view said "momentum chaser" and the flow-weighted view said "contrarian" for the
same wallets on the same data; only the second controls for the crowd.
