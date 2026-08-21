# Wallet `FBvx` (2026-08-20): a profitable intra-slot absorption bot, unreachable at +1 slot

`FBvxneTq8dY7WKxj924CseuveWzDL5tN9JuSW3S9nJkN`, `wallet_dict.id` 1092149. 51,414 closed
episodes over 22 days (2026-07-30 .. 2026-08-20), 33,850 distinct mints, 33,885 SOL cycled.

The first study wallet that is **net positive on every single day** and whose payoff is not a
lottery. It is also the cleanest measurement yet of why that does not help us: the entire
edge is a **+6.15% median price gap that opens and closes inside one slot**.

## Verdict

| | |
| --- | --- |
| net of the 125 bps/leg fee, before tips | **+398.2 SOL**, +1.175% per SOL cycled |
| days positive | **22 / 22** |
| mean / median episode | +1.285% / **-2.469%** (= the fee exactly: the median round trip is flat) |
| win rate | 39.4% |
| break-even average tip | **0.00387 SOL/leg** - not checkable, `raw_txs` is empty |
| same book at **+1 slot** entry latency | **-2.44%, 0 / 22 days positive, -937 SOL** |

Not a dev volume wallet, and the question is closed on four independent counts: he creates
**0** tokens; his 33,850 mints span **11,104 distinct creators** with the top ten at 13.4%;
he round-trips against the bonding curve where there is no counterparty to wash with; and a
volume-painting wallet is a cost centre while this one **makes money every day**.

## Mechanics

Strictly **1 buy -> 1 sell**, never laddered, never re-entered inside an episode. 0.26% of
episodes end bagged. No AMM leg ever - he is gone long before graduation.

- **Size** fixed ~0.6 SOL, tiers at 0.4 / 0.6 / 1.2 / 1.8 SOL gross. 9.5% of buys land on
  exactly `1.1852 = 1.2 * 10000/10125`, which re-confirms the 125 bps fee off his own tape.
- **Entry** median 8.3 s after creation (p25 2.0 s, p95 76 s), decision-slot depth `vsol`
  median 44, tightly banded 35-70.
- **Hold** median **1.5 s** (2-3 slots), hard timeout at **25 slots / ~10 s** (p99 10.26 s).
- **Exit is ONE trailing stop with `peak` initialised at the entry price.** Retrace at exit
  is flat at **-8% median / -11 to -12% mean across every MFE bucket** (-11.17 at MFE<0
  through -14.92 at MFE>40%). No take-profit: the >40% bucket still books +51% mean. He
  deliberately leaves the right tail open.
- **He wins the race out.** At his exit slot he is rank 2.72 with ~1 sell ahead of him and
  ~1.1 after - he is near the front of the dump, not reacting to it.

Infrastructure is professional and its split is informative: `AdvanceNonceAccount` on 100%
of legs, `CreateAccountWithSeed` + `InitializeAccount3` on the buy and `CloseAccount` on the
sell to recycle rent across 51k round trips, and a Jito-style `System Program: Transfer` on
~49% of sells but only ~19% of buys. The **buy** router
`L2TExMFKdjpN9kozasaurPirfHy9P8sbXoAN1qA3S95` is shared by **5,124 wallets** - commercial bot
infrastructure. The **sell** router `8Ufce7KwjbuwMvTm5XE6hmDDqHQGUBxB1u8vNpaA7iFC` is used by
**him alone**. He buys off the shelf and writes his own exit.

## The signal: he buys absorption, not momentum

The price path around his entry, marked on per-slot closing reserves (price ~ `vsol^2`):

| slots from entry | -25 | -10 | -4 | -2 | **0** | +2 | +3 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| median price vs decision-slot close | +5.4% | +4.0% | 0.0% | -0.4% | **+15.4%** | +13.7% | +13.3% |

Price **declines for ~25 slots into his decision slot**, flattens, and then his own slot is a
violent single-slot reversal burst. Inside that slot a median of **3 buys / 2.94 SOL land
ahead of him with zero sells**, and he is rank 4 of 6. He is not the trigger and he is not
the follow-on - he is inside the impulse.

Every decision-time feature is **hump-shaped**, which is the signature of a Goldilocks
selection rather than a monotone screen. Mean net %/episode by decile:

| feature | dead end (low) | peak | dead end (high) |
| --- | --- | --- | --- |
| active slots before entry | -0.71 (<=1) | **+3.18** (10-17) | +0.41 (>=86) |
| distinct buyers before entry | -2.03 (<=3) | **+3.04** (13-19) | -0.27 (>=186) |
| buy SOL before entry | -1.86 (<3.6) | **+2.89** (15-20) | -0.45 (>119) |
| decision-slot `vsol` | -0.99 (30-32.6) | **+2.79** (40.6-43.4) | -1.24 (>63.7) |
| buy share of gross flow | +1.27 (0.15-0.53) | **+2.94** (0.70-0.80) | **-0.56 (= 1.00)** |
| age at entry | -1.91 (0.98-1.55 s) | **+2.93** (4.5-8.3 s) | +1.06 (>59 s) |
| sells ahead of him in-slot | +0.88 (none) | - | **+3.12** (>1.85 SOL, monotone) |
| buys ahead of him in-slot | +0.94 (none) | **+2.70** (2.9-3.7) | -0.81 (>8.5) |

