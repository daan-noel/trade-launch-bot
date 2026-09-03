# Fill & cost models — what each mode does, with numbers

The two dropdowns on Simulate / dry-run / sweep decide what a backtest number
*means*. They are the run's identity: **two runs under different fill or cost models
are not comparable**, which is why both are stored on the run and shown on its header.

This doc is the worked-example companion to
[execution-costs.md](execution-costs.md) (which derives *why* the cost terms are what
they are). Code: [`core/src/strategies/paper_fill.rs`](../../../core/src/strategies/paper_fill.rs)
(`FillModel`) and [`core/src/strategies/kernel.rs`](../../../core/src/strategies/kernel.rs)
(`CostModelKind` / `round_trip_multi_leg`).

**The one-line version**

| dropdown | mode | picks / charges | use it when |
| --- | --- | --- | --- |
| Fill | `worst_case` | most adverse print in the window | you want the pessimistic bound, or parity with live paper / the sweep |
| Fill | `first_in_window` | the *next* print after the signal | you want the loosest "we react to the tape" reading |
| Fill | `next_slot_first` | the first print at slot **S+1** | you want the earliest price a +1-slot landing can reach |
| Fill | `next_slot_median` | the adverse median at slot **S+1** | you want the middle of the fill dispersion, not either tail |
| Fill | `signal_price` | the signal trade's own spot | you want the zero-slippage ceiling |
| Fill | `lag_<ms>` | the last print that has landed by signal + `<ms>` | **you want the verdict.** The only model keyed to the bot's measured reaction time |
| Cost | `pumpfun_impact` | fee + tip + **our own** `B/vsol` impact | **the default, and the answer.** The only size-aware one |
| Cost | `pumpfun_fee_only` | fee + tip | you want a zero-impact upper bound — the "any edge at all?" screen |

`lag_<ms>` is a **bare string** on the wire like every other model — `lag_115`, not
`{"lag_ms": 115}`. That is what lets it live in the sweep's `TEXT` column, a request DTO
and a TypeScript union without a payload-variant special case; a JSON object there
serialized into the sweep as `NULL` (silently re-reading as `worst_case`) and rendered in
the UI as `[object Object]`. `FillModel::parse` also accepts the legacy object form, so
anything already stored keeps its meaning. The dropdown carries `lag_115` (the bot's
measured decide-to-fill p50) and `lag_235` (its p90 — the stress read).

---

# Part 1 — Fill models

## The fill window

A signal (entry trigger or exit ladder fire) does **not** fill at the trade that
triggered it. Candidates are the trades **after** it in a short window:

```
window = the signal's own slot S      (always)
       + the next observed slot after S, if that slot is within MAX_FILL_WAIT_SLOTS (= 3)
```

Only *one* later slot is ever admitted, and only if it is close. A slot 4+ away is out
of the window entirely, and the window is then just `S`.

The fill model chooses **which candidate in that window prices the leg**. Nothing else.

## Half that window is unreachable — but not the half the slot models assume

Two different things get confused here, and the distinction decides which model is
honest.

**Landing in slot `S` is reachable.** The live book puts `entry_slot - target_slot` at
p50 **0**, with **52.6%** of real buys landing in the trigger's own slot (n=76 real
positions). A slot is ~400 ms wide; a trigger that prints early in one leaves room for
a 115 ms round trip to make the same block.

**Being sequenced immediately after a chosen print is not.** Order inside a block is the
leader's call, not something speed buys. A bundle leg is unreachable outright — bundle
txs are atomic, so nothing sequences between them.

So `first_in_window` is optimistic for a reason that has nothing to do with speed: it
assumes we are always the *very next print*, an ordering privilege no latency buys. The
`next_slot_*` pair removes that privilege, but overcorrects — it drops slot `S`
entirely, and we land there half the time. The two bracket reality without expressing
it, which is what `lag_ms` exists for.

## Entry, priced five ways

Signal fires on the trade at index 0. Real tape after it:

| idx | slot | side | price | in window? |
| --- | --- | --- | --- | --- |
| **0** | 100 | buy | **1.0** | ← the signal itself |
| 1 | 100 | buy | 1.2 | yes (same slot S — *unreachable*) |
| 2 | 101 | buy | 1.5 | yes (the one admitted next slot) |
| 3 | 101 | buy | 1.8 | yes |
| 4 | 101 | buy | 1.6 | yes |
| 5 | 102 | buy | 2.0 | **no** — slot 102 is a *second* later slot |

