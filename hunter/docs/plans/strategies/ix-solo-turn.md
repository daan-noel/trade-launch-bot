# Solo as a turn (dip already true behind the print)

The named several-wallet burst does not need a dip
([ix-perm.md](ix-perm.md)). A quiet-resume **solo** might: one
first-on-mint working-template buy **into selling / a red candle** that is
already true behind that print. Completing print = the solo buy. Thermometer:
`ixg-solo-turn.sql` + `ixg-solo-turn.py`. Scratch: `ixg.scand`,
`ixg.stape`, `ixg.srun`, `ixg.sslot`, `ixg.sgap`, `ixg.sbefore`,
`ixg.sturn`. Same corpus as [ix-burst-kinds.md](ix-burst-kinds.md).
Full-tape money: `ixg-solo-turn-money.sql` + `ixg-solo-turn-money.py`.
Scratch: `ixg.tmint`, `ixg.trun`, `ixg.tlag`, `ixg.tslot`, `ixg.tgap`, <!-- ref-ok: DuckDB scratch tables, not paths -->
`ixg.tcand` on top of [ix-new-wallets.md](ix-new-wallets.md) `ncand`.

Gap sells = curve sells in slots `[S-5, S)` (the buy-quiet can still print
sells). `last_side` = the last print of any kind strictly before the solo.
`trail` = `m_price_lifetime.trail` at that last print. Dust `< 0.3` and
`>= 4` stay dead.

## The live cell still needs first-on-mint + working + vsol < 46

`tot` in [0.9, 4):

| wal_new | working | gap sell | n | resp / causal |
| --- | --- | --- | ---: | ---: |
| yes | yes | yes | 3,850 | **4.10 / 1.95** |
| yes | yes | no | 8,506 | 2.90 / 1.42 |
| yes | no | yes | 6,571 | 2.63 / 1.14 |
| no | yes | yes | 229 | 0.00 / 0.00 |

Gap sells without the working template do not lift. Repeat + working stays
dead. `vsol >= 46` is **0%** on both sides, same permission as the named
burst. Smaller solos [0.3, 0.9) only move 2.64 → 3.20 with gap sells.

## Dip is the turn; gap sells only help a deep one

Inside `all_new` + working + tot [0.9, 4) + `vsol < 46`:

| trail | gap sell | n | resp / causal |
| --- | --- | ---: | ---: |
| 15–30 | no | 611 | **9.00 / 4.09** |
| 15–30 | yes | 470 | 5.53 / 2.13 |
| 30–60 | yes | 975 | **10.26 / 5.03** |
| 30–60 | no | 1,603 | 6.74 / 3.18 |
| ge60 | yes | 269 | 7.43 / 3.72 |
| lt5 | either | 493 | 3.85–4.10 |

No-dip (`trail < 5`) is the weak side. A moderate dip wants **no** gap
sells (true quiet, then the solo). A 30–60% trail wants gap sells (selling
through the quiet, then the solo). `last_side = sell` and `trail >= 15`
together: **8.55 / 4.07** with gap sells (n=1,696) vs **4.22 / 2.88** with
neither (n=521).

Unfiltered tot [0.9, 2) barely moves (2.33 vs 2.15). The turn only shows
inside the live solo cell, not on every single buy.

Of his live-cell solo fires, **81%** have `trail >= 15`. That is prevalence
plus a real precision lift (7.22% with the dip vs 4.81% without, still
inside `vsol < 46`).

## What this is

Two shapes, not one conjunct on the crowd:

1. **Crowd** — several first-on-mint wallets after a buy-gap, tot [0.9, 4),
   `vsol < 46`. No dip required.
2. **Turn** — one first-on-mint working print after a buy-gap, tot [0.9, 4),
   `vsol < 46`, **already off the peak** (`trail >= 15`, last print often a
   sell). Completing print is still that one buy.

The completing print is the same 0.9–4 SOL print already priced in
[ix-new-wallets.md](ix-new-wallets.md). A dip behind it changes which
solos he fires on; it does not leave remainder past 95 ms.

## Full-tape money (the bounce does not last past 95 ms)

Door, in-window create, 5-slot gap, tot in [0.9, 4), `vsol < 46`,
first-on-mint working template. Turn = that book with `trail >= 15` at
the last print before the solo. `no_dip` = `trail < 15`. Same fill as
[ix-machine-money.md](ix-machine-money.md). One episode per mint.

| Book | lag | n | mean | med | win | SOL | days+ | hold p50 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| turn first-gap | 0 | 22,143 | **+3.43%** | **+1.80%** | 55.7% | +76.1 | **12/12** | 0.1 s |
| turn first-gap | 95 | 22,143 | **−1.03%** | −2.97% | 22.3% | −22.8 | **0/12** | 0.2 s |
| no_dip first-gap | 0 | 11,046 | +3.14% | +1.86% | 55.1% | +34.6 | 12/12 | 0.1 s |
| no_dip first-gap | 95 | 11,046 | −1.83% | −2.98% | 16.3% | −20.2 | 0/12 | 0.1 s |
| trail 30–60 + gap sells | 0 | 6,032 | +3.10% | +1.55% | 54.4% | +18.7 | 12/12 | 0.3 s |
| trail 30–60 + gap sells | 95 | 6,032 | −1.10% | −2.97% | 23.7% | −6.6 | 0/12 | 0.3 s |
| trail 15–30, no gap sells | 0 | 4,477 | +2.92% | +1.30% | 54.0% | +13.1 | 12/12 | 0.1 s |
| trail 15–30, no gap sells | 95 | 4,477 | −1.26% | −2.94% | 19.5% | −5.6 | 0/12 | 0.0 s |
| turn clock 20 s | 0 | 22,143 | +0.30% | −3.10% | 41.4% | +6.7 | 7/12 | 17.1 s |

Zero-lag first-gap is the same body as unfiltered `solo_new_work`
(+3.36% / +1.84%). The dip does not add a money edge at the print: turn
and no-dip medians match. The two thermometer-best cells are slightly
weaker than the broad turn.

95 ms is red **every day** on every turn book, in sample and out. Turn
loses a little less than no-dip (−1.03% vs −1.83%) because the print is
already off the peak; that is not a fill. Clock-20 at 0 ms is
median-negative and OOS-negative: no 20-second tail.

On his mints only, turn first-gap at 95 ms is +0.35% (10/12, median still
−2.80%). On tokens he never touches it is −1.19% (0/12). Same leak as
`solo_new_work`.

Do not fund it. Do not re-price the 0 ms mark as a 20 s trade. Do not
walk another trail/gap-sell subset of this print. Combined with the
crowd, tight packs unfillable:
[ix-combined-machine.md](ix-combined-machine.md).
