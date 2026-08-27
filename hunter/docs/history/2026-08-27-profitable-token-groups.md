# 2026-08-27 — profitable token groups, found before any tuning

Finding groups is a separate job from tuning a rule for one. This entry finds the groups:
one identical probe over every launch identity in the lake, judged on **how many of its
configurations pay**, then split fit/held-out. Per-group entry and exit logic comes after.

Method: [`cohort-screen.md`](../plans/strategies/cohort-screen.md).
Corpus 07-28..08-21, 46.5M curve prints, 548,986 tokens, 578 templates.

## The probe

Identity = `(ix_labels, max_cost_lamports)`. Nine cells per identity: entry band
`vsol ∈ {45, 55, 65}` × target `{1.5x, 2x, 3x}` **relative to the fill**, so the geometry
scales to each group instead of borrowing another cohort's finish line. Stop 40% retrace
from the since-fill peak. Entry is the first print at the band whose slot is **after the
launch slot**, filled at the last print at or before fire + 115 ms; the exit trigger pays
the same 115 ms. One trade per token. 1,335 cells survive 40 trades.

**A group is judged on its median cell and how many of nine pay** — one winner in nine is
what a multiple-comparison artifact looks like.

## Result

Held-out split at 08-14. `n` is trades across all nine cells.

| max_cost | template | mints | days | fit med | **held med** | held + | status |
| ---: | --- | ---: | ---: | ---: | ---: | ---: | --- |
| 0.0108 | 3ix `Create_v2/CreateIdempotent/Buy` | 185 | 22 | +13.4% | **+17.1%** | 9/9 | **new** |
| 0.540 | 3ix | 209 | 25 | +14.9% | +13.1% | 9/9 | live (`g12`) |
| 0.130 | 7ix `CB/CB/Transfer/Create_v2/Extend/CreateIdem/Buy` | 139 | 14 | +20.2% | **+10.1%** | 9/9 | **new** |
| 7.070 | 5ix `CB/CB/Create_v2/CreateIdem/BuyV2` | 506 | 25 | +10.7% | **+7.7%** | 7/9 | **new** |
| **3.030** | 5ix `BuyV2` | **3,015** | 25 | +4.8% | **+6.4%** | 8/9 | **new, largest** |
| 0.0432 | 3ix | 165 | 21 | +4.0% | +4.7% | 9/9 | live (`g13`) |
| — | `Create_v2/Extend/CreateIdem/BuyExactSolIn` | — | — | +14.9% | +3.7% | 7/9 | new, thin |
| — | `CB/Create_v2/Extend/CreateIdem/BuyExactSolIn` | — | — | +6.6% | +2.8% | 7/9 | new, thin |
| 0.808 | 5ix | 751 | 25 | +2.9% | +1.4% | 3/6 | weak |
| 0.505 | 5ix | 3,946 | 25 | +9.8% | **−9.6%** | 2/9 | **fails held out** |

**Calibration.** The probe independently rediscovers `mc 0.540` (`g12`) and `mc 0.0432`
(`g13`) — both live — at 9/9 cells in both halves. A screen that cannot find the running
rules may not propose new ones.

**The split earns its keep.** `mc 0.505` is the second-largest group and reads 8/9 cells
positive with a +9.8% fit median. Held out it is **−9.6%, 2/9**. In-sample cell counts do
not survive on their own.

## Not a candidate yet

`max_cost 0.900067` (1,212 mints, 9/9 cells, 3,240 trades) exists only **08-17..08-21** —
five days, no history to hold out. A new launcher running ~240 mints/day. Re-screen it
once it has two weeks.

## Loaded

Inactive paper rules on the validated middle cell (band 55, target 2x via
`m_position.pnl >= 100`, stop `retrace >= 40`), tagged `group,probe-baseline,held-out-ok,untuned`:

* `-- group mc0.0108 band55-2x-stop40 (probe 08-27)`
* `-- group mc0.13 band55-2x-stop40 (probe 08-27)`
* `-- group mc3.03 band55-2x-stop40 (probe 08-27)`

plus the earlier `-- cand buyv2 mc7.07 band60-grad (screen 08-27)`, which carries a
graduation exit and its own robustness work
([`2026-08-27-launch-identity-screen-first-candidate.md`](2026-08-27-launch-identity-screen-first-candidate.md)).
These are **baselines, not tuned rules** — each group gets its own entry/exit next.