| fill model | picks | price you buy at |
| --- | --- | --- |
| `worst_case` | idx 3 — the **highest** buy in the window | **1.8** |
| `first_in_window` | idx 1 — the **first** buy after the signal | **1.2** |
| `next_slot_first` | idx 2 — the first buy at **slot 101** | **1.5** |
| `next_slot_median` | idx 4 — the **middle** of slot 101's three | **1.6** |
| `signal_price` | idx 0 — the signal's own spot | **1.0** |

(These are the exact numbers asserted by `fill_models_reprice_a_fixed_entry_set` in
`paper_fill.rs` — the doc and the test can't drift.)

## Exit, priced five ways — and the one that surprises people

Ladder fires on the trade at index 0:

| idx | slot | side | price | in window? |
| --- | --- | --- | --- | --- |
| **0** | 100 | buy | **1.0** | ← the fire itself |
| 1 | 100 | buy | 1.4 | yes (same slot S — *unreachable*) |
| 2 | 101 | sell | 1.1 | yes |
| 3 | 101 | buy | 1.3 | yes |
| 4 | 101 | buy | 1.2 | yes |

| fill model | picks | price you sell at |
| --- | --- | --- |
| `worst_case` | idx 2 — the **lowest** print in the window | **1.1** |
| `first_in_window` | idx 1 — the **first** print after the fire | **1.4** |
| `next_slot_first` | idx 2 — the first print at **slot 101** | **1.1** |
| `next_slot_median` | idx 4 — the **middle** of slot 101's three | **1.2** |
| `signal_price` | idx 0 — the fire's own spot | **1.0** |

Note what `first_in_window` did: **1.4 — better than every other model.** It is *not*
"halfway between worst and signal". It takes whatever printed next, which can be
anywhere — and here that next print is in slot 100, a price no order of ours reaches.

Note also that `next_slot_first` lands on **1.1, the same row as `worst_case`**. That is
coincidence, not design: the first print at S+1 happened to be the low one. The
next-slot models are unbiased within their slot, not systematically kinder.

## How the two next-slot models pick

Both drop slot `S` and keep only slot `S+1`'s qualifying prints. They differ in which
of those they take:

- **`next_slot_first`** takes the earliest. It reads as "we react to the signal and hit
  the first thing available", which is mildly optimistic — ordering inside a block is
  the leader's call, so being first in it is not something speed buys.
- **`next_slot_median`** takes the **adverse median**: on an odd count the middle print;
  on an even count the *adverse* of the two middles (entry the higher, exit the lower,
  mirroring how `worst_case` mirrors). Equal prices break by tape order, so the pick is
  deterministic.

The median is always **a real print**, never an average of two. A synthetic price would
have no corpus row behind it, and a `PaperFill` carries `trade_idx` / `slot` /
`tx_signature` pointing at one.

**When the window admits no `S+1`** — the next observed slot is more than
`MAX_FILL_WAIT_SLOTS` away, so the window is slot `S` alone — both fall back to
`worst_case` over what remains. Returning "no fill" instead would change the
taken-position set and break the reprice invariant below; sparse tape means filling into
a gap at an unknown price, so the adverse end is the defensible assumption.

## The wall-clock model — `lag_ms`

Every model above is shaped by **slot structure**. `lag_ms` is shaped by the clock: it
takes the **last** qualifying candidate whose `block_time` is at or before `signal + N` ms
— the pool state a transaction landing `N` ms out actually executes against. When nothing
lands inside the lag, the state is still the signal's own and the fill prices there.

**It must never be the first print at or after the deadline.** A row's price is the state
*after* that trade, so a print at-or-after `signal + N` is a trade we could not have landed
behind; pricing from it reaches forward past our own fill. The error does not cancel across
the legs — it overcharges entries (measured -0.53 pp/trade on one real rule set) and
undercharges exits (+8-12 pp, concentrated on a take-profit firing into a rise, where the
next print is usually another buy). Pinned by
`the_lag_model_never_prices_from_a_trade_that_lands_after_the_fill`.

That is the only model that can be pointed at a *measured* number. The bot's
decide→fill is p50 **115 ms** (send path 8 ms, ACK→fill 107 ms), so `{"lag_ms": 115}`
grades a rule at the latency it actually trades under, instead of at a bound.

Signal fires at t=0 in slot 100:

| idx | slot | t | price |
| --- | --- | --- | --- |
| **0** | 100 | 0 ms | **1.0** ← the signal |
| 1 | 100 | +50 ms | 1.2 |
| 2 | 100 | +200 ms | 1.4 |
| 3 | 101 | +1000 ms | 1.5 |

| fill model | picks | price |
| --- | --- | --- |
| `first_in_window` | idx 1 — the next print, no delay charged | **1.2** |
| **`lag_ms: 115`** | **idx 1 — the last print that has landed by +115 ms** | **1.2** |
| `lag_ms: 300` | idx 2 — the +200 ms print has landed too | **1.4** |
| `next_slot_first` | idx 3 — slot 100 dropped entirely | **1.5** |

This is the behaviour neither bracket has: a delay is charged **without** pretending we
missed the block. `lag_ms: 0` prices at the signal's own spot (nothing has landed yet),
so it coincides with `signal_price`, not with `first_in_window`; and a lag past the whole
window prices at the window's **last** print, not its adverse one. (Exact numbers from `the_lag_model_charges_wall_clock_not_slot_structure`
in `paper_fill.rs`.)

