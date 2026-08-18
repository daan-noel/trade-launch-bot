# Wallet `8dtx2tr4` — the reconstructed dip-turn logic

`8dtx2tr4TuJsYpri2suggFu1pg3DVjFLBBVmhtDy1MEF` = `wallet_id` 2720. Reverse-engineered from
the PG curve firehose over 07-22..08-16 (6,497 mints, 6,409 completed holds), authored as a
generic rule and simulated. **This file is the mechanism spec** — read it instead of
re-deriving the wallet.

Two companions carry what is deliberately not repeated here: the refutation grid and the
measurement corrections are in
[`@history/2026-08-17-wallet-8dtx-clone-refuted.md`](../../history/2026-08-17-wallet-8dtx-clone-refuted.md),
and the two conclusions that generalise past this wallet are in
[wallet-analysis.md](wallet-analysis.md) (which also holds the four-wallet family table this
one joins). Cost model: [execution-costs.md](execution-costs.md). Exit param:
[armed-trailing-stop.md](armed-trailing-stop.md).

## Verdict in one paragraph

The behavioural reconstruction holds and **the mechanism clears our real execution**: on his
own token set, under the pessimistic `next_slot_median` fill with `pumpfun_impact` costs, the
authored rule prints **+4.85 % mean, PF 1.32, 49.4 % win over 1,060 trades**. The clone still
fails, because his edge is **which token he picks**, not the trigger — and no feature this DB
ingests reproduces that pick. Build from the exit and the liveness floor; do not rebuild the
selector.

## The book

| | |
| --- | --- |
| venue | pump.fun bonding curve only, never post-migration |
| entry age (median) | **68 s** after creation |
| size | **fixed 0.6567 SOL**, no scaling, no averaging down |
| structure | 1 buy → 1 full sell, one-shot |
| exit | **armed trail**, ~18 % off the in-hold peak |
| rate | ~250 fills/day |
| win rate | 36 % |
| result | +327 SOL gross over 25 days |

His own price impact is **1.4–2.0 % per leg** at 0.657 SOL into a 33–46 vsol pool. The
impact denominator is `reserve_sol` = **vsol** (`real + 30`), not the real reserve — the
fold holds the virtual figure (`replay.rs`, `reserve_sol: Some(real_reserve + 30.0)`).

## Entry — five conditions, all expressible today

He buys a **dip that is turning up on a quiet tape**. No new metric is required; a
per-print state replay over 4.88 M rows / 10,500 tokens reproduces every independently
measured distribution at his entries.

| # | Condition | Measured at his entries | Engine expression |
| --- | --- | --- | --- |
| 1 | early curve | real reserves **3–16 SOL** | `liquidity` band 3–16 |
| 2 | fresh | median **68 s** old | `time` |
| 3 | the dip | **5–60 %** off the lifetime peak | `m_price_lifetime.trail` band |
| 4 | **the quiet gate** | 10 s churn median **7.0 SOL** vs **37** at setups he skips | `m_flow_window(10).gross <= ~10` |
| 5 | the turn | the **first small buy run** after the quiet | `m_flow_window` net/gross rise |

**Condition 4 is the distinctive one.** It inverts the usual instinct: most rules chase
activity, he waits for activity to *stop* and then buys the first buy run into the vacuum.
The 5× separation between taken and skipped setups is the sharpest single discriminator in
his behaviour. Its standalone PnL contribution is **not** isolated — it is proven in
behaviour, unproven in money.

**Replay the trigger with wallet 2720 excluded.** Leave his own trades in and his buy
completes the buy run, so condition 5 fires on itself.

## Exit — an armed trail, and the trap in authoring it

Measured over 6,409 completed holds:

- median hold peaks at **+11.6 %**, then exits at **−18.4 % off that peak** (p25 12.7,
  p75 26.0), for a **−6.2 %** median outcome
- **52.5 %** of holds touch +10 % at some point; **28.4 %** end worse than −12 %

So the exit is `m_position.retrace` **with `arm_above_pct` set** — arm around +10, trail
~18. Authoring it without `arm_above_pct` makes it a **−12 % hard stop from entry**, because
the peak seeds at the entry fill. That is a categorically different rule and it is the first
thing this analysis got wrong.

**A trail amplifies selection quality in both directions.** Swapping the hard stop for his
measured armed trail moves his picks **+3.63 → +7.70 %** and the unselected corpus
**−16.4 → −30.3 %**. A trail needs the token to actually run, so it pays only on top of a
selector that works.

The 18 % width is wide by design and is **not** subject to the tight-trail latency problem
that killed the intra-slot turn rule
([`@history/2026-08-18-intra-slot-turn-refuted.md`](../../history/2026-08-18-intra-slot-turn-refuted.md)):
that failure is specific to reactive trails ≤ 4 %.

## Curve math

**Price = vsol², exponent measured 2.0000.** A reserve-space trail and a price-space trail
are therefore the same policy up to the virtual +30 offset — a 6,412-hold policy replay is
flat across both spaces and a 2.5× threshold range. The trail width is a **variance** knob,
not a mean knob.