Read together these say one thing: **he buys a token that has already proven it has a
distributed crowd (10-17 active slots, 13-31 buyers, 15-30 SOL in), which is taking real
selling pressure and absorbing it, at the moment the pullback turns.** The four sharpest
edges of that statement:

- **A pop drawing no selling is bearish.** `bshare = 1.00` (pure buying, nobody selling) is
  his single worst cell at **-0.56%**, while 0.70-0.80 is his best at +2.94%. This
  independently reproduces the inverted `a_deficit` sign already in the settled list.
- **Selling ahead of him in his own slot is his only monotone bullish feature** (+3.12%,
  45.4% win at the top decile). He buys into the knife, like `63ot`.
- **Being first is worse than being fourth.** In-slot rank 1-2 books +0.89% against +2.13%
  at rank 4-5. Confirmation from the crowd inside the same slot beats speed to the trigger.
- **`gap_dec >= 2` is bad.** He wants continuous printing, not silence - the opposite of the
  D->L->I lull chain.

## Why it is unreachable, measured

Same signal (his exact entries), same trail, same size, same cost model, marked on
`px_sell_med` only, moving nothing but the entry slot:

| entry latency | mean net | win | total SOL | days positive |
| --- | --- | --- | --- | --- |
| **0 slots** (inside his firing slot) | **+4.08%** | 41.5% | +1,260 | **22 / 22** |
| **1 slot** | **-2.44%** | 28.0% | -937 | **0 / 22** |
| 2 slots | -1.85% | 29.0% | -689 | 0 / 22 |

A **6.5pp** collapse from one slot. Confirmed independently and pairwise: slot `S+1`'s median
buy price is **+6.70% mean / +6.15% median** above slot `S`'s, and is higher **72.2%** of the
time. The L=0 row is itself optimistic - it marks at his slot's median buy price where he
actually filled at rank 4 of 6, which is why his real book is +1.285% and not +4.08%.

**No exit rescues it.** At +1 slot, every fixed hold from 1 to 15 slots lands in -1.5% to
-2.8% and the trail at -2.44% is the best of the family. (A 25-slot hold reads +4.52% purely
through survivorship - `n` collapses from 47,894 to 17,509.)

**No sub-population rescues it either.** Eight-way decile scans over ten decision-time
features at +1 slot return **0 positive cells out of 80**. The features keep their ordering -
the humps sit in the same places - so the information is real; the whole surface is simply
shifted down 6.5pp.

## The candidate that does not clear

The conjunction of his five best conditions is cleanly monotone in conditions met at +1 slot
(-4.51 -> -2.73 -> -2.01 -> -0.63 -> +0.30 -> **+4.78**), which is good evidence the features
carry real information. The 5/5 cell scores **z = 3.57** against 20 random same-size draws.

It is still **not a rule**: n = 207 over 22 days (9 trades/day), median **-11.61%**, only
**10 / 22 days positive**, and +5.87 SOL total - 0.27 SOL/day. The thresholds were also
chosen after seeing the surface, over five conditions. By the standing gates - median
negative, days positive at a coin flip, book immaterial - this is a lottery cell selected out
of a search, not a signal. Recorded as a lead, not a result.

## What transfers

1. **Absorption beats momentum, and "no sellers" is the bearish tell.** Sellers present and
   being absorbed is the bullish state; a pop nobody is selling into is the worst cell here.
2. **Proven distributed participation is a band, not a threshold.** Too few buyers is dead,
   too many is late. Every feature peaks in the middle; none is monotone except in-slot sell
   pressure.
3. **Confirmation inside the same slot beats being first.** Rank 4 outperforms rank 1.
4. **His exit shape is worth copying and is independent of the entry problem**: one trail off
   the running peak with `peak` initialised at entry, no take-profit, hard ~10 s timeout,
   0.26% bags. Flat retrace across every MFE bucket is what a single trail looks like.
5. **The cost bar is not what stops us - the slot boundary is.** He clears the 125 bps fee by
   1.175% per turn. One slot of latency costs 6.5pp, which is five times the entire fee.

## Reproducing

Scratch schema `fbv` in `hunter_bot`: `tr` (his 103k legs), `ep`/`e2` (episodes, `e2` is
closed-and-unbagged with the fee charged), `ent`/`f1`/`j` (per-entry decision features),
`mkt` (per mint-slot aggregates), `pv` (per-slot closing reserve), `sp` (per-slot
`px_sell_med`/`px_buy_med`, his own prints excluded), `ins` (in-slot ordering around his
buy), `path`/`ex` (price path and exit shape), `sim0`/`simp`/`simx`/`s1` (the latency
simulation), `coh` (the 5,124-wallet router cohort). Safe to drop.