`block_time` is the **ingest** clock — a Yellowstone transaction frame carries no chain
time, so the decoder stamps `received_at`. That is the correct clock here: it measures
when a print could first have been reacted to, which is exactly what a reaction-time
model needs.

**Fallback and scope.** When the window holds no candidate that late it degrades to
`worst_case`, exactly as the `next_slot_*` pair does, so eligibility stays identical.
Serde is `{"lag_ms": 115}` (alias `{"lag": 115}`). Simulate-only: the grouped sweep
persists its fill model as a bare string and cannot round-trip a payload variant, and
books `worst_case` by design regardless.

## The differences between entry and exit

1. **Direction of "adverse".** Entry worst = the *highest* price; exit worst = the
   *lowest*. Same pessimism, mirrored.
2. **What counts as a candidate.** Entry needs a real **buy** (non-dust, priced > 0).
   Exit accepts **any** priced trade, buy or sell.
3. **How the next slot is found.** Entry looks for the next slot containing a
   *qualifying buy*; exit takes the next slot containing *any* trade. So a slot full of
   sells extends the entry window past it, but ends the exit window at it.

4. **Which way the median leans.** Entry takes the higher of two middle prints, exit
   the lower — the same mirroring as `worst_case`, at half the amplitude.

## What is identical across all six models

**The set of positions taken.** Every model shares the same fill *eligibility* — a
qualifying candidate must exist in the window, or the empty-window fallback applies.
So switching model is a **controlled reprice of a fixed trade set**, never a different
trade population. If a rule takes 412 positions under `worst_case`, it takes exactly
412 under `signal_price`. That is what makes the comparison meaningful: the delta is
purely fill pessimism. The `next_slot_*` pair narrows the *candidates* and never
eligibility — hence its fallback, and hence
`fill_models_share_entry_eligibility` asserting the property across every model on both
legs.

**Empty window.** When no candidate exists, `market_fill_on_empty_window` decides:

- `true` (all analysis paths + live paper **entry**) → fill at the signal trade itself.
  All three models collapse to the same price here.
- `false` (live paper **exit**) → no fill; the caller waits or fails closed.

## Which fill model to use

- **`worst_case`** — what live paper always books and what every surface defaults to,
  so it is the only setting with sweep↔paper parity. It is also the right model
  for **stop-type exits**: a stop fires *because* price is falling, so the next prints
  really are skewed lower — the pessimism is not a safety margin, it is the mechanism.
  Its cost: it penalises short holds and fast exits hardest, so a grid run under it
  drifts toward wide retraces and long holds.
  What `worst_case` is *not* is a latency penalty you can outrun. The dump-scalp
  measurement puts the ~6 pp first-fill-to-worst-fill gap on fill **dispersion** —
  prints inside one window are simply far apart, and this model takes the tail of that
  spread every time. Speed moves you to *one arbitrary print* in the window, not toward
  its good end.
- **`first_in_window`** — "we react to the print and take the next one", and the model
  the fill-sensitivity work reported its bottom line under. Read it knowing it is biased
  optimistic: whenever the next print sits in the signal's own slot, it prices something
  unreachable.
- **`next_slot_first`** / **`next_slot_median`** — the reachable central case. Same
  reaction story as `first_in_window` with the impossible half of the window removed.
  Prefer `next_slot_median` when the question is *what does a typical fill look like*
  (it reads the middle of the dispersion `worst_case` takes the tail of) and
  `next_slot_first` when you want that reading's optimistic edge.
- **`signal_price`** — zero feed-reaction slippage, and not reachable at all. Only ever
  an upper bound. **If a strategy is not profitable under `signal_price`, no amount of
  speed will save it** — that is exactly the question this mode answers, and the reason
  it exists.
