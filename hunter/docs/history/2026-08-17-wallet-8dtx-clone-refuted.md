# 2026-08-17 — Wallet 8dtx dip-turn clone: mechanism confirmed, selection unreproducible

Reverse-engineered wallet `8dtx2tr4TuJsYpri2suggFu1pg3DVjFLBBVmhtDy1MEF` (`wallet_id`
2720) from scratch over the PG window 07-22..08-16, authored its logic as a generic rule,
and simulated it. The behavioural reconstruction held up. The clone did not: the wallet's
edge lives almost entirely in **which token it picks**, and none of the creation-time or
tape features we ingest reproduces that pick.

Do not re-run this search without a genuinely new data source.

## What the wallet does (this part replicated)

Curve-only, fresh tokens (median entry 68 s after creation). Enters on a dip-turn: real
reserves 3–16 SOL, lifetime price trail 5–60 % off the peak, the 10 s churn decayed to
single digits ("quiet gate", median 7.0 SOL vs 37 at setups it skipped), then the first
small buy run. Fixed 0.6567 SOL, one-shot full exit on a trail, ~250 fills/day, +327 SOL
gross over 25 days at a 36 % win rate.

Two measurement corrections made here:

- **Price = vsol² exactly** (measured exponent 2.0000), so a reserve-space trail and a
  price-space trail are the same policy up to the virtual +30 offset. A 6,412-hold policy
  replay was flat across both spaces and a 2.5× threshold range — the trail width is a
  variance knob, not a mean knob.
- **His exit is an *armed* trail, not a stop.** Measured over 6,409 completed holds: the
  median hold peaks at **+11.6 %** before exiting at **−18.4 %** off that peak (p25 12.7,
  p75 26.0), for a −6.2 % median outcome; 52.5 % of holds reach +10 % at some point and
  28.4 % end worse than −12 %. An unarmed `m_position.retrace >= 12` — the first thing
  authored here — is a −12 % hard stop from entry and is categorically not his rule.

## The trigger-replay harness, and why its verdict was too kind

Built `wstudy.h_state3`: 4.88 M per-print rows of engine-metric state over 10,500 tokens
(6,497 he entered + 4,003 band-reaching ones he skipped), computed with **wallet 2720
excluded** — leave his own trades in and his buy completes the buy-run, so the trigger
fires on itself. Reconstructed state at his entries reproduced every independently
measured distribution.

Against a 10.7 % base rate, precision stacked: tape only 14.9 % → + creation-bundle
(`first_slot_buy <= 5 SOL`) 24.6 % → + creation `ix_labels` shape 33.5 %. Timing match
within ±5 slots never exceeded 12 %.

**The control set was contaminated.** It was sampled with `ath_price >= 4.87e-14 AND
trade_count >= 100` — forward-looking filters — so the "tokens he skipped" were already
screened for traction. Precision therefore measured *identity* against a flattered
control. The simulator, which runs the true unfiltered 403 k / 74 k-token corpora, does
not share the bias, and it disagreed: the `ix_labels` gate that lifted precision
24.6 → 33.5 % moved PnL only −30.3 % → −25.2 %. **Precision against a biased control was
the wrong objective.**

## The simulate result

All runs: `next_slot_first` fill (our measured p50 is +1 slot), `pumpfun_impact` costs
(125 bps/leg + our own `buy/vsol` impact), 0.10 SOL fixed buy, `max_concurrent` 5, full
07-22..08-16 window.

| Rule | Corpus | n | mean | win | PF | dead |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| unarmed `retrace 12` | full 403 k | 10,731 | −16.44 % | 22.3 % | 0.34 | 4.6 % |
| armed trail (his exit) | full 403 k | 8,092 | −30.33 % | 25.8 % | 0.29 | 20.9 % |
| armed trail | **his 6,497 mints** | 2,168 | **+7.70 %** | 52.7 % | **1.53** | 3.5 % |

The within-run split is the cleanest form of the result. Inside the single shape+bundle
run — same rule, same fills, same costs, same period — the only variable is which token:

| | n | mean | win | dead |
| --- | ---: | ---: | ---: | ---: |
| tokens he traded | 1,028 | **+7.84 %** | 51.6 % | **3.3 %** |
| tokens he did not | 5,401 | **−32.82 %** | 22.2 % | **25.3 %** |

A 40.7 pp spread, and the mechanism is token death: a 7.7× difference in the Dead-exit
rate. His selector's job is to avoid tokens that stop trading under you.

**A better exit amplifies selection rather than substituting for it.** Swapping the −12 %
hard stop for his measured armed trail improves his picks (+3.63 → +7.70 %) and worsens
ours (−16.4 → −30.3 %), because a trail needs the token to actually run.

## Why the clone cannot be closed with what we ingest

Every creation-time feature was scored on the fired rows against a +7.84 % oracle ceiling.
None is worth more than a few points: `is_cashback_enabled` −26.3 → −22.4 %,
`initial_buy_sol >= 2` → −21.5 %, and `first_slot_buy`, `cu_limit`, `cu_price`,
`ix_labels_count`, `first_slot_sell`, `initial_supply` are inert (the Dead and survived
medians are equal). `tokens.meta` is empty in this DB, so there is no socials signal to
mine, and creator prior-launch count does not separate either.

The one entry term that does real work is **`m_flow_lifetime.gross_flow`** — a liveness
floor. Adding `>= 30` with `time <= 300` / `trail <= 30` cut the Dead rate 21.8 → 4.6 %
and lifted mean −26.3 → −9.3 %. Ablations isolate it: remove the floor and the same rule
prints −16.1 %; keep only the floor at `>= 100` and it prints −2.5 %.

But every tightening buys its improvement by **removing trades**, and the sequence
converges on zero from below, never through it:

| Config | n | mean | PF |
| --- | ---: | ---: | ---: |
| no liveness floor | 5,859 | −16.08 % | 0.36 |
| `gross_flow >= 30` | 1,712 | −3.59 % | 0.72 |
| `gross_flow >= 100`, loose bands | 693 | −2.50 % | 0.80 |
| `gross_flow >= 100` + tight bands | 143 | −0.81 % | 0.94 |
| tighter still | 49 | −3.34 % | 0.72 |

That shape is the signature of no edge: the limit of trading less is 0, not profit.

## The number that says the family is worth revisiting

Under the **pessimistic** `next_slot_median` fill, his token set still pays: **+4.85 %
mean, PF 1.32, 49.4 % win over 1,060 trades.** So the mechanism survives our real latency
and our own price impact. The blocker is selection alone, and the best selector buildable
from current data returns −1.60 % on the same fill.

An IS/OOS split was not run: nothing reached positive in-sample, so there was nothing to
validate.

## Corrections to earlier notes

- **Impact denominator is `vsol`, not real reserves.** `CostModel::price_impact` is
  charged against the `reserve_sol` the fold holds, which is virtual (`real + 30`). His
  0.657 SOL buy is therefore 1.4–2.0 % per leg in a 33–46 vsol pool, not the 4–20 % an
  earlier note claimed against real reserves.
- **The "edge dies in 1 second" kill gate overstated the gap.** It stacked our delay on
  top of his already-latent landed fills. Repricing through the fill models on decision
  rows is the honest arbiter, and it leaves the mechanism alive (see above).

## Artifacts

Scratch tables in schema `wstudy`: `h_tok`, `h_state`/`h_state2`/`h_state3`, `x_exit`,
`x_org`, `w8dt*`. Seventeen paper rules tagged `8dtx-clone` in `strategy_rules`, all
`is_active = false`, kept as the record of the grid.
