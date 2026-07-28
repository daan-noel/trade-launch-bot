# Execution costs — what a round trip actually costs

Reference for `CostModel` / `round_trip_with_costs`
(`core/src/strategies/kernel.rs`). Applies to **every** strategy, not just the
flow-scalper work that produced it. Measured 2026-07-28 on the local lake +
Postgres; no Helius calls involved.

A round trip pays four things. Two are proportional, one is fixed, one scales with
how big you are relative to the pool:

| term | size | who charges it |
| --- | --- | --- |
| protocol fee | **125 bps/leg** (2.53% round trip) | pump.fun |
| tip + priority | **0.001025 SOL/leg**, fixed | Jito + the validator |
| our price impact | **`buy_amount_sol / reserve_sol`** per leg | the bonding curve |
| market slippage | whatever the `FillModel` prices | the market |

## 1. The protocol fee is 125 bps, not 100

`FEE_BPS_PER_LEG` was `100.0` until 2026-07-28. It is **125**.

Measured, not assumed. `trades.amount_lamports` is the *curve-side* amount and
excludes the fee — confirmed by `|Δreserve_lamports| / amount_lamports` = **1.00000**
at p25/median/p75 over 5.6M legs (the ingest never decodes the `fee` /
`fee_basis_points` IDL fields). So a dev who asks to spend a round gross amount lands
a curve-side amount of `gross × 10000/(10000 + fee_bps)`. Bucketing dev buys by that
ratio:

| ratio | implies | count |
| --- | --- | --- |
| **0.987654** (= 10000/10125) | **125 bps** | **16,544** |
| 0.990099 (= 10000/10100) | 100 bps | 310 |

(56,908 dev buys, against the nearest round 0.1 SOL. The same split holds against
round 1.0 SOL: 13,503 vs 283.)

**Every backtest run before 2026-07-28 is 0.5 pp/round-trip optimistic.** The
constant is not persisted per run, so stored `strategy_run_metrics` rows keep the
number they were computed under and are *not* comparable to new ones. Re-run anything
whose margin was inside 0.5 pp.

## 2. Our own price impact — `buy_amount_sol / reserve_sol`

On a constant-product curve, spending `B` SOL against virtual reserves
`(vsol, vtok)` yields `vtok·B/(vsol+B)` tokens. The **average** price paid is
therefore `(vsol+B)/vtok`, which is exactly `(1 + B/vsol)` times the pre-trade spot
`vsol/vtok`:

```
impact_per_leg = B / vsol          (independent of vtok)
```

Two consequences worth internalising:

- A wallet that sizes at a **fixed fraction of the pool** shows a *constant* realised
  slippage. That is why omego's fills are a flat −1.1621% off post-trade spot with a
  stddev of 0.08 over 3,160 buys (he sizes at 1.18% of vsol), and why `64hP`'s are a
  flat +3.82% (1.859% of vsol). If you see suspiciously constant slippage in a wallet
  study, this is why — it is not a bug.
- **The flat `slippage_bps` is wrong in both directions.** It over-charges a small
  order in a deep pool and under-charges a large one in a shallow pool.

### Which cost model to use

| kind | charges | use when |
| --- | --- | --- |
| `pumpfun_impact` | fee + fixed + **real `B/vsol` impact** | **default.** The honest pairing with an explicit `FillModel` |
| `pumpfun_fee_only` | fee + fixed | size-blind; an optimistic bound. Only for old-vs-new comparison |
| `pumpfun_default` | fee + fixed + flat `slippage_bps` | legacy. Double-counts against any explicit fill model |

Impact is **orthogonal to the fill model** and composes with it without
double-counting: a `FillModel` chooses *which market print we transact against*,
impact is *how far our own order moves the curve*. A live trade pays both. What must
never combine is `slippage_bps` and `price_impact` — the former is a crude stand-in
for exactly the latter.

Depth reaches the kernel as `Option<f64>`: `None` charges **no** impact rather than
guessing one. It is read from `MetricSeries::reserve_sol` in the sweep and from the
fill's `TradeLite` in simulate (`PositionOutcome::entry_reserve_sol`). The same entry
depth prices the exit leg, which slightly over-charges it whenever the pool grew
during the hold — the common case on a winner, so the approximation errs toward
pessimism on exactly the trades that matter most.

## 3. Cost is U-shaped in size — there is an optimum

The tip is fixed SOL per leg, so it dominates *small* orders; impact grows with
*large* ones. Total size-dependent cost per round trip is

```
2·F/B + 2·B/vsol        (F = fixed_cost_sol_per_leg = 0.001025)
```

which is minimised at **`B* = sqrt(F · vsol)`**. On the measured median pool depth of
~70 SOL:

| buy size | impact | tip + priority | size-dependent total |
| --- | --- | --- | --- |
| 0.1 SOL | 0.28% | 2.05% | **2.34%** |
| **0.27 SOL** (optimum) | 0.77% | 0.76% | **1.53%** |
| 0.5 SOL | 1.42% | 0.41% | 1.83% |
| 1.0 SOL | 2.86% | 0.21% | **3.07%** |

Shallower pools move the optimum down: on a 45 SOL pool it is ~0.21 SOL.

**A fixed `buy_amount_lamports` cannot hold impact constant** across a liquidity
band — that is what percent-of-vsol sizing buys, and it is the one real argument for
it. With a fixed size the `liquidity` entry gate is doing double duty as an impact
control, so narrowing that band is also a cost decision.

Pool depth at entry, measured over 3,160 reference buys: p10 48.5, p25 57.3, **median
70.5**, p75 85.8, p90 101.5 SOL.

## 4. The bar a strategy has to clear

Fee alone is 2.53%/round trip. At the optimal size add ~1.5%, so **a strategy needs
roughly 4% gross per round trip to break even**, before any market slippage. That is
the number to check a candidate against first — it kills most ideas before a backtest
is worth running. See [flow-scalper-findings.md](flow-scalper-findings.md) for a
worked case where a real, profitable-looking pattern turned out to sit just under it.
