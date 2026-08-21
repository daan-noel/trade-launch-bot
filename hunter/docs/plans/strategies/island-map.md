# The island map

The islands this market actually has, each with its mechanism and a runnable rule. Found by
partitioning the **whole** decision-point space, not by fitting a wallet.
[island-search.md](island-search.md) holds the extract method,
[signal-search-mandate.md](signal-search-mandate.md) the standing gates.

All numbers: 7 token-creation cohorts 2026-08-13..19, universe-wide, 0.05 SOL, one episode
per mint, `NextSlotFirst` fills, kernel cost math on the virtual reserve. Thresholds are
selected on 08-13..16 and the holdout is read once.

## The map in one picture

```
                      liquidity <= ~64          <- above this the token has already run
                             |
                    creation ix count <= 5      <- above this the edge dies at +1 slot
                             |
                    gross_life <= ~148          <- above this the move has happened
                             |
        +--------------------+--------------------+
        |                    |                    |
   ISLAND 1             ISLAND 2             ISLAND 3
   absorption           quiet accumulation   impulse inception
   (read over 60s)      (read over 30s)      (read over 0.4s)
```

The first three conditions are **where to stand**. The three islands are **when to buy**,
and they are the same event - demand arriving at a token that has not been re-priced yet -
read at three different timescales.

## The three islands

| | trades/day | week | IS | OOS | days + | net/trade | win | median | runner |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 absorption | 1,477 | **+46.34** | +27.32 | +19.02 | 7/7 | +8.96% | 27.9% | -3.99% | 10.43% |
| 2 quiet accumulation | 236 | +10.01 | +6.87 | +3.14 | 7/7 | +12.09% | 31.2% | -4.49% | 12.02% |
| 3 impulse inception | 2,368 | **+48.71** | +24.77 | +23.94 | 7/7 | +5.88% | 26.4% | -3.96% | 8.46% |
| 1 AND 3 (both agree) | 693 | +31.71 | +17.34 | +14.37 | 7/7 | **+13.06%** | 34.3% | -3.68% | **13.84%** |
| **union of 1, 2, 3** | 2,736 | **+52.80** | +29.76 | +23.04 | **7/7** | +5.51% | 25.2% | -4.05% | 8.09% |

`net/trade` is money over capital at 0.05 SOL, after fee, impact and fixed cost - the figure
a wallet book is quoted in. The gross price move runs ~3.8 points higher; ranking on it
flatters every rule and is not what the account earns.

Every one carries a **negative median** and a **sub-35% win rate**. That is the shape a
convexity book has to have, and nothing here is fitted to produce it.

### Island 1 - absorption

```
ENTRY  liquidity <= 64  AND  creation ix count <= 5  AND  gross_life <= 148
  AND  m_flow_window(60).buy_share > 0.84      # 84% of a minute's SOL volume is buys
  AND  m_flow_window(30).trade_count > 8       # and the tape is live, not dead
EXIT   stop_loss 3  OR  m_position.retrace >= 20
```

**Somebody is buying everything that is offered.** Over a full minute, five SOL in six
arrives on the buy side while sellers keep hitting into it. The buying is patient and
size-weighted rather than frantic: the deeper split shows the best sub-case is
`buy share by SOL > 0.87` while `buy share by trade count <= 0.78` - **a few large buys
absorbing many small sells** (2,037 episodes, 14.53% runner - the highest of any leaf). Retail is
distributing into someone who wants the whole float.

### Island 2 - quiet accumulation

```
ENTRY  liquidity <= 64  AND  creation ix count <= 5  AND  gross_life <= 148
  AND  m_flow_window(60).buy_share > 0.75
  AND  m_flow_window(30).trade_count <= 8      # almost nobody is watching
  AND  m_flow_window(30).net_flow > 6.5        # but 6.5 SOL net has still gone in
```