- **`lag_ms`** — the one that settles an argument. The slot models give a bound; a
  negative under `next_slot_first` only proves "negative at ~400 ms" and says nothing
  about 115 ms. Grade the candidate at the measured decide→fill number and read the
  sign there. Run 50 / 115 / 200 together: where the sign flips tells you whether there
  is a latency budget worth spending on, or none at all.

The productive move is to run the spread rather than one number. A rule that is +8%
under signal and −4% under worst is a latency bet; one that is +6% / +5% / +4% is a real
edge. A large gap between `first_in_window` and `next_slot_first` says the specific
thing that the edge lives in same-slot prints — i.e. it is a same-block race, not a
reaction.

**Rank on a priced fill, never on `first_in_window`.** A search that scores candidates
at the next print walks straight into the densest bursts, because that is where the
unearned ordering privilege is worth most. Any entry search reports the
gap-to-next-print distribution of its selected trades beside the PnL: money concentrated
under ~50 ms is an artifact, not an edge. See
[the refuted-lines ledger](../../history/2026-09-03-refuted-lines-ledger.md)
for the case that established this.

---

# Part 2 — Cost models

## What is actually charged

Per leg (a round trip is 1 entry + N exit legs):

| term | value today | source |
| --- | --- | --- |
| pump.fun protocol fee | **125 bps** of the leg's SOL | measured, `FEE_BPS_PER_LEG` |
| Jito tip + priority fee | **0.000225 SOL**, fixed | `FeeTuning`: `JITO_MIN_TIP_SOL` (0.0002) + avg CU priority fee (0.000025) |
| our own price impact | `leg_notional / reserve_sol` | `pumpfun_impact` only |

The fixed term is **read from `.env`**, so it moves when you change
`JITO_MIN_TIP_SOL` / `CU_PRICE_MICRO_LAMPORTS`. It was 0.001025 SOL/leg when
[execution-costs.md](execution-costs.md) was measured (tip 0.001); at today's 0.0002
tip it is 0.000225. Restart the lab bin after editing those keys.

Only `pumpfun_impact` cares about **buy size**. `pumpfun_fee_only` is size-blind: a
0.1 SOL buy and a 10 SOL buy are charged the same percentage.

**There is no flat slippage term, and there must not be one.** A third model,
`pumpfun_default`, charged 100 bps/leg. It is deleted, along with the `slippage_bps`
field it set — the section below is why.

## One round trip, priced three ways — and why one of them is gone

The `pumpfun_default` row below is the **deleted** flat-slippage model. It is kept
here as the evidence that removed it — the numbers are why it went, and they are
the reason to reach for `pumpfun_impact` rather than assume a flat haircut is
close enough.

**0.1 SOL buy · price rises 20% · pool depth 70 SOL** (the measured median):

| cost model | gross proceeds | fee + fixed | **net PnL** |
| --- | --- | --- | --- |
| `pumpfun_impact` | 0.119658 ◎ (impact 0.143%/leg) | 0.003196 ◎ | **+16.46%** |
| `pumpfun_fee_only` | 0.120000 ◎ | 0.003200 ◎ | **+16.80%** |
| ~~`pumpfun_default`~~ (deleted) | 0.117624 ◎ (flat 1%/leg) | 0.003170 ◎ | **+14.45%** |

The same trade at **1.0 SOL** into the same 70 SOL pool:

| cost model | impact charged/leg | **net PnL** |
| --- | --- | --- |
| `pumpfun_impact` | 1.43% | **+13.87%** |
| `pumpfun_fee_only` | 0% | **+17.21%** |
| ~~`pumpfun_default`~~ (deleted) | flat 1% | **+14.86%** |

**Read those two tables together.** At 0.1 SOL the flat 1% is *harsher* than reality
(14.45% vs 16.46%); at 1.0 SOL it is *kinder* than reality (14.86% vs 13.87%). That
is the whole case against it in one number — it is wrong in both directions, and the
error **flips sign** somewhere in the middle.

A cost model that is merely too harsh is survivable: it shifts every candidate down
by roughly the same amount and the ranking holds. One whose error changes sign with
size does not shift the board, it **reshuffles** it — and a grid exists to rank.

It also double-counted. A `FillModel` chooses *which market print we transact
against*; a flat `slippage_bps` is a stand-in for that same quantity, so charging
both charged execution slippage twice. Two independent reasons, one conclusion: the
kind, the `CostModel::slippage_bps` field and the wire name are **deleted**, not
deprecated. `pumpfun_default` does not decode — an unrecognized cost model is a hard
error, because a run reporting a model it was not priced under is worse than one
that fails to load.

