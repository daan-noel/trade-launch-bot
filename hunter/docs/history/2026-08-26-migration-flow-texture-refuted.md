# 2026-08-26 — aiming at migration with multi-window flow texture: predictor real, trade refuted

Thesis under test (operator's, from manual observation): a token whose tape is *active* early
— many trades, high gross/net flow across several windows — migrates more often than one that
moves slowly, and high flow on a *small* trade count is unsafe. Buy the ones with migration
potential, aim at migration.

Cohort: the 6ix launch shape
`[SetComputeUnitLimit, SetComputeUnitPrice, Create_v2, CreateIdempotent, Pump.Fun: Buy,
System Program: Transfer]`, 07-28..08-22, 67,535 tokens enterable at a 20 s decision point.
No wallet features anywhere — devs rotate wallets and bundle trades, so `ix_labels` structure
count (`npid`) stands in for "how many distinct actors".

## Both halves of the thesis are correct as a predictor

P(reach vsol 105), measured inside fixed entry-depth bands so the comparison is not just
re-reading depth:

| entry vsol | ntx 1-5 | 6-15 | 16-40 | 40+ |
| --- | ---: | ---: | ---: | ---: |
| <34 | 0.07 | 0.12 | 0.24 | 0.36 |
| 34-38 | 0.00 | 0.37 | 0.76 | 1.49 |
| 38-44 | 0.00 | 0.46 | 1.04 | 2.60 |
| 44-55 | 0.00 | 3.33 | 1.57 | 5.52 |
| 55-75 | 0.00 | 50.00 | 4.76 | 18.06 |

Monotone in every band, 4-6x lift. `npid` behaves the same way. The "unsafe" half is sharper
than stated: **high trade count with no depth progress is the wash-trade cell** — ntx>100 with
`rise_all < 1` reads P(grad) 0.46 % against 15.87 % for the same trade count with rise >= 10.
Activity only counts when the curve actually moves.

## And the prediction is priced, to a fraction of a point

Payoff is mechanical: `price ~ vsol^2`, so entry depth fixes it. Break-even P against actual P
at a launch entry, split by creation-slot bundle:

| bundle | actual P(grad) | win | loss | break-even P | actual / needed |
| --- | ---: | ---: | ---: | ---: | ---: |
| <8 SOL | 0.89 % | +879 % | -25.3 % | 2.80 % | 0.32 |
| 8-18 | 1.88 % | +630 % | -43.4 % | 6.45 % | 0.29 |
| 18-30 | 8.74 % | +388 % | -57.6 % | 12.9 % | 0.68 |
| 30+ | **28.79 %** | +159 % | -64.8 % | **28.96 %** | **0.994** |

The bundle axis lifts P(migration) **32x** and earns nothing: entry vsol goes 35.5 -> 63.7 and
the payoff collapses exactly as fast. **Any filter that raises P(migration) also raises entry
depth, and the two cancel.** Every flow result must therefore be reported inside a depth
stratum; pooled, it only re-reads depth.

## Two measurement traps that manufacture a false positive

1. **Unsellable bags.** At entry vsol < 32, **46.6 %** of tokens have <= 1 print after entry —
   there is nothing to sell into. Pricing those at `last_px / e_px - 1` books them at **0.0 %**
   and turns the floor band positive (+1.0 %). Booked honestly at -100 % the band reads -21 %
   to -60 %. Any launch-entry study must charge the stuck bag.
2. **Hold-to-death as the failure exit.** Grading failures at the token's last print is the
   worst exit available and is not what any rule does. Re-graded with a stop + timeout the
   whole surface moves ~20 pp, so a refutation measured that way is not a refutation.

## Refuted on money, at every decision point

Real exits (stop breach / vsol-105 target / timeout, whichever comes first; stuck = -100 %).
Tighter stops beat wider monotonically (stop15 -35.0 > stop30 -36.9 > stop50 -38.1 mean), the
recorded signature of zero gross alpha rather than a bad exit. Decision point swept at
10 / 20 / 40 / 80 / 150 s; best grid cell at each:

| decision point | best cell | /day | EV |
| --- | --- | ---: | ---: |
| 10 s | ntx lots, npid hi, accel hi | 14.2 | +0.68 % |
| 20 s | ntx 61-150, npid hi, accel hi | 18.2 | **+2.63 %** |
| 40 s | ntx lots, npid hi, accel hi | 47.1 | -1.58 % |
| 80 s | ntx lots, npid hi, accel hi | 38.6 | -2.26 % |
| 150 s | ntx lots, npid hi, accel hi | 28.6 | +1.37 % |

The best of all of them is **+2.63 % gross against a ~3.5 % cost bar**, and it does not
survive inspection: median **-15.95 %**, win rate 17.3 %, weekly -7.46 / +4.97 / +7.67 / +0.81
(1 of 4 negative), 11 of 26 days positive, and **the top 5 trades of 473 are 177 % of the
profit — removing them gives -2.05 %.** It is five trades, not an edge.

**The migration target is inert in the configurations that do best.** At the winning exit only
**0.34 %** of trades exit via graduation; 29.5 % stop out and 28.7 % are stuck. What the grid
selected is a short-horizon momentum trade wearing a migration label.

## What stays useful

- `npid` (count of distinct `ix_labels`) is a working wallet-free "how many actors" instrument
  and ranks correctly in every depth band. Reuse it.
- **Acceleration** (`gross` in the last fifth of the window / `gross` over the window) is the
  single strongest term: top 5 % reads -0.21 % where every other single term reads -3.9 % to
  -18.4 %. Net/gross buy share is the *worst* term (-18.4 %) — high buy pressure means the move
  already happened.
- The wash-trade cell (high ntx, flat curve) is a clean exclusion.

Companion: [`2026-08-18-graduation-and-identity-space.md`](2026-08-18-graduation-and-identity-space.md),
which reached the same constant-gap conclusion from mid-curve thresholds. This entry extends it
to the launch end and to flow texture, which that session did not test.