## Recent groups — the same probe, split on each group's own life

A `>= 15 days` filter excludes a launcher that started last week **by construction**, and
launchers rotate. Re-run scoped to identities that are **still active** (last launch on or
after 08-19) with `>= 150` mints — 279 of them — keyed on the **label text**, and split
each group at **its own median launch day** rather than a fixed date. That split reads the
same for a 25-day launcher and a 5-day one.

Groups whose median cell is positive in **both** halves:

| max_cost | mints | life | days | cells + | late + | early | **late** | template |
| ---: | ---: | --- | ---: | ---: | ---: | ---: | ---: | --- |
| 0.0108 | 185 | 07-30..08-21 | 22 | 9/9 | 9/9 | +10.8% | **+20.2%** | 3ix |
| **0.900067** | **1,211** | **08-17..08-21** | **5** | 9/9 | 8/9 | +3.2% | **+16.3%** | 5ix `CB/CB/`**`Create`**`/CreateIdem/Buy` |
| 0.540 | 208 | 07-28..08-21 | 25 | 9/9 | 8/9 | +15.7% | +15.4% | 3ix (live `g12`) |
| 3.030 | 2,633 | 07-28..08-21 | 25 | 8/9 | 9/9 | −1.5% | **+12.5%** | 5ix `BuyV2` |
| **5.250** | **191** | **08-18..08-21** | **4** | 4/6 | 6/6 | +7.4% | **+11.9%** | 6ix `CB/CB/Transfer/Create_v2/CreateIdem/Buy` |
| 7.070 | 469 | 07-28..08-21 | 25 | 9/9 | 7/9 | +11.3% | +10.1% | 5ix `BuyV2` |
| 0.101 | 4,889 | 07-28..08-21 | 25 | 4/6 | 4/6 | −2.6% | +6.8% | 5ix — marginal, see below |
| 0.0432 | 165 | 07-30..08-21 | 21 | 9/9 | 9/9 | +4.6% | +4.0% | 3ix (live `g13`) |
| none | 750 | 08-08..08-20 | 11 | 8/9 | 7/9 | +6.6% | +2.8% | `CB/Create_v2/Extend/CreateIdem/BuyExactSolIn` |

**`max_cost 0.900067` is a real group, not an artifact.** Five days old, 1,211 mints,
3,309 probe trades, positive in 9/9 cells overall and 8/9 in its own late half. It launches
on **`Pump.Fun: Create`** — the legacy instruction, not `Create_v2` — which is what makes
it a distinct tool. Its late half is 3 days and 50–115 trades a cell, so the number is
thin; treat it as live-watch, not size.

**`max_cost 5.25` is three different templates** (2,818 / 614 / 191 mints). Only the
191-mint `…Transfer/Create_v2/CreateIdempotent/Buy` variant, first seen 08-18, ranks.
Keying identity on the label text rather than a hash is what separates them.

**`max_cost 0.101` is not recommended** despite 4,889 mints: it yields only 522 reachable
probe trades, because most of its tokens clear the band inside their own launch slot.
Volume is not reach.

**`max_cost 0.13` is absent here only for the 150-mint floor** (134 mints), not for
failing. Its fixed-date held-out result stands.

Loaded as inactive paper rules, tags `group,probe-baseline,recent,short-history,untuned`:

* `-- group mc0.900067 band55-2x-stop40 RECENT (probe 08-27)`
* `-- group mc5.25t band55-2x-stop40 RECENT (probe 08-27)`

**Re-run this screen weekly.** A rotating launcher is only findable while it is running,
and both of these would have been invisible a week ago.

## Method notes

* **Two identity keys collide** under `hash(ix_labels) % 100000` (578 templates, 576
  hashes): `["Pump.Fun: Create"]` with a `CB/CB/Create_v2…` template, and a
  `CB/CB/Create…` with a `Transfer/Create_v2/Create/BuyExactSolIn` template. No shortlisted
  group is affected, but key on the label text, not the hash.
* **Round `max_cost` for display only.** `0.900067` and `0.900189` are different launchers
  that both render as `0.9001`.
* A group with no fit-half rows returns `NULL`, not zero — guard the median before
  ranking, or a five-day-old launcher silently disappears instead of being flagged.
