# The `5ix:Transfer · 600K/160K` operator: 42 SOL bundle, 26% graduate, 73% rug — priced against us

Behavioural profile of fingerprint `5ix:Transfer · cu_limit=600K · cu_price=160K · bkt=exact`
(406 launches, 07-29 .. 08-18, 22.5/day) and why no entry or exit rule pays on it. Measured on
the 26-day lake, fills and costs ported from `paper_fill.rs` + `kernel.rs` (125 bps/leg,
0.000225 SOL/leg, `B/vsol` impact, 0.126 SOL).

## The habit

One operator, one template, no variation:

| | |
| --- | --- |
| shape | `CU limit`, `CU price`, `Create`, `ExtendAccount`, `System Transfer` — **no dev-buy ix** |
| creation-slot bundle | 41-44 SOL, **sd 3.0**, from the **same ~12 wallets on 404 of 406 launches** |
| depth after the bundle | `vsol` 71.7 -> 72.4 (p25 -> p75) — constant to ±1% |
| median lifetime / volume | 252 s / 189 SOL traded |

Every launch starts identically. He then picks one of two endings.

## When he rugs, when he migrates

**He never rugs a token he has graduated — 0 of 296 rugs happened after `vsol` reached 115.**
The decision is made on the way up, and it is final.

| ending | share | timing |
| --- | ---: | --- |
| **rug** | **73%** | median **46 s** from creation; 33% inside 30 s, 59% inside 60 s, 94% inside 300 s |
| **graduate** | **26%** | median **134 s** (p25 89 s, p75 212 s) |

The rug hazard decays with age — `P(rug in the next 30 s)` is 24% at 10 s, 25% at 30 s, then
9% at 60 s and 4.5% at 180 s. Surviving the first minute roughly halves the remaining risk
(72.9% ever-rug at 0 s -> 52.2% at 60 s), but never clears it.

His tell is **whether outsiders show up**: `vsol` at 20 s under 75 (i.e. net inflow under ~3 SOL
on top of his own bundle) rugs before 120 s **77%** of the time; over 75 it is 41-45%. AUC 0.69 —
real, and far too weak to trade on.

## Why nothing pays — the arithmetic is his, not ours

Entry after the bundle sits at `vsol` 72. Price tracks `vsol^2`, so the two endings are fixed
sizes, the same on every launch:

- **graduate:** `(115/72)^2 - 1` = **+155%**
- **rug:** the bundle unwinds to the base curve, `(30/72)^2 - 1` = **-83%** (measured floor:
  p1..p25 of every blind hold pin at -84 to -85%)

Break-even graduation rate is `84 / (84 + 155)` = **35.1%**. He graduates **26.4%**. The gap is
his margin, and it is stable: 23.2% / 31.4% / 24.9% across the three weeks, and only 4 of 18 days
clear the bar. Riding to graduation measures **-22.2%/trade**, which is exactly what
`0.26 x 155 + 0.73 x -84` predicts.

That is the whole finding. **The payoff is a fixed two-outcome lottery whose odds the operator
sets, and he sets them ~9 pp under fair.**

## What was tried

- **510 cells** — entry delay {0,5,10,15,20 s} x 8 flow gates x 17 exits (flat holds, TP ladder,
  armed trails, TP+trail), priced `worst_case`. One survivor of `IS>0 & OOS>0 & lower bound >0`,
  **zero** once every-day-positive is required; the survivor books +0.03 SOL/day on a bootstrap
  CI of [-0.45, +13.15].
- **`vsol`-space exits** (target 90/100/115, drop-from-peak 3/5) with and without a `vsol >= 75`
  entry gate: all 28 combinations negative. The gate makes it worse, because confirming costs
  depth — entering at `vsol` 80 cuts the graduation payoff from +155% to +107% while 41% still rug.
- **Sell-impulse exits** (fire on the first sell >= K SOL): negative at every K but a lone spike
  at K=2 whose neighbours K=1 and K=3 are both negative.

Two mechanical reasons no exit rescues it:

1. **The dump is atomic at slot resolution.** Peak to -50% is median **5 slots / 1.9 s**, 31.8%
   inside 2 slots. It looks gradual in print space (~90 prints, worst single print -15%), which
   is why a trail backtests as if it could react; the fill window (`S` + one next slot) swallows
   the cascade, so `arm0/trail3` books a 3-6 s hold and still realizes -84%.
