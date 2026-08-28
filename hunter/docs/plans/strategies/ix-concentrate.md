# Concentrate the combined-machine harvest

Same fillable events as [ix-combined-machine.md](ix-combined-machine.md)
(door + 5-slot gap + `vsol < 46` + not-all-repeat + working completing
print, crowd or turn; tight packs out). Facts are live at the completing
print. His mint list is not a gate. Reproduce: `ixg-concentrate.sql` +
`ixg-concentrate.py`. Scratch: `ixg.cm_fact`. Fill = last print with
`ts <= fire + 95 ms`. Cost = 125 bps/leg + own `B/vsol` at B = 0.10 SOL.
One episode per mint. Window 2026-08-11 .. 2026-08-23 exclusive.

Habit = `he1`: he buys this mint in S or S+1. Tape = clock-20 net at
that fill. `he1` itself is lookahead and is an oracle ceiling, not a
rule.

## Ceiling: the events he actually sends on harvest

| Book | n | /day | mean | med | win | SOL | days+ | OOS |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| fillable | 40,254 | 3,354 | −7.28% | −11.24% | 29.1% | −293.1 | **0/12** | −7.84% |
| he1 oracle | 925 | 77 | **+2.91%** | −0.53% | 48.3% | +2.7 | 10/12 | +1.87% |
| he_causal (S+1 only) | 244 | 20 | **+7.58%** | +4.69% | 61.9% | +1.8 | **12/12** | +5.30% |

The unconcentrated body dumps at a 20 s hold. The subset he sends on
does not. Concentration is the remaining problem. Waiting for S+1
(`he_causal`) is a confirmation delay, not a live fire.

## One-ways: habit and tape on the same rows

`age < 20` is the poison. He almost never sends (he1 0.28%) and the
book is −10.93% **0/12** (25,653 first-per-mint, 2,138/day). That band
is most of the fillable mass. `age < 180` as a tightener
([ix-perm.md](ix-perm.md), measured on his mints) keeps this dump on
the full tape and is net negative.

| age | first n | he1 | mean | days+ | OOS |
| --- | ---: | ---: | ---: | --- | ---: |
| lt20 | 25,653 | 0.28% | −10.93% | **0/12** | −11.58% |
| 20–60 | 12,406 | 1.43% | −2.02% | 0/12 | −2.22% |
| 60–180 | 8,943 | 2.10% | −0.12% | 4/12 | −0.34% |
| 180–600 | 6,057 | 2.12% | +1.16% | 9/12 | −0.02% |
| 600+ | 3,607 | 1.24% | +0.47% | 7/12 | +0.29% |

Named `separated` crowds dump unless price is already off the peak.
Solos already require `trail >= 15` and still do not harvest.

| separated trail | first n | he1 | mean | days+ | OOS |
| --- | ---: | ---: | ---: | --- | ---: |
| unk | 11,039 | 0.66% | −14.82% | **0/12** | −15.92% |
| lt15 | 7,274 | 0.70% | −6.49% | **0/12** | −6.77% |
| 15–30 | 1,614 | 1.03% | +1.04% | 8/12 | +0.27% |
| 30–60 | 2,365 | 2.12% | +2.19% | 9/12 | +2.86% |
| 60+ | 1,394 | 0.84% | +2.53% | **12/12** | +1.81% |

Perm-style cuts that keep `age < 180` (`vsol` [33, 40), `age < 180` ∧
separated, init [2, 5)) stay 0/12. `init` 2–5 is his door mass and the
worst tape cell (−8.23%). Cashback-off is less bad than on, still red.
Gap 5–9 is the weak quiet; 40+ is flat. Bloom is a small green template
cell (834 first, +2.30%, 9/12, 70/day) and is not stacked here.

## The conjunction

Fillable **and** `shape = separated` **and** `trail >= 15` **and**
`age >= 20`. Mechanism: a same-template crowd with a `tx_index` hole,
already off peak, on a token that is not brand-new. Solos are out.
No-dip crowds are out. The first 20 s of the mint are out.

| Book | exit | n | /day | mean | med | win | SOL | days+ | OOS |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| **sep ∧ trail≥15 ∧ age≥20** | **clock 20 s** | **3,917** | **327** | **+2.38%** | −1.42% | 45.8% | **+9.33** | **11/12** | **+1.90%** |
| same | harvest_clock | 3,917 | 327 | +0.58% | −2.78% | 33.3% | +2.28 | 9/12 | +0.55% |
| same | first-gap | 3,917 | 327 | −0.19% | −2.89% | 29.5% | −0.73 | 6/12 | −0.22% |
| sep ∧ trail≥15 ∧ age≥180 | clock 20 s | 1,626 | 136 | +1.63% | −0.99% | 46.1% | +2.64 | 9/12 | +0.97% |
| age≥20 ∧ trail≥15 (ones in) | clock 20 s | 16,441 | 1,370 | +0.11% | −3.05% | 39.7% | +1.78 | 7/12 | −0.14% |
| age≥20 only | clock 20 s | 21,943 | 1,829 | −0.91% | −3.10% | 37.7% | −19.90 | 2/12 | −1.30% |
| trail≥15 only | clock 20 s | 23,187 | 1,932 | −2.75% | −6.13% | 34.7% | −63.68 | **0/12** | −3.11% |

Clock-20 on the conjunction: IS +1.54% (5/6 days), OOS +1.90% (5/5).
Dropping 2026-08-17 (the fat day, +9.2%) leaves +6.11 SOL. First-gap
on the same entries is red: this is a harvest, not a scalp of the
crossing slot. `harvest_clock` (leave on dump/death, stay on a second
wave) cuts the hold to 0.8 s and most of the edge.

Habit check, not the score: this cell is 4,882 events / 3,917 mints,
he1 1.72% (84 hits, 7/day) against fillable 1.19% and fillable
first-per-mint 0.85%. It covers 84 / 965 of his fillable-event fires
(8.7%). Most of his fires on this trigger are still solos. The book
is a tape concentration, not a clone of his send set.

## What this is

The combined-machine trigger has a harvest at 95 ms after three live
cuts: separated crowd, already in a dip, not brand-new. The
unconcentrated body in [ix-harvest-path.md](ix-harvest-path.md) dumps
because `age < 20` and no-dip crowds dominate it. Perm `age < 180` is
the wrong direction on the full tape.

Do not add `he1` or his mint list to the rule. Do not swap clock-20
for first-gap on this cell. Do not fold solos back in. Twelve days,
negative median, one fat day. Paper it forward; do not stack another
filter on this print.
