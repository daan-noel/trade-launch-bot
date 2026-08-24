# The island map

The islands this market has **at the latency this bot actually reaches**, confirmed on the
real kernel rather than on a search harness.
[island-search.md](island-search.md) holds the extract method,
[signal-search-mandate.md](signal-search-mandate.md) the standing gates,
[fill-and-cost-models.md](fill-and-cost-models.md) the fill doctrine.

**One island survives: continuation, exited on a clock.** Everything else in this file is
the record of what does not, and why - which is the more reusable half.

## The two things that decide every number here

**1. The bot reacts in ~95 ms, measured.** From `strategy_positions`, 808 real fills over
2026-07-27..08-22, stable across all 25 days (per-day p50 spans 58-136 ms):

| leg | p05 | p25 | **p50** | p75 | p90 | p99 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| decision -> our own fill observed | 30 | 54 | **94 ms** | 169 | 233 | 463 |

Trigger staleness is nil - `target_time -> created_at` is 0.3 ms at p50 over 48,631
positions - so the honest lag is one number, not a rule-shape-dependent sum.

**2. BOTH LEGS PAY IT.** The exit reaction is the same reaction as the entry. A backtest
that lags the entry and books the exit at the next print is measuring a bot that panics
instantly and buys slowly, which is not this bot or any bot. On the absorption island that
asymmetry alone is worth **+13.91 -> -8.24 SOL** over three days, and it is what made an
earlier version of this file report a working map. `FillModel::LagMs` lags both legs;
grade with it.

## The surviving island: continuation, exited on a clock

```
ENTRY  m_flow_window(30).net_flow  >= 26.9      # 27 SOL net in over thirty seconds
  AND  m_flow_window(30).buy_share >= 92.1      # 92% of it on the buy side
  AND  m_price_window(30).rise     >= 207       # and price is ALREADY up 3x
EXIT   stop_loss 20  OR  m_position.held >= 40  # a WIDE stop and a 40-second clock
```

**Buy what has already moved and is still being bought, then leave on a timer.** The
inversion of the impulse island it replaces, which required `rise(3) <= 9` - buy *before*
price moves. Anticipating a move needs a reaction the bot does not have; joining one that
is still running does not.

Measured on `hunter-engine::reduce` through `POST /api/strategies/simulate`, fill
`{"lag_ms": 95}`, cost `pumpfun_impact`, copycat guard pinned off, 0.05 SOL:

| | trades | SOL | per position | days + |
| --- | ---: | ---: | ---: | ---: |
| fit 08-13..15 | 624 | **+2.53** | +8.1% | 3/3 |
| **forward 08-20..21** | 421 | **+0.89** | +4.2% | **2/2** |

The forward cohorts were not extracted until every threshold was frozen. Forward runs
about half the fit rate, which is the honest expectation to carry.

## Why the exit is a clock and not a stop

Same entry, same trades, same 95 ms fill - only the exit shape moves:

| exit | engine, 08-13..15 |
| --- | ---: |
| `stop 5` + `retrace >= 20` | **-2.56** |
| `stop 20` + `held >= 40` | **+2.53** |

**A reactive exit is adversely selected at its own fill.** A stop or a trail fires
immediately after an adverse move and then waits 95 ms - straight into the continuation of
that move. A clock fires at an instant the market did not choose, so its fill is unbiased.
The wide stop is a disaster brake, not a working part: tightening it to 8 costs 12%
in-sample and 51% forward.

On the harness, with the exit leg lagged, this shows up as a clean sweep: of 165 exit
policies, absorption has **0** positive and the continuation conjunction has **165**.

## What does not survive

| | verdict |
| --- | --- |
| absorption | **dead** - engine -26.88 SOL / 3 days on 4,658 trades; 0 of 165 exit policies positive |
| impulse inception | **dead** - -1.30 at 95 ms, and -0.78/day forward |
| quiet accumulation | **dead** - negative in every reading |
| B, the quiet pause | not established - 58 of 165 exit policies positive, weakest forward |
| A AND B | **fails forward** - +1.16 fit (3/3) but **-0.74 forward, 0/2 days** |

The conjunction failing forward while A alone holds is the useful warning: it was the
highest per-trade cell in-sample (+12.7%/position) on the smallest sample (182 trades),
which is exactly the shape a selection artifact takes.

**Instruction structure produces no island.** Ranked on uncollapsed per-row SOL, creation
structure 415 reads +99.29; collapsed to non-overlapping episodes it is **+2.50 with a
held-out of -0.03**. Leave-one-day-out blacklists on all four actor identities
(`ix_create`, `ix_first_buy`, `ix_top_buy`, `ix_dp`) climb from -1,231 SOL to roughly zero
and stop. Identity removes losers; it does not select winners.

## Gates this clears

- **Artifact detector.** Money by gap-to-next-print: 11% under 50 ms, against 95% for the
  refuted impulse island, whose sub-10 ms next-prints are 99.8% same-slot at a 0.616 ms
  median. Any entry search reports this distribution beside its PnL.
- **Perturbation.** 25 of 25 cells positive on 6+ days over `net_30 18..38` x
  `rise_30 150..280`. No threshold is load-bearing.
- **Stress lag.** Positive on the same number of days with every fill resolved at the p90
  reaction (235 ms) instead of the p50.
- **The real kernel.** Not a harness result: the numbers above come from the same
  `reduce` live paper trades on.

## What this does not claim

- **Two forward days, 421 trades.** Enough to refute, thin to confirm.
- **The exit was fitted on the fit half** and only the whole rule was carried forward.
- The money is small: **~+0.45 SOL/day at 0.05 SOL**. Per-capital return is roughly flat
  in size on the harness, but that was measured under the un-lagged exit and is not
  re-established.
- **Nothing here is armed.** `hunter/scripts/seed-island-rules.sql` seeds paper +
  `is_active=false`.