**Real money arrives while the tape is empty.** Fewer than nine trades in thirty seconds,
yet 6.5 SOL of net inflow - so the average trade is large and one-directional. Highest
per-trade of the three single islands (+12.09% net) and the lowest volume (236/day): it is
a rare configuration, and it is the purest form of the same signal.

### Island 3 - impulse inception

```
ENTRY  creation ix count <= 5
  AND  m_flow_window(0.4).net_flow >= 0.5      # a buy impulse, one slot wide
  AND  m_price_window(3).rise <= 9             # the move has not happened yet
```

**Buy the first slot of a buy impulse, before price moves.** The full derivation, gates and
exit surface are in [impulse-inception-island.md](impulse-inception-island.md). It carries
the most volume of the three and the most balanced IS/OOS split.

### Where they overlap, and where they do not

- At the **moment** level they are nearly disjoint: island 1 and island 2 share **0%** of
  their decision points by construction, and island 1 shares only 10.9% with island 3.
- At the **token** level they overlap heavily - 82% of island-1 tokens also produce an
  island-3 entry. **Same tokens, different moments**, which is exactly the intended
  structure: the token filter picks the population, each island picks an instant inside it.
- Removing the other two leaves each island almost intact (1: +35.80, 2: +22.84, 3: +36.53,
  all 7/7), so none is an artefact of the others.
- **When islands 1 and 3 agree, quality peaks**: +13.06% net/trade, 34.3% win, 13.84%
  runner. Use the intersection when trade count is capped, the union when it is not.

## The dead zones

37 of the 58 leaves lose money on the fit half. Their union is **-79.01 SOL over 60,773
episodes, 0 of 7 days positive** - roughly 8,700 opportunities a day that cost about
1 percentage point each after cost. Three conditions account for nearly all of it:

| dead zone | condition | runner rate |
| --- | --- | ---: |
| the token has already run | `liquidity > 64` | 0.00% - 1.84% |
| a complex launch, still young | `ix count > 5 AND age <= 23s` | 2.6% - 3.2% |
| a complex launch, thin participation | `ix count > 5 AND trades_life <= 318 AND age <= 107s` | 3.18% |

The last one alone is **-54.71 SOL over 33,966 episodes, 0/7 days**. Not entering is worth
more than any exit tuning: an exit can only shrink a loss it has already taken.

**Why `liquidity > 64` is dead:** liquidity is real SOL in the pool, graduation sits near
85, and price grows roughly with the square of the pool. Past ~64 there is not enough room
left below the ceiling for a +50% move to exist, so the right tail the book depends on is
gone while the left tail is unchanged.

## Gates these clear

- **Tie fraction.** `buy_share(60)` has 5.3M distinct values and its most common value holds
  1.33% of rows. It is a real axis, not a near-constant one.
- **Perturbation.** 15 cells around island 1 over `buy_share 0.80..0.94` x
  `trade_count 8..30`: **all 15 positive on all 7 days**, +11.97 to +47.50 SOL. Per-trade
  rises with the buy-share threshold and volume falls with it, smoothly. No threshold is
  load-bearing.
- **Same-mint control.** A random decision point in the *same* token loses money on 6 of 7
  days against island 1's 7/7. Picking the token is not the trade.
- **Placebo.** Shifting the entry later in the same token collapses it: +30s takes island 1
  to 2/7 days and +120s to 0/7, both with the gross move down two thirds. The edge is
  momentary state.
- **Out of sample.** Every island and the union are positive on the fit half, the holdout
  half, and all seven days individually.

## Exit, re-entry, size and latency

**Each island's own exit surface confirms the shared exit.** All 25 `(stop x trail)` cells
are positive on all 7 days for every island. Re-fitting per island gains under 1% on the fit
half and *loses* on the holdout - island 1's IS-best `stop 8 / trail 25` reads OOS +17.18
against the inherited `stop 3 / trail 20` at +19.02. **Keep `stop 3 / trail 20` everywhere.**

**Re-entry is where the volume is.** Allowing repeated non-overlapping holds per token per
day, instead of one:

| | trades/day | week | IS | OOS | days + | net/trade |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| union, one per token | 2,736 | +52.80 | +29.76 | +23.04 | 7/7 | +5.51% |
| **union, repeats** | **11,278** | **+112.92** | +57.23 | +55.69 | **7/7** | +2.86% |
| island 3, repeats | 10,642 | +101.45 | +47.85 | +53.59 | 7/7 | +2.72% |
| island 1, repeats | 1,802 | +53.92 | +32.60 | +21.32 | 7/7 | +8.55% |

Re-entry **doubles the money** and balances the halves (IS +57.23 against OOS +55.69).
Island 3 gains most (4.5x the trades) because its trigger recurs inside a token; island 2
barely moves - it is a once-per-token event.

**Size.** Net per-trade peaks at **0.10 SOL** (+3.09%), which is the `sqrt(F x vsol)`
optimum the cost model predicts independently. Total SOL keeps rising past it because size
grows faster than impact, but per-trade quality falls: +2.86% at 0.05, +3.09% at 0.10,
+2.88% at 0.20, +2.13% at 0.40.

**Latency tolerance is the strongest result here.** The union stays 7/7 days positive out to
five slots late:

| fill | next print | next slot | +2 slots | +3 slots | +5 slots |
| --- | ---: | ---: | ---: | ---: | ---: |
| week SOL | +183.52 | +112.92 | +84.55 | +65.06 | +49.14 |
| net/trade | +4.82% | +2.86% | +2.13% | +1.64% | +1.25% |
| days + | 7/7 | 7/7 | 7/7 | 7/7 | 7/7 |

Decay is gradual, not a cliff. This is what separates these islands from every copy-trade
attempt in the history file, each of which died within one slot.

## Which union to run: 1 or 3

**Island 2 is real but redundant.** Standalone it has the best per-trade of the three, yet
adding it to islands 1 and 3 is worth +0.35 SOL of +112.92 - three tenths of one percent,
for 41 extra trades a day. Its moments are already inside the other two.

| | trades/day | week | IS | OOS | days + | net/trade |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| union 1 or 3, re-entry | 11,237 | +112.57 | +56.63 | +55.93 | 7/7 | +2.86% |
| union 1 or 2 or 3, re-entry | 11,278 | +112.92 | +57.23 | +55.69 | 7/7 | +2.86% |

**Run `island 1 OR island 3`.** Keep island 2 documented as a distinct mechanism, not as a
third trigger to implement.

## Concurrency: the cap is what binds

Unlimited concurrent positions is not a live setting. Capping the whole book:

| max concurrent | 3 | 5 | 10 | 20 | 40 | unlimited |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| week SOL | +28.23 | +47.08 | +80.27 | +107.09 | +112.48 | +112.92 |
| trades/day | 3,476 | 5,551 | 9,143 | 11,008 | 11,251 | 11,278 |
| days + | 7/7 | 7/7 | 7/7 | 7/7 | 7/7 | 7/7 |

**A cap of 20 keeps 95% of the edge.** Net per trade barely moves (+2.32% at 3 against
+2.86% unlimited), so the cap rations volume rather than selecting better trades.

**The `1 AND 3` intersection is cap-proof**: 792 trades/day unlimited, and a cap of **5**
already takes 790 of them - 99.7% of the money at **+12.40% net per trade**. That is the
capital-efficient operating point, and it is the one to run first.

## Forward test: cohort 2026-08-12, nothing refitted

An unseen creation day, every threshold, the exit and the size frozen from the 08-13..16 fit:

| | trades | fit avg/day | SOL | fit avg SOL/day | net/trade | win | median | runner |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 absorption | 1,595 | 1,477 | **+10.02** | +6.62 | +12.57% | 30.0% | -3.93% | 12.16% |
| 3 impulse | 2,623 | 2,368 | **+8.55** | +6.96 | +6.52% | 28.3% | -3.93% | 8.65% |
| union 1 or 3 | 2,947 | 2,701 | **+10.08** | +7.53 | +6.84% | 27.3% | -4.00% | 8.86% |
| 1 AND 3 | 776 | 693 | **+6.15** | +4.53 | +15.85% | 37.0% | -3.62% | 14.56% |
| union 1 or 3, re-entry | 11,858 | - | **+21.51** | +16.13 | +3.63% | 24.5% | -3.77% | 7.04% |

**Every rule beats its fit-period average, by 1.2x to 1.5x**, at matching trade counts and
an identical book shape (median near -4%, win near 27%, runner near 9%). One day only, and
it sits *before* the fit window rather than after, so it tests regime-independence rather
than forward decay - but nothing was tuned to it.

## Shipping it: what the engine already has

### The kernel's impact basis — FIXED

`leg_impact` charges `size_sol / reserve`, and every caller used to hand it the **real**
reserve (`vsol - 30` on the curve) because one `TradeLite` field served both the
`liquidity` metric and the impact depth. The constant-product identity is `B / vsol`, so
that overcharged by `vsol / (vsol - 30)`. These islands sit in thin pools by
construction - median liquidity **17.8 SOL**, so the **median overcharge was 2.68x** and
42% of entries were charged more than 3x their real impact.

`TradeLite` now carries `priced_reserve_sol` beside `reserve_sol`, and the sweep, the
oracle, the live producer and the readout all charge impact against the priced one.
The depth is **carried, not derived**: the real reserve is clamped at zero, so it cannot
be inverted back to `vsol` exactly where the pool is thinnest and the error is largest.
`impact_depth_is_the_priced_reserve_not_the_real_one` in `sweep::projection` guards it.

What the old basis cost, and therefore what the fix returns:

| rule | size | correct | kernel | gap |
| --- | ---: | ---: | ---: | ---: |
| union 1 or 3, re-entry | 0.05 | +112.57 | +82.22 | **-27%** |
| union 1 or 3, re-entry | 0.10 | +243.37 | +130.40 | **-46%** |
| **1 AND 3, re-entry** | 0.05 | +34.37 | +32.42 | **-5.7%** |
| 1 AND 3, re-entry | 0.10 | +69.90 | +62.33 | -10.8% |

**The concentrated rule was nearly immune, the high-volume union was not.** A thin
+2.86%/trade edge loses a quarter of itself to the overcharge; a +12.40% edge loses a
twentieth. At 0.10 SOL the old basis even flipped the union's one-per-token stability to
6/7 days. Backtests run before the fix understate every island, most where the pool is
thinnest — re-run anything being compared against these numbers.

### Metric availability — the two gaps are now filled

`m_flow_window.buy_share` (percent of the window's SOL that is buys, `NaN` on an empty
window) and `m_snapshot.ix_count` (the creation transaction's instruction count, seeded
once from `TokenCreated` and static thereafter) are both in the registry, so all four
rules are authorable directly. `ix_count` is seeded on the live path (`reduce`), the
sweep (`build_series_with_flow`) and the readout replay; an unseeded path reads `NaN`,
which matches no condition rather than silently matching every one.

The measured substitutions below still stand, and one is worth keeping on its merits:

- **`UniqueWallets(30) > 6` replaces a raw trade count outright** - and slightly improves
  it: +48.23 SOL against +46.34, 7/7 days, +9.26% net/trade against +8.96%. The two axes
  correlate 0.91-0.92, so no trade-count metric was added; the shipped rules use
  `unique_wallets`.
- **Fingerprint scope replaces `ix count <= 5`** at a cost: scoping island 3 to the standard
  client takes +48.71 to +33.81 (-31%) but lifts net/trade from +5.88% to +9.00%. It is a
  subset of the population, not a different one.

### The order to ship in

