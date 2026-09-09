# 2026-09-08 - the documented-project rule: first positive holdout, and how weak it is

Follow-on to [2026-09-08-the-edge-is-token-level-and-not-yet-reproducible](2026-09-08-the-edge-is-token-level-and-not-yet-reproducible.md).
Same kernel, both legs at 115 ms, re-entry unlimited, tickets rather than tokens, funnel before every number.

## The missing axis was the metadata document

`tokens.meta` stores only a URI and nothing had ever fetched the JSON behind it. That document carries the
description and the twitter / website / telegram links - the attention panel a person reads before buying.
Fetched for the candidate universe through pump.fun's own IPFS gateway (ipfs.io and dweb.link rate-limit).

Each field is a real gradient and none crosses zero alone. On a 30 % trail with a 600 s cap: a website is
worth 5.8 points (-9.9 % without, -4.1 % with), a telegram 6.6, a description over 80 characters 3.3.

## The conjunction

Six story-derived terms, each with its own sentence, stacked one at a time on a 40 % trail with a 1800 s cap:

| step | mean % of clip a trade |
| --- | ---: |
| every documented token | -9.41 |
| + door: website AND (telegram OR description > 80 chars) | -6.42 |
| + age >= 300 s | -3.64 |
| + reserve 45-90 | -1.22 |
| + at least 4 distinct professional builds already in | +0.75 |
| + at least 8 | +1.39 |

A professional build is one run by 50 or fewer wallets with 200+ prints: one operator's own tool rather than
a retail terminal, so several of them arriving independently is agreement between separate research
processes. Adding holder-book terms (supply sold back, bundle share, holder count) makes it worse; step 6 is
the peak. The positive region is a plateau, not a spike - every age and professional-build combination inside
reserve 50-85 is positive at +1.5 to +2.4 %, and widening the band to 40-95 flips it negative.

## The holdout

The sentence was frozen in full, exits included, before any holdout week was read
(`study-kernel/frozen-sentences.md`). Metadata capture begins 2026-08-18, so the earliest holdout week
has no document at all and there are two usable holdout weeks, not three. Run through one identical
pipeline, primary exit (40 % trail, 1800 s cap):

| week | tickets | tokens | net SOL | % a trade | days positive | worst day | standard error |
| --- | ---: | ---: | ---: | ---: | --- | ---: | ---: |
| 09-01..09-07 (fitted) | 519 | 192 | +0.42 | +0.40 | 4/7 | -0.63 | 2.55 |
| 08-25..08-31 (holdout) | 768 | 306 | +2.23 | +1.46 | 4/7 | -1.07 | 2.34 |
| 08-18..08-24 (holdout) | 869 | 336 | +1.12 | +0.64 | 4/7 | -0.87 | 2.18 |

Both holdout weeks meet the pre-registered bar - positive total SOL and a majority of days. Pooled across
the three weeks: +3.77 SOL on 2,156 tickets, about +0.85 % a trade at a 0.2 SOL clip, at most four positions
open at once.

## How much this is worth, stated honestly

It is the first thing this session that is positive out of sample under the honest kernel, and it clears the
toll: +0.85 % net a trade means roughly 4.3 % gross against a 3.5 % round-trip cost.

It is also weak. Each week sits under one standard error from zero, so the sign consistency across three
independent weeks is the whole of the evidence, not the magnitude. Days positive is 4 of 7 every week, which
is a bare majority. The declared alternative exit (40 % trail, 900 s cap) reads -0.33 %, +1.16 % and -0.02 %
across the three weeks, so the result does not survive a change of cap - that is a warning, not a detail.
About 700 tickets a week on 0.8 SOL of capital is roughly 1.3 SOL a week gross of nothing.

Treat it as a lead to carry into paper trading with per-trade reconciliation, never as a rule to size up.
The honest next steps are more weeks (the metadata window only opens on 08-18, so time supplies them), a
permutation null on the door, and an engine run that reproduces the offline book trade by trade.

Scripts: `study-kernel/` - `fetch_meta.py`, `analyze_meta.py`, `stacked.py`, `holdout_v2.py`, and the frozen
sentence in `study-kernel/frozen-sentences.md`.