2. **A take-profit cannot outrun the no-pop tail.** TP+15 hits on 80% at +13.9% net, but the 20%
   that never pop lose the full -84%. Every TP from +5 to +40 lands between -5% and -8%/trade.

It is genuinely **latency-flat** (`signal_price` +4.31 -> `worst_case` +3.18), so execution is
not the obstacle — there is no edge to execute.

## Why `3ix:BuyExactSolIn spend=5` pays and this one does not

Both are launcher cohorts, both bundle at creation, both are mostly rugs. The one number that
separates them is **where the bundle leaves you on the curve**. Same harness, blind entry,
`worst_case`, hold 60 s, 0.126 SOL:

| | `3ix spend=5` | `5ix:Transfer 600K/160K` |
| --- | ---: | ---: |
| creation bundle | 15-30 SOL | 41-44 SOL |
| **entry depth `vsol`** | **49.6** | **71.9** |
| headroom to the `vsol` 115 ceiling | **+429%** | **+156%** |
| floor if the bundle unwinds | -63% | -83% |
| reward : risk | **6.8 : 1** | 1.9 : 1 |
| graduation rate | 11.9% | **26.4%** |
| win rate / median | 30% / **-62%** | **55%** / **+19%** |
| p90 | **+240%** | +87% |
| **mean per trade** | **+13.9%** | **-6.1%** |

The cohort that looks better on every quality metric is the one that loses money. The
decomposition is exact:

| | payoff if it graduates | cost if it does not | rate | expectancy |
| --- | ---: | ---: | ---: | ---: |
| `3ix spend=5` | **+290.6%** | -23.7% | 11.9% | `0.119 x 291 - 0.881 x 23.7` = **+13.8** |
| `5ix:Transfer` | **+65.8%** | -31.8% | 26.4% | `0.264 x 66 - 0.736 x 31.8` = **-6.5** |

(measured means +13.87 and -6.05 — the two endings account for the whole result in both cohorts.)

**The same event — the token graduates — is worth 4.4x more in one cohort than the other, and
the failure is cheaper too.** Price is `vsol^2` against a fixed ceiling, so a bundler who buys
himself from `vsol` 30 to 72 has already taken `(72/30)^2` = **5.8x** of a 14.7x curve and left
2.6x for everyone after him; `spend=5`'s bundler takes 2.8x and leaves 5.3x. The 5ix operator
graduates **more than twice as often** and still cannot pay a copier, because he pre-consumed
the part of the curve that pays.

**The rule: rank a launcher cohort by entry depth, not by success rate.** `(115 / entry_vsol)^2`
is the whole upside available to anyone who is not in the bundle, it is known at the creation
slot, and it is a fingerprint-expressible axis (`first_slot_buy_lamports`). A cohort whose bundle
puts entry depth above ~60 has under 2:1 reward-to-risk and needs a graduation rate no pump.fun
launcher sustains.

`spend=5` is not immune to the same fate: its own graduation rate is climbing (2% -> 10% -> 17%
-> 20%) while its blind 60 s mean turned negative for the first time on 08-17..08-18
(-3.2%, n=50). Re-measure before sizing.

## The transferable gates

- **Price the operator's two endings before searching his axes.** Two fixed outcomes plus a
  measured frequency answers "can this pay" in one query. Here it is `+155% x 0.264` against
  `-84% x 0.729`, and no entry filter or exit shape changes either number.
- **Print each axis's spread inside the cohort first.** A fingerprint identifies launch
  *software*; searching its axes only works where the operator *varies* them. Bundle sd 3.0,
  depth constant to ±1%, one wallet list — a fingerprint-scoped search here has zero degrees of
  freedom before it starts.
- **Read a collapse in slots, never in prints or seconds.** Print-space smoothness is what makes
  an unreachable dump backtest as a tradeable trail.

Related: [maxbuy-launcher-fingerprint.md](../plans/strategies/maxbuy-launcher-fingerprint.md),
[signal-search-mandate.md](../plans/strategies/signal-search-mandate.md),
[graduation finish line](2026-08-18-graduation-and-identity-space.md).
