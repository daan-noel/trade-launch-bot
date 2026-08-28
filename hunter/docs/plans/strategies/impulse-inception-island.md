# The impulse-inception island

> **REFUTED on execution, 2026-08-22. Do not activate.** Every number below is a
> next-print fill. The edge exists only in the gap between the trigger print and the next
> print, and 95% of it is in trades where that gap is under 10 ms — pairs that are 100%
> same-slot, a median 0.49 ms apart on the feed clock. At a +50 ms fill the rule is
> −22.30 SOL and 1/7 days; at +100 ms a search re-run from scratch finds no positive
> threshold on any axis. Kept as the record of the state search and of the
> instruction-structure cuts, which do hold up.
> [`history/2026-08-22-island-is-a-same-slot-artifact.md`](../../history/2026-08-22-island-is-a-same-slot-artifact.md)
> · [`history/2026-08-22-ix-structure-cuts.md`](../../history/2026-08-22-ix-structure-cuts.md)

The profitable region found by searching market **state** universe-wide rather than fitting
a wallet. Read [signal-search-mandate.md](signal-search-mandate.md) for the standing gates
and [island-search.md](island-search.md) for the decision-point extract this runs on.

Supersedes the entry of [wallet-8dtx-logic.md](wallet-8dtx-logic.md) for live use: the
mechanism here keeps only that wallet's "a modest buy is arriving" idea and replaces both
its quiet gate and its dip band.

## The rule

```
ENTRY   m_flow_window(0.4).net_flow  >= 0.5      # a live buy impulse, one slot wide
  AND   m_price_window(3).rise       <= 9        # the move has not happened yet
  AND   m_state.time > 5, liquidity >= 3      # matches the measured population
  AND   creation ix count <= 5                   # a simple launch transaction
EXIT    stop_loss 3   OR   m_position.retrace >= 20
```

No take-profit, no hold cap. Size 0.05 SOL.