`pumpfun_fee_only` never charges impact, so it is a clean **upper bound**: 3.3 pp too
generous at 1 SOL, only 0.34 pp too generous at 0.1 SOL. That is a bound you can
reason with — it errs one way, and it errs more as size grows. It is the reason the
size-blind model that survived is the one that charges *nothing* rather than the one
that guessed.

## Depth is optional, and missing depth silently downgrades the model

Depth reaches the kernel as `Option<f64>` (`MetricSeries.reserve_sol` in the sweep,
`PositionOutcome::entry_reserve_sol` in simulate). If it's absent, **no impact is
charged rather than a guessed one** — so `pumpfun_impact` without depth is exactly
`pumpfun_fee_only`. The entry's depth also prices the exit leg, which over-charges
whenever the pool grew during the hold, i.e. it errs pessimistic on winners.

## Pairing the two dropdowns

|  | `pumpfun_impact` | `pumpfun_fee_only` |
| --- | --- | --- |
| `worst_case` | **honest floor** | floor, no size cost |
| `first_in_window` | loose central case | central, no size cost |
| `next_slot_first` | reachable, optimistic edge | same, no size cost |
| `next_slot_median` | **honest central case** | central, no size cost |
| `lag_<ms>` | **the verdict** | the verdict's upper bound |
| `signal_price` | ceiling, size-aware | absolute ceiling |

**Every cell is valid**, which is the point of deleting the third column. A
`FillModel` chooses *which market print we transact against*; impact is *how far our
own order moves the curve*. Those are orthogonal, and a live trade pays both, so
`fill model + impact` is always the correct pairing. There is no incoherent pair left
to warn about, and no surface warns about one.

## The bar a strategy has to clear

Gross move needed to break even, `pumpfun_impact`, 70 SOL pool, at today's 0.000225
fixed cost:

| buy size | break-even gross |
| --- | --- |
| 0.10 ◎ | +3.28% |
| **0.1255 ◎** (optimum = `sqrt(F · vsol)`) | **+3.26%** |
| 0.27 ◎ | +3.50% |
| 1.00 ◎ | +5.55% |

Cost is U-shaped in size — the tip is fixed SOL/leg so it dominates small orders,
impact dominates large ones. The optimum moved from ~0.27 ◎ to ~0.126 ◎ purely
because the tip dropped from 0.001 to 0.0002; recompute it whenever you retune tips.

Check a candidate against this bar **before** running a backtest — it kills most ideas
for free. And note it is the *cost* floor only: it assumes the fill model is free,
which `worst_case` very much is not.

---

# Part 3 — Reading a stored run

- Both models are persisted per run and rendered on the run header and in the
  Simulate history columns.
- **Every stored run names a live cost model.** Runs priced under the deleted
  flat-slippage model are deleted rather than migrated: relabelling one would have
  it report pricing it was never computed under, and its numbers double-counted
  execution cost, so there was nothing worth relabelling. A `cost_model` naming a
  model that no longer exists is a decode error, not a fallback.
- **`localStorage` outlives a deploy.** Saved Simulate / sweep / rule-search /
  family-search configs can still name a removed model, so each reads its cost model
  through `storedCostModel()` rather than spreading it into a request. Without that,
  the backend's strict decode is a 400 on every run from a value the user cannot see
  or clear.
- Runs from **before 2026-07-28** additionally used a 100 bps fee (the real one is
  125) and charged no impact at all. They understate cost by up to ~3 pp per round
  trip and are not comparable to anything newer. The constants are not stored per
  run, so there is no repricing them — re-run.
- Simulate, dry-run and the grouped sweep all take both dropdowns (the sweep carries
  them as one `Pricing` struct, so a scan can never get a fill model without a cost
  model). **Live paper is the exception: it always books `worst_case`** — it has no
  choice, since it fills forward off the live feed. That is what makes `worst_case`
  the only setting with sweep↔paper parity.
- **A live rule's open positions** are marked through `pumpfun_impact` too, off the
  mint's live cache price and depth (`MarkQuote`), so the unrealized figure on the
  positions panel is comparable to a backtest's `open_pnl_sol` rather than a raw price
  delta. One caveat it is worth knowing: it charges impact at **current** depth, while
  a backtest charges an open position's at **entry** depth. Neither is exact — one
  reserve cannot price two legs struck at different times — but the entry leg's impact
  is already sunk, so the leg the number is actually deciding is priced at the depth it
  would execute into. No cached depth ⇒ no impact charged, never a guess.
- Per the root rule, a sweep result is a *ranking screener*, not a backtest — re-run a
  promoted combo through simulate before believing its PnL, at the same fill and cost
  models.
