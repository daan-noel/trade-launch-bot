# 2026-08-26 — the 6ix cohort has no long edge at 115 ms

**Verdict: closed.** Not "no rule found yet" — the cohort was measured unconditionally,
per-feature, per-gate, and against an oracle exit, and there is nothing to condition on.
Five independent lines, any one of which would have been enough.

Corpus: the 6ix `Buy`+fee launch template, 07-28..08-22, 76,156 mints, 9.9M prints.
Fills at the measured 115 ms decide-to-fill, costs at 125 bps a leg plus constant-product
impact at `B = 0.10` SOL, exits priced on the curve per
[`curve-honest-pricing.md`](../plans/strategies/curve-honest-pricing.md).

## 0. First, a correction that changed the question

The one result that survived the 08-26 refutation was survival gating: an **85 pp** swing,
64.7% of the control set "unsellable" against 2.0% under the full rule. That number was an
artifact of booking a token with no print in the hold window at **−100%**.

A pump.fun price is `vsol^2 / k`, and `vsol` moves only when somebody trades. A silent
token has the `vsol` it had at the fill, so the sell fills at the entry price less the
toll. The data agrees: of the control entries with no print in 30 s, 51.5% never traded
again at all, and the 13.25% that came back did so at **−1.97% mean / −1.30% median**, at
a median gap of 88 s. Not −100%. It is not a feed artifact either — the silent share is
58–72% on every day of the corpus, and *lowest* on the three degraded low-volume days.

Repriced, the same ablation reads:

| gate | fires/day | silent | mean OLD (dead = −100%) | mean HONEST @115 ms |
| --- | ---: | ---: | ---: | ---: |
| `age>=60` only | 2145.4 | 67.0% | −68.87% | **−3.93%** |
| `+ gross>=43.6` | 840.9 | 32.9% | −36.75% | −4.83% |
| `+ ntx<=140` | 473.4 | 51.0% | −53.46% | −4.00% |
| `+ buy(5)>=2.94` | 84.0 | 2.8% | −4.44% | **−1.69%** |
| full rule | 84.0 | 2.8% | −4.41% | −1.70% |

Survival gating is worth about **2 pp**, not 85. The lead it justified was never there.

The tell was visible in the honest table all along: the median is **−3.12% at every
gate**, which is the toll on a token that does not move. A column of identical medians
means the median token is inert and the gate is sorting noise.

## 1. Unconditional drift is negative at every age

Gross forward move at a 115 ms fill, 1.95M sampled prints:

| age | n | fill gap | +10 s | +30 s | +60 s | +300 s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 0–5 s | 269k | **+6.09%** | −3.62 | −13.25 | −16.94 | −21.66 |
| 5–15 s | 299k | +0.47 | −4.82 | −11.41 | −14.98 | −20.04 |
| 15–30 s | 252k | −0.45 | −3.79 | −6.98 | −10.34 | −15.71 |
| 30–60 s | 251k | −0.35 | −1.20 | −3.84 | −7.07 | −12.62 |
| 60–120 s | 232k | −0.13 | −0.95 | −2.94 | −4.49 | −8.88 |
| 2–5 m | 273k | −0.16 | +0.28 | −0.30 | −0.91 | −5.24 |
| 5–15 m | 233k | −0.17 | +0.14 | −0.31 | −0.97 | −4.12 |
| 15 m+ | 144k | −0.04 | +0.38 | **+0.54** | +0.44 | −0.23 |

Every cell but three is negative, the largest positive is +0.54% against a 3.1% toll, and
longer holds are monotonically worse. The `fill gap` column localises the only real number
in the cohort: **+6.09% at 0–5 s and ~0 everywhere after**. That is the launch burst, and
it is paid to whoever is already in the slot.

## 2. All 130 feature deciles are negative

Thirteen features — depth, maturity, lifetime gross, window trade count, burst share, buy
share, trailing return, drawdown, silence, and three flow levels — cut into deciles. Not
one cell of 130 is positive, and every feature is monotone toward *less activity, smaller
loss*. The signs are the opposite of the folk intuition:

* `buyshare60` top decile (81.8–100% buys) is the **worst** cell in the table, −17.07%.
* `ret60` deciles 5–8 (up 13% to 129% on the minute) run −8% to −12%.
* `dd = 0` (sitting on its own high) is −15.26%.

