# 2026-08-23 — 3Xk2 decomposed; a first-launch impulse rule derived in its place

## What 3Xk2 turns out to be

Three parts, graded separately on 08-13..21 at a 115 ms fill on both legs.

- **Token selection: refuted.** His mints look strong blind, but a survivorship + depth +
  age matched control removes the edge. His picks are tokens still alive at ~42 s in mid
  depth, nothing more.
- **Entry: at-the-highs continuation, not weakness.** `trail_30 <= 2`, `rise_30 >= 35`.
  Buying pullbacks is worse than baseline on every panel and degrades monotonically with
  pullback depth.
- **Exit: a wide trail.** Correct, and the load-bearing part of his book.

His signature reproduced market-wide is **-0.001069/trade**. He fires ~385 entries/day; a
gate built from his medians fires ~19,000. He is ~50x more selective than his own medians
can express, and that discrimination is not recoverable from them.

The rule below was derived from the axes instead. It overlaps his mints **3.2%** — chance
level — and loses money on the few it shares. It is not his logic; it is a different one
found while looking for his. His residual edge stays unexplained, the same ending as the
8dtx and 64hP studies.

## The rule

```
TOKEN    prior_launches == 0        creator's first launch in the trailing window
MOMENT   buy_share(30s) >= 0.80  AND  rise(10s) >= 150%
EXIT     no stop, 30% trailing stop
SIZE     B = 0.75 SOL
FILL     115 ms, both legs
```

`liq >= 18` is implied by the other three and drops out with no change in any statistic.

| | FIT 08-13..17 | OOS 08-18..21 |
| --- | ---: | ---: |
| expectancy / trade | +0.088809 | **+0.073558** |
| SOL / day | +30.80 | +24.48 |
| days positive | 5/5 | 4/4 |
| entries / day | 347 | 333 |
| concurrency | 0.56 | 0.52 |
| median hold | 26.2 s | 26.9 s |
| runner rate (r >= +50%) | 19.90% | 20.51% |

OOS bootstrap, mint-clustered, 4000 resamples: 1,331 episodes over 1,288 mints,
**CI95 [+0.039442, +0.108794], P(>0) = 1.0000**. Win rate 34.3%, median -0.1397,
p90 +0.9089 — a convexity book, negative median, tail-carried.

## Why each term is there

Dropping one term at a time, OOS at B=0.75:

| | OOS exp | days+ |
| --- | ---: | :---: |
| full rule | +0.073558 | 4/4 |
| drop `prior_launches` | +0.002251 | 3/4 |
| drop `buy_share` | +0.013800 | 4/4 |
| drop `rise_10` | +0.002442 | 2/4 |
| `rise_10` **alone** | -0.004889 | 1/5 (FIT) |

Each term is negative or near-zero on its own and the three together are not. This is the
level-versus-differential distinction again: the rise term does not select rising tokens,
it selects rising tokens *that a first-time creator launched into one-sided flow*.

## The exit has an interior optimum

Trail width, both panels, inside the gate:

| trail | FIT exp | OOS exp | OOS days+ |
| ---: | ---: | ---: | :---: |
| 10% | +0.007295 | +0.020031 | 4/4 |
| 20% | +0.046994 | +0.061316 | 4/4 |
| **30%** | **+0.088809** | **+0.073558** | **4/4** |
| 40% | +0.049904 | +0.032486 | 2/4 |
| 50% | -0.001064 | -0.001394 | 2/4 |
| 60% | -0.046610 | -0.046864 | 2/4 |

A stop is never worth adding: `s15_t30` matches `sN_t30` within noise, and every tighter
stop costs money. `s3_t5` is -0.007626 OOS, 0/4.

## Size

Optimum at B = 1.5-2.0 SOL; the sign flips at ~3.3. B = 0.75 keeps ~72% of the peak money
with a 4x margin to the flip. Concurrency 0.52 means one position at a time: at entry, 49%
of the time nothing else is open, 32% one, 20% two or more, max 5.

## Latency

f115 -> f235 (bot p50 -> p90): FIT +0.088809 -> +0.087976, OOS +0.073558 -> +0.073063.
**99.3% retention.** A same-slot fill artifact collapses under lag; this does not move.

## The search is saturated

A second marginal round on top of the three terms returns nothing that survives OOS. The
best FIT additions — `bshare_30 >= 0.9`, `ntr_30 <= 50`, `trail_10 <= 3` — either lose money
out of sample or move expectancy by less than 4% while cutting SOL/day. `bshare_30 >= 0.9`
looks strong on FIT (+0.1097) and fails OOS (+0.0600, 3/4). Every round-2 candidate is
positive on FIT, which is itself the diagnostic: nothing is repairing a broken subset any
more, the terms are only trading less.

## Shipping status

- `rise(10s) >= 150` — `MetricId::WinRise`, window 10 s. Exists. Same formula
  (`(cur - low) / low * 100`).
- `buy_share(30s) >= 80` — `MetricId::BuyShare`, window 30 s. Exists. **Engine scale is
  0-100, the analysis scale is 0-1** — the threshold is 80, not 0.8.
- `prior_launches == 0` — **missing.** The engine carries `creator_wallet_hash` only for
  flow-split classification. This needs a per-creator rolling launch counter resolved at
  token creation, and it is the term carrying the rule.

The rule cannot be authored as a `strategy_rules` row and cannot go through `simulate`
parity until that metric exists. The shippable substitute `cu_price == 100000` holds on FIT
(+0.004584, 3/5) and fails OOS (-0.000364, 1/4).

## The bug that invalidated the first pass

`collapse()` takes **microsecond** timestamps; the hold column is in **seconds**. Passing
`t_out = fill_t + h` gives zero-length episodes and no de-overlapping: 74,250 positions/day
instead of 6,545, an 11.3x inflation that biases *upward* under a momentum gate, because
such a gate selects clusters of decision points on one rising token. Every conclusion from
that pass — a depth corridor, an exit that inverts with selection, +2.51%/trade matching his
book — was discarded and redone.