| tier | needs | rule | trades/day | week | net/trade | days + |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| **0** | nothing | island 3, fingerprint-scoped | 1,072 | +33.81 | +9.00% | 7/7 |
| **0** | nothing | same, with re-entry | 4,550 | +56.74 | +3.56% | 7/7 |
| **1** | `buy_share` metric | 1 AND 3, scoped, `uw > 6` | 379 | +21.45 | **+16.16%** | 7/7 |
| **2** | + ix-count term | 1 AND 3, `ix <= 5`, `uw > 6` | 720 | +33.35 | +13.23% | 7/7 |
| **2** | + ix-count term | union 1 or 3, re-entry | 11,237 | +112.57 | +2.86% | 7/7 |

Both tiers are now unblocked: `buy_share` and `ix_count` ship in the registry, so every
rule above is authorable as written, `ix count <= 5` spans the whole population rather
than one fingerprint, and the fingerprint scope is optional rather than a workaround.

### The four rules, as authored

`hunter/engine/tests/island_rules.rs` holds all four in the canonical
`strategy_rules.params` JSON and asserts each one parses through `RuleParams::parse` -
the same gate a rule save runs - so a registry typo fails the build instead of producing
a rule that silently never matches. They are kept as **four separate rules**, one per
island plus the conjunction, so each can be armed and measured on its own.

Every one shares the settled exit and carries **no take-profit**: `stop_loss 3` plus
`m_position.retrace >= 20`.

## Confirmed on the engine

Seeded by [`seed-island-rules.sql`](../../../scripts/seed-island-rules.sql) and run
through the lab's own `POST /api/strategies/simulate` - the real kernel, not the search
harness. One creation day (2026-08-13), `first_in_window`, `pumpfun_impact`, 0.05 SOL,
26,470 tokens matched:

| rule | entered | engine/day | harness/day | win | median | mean | PF |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `isl-3-impulse` | 2,008 | **+3.56** | 2,368 | 28.3% | -9.43% | +3.55% | 1.30 |
| `isl-1and3-confirmed` | 678 | **+3.25** | 693 | **32.4%** | -8.67% | **+9.57%** | **1.84** |
| `isl-1-absorption` | 1,282 | +1.86 | 1,477 | 24.6% | -10.13% | +2.91% | 1.22 |
| `isl-2-quiet-accum` | 298 | **-1.46** | 236 | 21.1% | -14.63% | -9.78% | 0.60 |

**The entries transfer.** Engine trade counts land within ~15% of the harness on every
rule, so the conditions mean the same thing in both. **The ranking transfers too**:
`1 AND 3` is the best rule by win rate, mean and profit factor, exactly as derived.

**The engine is more pessimistic, for two identified reasons - not noise:**

- **Dead exits book near -100%** (`worst_pnl_pct` = -102). This method marks a dead exit
  at the curve instead, because a pre-migration bonding curve is always its own
  counterparty. 3% of island-3 exits are dead, so the convention alone is worth ~2pp.
- **The stop fills ~6pp past its threshold** - median -9.4% on a 3% stop. The first print
  past a threshold has already gapped; that is the known ~2.5pp exit-fill tax, and it
  compounds with booking dead at zero.

**`isl-2-quiet-accum` does not survive the engine** (-1.46 SOL, PF 0.60, on only 298
trades). It was already the island that adds 0.3% to the union and is the smallest
sample of the four. Treat it as **refuted for live use** until re-derived; keep islands
1, 3 and their conjunction.

**Running the full week needs scoping.** The broad fingerprint matches all 172,477 tokens
in the window and the load phase alone exceeds this box's RAM. Run day by day, or narrow
with `mints`.

## What this does not claim

- **These are three readings of one mechanism, not three unrelated trades.** They share a
  token population and a direction. Treat the union as one strategy with three triggers.
- Re-entry assumes each hold closes before the next opens in the same token. Concurrency
  across *different* tokens is uncapped here; a live cap cuts the trade count first.
- Impact is charged as `B / vsol` per leg. That is the correct constant-product identity,
  but at 0.4 SOL into a 3 SOL pool it is an extrapolation, not a measurement.
