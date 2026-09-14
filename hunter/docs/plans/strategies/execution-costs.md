# Execution costs — what a round trip actually costs

Reference for `CostModel` and the round-trip functions in
`core/src/strategies/kernel.rs` (`buy_fill`, `sell_proceeds`, `round_trip_multi_leg`).
Applies to **every** strategy and to every surface: simulate, the sweep, live paper and
the open-position marks all price through these functions, and a live real position
books the wallet flow they reproduce.

> For **worked examples** of each cost model (and of each `FillModel`) priced
> side by side on the same trade, see
> [fill-and-cost-models.md](fill-and-cost-models.md). This doc is the derivation;
> that one is the "which dropdown do I pick" companion.

## The formula

```
fee   = 125 bps (pump curve)
BUY   c      = B / (1 + fee)                        B = buy size, what the order spends
      tokens = c / (P0 * (1 + c / V0))              P0, V0 = spot and priced (virtual) SOL landed into
      paid   = B + fixed_buy
SELL  out    = t * P1 / (1 + t * P1 / V1)           P1, V1 = spot and priced SOL at the sell
      got    = out * (1 - fee) - fixed_sell         the leg that empties the bag also pays close_fee
PnL = sum(got) - paid        PnL % = PnL / paid     (paid = CostModel::capital_sol(B))
```

Fed the pool state each landed in, this reproduces the wallet to the lamport: on the
34 real round trips of 2026-09-13/14 every trade's percent matches the on-chain balance
changes to 0.0000 pp (`a_real_round_trip_reproduces_the_wallet` pins one of them).

| term | size | who charges it |
| --- | --- | --- |
| venue fee | **125 bps/leg** — on top of the curve SOL on a buy, out of the curve's output on a sell | pump.fun |
| venue fee, migrated coin | the PumpSwap pool's own fee, charged the same way on its constant-product amount; it follows the pool's market-cap tier | PumpSwap |
| fixed cost per transaction | base fee + priority on the requested CU limit + tip: **0.000227 SOL a buy, 0.000225 a sell** at the current `.env` | the network + the tip rail |
| close | **5 000 lamports** once per round trip — the rent-reclaim `closeAccount` | the network |
| our price impact | the exact constant-product curve, on the leg's own depth | the bonding curve |
| market slippage | whatever the `FillModel` prices | the market |

The rent a buy parks in its fresh token account (1 574 800 lamports at the current rent)
is **not** a cost: the close returns it. It is SOL the wallet still owns.

## 1. The protocol fee is 125 bps, charged the way `buy_exact_sol_in` charges it

`trades.amount_lamports` is the *curve-side* amount and excludes the fee — measured:
`|Δreserve_lamports| / amount_lamports` = **1.00000** at p25/median/p75 over 5.6M legs.
So an order that spends a round `B` lands `c = B × 10000/(10000 + fee_bps)` on the
curve. Bucketing dev buys by that ratio:

| ratio | implies | count |
| --- | --- | --- |
| **0.987654** (= 10000/10125) | **125 bps** | **16,544** |
| 0.990099 (= 10000/10100) | 100 bps | 310 |

(56,908 dev buys, against the nearest round 0.1 SOL. The same split holds against
round 1.0 SOL: 13,503 vs 283.) On chain, a bot 0.05 SOL buy puts 49 382 715 lamports on
the curve and 617 285 into the protocol and creator fee accounts; a sell of 43 091 829
curve lamports pays 538 649 of fee out of them.

Runs stored before 2026-07-28 are priced at 100 bps and do not compare.
<!-- pt-ok: cutoff, those runs are still in the DB -->

## 2. Our own price impact is the curve itself

On a constant-product curve, `c` SOL into virtual reserves `(vsol, vtok)` returns
`vtok·c/(vsol+c)` tokens, an average price of exactly `(1 + c/vsol)` × the pre-trade
spot. Selling a bag worth `g` at spot returns `g/(1 + g/vsol)`. Both are exact; to first
order each leg costs `size / vsol`:

```
impact_per_leg ≈ size / vsol          (independent of vtok)
```