## How the numbers are priced

All runs: `pumpfun_impact` costs (125 bps/leg + own impact), 0.10 SOL fixed buy,
`max_concurrent` 5, full 07-22..08-16 window, `next_slot_first` unless stated.

**Entry and exit are both charged +1 slot.** `find_paper_exit_at` under `NextSlotFirst` /
`NextSlotMedian` drops the firing slot entirely and prices at a real print in the next slot
— it never fills at the trigger level. These figures already satisfy the symmetric-pricing
rule the intra-slot refutation imposes.

| Rule | Corpus | n | mean | win | PF | dead |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| unarmed `retrace 12` | full 403 k | 10,731 | −16.44 % | 22.3 % | 0.34 | 4.6 % |
| armed trail (his exit) | full 403 k | 8,092 | −30.33 % | 25.8 % | 0.29 | 20.9 % |
| armed trail | **his 6,497 mints** | 2,168 | **+7.70 %** | 52.7 % | **1.53** | 3.5 % |
| armed trail, `next_slot_median` | **his mints** | 1,060 | **+4.85 %** | 49.4 % | **1.32** | 3.4 % |

## The finding that decides everything

Inside **one** run — same rule, same fills, same costs, same period — split only by whether
wallet 2720 ever traded the mint:

| | n | mean | win | dead |
| --- | ---: | ---: | ---: | ---: |
| tokens he trades | 1,028 | **+7.84 %** | 51.6 % | **3.3 %** |
| tokens he does not | 5,401 | **−32.82 %** | 22.2 % | **25.3 %** |

A **40.7 pp** spread with execution held constant, and the mechanism is **token death** — a
**7.7×** difference in the `Dead` exit rate. His selector's whole job is avoiding tokens
that stop trading under you.

## Why the selector is not rebuildable from this DB

Every creation-time feature scored on the fired rows against the +7.84 % oracle ceiling.
Best single screen: `is_cashback_enabled` −26.3 → −22.4 %; `initial_buy_sol >= 2` → −21.5 %.
`first_slot_buy`, `cu_limit`, `cu_price`, `ix_labels_count`, `first_slot_sell` and
`initial_supply` are **inert** — Dead and survived medians are equal. Creator prior-launch
count does not separate. **`tokens.meta` is empty in this DB**, so socials/website/name
signal is not ingested at all.

The one entry term that does real work is **`m_flow_lifetime.gross_flow`**, a liveness
floor: `>= 30` (with `time <= 300`, `trail <= 30`) cuts Dead 21.8 → 4.6 % and lifts mean
−26.3 → −9.3 %, and ablation isolates the floor as the whole effect. But tightening it buys
every further gain by **removing trades**, converging on zero from below and turning back:

| Config | n | mean | PF |
| --- | ---: | ---: | ---: |
| no liveness floor | 5,859 | −16.08 % | 0.36 |
| `gross_flow >= 30` | 1,712 | −3.59 % | 0.72 |
| `gross_flow >= 100`, loose bands | 693 | −2.50 % | 0.80 |
| `gross_flow >= 100` + tight bands | 143 | −0.81 % | 0.94 |
| tighter still | 49 | −3.34 % | 0.72 |

That shape is the signature of no edge: the limit of trading less is 0, not profit. An
IS/OOS split is not run because nothing reaches positive in-sample.

## Measurement traps this wallet exposed

- **Do not score a selector against a filtered control.** `wstudy.h_tok`'s control set is
  sampled with `ath_price >= 4.87e-14 AND trade_count >= 100` — forward-looking — so
  precision measures identity against a flattered control. It reported tape 14.9 % →
  +bundle 24.6 % → +`ix_labels` shape 33.5 % against a 10.7 % base rate, while the same
  shape gate moved simulated PnL only −30.3 → −25.2 %. The simulator runs the true
  unfiltered corpus and is the only honest instrument here. `wstudy.h_fwd` carries the same
  defect.
- **Do not stack our latency on a landed-fill timeline.** The wallet's own tx latency is
  already inside its landed prints; adding ours on top double-charges it.

## What transfers, ranked

1. **The armed trail** (arm ~+10, trail ~18) — the only component that *creates* return
   rather than avoiding loss (+4.07 pp of pure exit improvement), and portable onto any
   entry. Needs a working selector under it.
2. **The `m_flow_lifetime.gross_flow` liveness floor** — the largest single lever found
   anywhere in this work (+12.5 pp by ablation). Put it on any rule that arms a broad
   universe. It is a cost-reducer, not an edge.
3. **The quiet gate** — the most interesting idea, unproven in money. Worth isolating on a
   rule whose entry is already positive.

## Artifacts

Scratch tables in schema `wstudy`: `h_tok`, `h_state`/`h_state2`/`h_state3`, `x_exit`,
`x_org`, `w8dt*`. Seventeen rules tagged `8dtx-clone` in `strategy_rules`, all
`trade_mode = 'paper'`, `is_active = false`, kept as the record of the grid.
