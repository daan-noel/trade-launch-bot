# Harvest path after the combined-machine fill

Same fillable events as [ix-combined-machine.md](ix-combined-machine.md)
(door + 5-slot gap + `vsol < 46` + not-all-repeat + working completing
print, crowd or turn; tight packs out). Fill = last print with
`ts <= fire + 95 ms`. Marks are last-print **gross** return from that
fill. Money walks charge 125 bps/leg + own `B/vsol` at B = 0.10 SOL.
One episode per mint except the re-entry row. Window 2026-08-11 ..
2026-08-23 exclusive.

Reproduce: `ixg-harvest-path.py` (marks) + `ixg-harvest-money.py`
(exits). Scratch: `ixg.cm_cand`. 8dtx's own median hold is ~20 s
([wallet-8dtx-latch.md](wallet-8dtx-latch.md)); the first-gap 0.2 s
hold is a scalp of the crossing slot, not that harvest.

## Path (gross, no exit)

| Book | n | r1 med | r8 med | r20 med | r20 p>0 | r20 p>+10 | mfe med | mae med | dump | wave |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| fillable | 40,254 | 0.0% | −1.25% | **−8.43%** | 34.4% | 21.6% | +12.7% | −31.6% | 86% | 14% |
| HIS mints | 3,082 | +0.46% | +2.56% | **+3.25%** | 59.0% | 37.6% | +47.4% | −24.3% | 60% | 40% |
| OTHER | 37,172 | 0.0% | −1.81% | **−9.92%** | 32.4% | 20.2% | +10.9% | −32.2% | 88% | 11% |

Fill stays in the trigger slot 97% of the time (median 1 extra print
inside 95 ms). 63% of peaks sit **after** the first 2-slot buy pause
(`post_gap`); the spike exists. The body does not ride it: median
return is 0 at 1–4 s and down from 8 s. MFE median +12.7% against MAE
median −32% is the same lottery the mandate already records.

`after = wave` (post-gap buy SOL > sell SOL, ≥2 buys) is 14% of
fillable and those paths mark r20 median **+9.2%**. `dump` is 86% and
marks r20 median **−12.3%**. Wave membership uses the forward tape; it
is not an entry gate.

His-mint paths are the harvest: median stays positive through 20–60 s,
arm-at-+10 rate 82.8% at 6.9 s. That list is who he traded, not a live
filter on this event.

## Money at 95 ms

Live second-wave confirm: after the first 2-slot buy silence, two
further buys before a sell or a second silence → HOLD; else dump/death
exit. HOLD is clock-20 from fill, or arm-10 / trail-18 (unarmed falls
back to the next buy-gap).

| Book | exit | n | mean | med | win | SOL | days+ | hold p50 |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| fillable | first-gap | 40,254 | −0.98% | −2.98% | 23.2% | −39.6 | **0/12** | 0.3 s |
| fillable | clock 20 s | 40,254 | −7.28% | −11.24% | 29.1% | −293.1 | **0/12** | 17.5 s |
| fillable | trail 10/18 | 40,254 | −1.81% | −3.00% | 17.6% | −72.7 | **0/12** | 0.3 s |
| fillable | harvest_clock | 40,254 | −2.36% | −3.04% | 23.1% | −94.9 | **0/12** | 1.1 s |
| fillable | harvest_trail | 40,254 | −1.76% | −3.03% | 23.6% | −70.7 | **0/12** | 1.1 s |
| fillable harvest_clock re-entry | harvest_clock | 79,114 | −1.45% | −2.96% | 25.7% | −114.4 | **0/12** | 0.9 s |
| HIS mints | clock 20 s | 3,082 | **+6.15%** | +0.19% | 50.5% | +19.0 | **12/12** | 18.9 s |
| HIS mints | harvest_clock | 3,082 | +1.26% | −2.66% | 36.3% | +3.9 | 10/12 | 1.1 s |
| OTHER | harvest_clock | 37,172 | −2.66% | −3.05% | 22.1% | −98.8 | **0/12** | 1.1 s |

Harvest_clock confirms a wave on 4,579 / 40,254 (11%). The other 88%
leave at dump or death. That split does not lift the full-tape book
above first-gap, and first-gap is already 0/12.

His-mint clock-20 at this fill is a harvest (12/12, OOS +6.02%). The
mint list is lookahead: it is tokens he already traded, not a gate
available at the completing print. Requiring a live second-wave
confirm on those same mints cuts the hold (1.1 s) and the median goes
negative.

## What this is

The unconcentrated fillable body has no harvest after-move. A 20 s
hold, an armed trail, and a stay-on-wave / leave-on-dump exit all lose
at 95 ms, every day. His-mint clock-20 at the same fill is a harvest
and is lookahead (the mint list). Event-level `he1` is also a harvest
and is lookahead (his send). Live concentration of this print:
[ix-concentrate.md](ix-concentrate.md). On that cell the after-move
is present (r20 median +1.49%) and the matching exit is
`arm_death 8`: [ix-cell-exit.md](ix-cell-exit.md).

Do not fund a harvest exit on the unconcentrated event. Do not treat
his-mint clock-20 as a live rule.
