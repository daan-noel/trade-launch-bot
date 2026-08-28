# Harvest path and exit on the concentrated cell

Same cell as [ix-concentrate.md](ix-concentrate.md): fillable combined
machine, `shape = separated`, `trail >= 15`, `age >= 20`. Fill = last
print with `ts <= fire + 95 ms`. Path marks are last-print **gross**
return from that fill. Money walks charge 125 bps/leg + own `B/vsol`
at B = 0.10 SOL. One episode per mint except the re-entry rows.
Window 2026-08-11 .. 2026-08-23 exclusive.

Reproduce: `ixg-cell-path.py` (path + exits). Scratch: `ixg.cm_fact`.
Exits are pre-committed from the path, not mined. Trail is 8dtx's
arm 10 / trail 18 ([wallet-8dtx-logic.md](wallet-8dtx-logic.md)).

## Path (gross, 95 ms fill)

Unconcentrated fillable from [ix-harvest-path.md](ix-harvest-path.md)
is the contrast: r20 median **−8.43%**, 86% dump, 14% wave.

| Book | n | r1 med | r8 med | r20 med | r20 p>0 | r20 p>+10 | mfe med | mae med | dump | wave |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| **cell** | **3,917** | +0.07% | **+1.93%** | **+1.49%** | 55.6% | 31.4% | +24.5% | −23.9% | 72% | 27% |
| cell OOS r20 | 1,715 |  |  | +1.15% | 54.8% |  |  |  |  |  |

r20 mean +5.52% (OOS +5.04%). Median stays positive through 8–20 s
and turns at 60 s (r60 median **−1.75%**, r300 median **−18.5%**).
Arm-at-+10 is 68% at 9.6 s. Run-+50 is 31.8% at 50 s.

83% of peaks sit **after** the first 2-slot buy pause (`post_gap`
3,259 / 3,917). First pause median is 0.90 s; 0% last longer than
8 s. First sell median is 3.4 s, before typical arm.

| Split | n | r20 mean | r20 med | p>0 |
| --- | ---: | ---: | ---: | ---: |
| peak `post_gap` | 3,259 | +9.2% | **+4.2%** | 64% |
| peak `pre_gap` | 361 | −12.1% | −8.6% | 23% |
| peak `at_fill` | 296 | −13.5% | −10.6% | 0% |
| armed | 2,664 | +13.0% | **+8.2%** | 70% |
| never arms | 1,253 | −10.4% | −6.1% | 24% |
| after `wave` | 1,060 | +15.2% | +7.7% | 73% |
| after `dump` | 2,832 | +1.9% | 0.0% | 49% |

The after-move is the post-gap peak on paths that arm. The 0.8 s
pause is inside the harvest, not the end of it. `after = dump` here
is not a 20 s price dump (median 0); leave-on-dump at that pause
exits before the peak.

## Money at 95 ms

| Exit | n | mean | med | win | SOL | days+ | hold p50 | OOS |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| clock 20 s | 3,917 | +2.38% | −1.42% | 45.8% | +9.33 | 11/12 | 17.7 s | +1.90% |
| **arm_death 8** | **3,917** | **+2.94%** | −3.02% | 33.4% | **+11.50** | **12/12** | 11.5 s | **+2.04%** |
| trail_hold | 3,917 | +2.49% | −7.50% | 38.7% | +9.74 | 10/12 | 110.6 s | +0.85% |
| clock 60 s | 3,917 | +1.33% | −4.70% | 40.3% | +5.21 | 9/12 | 55.4 s | **−0.20%** |
| death 8 s | 3,917 | +0.93% | −2.79% | 39.6% | +3.65 | 7/12 | 10.0 s | +0.84% |
| harvest_clock | 3,917 | +0.58% | −2.78% | 33.3% | +2.28 | 9/12 | 0.8 s | +0.55% |
| trail (unarmed = gap) | 3,917 | −0.14% | −2.91% | 24.9% | −0.55 | 4/12 | 0.2 s | −0.23% |
| first-gap | 3,917 | −0.19% | −2.89% | 29.5% | −0.73 | 6/12 | 0.2 s | −0.21% |
| death 20 s | 3,917 | −0.49% | −7.06% | 31.2% | −1.91 | 3/12 | 39.2 s | −2.12% |

`arm_death 8`: if price reaches +10%, trail 18% off the in-hold
peak; if it never arms, leave on 8 s of buy silence. Mix: death
2,242 / trail 1,596 / cap 79. IS +2.83% (6/6), OOS +2.04% (5/5).
Every day is green. Drop 2026-08-17 and the other eleven stay
positive.

Re-entry (non-overlap) on `arm_death 8`: 4,644 trades, 387/day,
+2.52% mean, **12/12**, OOS +1.79%. Clock-20 re-entry is 10/12.

## What this is

Crowd-into-dip on a living token has a harvest after-move at this
fill: r20 median is positive, the peak is after the first pause, and
paths that never arm dump. The matching exit is **arm then trail;
unarmed, demand-died**. First-gap and `harvest_clock` fire at the
0.8 s pause, before the peak. A 60 s clock and a 20 s buy-silence
hold through the fade. `trail_hold` (unarmed to cap) keeps the
never-arm dump and the median collapses. Trail with first-gap
fallback is first-gap on this cell (3,464 / 3,917 gap).

Do not sweep trail width or death seconds on this print. Do not
fold solos in. Clock-20 is the probe that selected the cell and
still clears; `arm_death 8` is the tape exit that matches the path.
