# Wallet books were quoted GROSS, not net — 4 of 6 wallets do not clear the fee (2026-08-18)

Found while re-deriving all six studied wallets on one shared episode builder, prompted by
a challenge that the wallet work was "too bounded to the old analysis method". The method
error is not in the features — it is in the **headline number**.

**Claim.** The `64hP` and `63ot` verdicts in
[`../plans/strategies/wallet-analysis.md`](../plans/strategies/wallet-analysis.md) are raw
curve-side returns that never had the 125 bps/leg protocol fee charged, despite that file
asserting "Every verdict below is net of it". Net of the repo's own SSOT constant, neither
wallet is profitable.

## Evidence

Three independent computations agree, over the 18 ingest-clean days 07-24..08-15.

**1. Reproduce the prior number.** The 08-18 `64hP` study reports, for 08-09..08-15,
completed +5.20% / true book +1.34% on 31,162 episodes, 1,209 unsold, 30,050 SOL deployed.
Same window, my builder: 30,898 episodes, 1,235 unsold, 31,088 SOL deployed — and

| | completed | true book |
| --- | --- | --- |
| **gross** (raw `sell − buy`) | **+4.69%** | **+0.67%** |
| **net** (125 bps/leg) | +2.10% | **−1.81%** |

The prior figures sit next to the *gross* row, ~2.5 pp above the net row. The episode
structure matches to within 1%, so the discrepancy is the fee, not the reconstruction.

**2. Structure-free check.** Summing every leg with no episode logic at all gives `64hP`
gross +0.68% / net **−1.81%** — the net figure matching the episode builder exactly.

**3. The engine agrees.** `kernel.rs::round_trip_multi_leg` charges
`costs_sol = (notional_sol + gross_proceeds) * fee`, which is algebraically identical to
`sell × 0.9875 − buy × 1.0125`. My arithmetic is the kernel's arithmetic.

## Corrected table (18 clean days, bags charged as total loss, tips NOT charged)

| wallet | gross | **net** | net median ep | win | net SOL | day-block bootstrap 95% CI |
| --- | --- | --- | --- | --- | --- | --- |
| `3Xk2` | +5.22% | **+2.62%** | −6.31% | 40.5% | +110 | [1.28, 3.99] · 100% draws + |
| `8dtx` | +4.31% | **+1.73%** | −4.87% | 27.2% | +93 | [1.03, 2.47] · 100% draws + |
| `63ot` | +2.45% | **−0.08%** | **+12.38%** | 66.2% | −1 | [−1.20, 0.93] · 43% draws + |
| `64hP` | −0.03% | **−2.50%** | −1.77% | 45.6% | −1,652 | [−3.08, −1.96] · 0% draws + |
| `omego` | −2.83% | **−5.23%** | −1.39% | 45.2% | −1,439 | [−6.00, −4.48] · 0% draws + |
| `trunoest` | −6.46% | **−8.77%** | −2.98% | 41.7% | −250 | [−12.43, −4.39] · 0% draws + |

Against the documented verdicts: `64hP` "+2.54%/SOL cycled" and "+1.34% true book";
`63ot` "+2.3%/turnover, the one to build from"; `trunoest` "~break-even on landed".
Only `omego`'s verdict was stated correctly — that study *did* compare its +1.81% gross
against the 2.53% fee and refuted it. The inconsistency is inside a single file.

**Robust to the one soft assumption.** The 125 bps is measured on the *buy* side (dev-buy
clustering on `10000/10125`); the sell-side leg is taken from the IDL, not measured here.
Even at a buy-only fee (1.23% round trip) `64hP` (−0.55%), `omego` (−1.07%) and `trunoest`
(−7.07%) stay negative. Only `63ot`'s sign is assumption-sensitive, and it is ~0 either way.
No wallet's tip is charged anywhere, so every figure above is still an upper bound.

## What this does and does not invalidate

**Invalidated — the verdicts and what was seeded from them.** `fs3-*` is calibrated on
`64hP` and `fs4-*` on `63ot` ("everything needed already exists", "thin but positive"
against a margin that is actually zero). Both families are built on wallets that do not
clear the fee. `63ot`'s tip-drag note — "~0.4%/round-trip against a +2.3% margin" — compares
a real cost against a margin that does not exist.

**Not invalidated — every behavioural measurement.** Dip depth, entry age, vsol bands, hold
times, exit-retrace-by-MFE shape, re-entry monotonicity, sizing as a fraction of vsol, the
+1-slot latency table. Those are distributional facts about behaviour and carry no fee term.
The `omego` refutation stands, as does the `8dtx` selection-vs-mechanism finding (run
through simulate, which charges correctly).

## The second finding: both survivors are convexity harvesters, not edge-per-trade traders

| | `3Xk2` | `8dtx` |
| --- | --- | --- |
| net book | +2.62% | +1.73% |
| **net median episode** | **−6.31%** | **−4.87%** |
| win rate | 40.5% | 27.2% |
| book with top 1% of episodes removed | **+0.01%** | **−1.07%** |
| episodes needed to zero the book | **59 (1.01%)** | **29 (0.43%)** |
| weeks positive | 4 / 4 | 4 / 4 |

Neither has a body edge — remove the best 1% and both are at or below zero. But the tail is
**reliably produced**: 4 of 4 weeks positive for both, and inspection of the top episodes
shows clean single-buy/single-sell round trips returning +247…+586% over 5–787 s. It is a
*population* of 3–6x wins, not one outlier. So "tail-driven" here means convex, not lucky.

Three consequences follow directly:

- **A take-profit destroys these strategies**, confirming the note already carried for
  `3Xk2`. The trail is the mechanism, not a detail.
- **They are maximally fragile to per-trade cost.** The body is at ~zero by construction, so
  any added cost — tip, worse fill, one slot of latency — comes straight out of the 99% and
  the 1% cannot carry it alone. This is the structural reason the `3Xk2` clone fell from
  +8.00% to −1.67% on a single slot, and it predicts the same for `8dtx`.
- **`63ot` is the exact opposite shape** — median **+12.38%**, 66.2% win, body-driven — and
  is the only one of the six that would be robust to latency. His problem is not fragility;
  it is that a TP +17 / SL −28 bracket at 66% win yields ~+1.7% gross, under the 2.47% fee.

## Consequence for the wallet programme

Deriving "the deep logic" from `64hP`, `omego` or `trunoest` means reverse-engineering
wallets that lose money net. `64hP` in particular absorbed four sessions of entry search and
one of exit search on the strength of a gross number. Only `3Xk2` and `8dtx` are net
positive, and both are the same convex archetype.

**Standing gate, added:** every wallet book is quoted **net**, with the gross figure beside
it, and no wallet is promoted to a template before its net book clears zero with a
day-block bootstrap CI that excludes zero.

## Data

PG schema `wl`: `w` (wallet map), `clean` (18 ingest-clean days), `ep`/`ep2` (136,808
episodes across all six wallets on one builder), `dayagg` (bootstrap blocks). Schema `x3k`
holds the `3Xk2`-specific tables. Drop when finished.