The entry token term and the wide trail are both settled at `NextSlotFirst`, where they
reverse what a next-print fill reports. Sections
[the token filter](#the-token-filter-a-launch-client-not-a-count) and
[the exit](#the-exit) carry the measurement; the discovery numbers below are next-print.

**Buy the first slot of a buy impulse, before price moves.** The 0.4 s window is about one
Solana slot, so `net_flow` there is "this slot is a net buy of ≥ 0.5 SOL". `rise(3)` is the
conditioner: enter at the inception of a move, never after it.

## Where the money comes from

7 cohort days 2026-08-13..19, one episode per mint, exit `stop 3 / trail 5`:

| | n | SOL | exp | days + |
| --- | ---: | ---: | ---: | ---: |
| blind (earliest decision point of every mint) | 66,574 | −14.37 | −0.00022 | 2/7 |
| `net_flow(0.4) >= 0.5` alone | 46,226 | **+119.10** | +0.00258 | 7/7 |
| `rise(3) <= 9` alone | 64,950 | −20.27 | −0.00031 | 1/7 |
| **both** | 40,044 | **+132.37** | +0.00331 | 7/7 |

The impulse term is the engine and carries ~90% of the result. `rise(3)` is worthless
standalone and pays only as a conditioner on top of it (+11%).

## Terms that are tested and rejected

Each is added **on top of** the island, so the column is what the term costs:

| added term | n | SOL | exp |
| --- | ---: | ---: | ---: |
| `gross_flow(10) <= 15` — the 8dtx quiet gate | 30,632 | +100.15 | +0.00327 |
| `gross_flow(10) <= 8` | 25,452 | +84.17 | +0.00331 |
| `m_price_lifetime.trail` 5–60 — the dip band | 33,698 | +99.34 | +0.00295 |
| `time > 10` | 33,479 | +106.70 | +0.00319 |
| `net_flow(3) >= 0.8` — the 8dtx trigger | 31,180 | +95.47 | +0.00306 |
| `liquidity <= 50` | 39,775 | +132.43 | +0.00333 |

**A quiet gate is a blunter `rise(3)`, not an independent signal.** Both ask *has this
already moved?*; `gross_flow` answers it through activity, `rise` through price. They
overlap heavily (75% of quiet rows are also un-risen rows) yet correlate only **0.38**, and
the direct measure wins on total and per-trade alike: `net_flow(0.4) AND gross_flow(10)<=15`
scores +95.06 (exp 0.00275) against the island's +132.37 (exp 0.00331). Stacked on top of
`rise(3)` the gate removes 23% of trades at unchanged expectancy — pure volume loss.

The dip band is worse than redundant: it costs SOL **and** lowers expectancy.

**`liquidity <= 50` is free** — keep it only if a separate reason wants the cap.

**One quiet gate survives on quality alone:** `gross_flow(30) <= 25` gives the best
expectancy measured anywhere here (+0.00356) at a lower total (+103.06). It returns the
moment trade count is capped or per-trade size rises; at unlimited count, total SOL rejects it.

## The exit

Twenty `(stop x trail)` policies, no TP and no hold cap, scored inside the island:

| | SOL | exp |
| --- | ---: | ---: |
| stop 3 / trail 5 | **+129.45** | +0.00332 |
| stop 3 / trail 8 | +128.82 | +0.00330 |
| stop 1 / trail 5 | +128.30 | +0.00329 |
| stop 1 / trail 8 | +126.30 | +0.00324 |
| no stop / trail 35 | **−9.7** | — |

The top eight sit within 2.5% of each other, so the exit is a plateau and not a fitted
choice. Two things it does settle: **a stop is required** (the no-stop wide-trail corner is
the only negative cell), and **tight beats wide** — the opposite of the 8dtx armed-18 shape,
which ranks near the bottom on this island.

## The token filter: a launch client, not a count

The creation bundle (`fsb_sol`) is not a selector - both sides of `fsb 5` pay about the
same. The **instruction count of the creation transaction** is, and it only shows up once
the fill is honest:

| scope | n | week NS | IS | OOS | days + | per trade | week FIW |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| island, unscoped | 40,034 | +41.16 | +17.19 | +23.96 | 7/7 | +2.06% | +123.09 |
| **`n_ix <= 5`** | **16,578** | **+48.71** | +24.77 | +23.94 | 7/7 | **+5.88%** | +77.39 |
| `n_ix >= 6` (the complement) | 23,456 | **+0.93** | -2.16 | +3.09 | **3/7** | **+0.08%** | +64.84 |

Per trade is **net** - money over capital at 0.05 SOL a trade, after fee, impact and fixed
cost. The gross price move is 3.8 points higher and is not what a book earns.

**Complex launches pay nothing to a reactor.** The `n_ix >= 6` half is 59% of the island's
trades and earns +64.84 SOL at a next-print fill but **+0.93 at a next-slot fill, positive
on only 3 of 7 days**. Its entire contribution is same-slot fill luck. Simple launches keep
their edge across the slot boundary, which is why the filter *raises* the total while
removing 59% of the trades.

Inside `n_ix <= 5` one launch client carries most of it:

| scope | n | week NS | IS | OOS | days + | per trade |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `SetCULimit, SetCUPrice, Create_v2, ATA:CreateIdempotent, BuyV2` | 7,510 | +33.81 | +17.37 | +16.44 | 7/7 | **+9.00%** |
| rest of `n_ix <= 5` | 9,068 | +14.9 | | | 7/7 | +3.29% |

That family is the current standard creation path. It is 18.5% of the island's decision
points, pays 69% of the money at `n_ix <= 5`, and is the most latency-robust scope
measured: it keeps 72% of its next-print edge against the unscoped island's 33%.

**Use the count for volume, the family for quality.** Both are creation-time facts, so
neither can look ahead, and both are settled on IS and confirmed on OOS.

## Gates it clears

- **Perturbation.** A smooth plateau over `net >= 0.4..0.75` x `rise <= 5..14` (119–129 SOL);
  no threshold is load-bearing. Dropping `rise` entirely costs ~14 SOL, which is how the
  term earns its place.
- **Placebo.** Same mints, same trigger times, entry shifted: **+66.55 → +2.99 at +30 s**
  (IS) and +62.90 → +6.60 (OOS). The edge is momentary state, and a time shift kills it.
- **Same-mint control.** One random decision point per selected mint scores −34.49 against
  the island's +66.55 (z 94). The edge is *when*, not *which token*.
- **Out of sample.** Fitted on 08-13..16, confirmed on 08-17..19 at matching magnitude
  (+64.88 → +61.42), 7/7 days positive.
- **Exit-independent ranking.** Region order is unchanged under a fixed exit, so it is not
  exit cherry-picking.

## Execution risk: priced, and the island survives it

The trigger is a one-slot measurement, and **~51% of `FirstInWindow` fills land in the
signal's own slot** — a block our transaction had to already be inside. Pricing **every**
row at the first print of a strictly later slot (`NextSlotFirst`, the honest reactor
counterfactual) rather than only the self-selected rows that happen to have a slot gap:

| fill model | week | IS 08-13..16 | OOS 08-17..19 | days + | per trade |
| --- | ---: | ---: | ---: | ---: | ---: |
| `FirstInWindow` | +132.37 | +67.64 | +64.73 | 7/7 | +6.61% |
| **`NextSlotFirst`** | **+36.95** | **+15.65** | **+21.30** | **7/7** | **+1.85%** |

**The island keeps 28% of its edge under a one-slot delay and stays positive on all seven
days**, with OOS above IS. Same 40,044 episodes, same `stop 3 / trail 5` exit, 0.05 SOL.
This supersedes the +9.40 / +15.25 lower bound estimated from the gap-only subset: that
subset is quieter tape by construction, so it understates the island.

Size the expectation to the slower number. Half the `FirstInWindow` edge is a same-slot
fill the bot wins only when it is already in the block, so real execution lands between the
two rows and closer to the lower one as latency grows.

## The book it produces

At `NextSlotFirst`, 0.05 SOL, `stop 3 / trail 20`, against the two reference operators
reconstructed from the same tape:

| | trades/day | win | median | expectancy | >= +100% | week |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 8dtx | 501 | 28.4% | -4.58% | +2.39% | 1.8% | +60.5 SOL |
| 3Xk2 | 383 | 40.1% | -6.44% | +7.89% | 4.7% | +155.1 SOL |
| rule A (`n_ix <= 5`) | 2,368 | 26.4% | -3.96% | +5.88% | 3.9% | +48.7 SOL |
| rule B (standard client) | 1,072 | 29.5% | -3.77% | **+9.00%** | 4.9% | +33.8 SOL |

The shape is the target shape: a **negative median**, a **sub-30% win rate**, and a right
tail that carries everything. Nothing about the rule was fitted to reproduce it.

**The token filter buys stability, not just money.** Unscoped, the bottom 90% of episodes
lose -132.28 SOL and the top 1% pays +59.02 - the whole book rides on the tail. Under rule
B the bottom 90% loses only -16.52 against a top 1% of +15.27. Same mechanism, a quarter of
the tail dependence, and an IS/OOS split that stops swinging (+17.19/+23.96 unscoped
against +17.37/+16.44).

## Dead zones do not bind here

The universe-wide dead zones - `liq > 57`, `net_life > 52 AND liq > 55`, and a busy tape at
`gross(60) > 74 AND net_life > 35` - are worth **+0.79 SOL a week over 400 of 40,034 rows**.
The impulse-inception trigger only fires on tokens that have not moved yet, so it has
already excluded them. Keep the exclusion as a cheap guard; do not expect it to pay.