Buying pressure, momentum, and strength all predict loss. What they have in common is a
`gap_pct` of +1% to +8%: they are not descriptions of a token, they are descriptions of a
buy that already landed.

## 3. All 23 single-term state gates are negative

Collapsed to one fire per token, best first: `vsol>=80` −3.18% (6/26 days positive),
`burst_share>=50` −4.54%, `vsol>=60` −4.83%, `buyshare60>=75` −2.69% (2/26 days). The
control is −3.93%. Nothing clears the toll and nothing is day-stable.

## 4. The oracle exit is positive; the reachable exit is not

Selling at the best price in the window — an upper bound on every exit rule that exists:

| age | clock @30 s | oracle @30 s | oracle @300 s | % touching the toll @30 s |
| --- | ---: | ---: | ---: | ---: |
| 0–5 s | −15.81 | **+35.32** | +50.37 | 58.2% |
| 60–120 s | −5.70 | +17.23 | +46.67 | 60.2% |
| 15 m+ | −2.28 | +4.14 | +20.28 | 34.8% |

So the money is real: 46–60% of entries touch a profitable price at some point. A reactive
take-profit, sending its market order with the same 115 ms as everything else:

| TP | hit rate | net on hits | net on **misses** | overall |
| --- | ---: | ---: | ---: | ---: |
| +5% | 58.4% | +8.29% | −24.95% | −5.52% |
| +10% | 48.3% | +13.56% | −23.78% | −5.73% |
| +20% | 33.9% | +23.87% | −21.64% | −6.19% |
| +50% | 14.0% | +54.24% | −17.06% | −7.11% |

Negative at every level and in every age band. The hit rate is near a coin flip and the
losing half loses roughly twice what the winning half wins. This is the same adverse
selection that books the engine run at −12.81%/trade: a `retrace>=10` stop is a reactive
exit, so it sits on the miss side of this table.

## 5. No launch fingerprint rescues it

`cu_price`, `cu_limit`, `initial_buy`, `first_slot_buy`, cashback and mayhem flags, all
split on the honest fill. Every cell with ≥200 mints is negative except creator
`initial_buy` in 0.2–0.5 SOL at +0.63% — which fails on inspection: 351 mints total, the
adjacent 0.35–0.5 band is −7.45% between two positive neighbours, and 8/26 days are
positive with the entire effect coming from 08-10 and 08-13. One best cell out of ~40 is
what a multiple-comparison artifact looks like. `first_slot_buy IS NULL` at +22.96% is 59
mints with a missing fingerprint.

## Why, mechanically

On a bonding curve the price is a deterministic function of `vsol`, and `vsol` moves only
when somebody trades. **The only thing that raises the price is a buy** — so every
observable state that correlates with a rise is a *consequence* of buying that has already
happened, and by the time it is observable its impact is already in the price. That is why
the one large positive number in the whole corpus is the `+6.09%` fill gap at 0–5 s, and
why it is negative from 25 ms onward.

The 6ix edge is real and it lives inside the first ~25 ms. It is reachable only by
predicting the buy, not by reacting to it — and prediction from observable state is what
sections 2 and 3 tested and closed.

## What would reopen it

* A **non-price signal that leads the buy** — an instruction-composition or mempool-side
  read that fires before the trade lands, not after. This is the only door left open.
* **Same-slot execution ahead of the trigger**, which is a transport problem, not a
  signal problem, and is priced in `wallet-mine-latency-and-tip-floor`.
* A different cohort. The harness that produced every number here is
  [`cohort-scan.py`](../plans/strategies/cohort-scan.py) and takes about a minute per
  cohort, so the next one is cheap to answer.

## Verification

The harness checks itself: on the 54,795 entries whose exit `vsol` equals the entry
`vsol`, the mean net is **−3.065%** (range −5.12% to −2.64%) against an analytic toll of
−2.86% at the median `vsol` of 50.3. Fire counts reconcile with the independent path —
gate `E` fires 2,183 times here, the same 2,183 the standalone SQL and `simulate` agreed
on in [`2026-08-26-6ix-cohort-rules-are-intra-slot-impact.md`](2026-08-26-6ix-cohort-rules-are-intra-slot-impact.md).
