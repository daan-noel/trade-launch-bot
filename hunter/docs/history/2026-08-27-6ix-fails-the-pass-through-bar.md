# 2026-08-27 — 6ix fails on pass-through, not on signal

Supersedes the mechanism section of
[`2026-08-26-6ix-cohort-closed-at-real-latency.md`](2026-08-26-6ix-cohort-closed-at-real-latency.md).
That entry closes 6ix correctly and then over-reaches: it argues the closure
**generalises**, because observable state is always a consequence of a buy already
priced in. The live 3ix rules refute that — same chain, same 115 ms, and they pay. The
closure is a fact about the 6ix token population, not about curves.

## What the live rules actually are

`-- promoted g8 c182107` and `-- promoted g12 c176905` are three things, and the first
one carries the weight:

1. **Identity** — `ix_labels = [Create_v2, CreateIdempotent, Buy]` *and* an exact
   `max_cost_lamports`. This is a fingerprint axis, matched at creation.
2. **A band wait** — `time` and `liquidity` with an upper *and* a lower bound
   (g8: time 5–75 s, liquidity 25–45; g12: 10–50 s, 10–45, `gross_flow(60s) >= 40`).
3. **Concentration** — one trade per token.

`liquidity = vsol − 30` and graduation is `liquidity = 85`, so the trade reads: **enter
mid-curve at vsol 55–75, ride to 115, bail on a 40% retrace.** `retrace` is unarmed here
(the peak seeds at the fill), so it doubles as a hard stop from entry.

`max_cost = dev_buy × slippage_preset`, so the axis is a **launch-tool signature**, not a
size. 3.3 = 3.0×1.10, 6.06 = 5.926×1.0225, 10.5 = 10.0×1.05 are three different tools.

## The one number the trade lives on

A 55 → 115 trip pays `(115/55)^2 − 1 = +337%`; the stop costs ~40%. So the trade needs

    P(graduate | reached the band) > 40/377 = 10.6%

Measured per identity, 07-28..08-21, one trade per token:

| identity | in band | pass-through | days over 10.6% |
| --- | ---: | ---: | ---: |
| 3ix 0.0432 | 164 | 75.0% | 15/16 |
| 3ix 0.54 (g12) | 137 | 32.1% | 16/19 |
| 3ix 0.108 | 831 | 28.4% | 24/25 |
| 3ix 0.432 (g8) | 227 | 18.1% | 14/20 |
| **6ix 3.85** | 112 | 17.9% | 11/19 |
| **6ix 5.5** (14.8k mints) | 3071 | 10.5% | 13/25 |
| **6ix 3.3** (19.8k mints) | 2546 | 9.7% | 9/25 |

3ix sits 2–7× over the bar. 6ix sits **on** it. That is the whole difference, and it is a
property of the tokens: 6ix carries *bigger* dev buys (2–10 SOL vs 0.04–0.5) and reaches
*lower* peaks (median peak vsol 40–56 vs 63–115), so the size confound runs against 6ix.
The ×1.08 tool every winning rule targets **never launches a 6ix token** — 6ix is 94% a
single ×1.10 tool.

## The dip thesis is real and still not enough

Among 6ix tokens reaching the band, deepest drawdown before arrival predicts graduation:
decile 1 (never dipped) **17.3%** and decile 2 (≤2.3%) 14.8% against an 11.45% base,
21/25 and 19/25 days over the bar. Time-to-band is as strong (under 0.9 s → 21.0%,
22/25 days). Both are causal — they read only pre-entry state.

In money at a 115 ms fill it does not survive. Tightening the dip gate lifts graduation
4.1% → 7.0% and lifts the entry `gap` 5.98% → 14.42%: ~3 pp of edge bought for ~8 pp of
fill. Best cell is `dip ≤ 0.1% AND age ≤ 1 s` at **−3.70%**, 8/25 days. Scoped by
identity, only `mc 5.5` turns positive (+1.61%, 447 trades) and it **fails held out**:
+4.77% through 08-14, **−3.55%** from 08-15.

Widening the stop is monotonically worse — retrace 40→100% lifts graduation 6.3→12.2%
and the mean falls −6.26% → −24.56%, because the failures then realise −72%.

**The gap is not the mechanism.** The 3ix control pays a **62.8%** entry gap under the
same rule and still earns +2.46%, because 40.8% of its tokens graduate. Cost of entry is
survivable; a thin pass-through is not.

## Latency is where the 6ix edge goes

Same rule, same population, fill at the fire print instead of +115 ms:

| | mean | days positive |
| --- | ---: | ---: |
| `dip ≤ 0.1%`, lag 0 | **+6.73%** | 16/25 |
| `dip ≤ 0.1%`, lag 115 ms | −6.36% | 7/25 |

Requiring quiet before the fire collapses the gap (11.8% → 3.4%) without rescuing the
mean (−6.36% → −7.87%) — tokens that go quiet are the ones that stop running.

## Reachability — check it before ranking any identity

Pass-through alone ranks untradeable fingerprints first. `0.101`, `0.001`, `0.0101` read
99–100% pass-through on 20/20 days and graduate **inside their own launch slot**: median
1 print in the band, 1 slot, 0.00 s from 55 to 115. `0.054` (93% in the launch slot) and
`0.0108` (90.5%) are the same. Even the traded `0.0432` is 77.3% unreachable.

Report `pct_band_in_launch_slot` beside every pass-through number. Reachable today:
`0.432` (7.9% in the launch slot), `0.108` (33.9%), `0.54` (38.2%), `0.324` (41.4%).

## Method notes for the next cohort

* **Scope identity first, then gate.** A mean over 76k mints cannot see a rule that fires
  on 200. `2026-08-26`'s 130-decile and 23-gate sweeps searched `>=` floors over the
  pooled cohort; the live rules are *bands* on a single fingerprint. Both were structurally
  blind to the rule family that works.
* **Grade per token, not per print.** Per-print averages weight by churn, which is the
  opposite of concentration.
* **Grade the exit the rule uses.** Clock and fixed-TP exits do not stand in for
  graduation-or-retrace.
* 08-15..08-21 is a **favourable regime for every 3ix identity** — all five turn positive
  there. Any window spanning it flatters. Split before believing.

## Verification

Harness: `scratchpad/step3..step8.py`, honest curve pricing per
[`curve-honest-pricing.md`](../plans/strategies/curve-honest-pricing.md), 115 ms on both
legs, `B = 0.10` SOL, 125 bps a leg. Control: the harness reproduces g12 paying on its own
fingerprint (+15.0%, 107 trades, 14/20 days) and losing on all four neighbours, so the
identity term is doing the work. It models 2 of g8's 5 exit conditions and so understates
g8 (−5.85% where the live rule pays) — usable as a filter, not as a replica. The lab
`simulate` on the real engine remains the arbiter.
