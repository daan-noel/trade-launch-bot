# Pricing a position on a bonding curve

The convention that decides most cohort results, and the one that is easiest to get
wrong in the flattering direction.

## The rule

**A pump.fun price is a function of `vsol` alone.** Constant product `vsol * vtoken = k`
gives `price = vsol / vtoken = vsol^2 / k`, so `price ~ vsol^2` and `vsol` moves only when
somebody trades. Three consequences, and they are not optional:

* **Silence freezes a price, it does not zero it.** A token with no prints for the whole
  hold has the `vsol` it had at the fill, so the exit fills at the entry price less the
  toll. Booking it `-100%` invents a loss the curve cannot produce.
* **The exit price is the last print at or before the exit instant**, defaulting to the
  entry fill itself. One expression covers a busy token and a silent one.
* **The curve always has liquidity.** There is no unsellable state on the curve to model.
  Migration is the one real exception and it is 0.02% of this corpus; check `venue` rather
  than assume it.

## Why it matters more than it sounds

On the 6ix cohort the `-100%` convention manufactured an **85 pp** effect out of nothing:
a control gate read `-65.33%` against a full rule at `+19.72%`, and the whole gap was
64.7% of the control set having no print to mark against. Priced on the curve the same
two gates are `-3.65%` and `+17.58%` at zero lag, and `-3.93%` against `-1.70%` at the
bot's 115 ms -- a ~2 pp effect, not 85. A session's worth of work was queued on that gap.

The error is silent, it is large, and it always points the same way: it punishes the
control (mostly silent tokens) far harder than the rule (mostly active ones), so **every
gate that selects for activity looks like a discovery.**

## The toll, and the bar it sets

Round trip at `B = 0.10` SOL, 125 bps per leg, constant-product impact both ways:

```
net = (vsol_x/vsol_e)^2 * (1 - B/vsol_x) * 0.9875 / ((1 + B/vsol_e) * 1.0125) - 1
```

At the cohort's median `vsol` of 50.3 that is **-2.86%** on a token that does not move,
and the measured median over 54,795 unchanged-price entries is **-3.065%** (range -5.12%
at low depth to -2.64% at high). Any candidate rule clears that bar before it is a rule.

Reading the median of a result table is the cheapest correctness check there is: a
cohort whose median sits exactly on the toll is telling you the median token does not
move, and no gate that fires on the median token can pay.

## Checking a harness

Two assertions, both cheap, both mechanical:

1. Filter to entries where the exit `vsol` equals the entry `vsol` and confirm the mean
   net equals the analytic toll. Any drift means the fill or the cost model is wrong.
2. Report the share of entries with no print in the hold window alongside every result.
   A number that moves when that share moves is a pricing artifact, not an edge.

Related: [`fill-and-cost-models.md`](fill-and-cost-models.md),
[`execution-costs.md`](execution-costs.md),
[`cohort-entry-rule-anatomy.md`](cohort-entry-rule-anatomy.md).