Each leg prices on **its own** depth — the exit on the pool it sells into, not the
entry's. A pool that drained during the hold charges the exit more.

Two consequences worth internalising:

- A wallet that sizes at a **fixed fraction of the pool** shows a *constant* realised
  slippage. That is why omego's fills are a flat −1.1621% off post-trade spot with a
  stddev of 0.08 over 3,160 buys (he sizes at 1.18% of vsol), and why `64hP`'s are a
  flat +3.82% (1.859% of vsol). If you see suspiciously constant slippage in a wallet
  study, this is why — it is not a bug.
- **A flat `slippage_bps` is wrong in both directions.** It over-charges a small
  order in a deep pool and under-charges a large one in a shallow pool.

### Sizing as a fraction of the pool — `buy_pct_of_vsol`

A rule can size each buy as a percent of the pool's SOL reserve instead of a fixed
amount (`RuleParams.buy_pct_of_vsol`; blank ⇒ the rule's `buy_amount_lamports`). Since
impact is `size / vsol` to first order, that is the knob that holds impact constant across
a liquidity band a fixed size varies over — `fs3-*` gates vsol 40–75, nearly 2×, so a
fixed size charges twice the impact at one end as the other.

It resolves in the kernel at the entry decision (`reduce::resolve_buy_lamports`) against
the depth the fold already holds, so live-real, live-paper and simulate size identically
and everything downstream still receives absolute lamports. Two guarantees worth knowing:
an unusable depth (`NaN` before the first decoded reserve, or a non-positive one)
**falls back to the fixed amount** rather than guessing, and the value is capped at
`MAX_BUY_PCT_OF_VSOL` = 10% at parse — a rail on real money, since this multiplies a
live pool balance into an order.

A percentage is not automatically the right size, only a *constant-impact* one: cost is
U-shaped (§3), so the target to sit near is `sqrt(fixed_per_leg × vsol)`. A percent
tracks that optimum only where the band is narrow enough for a straight line to fit the
square root. **The grouped sweep ignores this knob** and prices every combo at its flat
notional — divergence D8 in [../sweep/sim-parity.md](../sweep/sim-parity.md).

### Which cost model to use

| kind | charges | use when |
| --- | --- | --- |
| `pumpfun_impact` | fee + fixed + close + **the exact curve** | **default.** The honest pairing with any `FillModel` |
| `pumpfun_fee_only` | fee + fixed + close | size-blind; a zero-impact upper bound. The "is there any edge at all?" screen |

There is no flat-slippage model: it double-counted what the fill model already priced,
and being size-blind its error changed sign with buy size, reordering a grid rather than
shifting it. Its wire name no longer decodes.

Impact is **orthogonal to the fill model** and composes with it without
double-counting: a `FillModel` chooses *which market print we transact against*,
impact is *how far our own order moves the curve*. A live trade pays both.

Depth reaches the kernel as `Option<f64>`: `None` charges **no** impact rather than
guessing one. The sweep reads `MetricSeries::priced_reserve_sol` at the entry row and at
the exit fill's row; simulate reads each fill print's own reserve
(`PositionOutcome::entry_reserve_sol`, each exit leg's `reserve_sol`).

## 3. Cost is U-shaped in size — there is an optimum

The fixed cost is SOL per transaction, so it dominates *small* orders; impact grows with
*large* ones. Total size-dependent cost per round trip is, to first order,

```
2·F/B + 2·B/vsol        (F = the fixed cost per transaction)
```

which is minimised at **`B* = sqrt(F · vsol)`**. At the current `.env` (F ≈ 0.000226)
on the measured median pool depth of ~70 SOL that is **~0.126 SOL**. The table below
is at a 0.001 SOL tip (F = 0.001025), where the optimum sits at ~0.27 SOL:

| buy size | impact | tip + priority | size-dependent total |
| --- | --- | --- | --- |
| 0.1 SOL | 0.28% | 2.05% | **2.34%** |
| **0.27 SOL** (optimum) | 0.77% | 0.76% | **1.53%** |
| 0.5 SOL | 1.42% | 0.41% | 1.83% |
| 1.0 SOL | 2.86% | 0.21% | **3.07%** |

Shallower pools move the optimum down. **Recompute both numbers whenever you retune
tips** — the formulas hold, the numbers do not.

**A fixed `buy_amount_lamports` cannot hold impact constant** across a liquidity
band — that is what percent-of-vsol sizing buys, and it is the one real argument for
it. With a fixed size the `liquidity` entry gate is doing double duty as an impact
control, so narrowing that band is also a cost decision.

Pool depth at entry, measured over 3,160 reference buys: p10 48.5, p25 57.3, **median
70.5**, p75 85.8, p90 101.5 SOL.

## 4. The bar a strategy has to clear

Fee alone is 2.5% a round trip. Add the fixed legs and impact at the chosen size, and a
strategy needs roughly **3-4% gross per round trip to break even**, before any market
slippage. That is the number to check a candidate against first — it kills most ideas
before a backtest is worth running.

## 5. Multi-leg (scale-out) round trips

`round_trip_multi_leg` prices **one entry + N exit legs**. Each sell leg pays the fee,
its own fixed cost and its own impact on its own depth; the leg that empties the bag
pays the close. Fixed cost therefore scales with leg count — at 0.1 SOL size an extra
exit leg adds ~0.2% of notional at the current tip. That is the real economic bound on
stage count (see the partial-exits roadmap).

## 6. An open position is not a round trip with one price swapped

`mark_open_bag` prices a bag that is **still held**: what selling it right now would
return, minus what the entry actually took from the wallet. Every open-position figure
goes through it — the Holdings / Home / Console rows (`models::portfolio::mark_bag`),
the per-rule `PositionsSummary::open_pnl_sol`, and the frontend's live mark tip via
`GET /api/meta/cost-model` (`netProceedsSol` mirrors `sell_value_proceeds`).

- **The cost basis is a fact, not a model.** A position's `entry_lamports` is what its
  buy paid (wallet-exact on a real row, `buy_fill` on a paper one), taken pro-rata to
  the tokens still held (`OpenPositionBag::cost_basis_sol`). A holding with no position
  row has only a curve-side average price; its basis is `modeled_cost_basis`, the
  inverse of `buy_fill`.
- **The exit is charged in full**: the exact sell of the held bag into the mint's
  current depth, the fee, the sell's fixed cost and the close. Marking a fresh fill at
  a later price is exactly the round trip at that price
  (`mark_open_bag_equals_the_round_trip_it_would_close`).

A bag needs roughly a +3-4% move to be worth what it cost at the live clip size, the §4
bar restated for an open position.

## 7. What a real position books

A real fill's SOL is its transactions' **payer net flow** (`trades.payer_net_lamports`,
migration 0019): the payer's lamport change over the transaction, with the SOL in token
accounts the payer owns counted as still the payer's. The ingest decodes it from each
transaction's pre/post balances, so it costs no RPC. The buy books what left the wallet,
the sell what arrived; the sell that empties the bag and triggers the rent reclaim also
books the close fee. A real buy's `Fill::price` — the entry price take-profit and
stop-loss measure from — is the pool's spot right after that buy landed
(`SigLegs::entry_price`, off the buy's own trade row). It is the series paper and
simulate enter at, and it holds our own impact, which stays in the pool the position
is marked against, so a real position reads 0 % the moment it fills, as a paper one
does. A stored entry price is never rewritten: a real row an older binary wrote keeps
its execution price. A sell's `Fill::price` is its execution price; it prices nothing.
The closed PnL is then `(exit − entry) / entry`, all-in, and a PnL
tracker that reads the wallet sees the same number.

The PumpSwap fee is read off each swap event (`Trade::venue_fee_bps`: the user-side
amount against the pool's own constant-product amount), kept per token as the newest
swap's (`TokenState::current_venue_fee_bps`), and swapped into the model by
`CostModel::at_venue_fee` for live paper fills, the paper exit-stuck heal and every open
mark. The lake stores no per-swap fee, so a sweep leg on an AMM print prices the curve's
125 bps.

What the kernel cannot know, and a real row books anyway: a tip above
`JITO_MIN_TIP_SOL` from the tip feed.
