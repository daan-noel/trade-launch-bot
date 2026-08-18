# The max-buy launcher's tradeable axis is the creation bundle, not the funding tier

Why `3ix:BuyExactSolIn · spend=5 · bkt=exact` (fp `219e0772`) carries edge while its five
sibling presets do not, and what that says about deriving rules per fingerprint. Companion
to [maxbuy-launcher-fingerprint.md](maxbuy-launcher-fingerprint.md), which profiles the tool
itself. Measured over the 26-day lake (07-22 .. 08-16), fills and costs ported from
`core/src/strategies/paper_fill.rs` + `kernel.rs`.

## The finding in one line

`spend=5` is a **proxy for a creation-slot bundle of 15-30 SOL**. The band is the edge; the
funding tier only correlates with it. Gate any tier of the same tool on the band and it pays;
strip the band from `spend=5` and it stops paying.

## The cohort carries the edge, the rule does not

One battery, identical values, `next_slot_median` fills, `pumpfun_impact` costs, 0.05 SOL:

| cohort | n | hold 30 s | hold 60 s | TP+8/SL-15 | trail 36 + stall 30 | LIQ>20 + trail/stall |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| spend=1 | 877 | +8.8 | -1.7 | +0.3 | -9.3 | -19.1 |
| spend=1.5 | 247 | +2.6 | +0.2 | -6.5 | -9.7 | -22.8 |
| spend=2 | 2271 | -1.2 | -10.9 | -4.0 | -17.7 | -23.9 |
| spend=3 | 934 | -1.5 | -7.6 | -6.0 | -11.8 | -16.0 |
| spend=4 | 72 | +28.4 | +49.8 | +7.1 | +52.8 | +40.3 |
| **spend=5** | **395** | **+11.5** | **+18.3** | **-4.9** | **+16.5** | **+19.4** |
| max=0.108 | 1154 | -4.1 | -8.4 | -0.2 | -10.0 | -10.6 |
| max=4.08 | 1377 | -23.7 | -31.1 | -4.5 | -22.6 | -6.3 |

(net %/trade. The full 14-cohort table lives in the run scratch.)

A **blind 60-second hold** on `spend=5` returns **+18.3%/trade**. The promoted rule's entry
gate adds ~3 pp on the same exits (+16.5 ungated -> +19.4 with `liquidity > 20`) at the cost
of 89 trades, and its engineered exit set does not beat a flat 60-second timeout. The rule is
not what makes the money; admission to the cohort is.

`liquidity > 20` reads as `vsol > 50` ([liquidity-metric-is-real-reserves]) and `vsol` at the
first reachable print is itself a function of the creation bundle, so the one gate that pays
is a weak restatement of the band below.

## The band

Creation-slot buy SOL, pooled across the six tiers of the tool, blind 60 s hold:

| bundle SOL | n | mean %/trade | median | win % | p90 |
| --- | ---: | ---: | ---: | ---: | ---: |
| < 5 | 243 | -5.1 | -27.9 | 9.9 | -13 |
| 5-10 | 2484 | -10.8 | -40.3 | 12.3 | +97 |
| 10-15 | 1285 | -7.0 | -48.7 | 17.8 | +170 |
| **15-20** | **556** | **+16.3** | -58.8 | 31.3 | +209 |
| **20-30** | **186** | **+33.0** | -67.4 | 36.0 | +287 |
| > 30 | 42 | -15.1 | -78.1 | 21.4 | +234 |

The tiers sit in different parts of it: `spend=5` puts 85% of its launches in 15-30 SOL,
`spend=2` puts 87% below 15. That is the whole difference between the two fingerprints.

**Both directions confirm it.** Gate the five *other* tiers on 15-30 SOL and drop `spend=5`
entirely: **+20.5%/trade, n=408, 95% CI [8.1, 33.3], 4/4 weeks positive** — better than
`spend=5` itself. The same tiers ungated return **-6.8%** (CI [-9.6, -3.9], n=4401). Restrict
`spend=5` to bundles under 15 SOL and it returns **-4.9%** (n=47).

Pooled over every tier the band holds 742 launches, **+20.5%/trade, CI [10.6, 30.5]**, +7.60
SOL at 0.05 size, ~31 launches/day.

`spend=2` alone inside the band is a live cohort a fingerprint-level search discards as a
loser: **+22.5%/trade, n=166, CI [3, 43], 4/4 weeks positive**.

**The axis is already expressible.** `fingerprints.first_slot_buy_lamports` is a
bucket-matched axis and a deferred first-slot gate, so the band needs no new metric — it
needs a fingerprint whose identity is the bundle, with the funding tier left `NULL`.

## The payoff is a fat tail, and that dictates the exit

Inside the band: **win rate 31-36%, median -58%, top-5 trades = 29-38% of P&L, p90 +209 to
+287%**. Every profitable configuration is a lottery whose tickets are cheap enough.

That is why a take-profit destroys it (`spend=5`: +18.3% blind -> **-4.9%** with TP+8/SL-15)
and why any selection objective that rewards win rate, penalizes variance, or trims outliers
selects against the only trades that pay.

**It is latency-flat**, which is the property that makes it worth shipping: `signal_price`
+21.3 -> `next_slot_first` +19.4 -> `next_slot_median` +18.3 -> `worst_case` +16.6. A ~22%
spread across the entire fill-model range with no sign flip, unlike the 4.08 launcher's
collapse between slot 2 and slot 3 ([fp-4sol-launcher.md](fp-4sol-launcher.md)).

## Two payoff shapes need two rule families

The cohorts split cleanly, and the split predicts which rule family can work:

| shape | signature | cohorts | rule family |
| --- | --- | --- | --- |
| **skew** | median crashes, mean positive, p90 max-up +210-290%, life ~190 s | the `BuyExactSolIn` spend tiers | no TP, wide trail or a flat timeout; harvest the tail |
| **grind** | median slightly positive, mean ~0, p90 max-up +45-110%, life 320-480 s | the `Buy · max=` tiers | small fixed TP, hit-rate; a trail bleeds it out |

Applying the wrong family inverts the result in both directions: a TP costs `spend=5`
23 pp, and a trail costs `max=0.108` 10 pp against its flat-TP baseline. A search that does
not classify the cohort before choosing a template finds nothing on roughly half of them.

**Classify first, with three numbers from the cohort's own tape** — `mean - median` of the
forward-60 s return, the p90 max-up, and median lifetime — then pick the template family.

## How to apply

- **Search the bundle axis, not the funding axis.** A fingerprint identifies launch
  *software*; the tool's own launches are heterogeneous, and pooling them dilutes a real band
  to zero. Cut every candidate cohort by `first_slot_buy_lamports` before concluding it is
  dead.
- **Rank on total SOL with the tail intact.** On a skew cohort the mean is five trades; a
  bootstrap CI is the honest gate, not a t-stat.
- **Check the reverse direction.** A gate that pays is only a gate if removing it from the
  winning cohort kills the edge and adding it to a losing one revives it.
- The band's threshold is not tuned — bins are fixed a priori and the effect is monotone
  across three adjacent bins — but it is chosen on this window, so it wants an out-of-time
  re-measure before size.
- Everything here is backtest. Nothing in this family has traded forward.

Related: [maxbuy-launcher-fingerprint.md](maxbuy-launcher-fingerprint.md),
[fill-and-cost-models.md](fill-and-cost-models.md), [execution-costs.md](execution-costs.md),
[rule-search-method.md](rule-search-method.md).

[liquidity-metric-is-real-reserves]: metrics-reference.md
