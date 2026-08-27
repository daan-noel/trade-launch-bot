# Flow metrics and price are one variable on a bonding curve

A pump.fun curve holds `vsol * vtoken = k`, so `price = vsol^2 / k`, and **`vsol` changes
by exactly the net SOL that flowed in or out**. Therefore, over any window `w`:

    net_flow(w)  ==  vsol(t) - vsol(t-w)  ==  the price move over w

Measured on the 6ix tape (8.66M prints, 5% mint sample, windows 5s/15s/60s):

| window | corr(net_flow, price move) | slope | mean abs diff |
| --- | ---: | ---: | ---: |
| 5s | 0.975 | 0.888 | 1.08 SOL |
| 15s | 0.980 | 0.908 | 1.59 SOL |
| 60s | 0.982 | 0.925 | 2.56 SOL |

Slope sits under 1 for the frame edge and the fee, not for anything structural.

## What this means for a rule

A gate on `net_flow`, `buy`, `sell`, or `buyshare` at one or many windows is **a gate on
the price path**, restated in SOL. Several windows at once is the price path at several
resolutions — which is the right way to express "rises consistently", and is also why it
adds nothing to a gate already written on the price path. Confirmed empirically: after
qualifying 6ix tokens on shape (reached vsol 45, never dipped 15%), four different
multi-window flow gates each fire on 17,45x of the same **17,456** tokens as no gate at
all, with identical means. The conjunction never delays a single entry.

**`buyshare` is not independent either.** `buyshare = (1 + net/gross)/2`, so once `net`
is the price move, `buyshare` carries information only through `gross`.

## What IS orthogonal to price

Only quantities the price path does not determine:

* **`ntx`** — how many trades made the move. Ten 1-SOL buys and one 10-SOL buy give the
  same `net`, the same price, and different meanings.
* **`gross`** — two-way churn. The same `net` can come from 10 buy / 0 sell or from
  100 buy / 90 sell.
* Derived: `gross/|net|` (absorption), `gross/ntx` (ticket size).

On 6ix's climb phase even these collapse: **`gross/|net| = 1.000` for the bottom six
deciles** — 60% of climbs to vsol 45 have *zero* selling, so `gross == net == price` there
too. Only `ntx` stays genuinely independent, and it carries no edge (all deciles
−5.4% to −9.3%, 50/50 negative).

## Consequence for cohort work

Before adding a flow metric to a rule on the curve, ask what it measures that the price
path does not. If the answer is "the direction or size of the move", it is the same
column twice — the single-source-of-truth rule applies to metrics, not just constants.
The venue matters: on an AMM or an order book, flow and price are **not** the same,
because depth absorbs. This identity is a property of the bonding curve.

Related: [`curve-honest-pricing.md`](curve-honest-pricing.md),
[`metrics-reference.md`](metrics-reference.md).
