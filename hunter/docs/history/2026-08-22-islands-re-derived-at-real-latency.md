# 2026-08-22 — the island space, re-derived at the bot's measured latency

The map of [`plans/strategies/island-map.md`](../plans/strategies/island-map.md) was built
under a `first_in_window` fill. That fill is the very next print, and
[the same-slot refutation](2026-08-22-island-is-a-same-slot-artifact.md) showed it is
unreachable. This entry records re-running the whole search — not re-grading the old
rules — with the fill priced from measurement before anything is ranked.

## The latency, measured rather than assumed

`strategy_positions` carries three stamps on the ingest clock: `target_time` (the trigger
print), `created_at` (the decision), `entry_time` (our own fill observed).

| leg | n | p05 | p25 | p50 | p75 | p90 | p99 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| decision -> fill observed (real) | 808 | 30 | 54 | **94 ms** | 169 | 233 | 463 |
| trigger -> decision (all live) | 48,631 | — | — | **0.3 ms** | 0.4 | 0.8 | 348 |

Per-day p50 spans 58–136 ms across 25 days, so the number is stable, not a window artifact.

**Trigger staleness turned out to be nil**, which contradicts the prior expectation that a
metric-gated rule reads a stale grid tick. The engine's metric fold runs per print on the
rule path, so the decision is taken on the trigger print itself. That collapses the honest
lag from a rule-shape-dependent sum to a single number, and it is why one `FillModel`
constant is enough.

## Three gates before any search

The harness reproduced results established before it existed, or it would not have been
used:

- sub-10 ms impulse next-prints are **99.8% same-slot at a 0.616 ms median** (prior:
  100%, 0.49 ms);
- the whole decision-point universe books **−0.00166 SOL/trade** (prior: −0.0016, the
  round trip);
- **95% of the impulse island's money** sits under a 50 ms gap (prior: 95%).

## What the re-search found

Searching from the whole universe with a beam over 69 axes — including depth-relative
flow, whale-vs-retail split, persistence, acceleration and price under-response, none of
which the previous axis set could express:

| | week | exp/trade | days + | p50 -> p90 lag |
| --- | ---: | ---: | ---: | ---: |
| absorption (old island 1) | +18.93 | +0.16% | 6/7 | keeps 90% |
| A continuation (new) | +8.43 | +0.53% | 7/7 | keeps 94% |
| B quiet pause (new) | +7.66 | +0.36% | 7/7 | keeps 98% |
| impulse inception (old island 3) | **−1.30** | — | 4/7 | keeps 37% |

**The surviving direction is the opposite of the refuted one.** The impulse island bought
`rise(3) <= 9` — before price moves. Island A buys `rise(30) >= 207` — after it has
tripled and while it is still being bought. Anticipation needs speed the bot does not
have; continuation does not.

**Latency flatness is the diagnostic that separates them**, more cleanly than any PnL:
everything that survives keeps 90–98% of its money between the p50 and p90 reaction, and
the thing that does not keeps 37%.

## The error that made the first version of this entry wrong

**The harness lagged the entry leg and booked the exit at the next print.** The bot's exit
reaction is the same ~95 ms as its entry reaction, so charging one and not the other
measures a bot that panics instantly and buys slowly. Cost of the asymmetry, three days:

| island | exit un-lagged | exit lagged 95 ms | real kernel |
| --- | ---: | ---: | ---: |
| absorption | +13.91 | **-8.24** | **-26.88** |
| A continuation | +4.40 | +1.36 | -2.56 |
| B quiet pause | +3.54 | +0.47 | -2.62 |
| A AND B | +2.87 | +1.57 | -0.92 |

Every island reported as surviving was an artifact of that asymmetry. The engine caught it
because `FillModel::LagMs` lags both legs; the harness did not, and neither did any gate in
it. **A latency correction applied to one leg is not a latency correction.**

## Which then produced the finding that does survive

Re-running the exit surface with the exit leg lagged inverts it completely. Of 165 exit
policies: absorption has **0** positive, the continuation conjunction has **165**, and the
best shape everywhere is a WIDE stop with a **time cap** and no trail. Same entry, same
trades, same fill, on the real kernel over 08-13..15:

| exit | SOL |
| --- | ---: |
| `stop 5` + `retrace >= 20` | -2.56 |
| `stop 20` + `held >= 40` | **+2.53** (3/3 days) |

**A reactive exit is adversely selected at its own fill.** A stop or trail fires right
after an adverse move and then waits 95 ms, into the continuation of that move. A clock
fires at an instant the market did not choose. This is the mechanism behind the older
"a trail is the wrong exit shape" and "reactive rug exits refuted" results, and it is
general: it applies to any rule this bot runs, not just to these islands.

Forward on cohorts the search never saw (08-20..21): **+0.89 SOL, 421 trades, 2/2 days.**
The `A AND B` conjunction does NOT survive forward (-0.74, 0/2) despite the best
in-sample per-trade of any cell - the classic shape of a selection artifact on a small
sample.

## Two further method errors this round produced

Both were caught by instrument checks, not by the results looking wrong.

- **A greedy search gated on "positive at every step" cannot start.** From a universe at
  −769 SOL no single cut reaches positive, so the walk reported an empty space when the
  space was not empty. Intermediate steps must rank on expectancy and be allowed to be
  negative; the strict gates belong at the leaves.
- **An entry search under one fixed exit ranks the wrong thing.** Moving absorption from
  `stop 3 / trail 5` to `stop 5 / trail 20` nearly triples it (+5.62 → +18.93). The first
  marginal scan was run under the former and had to be discarded. Settle the exit first,
  or carry the exit surface into the search.

## The identity axis, corrected

Ranked on uncollapsed per-row SOL, creation structure 415 reads **+99.29**. Collapsed to
non-overlapping episodes it is **+2.50, held out −0.03**. Leave-one-day-out blacklists on
`ix_create` / `ix_first_buy` / `ix_top_buy` / `ix_dp` climb from −1,231 SOL to roughly zero
and stop there.

This also settles the ix-structure cuts recorded in
[`2026-08-22-ix-structure-cuts.md`](2026-08-22-ix-structure-cuts.md): they were improving
a rule that does not exist at a reachable fill, and the axis does not carry an island of
its own. **Identity removes losers; it does not select winners.**

## Left open

- 2026-08-12 is a 185k-decision-point fragment, so the unseen-cohort check produced 18–55
  trades and is inconclusive in both directions. A full forward day is still owed.
- The real kernel has not been run on islands A or B.
