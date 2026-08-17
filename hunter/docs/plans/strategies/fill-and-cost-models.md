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
| Cost | `pumpfun_impact` | fee + tip + **our own** `B/vsol` impact | **default choice.** The only size-aware one |
| Cost | `pumpfun_fee_only` | fee + tip | you want a zero-impact upper bound |
| Cost | `pumpfun_default` | fee + tip + flat 1%/leg slippage | never, for a new run — legacy only |

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

## Half that window is unreachable

Slot `S` is the block the signal landed in. Pricing against a print there means our
transaction was already inside that block — built and submitted before the signal
existed. When the print is a bundle leg it is unreachable outright: bundle txs are
atomic and ordered, so nothing sequences between them.

Measured p50 landing is **+1 slot**. So slot `S` prints are prices we cannot hit, and
slot `S+1` is where we actually arrive. That split is what the two `next_slot_*` models
encode — same window, same eligibility, candidates restricted to `S+1`.

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

## What is identical across all five models

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

The productive move is to run the spread rather than one number. A rule that is +8%
under signal and −4% under worst is a latency bet; one that is +6% / +5% / +4% is a real
edge. A large gap between `first_in_window` and `next_slot_first` says the specific
thing that the edge lives in same-slot prints — i.e. it is a same-block race, not a
reaction.

---

# Part 2 — Cost models

## What is actually charged

Per leg (a round trip is 1 entry + N exit legs):

| term | value today | source |
| --- | --- | --- |
| pump.fun protocol fee | **125 bps** of the leg's SOL | measured, `FEE_BPS_PER_LEG` |
| Jito tip + priority fee | **0.000225 SOL**, fixed | `FeeTuning`: `JITO_MIN_TIP_SOL` (0.0002) + avg CU priority fee (0.000025) |
| our own price impact | `leg_notional / reserve_sol` | `pumpfun_impact` only |
| flat slippage | 100 bps | `pumpfun_default` only (legacy) |

The fixed term is **read from `.env`**, so it moves when you change
`JITO_MIN_TIP_SOL` / `CU_PRICE_MICRO_LAMPORTS`. It was 0.001025 SOL/leg when
[execution-costs.md](execution-costs.md) was measured (tip 0.001); at today's 0.0002
tip it is 0.000225. Restart the lab bin after editing those keys.

Only `pumpfun_impact` cares about **buy size**. The other two are size-blind: a 0.1
SOL buy and a 10 SOL buy are charged the same percentage.

## One round trip, priced three ways

**0.1 SOL buy · price rises 20% · pool depth 70 SOL** (the measured median):

| cost model | gross proceeds | fee + fixed | **net PnL** |
| --- | --- | --- | --- |
| `pumpfun_impact` | 0.119658 ◎ (impact 0.143%/leg) | 0.003196 ◎ | **+16.46%** |
| `pumpfun_fee_only` | 0.120000 ◎ | 0.003200 ◎ | **+16.80%** |
| `pumpfun_default` | 0.117624 ◎ (flat 1%/leg) | 0.003170 ◎ | **+14.45%** |

The same trade at **1.0 SOL** into the same 70 SOL pool:

| cost model | impact charged/leg | **net PnL** |
| --- | --- | --- |
| `pumpfun_impact` | 1.43% | **+13.87%** |
| `pumpfun_fee_only` | 0% | **+17.21%** |
| `pumpfun_default` | flat 1% | **+14.86%** |

**Read those two tables together.** At 0.1 SOL the legacy flat 1% is *harsher* than
reality (14.45% vs 16.46%); at 1.0 SOL it is *kinder* than reality (14.86% vs
13.87%). That is the whole case against `pumpfun_default` in one number — it is wrong
in both directions, and it flips sign somewhere in the middle, so it doesn't even
preserve ranking between two combos of different size.

`pumpfun_fee_only` never charges impact, so it is a clean **upper bound**: 3.3 pp too
generous at 1 SOL, only 0.34 pp too generous at 0.1 SOL.

## Depth is optional, and missing depth silently downgrades the model

Depth reaches the kernel as `Option<f64>` (`MetricSeries.reserve_sol` in the sweep,
`PositionOutcome::entry_reserve_sol` in simulate). If it's absent, **no impact is
charged rather than a guessed one** — so `pumpfun_impact` without depth is exactly
`pumpfun_fee_only`. The entry's depth also prices the exit leg, which over-charges
whenever the pool grew during the hold, i.e. it errs pessimistic on winners.

## Pairing the two dropdowns

|  | `pumpfun_impact` | `pumpfun_fee_only` | `pumpfun_default` |
| --- | --- | --- | --- |
| `worst_case` | **honest floor** | floor, no size cost | ✗ double-counts |
| `first_in_window` | loose central case | central, no size cost | ✗ double-counts |
| `next_slot_first` | reachable, optimistic edge | same, no size cost | ✗ double-counts |
| `next_slot_median` | **honest central case** | central, no size cost | ✗ double-counts |
| `signal_price` | ceiling, size-aware | absolute ceiling | ✗ double-counts |

The ✗ column: a `FillModel` chooses **which market print we transact against**;
`slippage_bps` is a flat stand-in for *the same thing*. Charging both counts execution
slippage twice. `pumpfun_default` remains the wire default only so stored runs keep
the meaning they were computed under — **never pick it for a new run.**

Impact is *not* in that trap: it is our own footprint on the curve, orthogonal to
which print we hit. A live trade pays both, so `fill model + impact` is the correct
pairing.

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
  Simulate history columns. A run with **no** stored model was computed under the
  wire defaults — `worst_case` + `pumpfun_default` — i.e. it double-counts slippage.
- Runs from **before 2026-07-28** additionally used a 100 bps fee (the real one is
  125) and charged no impact at all. They understate cost by up to ~3 pp per round
  trip and are not comparable to anything newer. The constants are not stored per
  run, so there is no repricing them — re-run.
- Simulate, dry-run and the grouped sweep all take both dropdowns (the sweep carries
  them as one `Pricing` struct, so a scan can never get a fill model without a cost
  model). **Live paper is the exception: it always books `worst_case`** — it has no
  choice, since it fills forward off the live feed. That is what makes `worst_case`
  the only setting with sweep↔paper parity.
- Per the root rule, a sweep result is a *ranking screener*, not a backtest — re-run a
  promoted combo through simulate before believing its PnL, at the same fill and cost
  models.
