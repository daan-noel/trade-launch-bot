# Crowd-island harvest: every crowd shape

Same island as [ix-concentrate.md](ix-concentrate.md) / [ix-cell-exit.md](ix-cell-exit.md):
gap, then a several-wallet working burst in `[0.9, 4)`, `vsol < 46`,
not-all-repeat, `trail >= 15`, `age >= 20`. Fire is the completing
print. Fill = last print with `ts <= fire + 95 ms` — after a tight
pack when the burst is bundled, not inside it. Solos stay out.

Four crowd spellings of that event:

| Shape | Client | Block order |
| --- | --- | --- |
| `separated` | same template | hole in `tx_index` |
| `bundle` | same template | consecutive (tight pack) |
| `mixed_gap` | mixed templates | hole |
| `mixed_tight` | mixed templates | consecutive |

Reproduce: `ixg-crowd-island.py`. Scratch: `ixg.cm_cand`. Cost = 125
bps/leg + own `B/vsol` at B = 0.10 SOL. One episode per mint except
the re-entry row. Window 2026-08-11 .. 2026-08-23 exclusive.

A 0 ms first-gap fill on `bundle` / `mixed_tight` is still fiction
(interior of the pack). This book does not use that fill.

## Path (gross, 95 ms, first per mint)

| Book | n | r8 med | r20 med | r20 p>0 | arm+10 | peak post_gap |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| **ALL CROWD** | **6,002** | +1.69% | **+1.37%** | 54.8% | 68.5% | 83% |
| separated | 3,917 | +1.93% | +1.49% | 55.6% | 68.0% | 83% |
| bundle | 1,220 | +1.67% | **+2.25%** | 57.5% | 68.7% | 84% |
| mixed_gap | 2,011 | +0.83% | +0.53% | 52.1% | 69.4% | 84% |
| mixed_tight | 114 | +3.52% | +4.06% | 60.5% | 65.8% | 84% |

ALL CROWD OOS r20 median +1.13%. Never-arm paths dump; armed paths
mark r20 median ~+8%. First pause median 0.9 s on every shape. r60
median is already red except `mixed_tight` (n=114). Same after-move
as the separated cell. Tight vs hole does not change it.

Per-shape first-per-mint counts can overlap a mint; ALL CROWD keeps
the first matching crowd event on that mint (6,002, not the sum).

## Money at 95 ms

| Book | exit | n | /day | mean | med | win | SOL | days+ | OOS |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| **ALL CROWD** | **clock 20** | **6,002** | **500** | **+2.31%** | −1.62% | 45.7% | **+13.88** | **12/12** | **+2.19%** |
| **ALL CROWD** | **arm_death 8** | **6,002** | **500** | **+2.48%** | −3.06% | 32.8% | **+14.91** | **12/12** | **+1.93%** |
| ALL CROWD | first-gap | 6,002 | 500 | −0.16% | −2.89% | 29.5% | −0.99 | **3/12** | −0.24% |
| separated | arm_death 8 | 3,917 | 326 | +2.94% | −3.02% | 33.4% | +11.52 | **12/12** | +2.05% |
| separated | clock 20 | 3,917 | 326 | +2.38% | −1.42% | 45.8% | +9.33 | 11/12 | +1.90% |
| bundle | clock 20 | 1,220 | 102 | **+2.64%** | −0.79% | 47.9% | +3.22 | 11/12 | **+3.18%** |
| bundle | arm_death 8 | 1,220 | 102 | +0.54% | −3.04% | 29.7% | +0.66 | 9/12 | +1.34% |
| mixed_gap | clock 20 | 2,011 | 168 | +1.06% | −2.43% | 43.0% | +2.14 | **12/12** | +1.27% |
| mixed_gap | arm_death 8 | 2,011 | 168 | +0.70% | −3.91% | 31.5% | +1.41 | 7/12 | +0.26% |
| mixed_tight | clock 20 | 114 | 10 | +2.26% | +0.95% | 51.8% | +0.26 | 8/12 | +1.42% |
| mixed_tight | arm_death 8 | 114 | 10 | +3.15% | −2.98% | 33.3% | +0.36 | 8/12 | +3.36% |
| ALL CROWD re-entry | arm_death 8 | 7,940 | 662 | +1.85% | −3.04% | 31.6% | +14.68 | 11/12 | +1.44% |

First-gap on the union is still a scalp of the crossing slot and is
red. `arm_death 8` was derived on separated; on `bundle` and
`mixed_gap` a 20 s clock is the more even harvest. Do not fit a
different trail per shape. The union book with either harvest exit
is **12/12**. `mixed_tight` is the same path at n=114; it does not
stand alone.

## What this is

Crowd-into-dip on a living token is one island. Same-client or mixed,
hole or tight pack, is a spelling of that crowd, not a new family.
Landing after the pack is the 95 ms fill. Solos stay a different
event.

Paper ALL CROWD: any of the four shapes, `trail >= 15`, `age >= 20`,
completing print, 95 ms, clock-20 or `arm_death 8`. Do not stack
another filter. Do not use first-gap. Do not fold solos in.

Live engine mapping (two exclusive rules, `m_burst_slot`, exit DNF):
[ix-live-rule.md](ix-live-rule.md).
