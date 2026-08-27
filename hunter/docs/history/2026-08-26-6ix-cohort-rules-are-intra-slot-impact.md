# 2026-08-26 — the 6ix cohort rules were intra-slot impact, and the fire set was truncated

Both rules derived for the 6ix `Buy`+fee launch cohort
(`[SetComputeUnitLimit, SetComputeUnitPrice, Create_v2, CreateIdempotent, Pump.Fun: Buy,
System Program: Transfer]`) are refuted. They are kept in `strategy_rules` — inactive,
tagged `stage-refuted,intra-slot-impact` — because they are the reproduction case for two
mistakes worth not repeating.

Rules as shipped (`hunter/engine/tests/six_ix_cohort_rules.rs` pins the exact JSON):

* **winner** `8f5d56ab` — `time >= 60 AND buy(5) >= 2.94 AND trade_count <= 140 AND
  gross_flow >= 43.6 AND buy(3) <= 23.1`
* **runner-up** `1c5e0789` — `time >= 60 AND net_flow(5) >= 0.79 AND
  m_flow_burst{60,3}.trade_share >= 7.69 AND first_slot_buy >= 6.41 AND
  trades_per_wallet(10) <= 2`

## Two independent errors, found in that order

### 1. The fire set was 5-16x too small

The search reported 443 fires (17.0/day) for the winner and ~447 (17.2/day) for the
runner-up over 07-28..08-22. Re-counted over the full cohort at engine semantics:

| rule | search said | actually | ratio |
| --- | ---: | ---: | ---: |
| winner | 443 (17.0/day) | **2,183** (84.0/day) | 4.9x |
| runner-up | ~447 (17.2/day) | **7,248** (278.8/day) | 16.2x |

This is not a SQL disagreement. On 08-20..08-22 the standalone count is **300** tokens and
`simulate` — a wholly separate implementation, driving the live `reduce` — enters **294**.
Two independent implementations agree with each other and against the search harness.

The ratios differ per rule (4.9x vs 16.2x), so it is **not** a uniform token subsample; the
harness was dropping candidate *prints*, not tokens. The harness was in-session and is not
recoverable, which is itself the lesson: a fire count is a claim, and it was never checked
against anything.

### 2. The whole edge is entry price, and 80% of it is gone in 25 ms

Once the fire set is right, the rules invert. Entry-lag ladder on the winner's 2,183
fires — the fill is the last print at or before `fire + lag`, returns priced as spot
`(vsol_out/vsol_in)^2`, **gross** (no fee, no impact):

| entry lag | entry price paid vs the fire print | mean @30 s | mean @60 s |
| --- | ---: | ---: | ---: |
| **0 ms (signal price)** | — | **+19.74 %** | +17.46 % |
| 25 ms | +13.06 % | +2.14 % | -0.30 % |
| 50 ms | +15.02 % | +0.49 % | -1.88 % |
| **115 ms (measured bot latency)** | +16.38 % | **-0.97 %** | -3.30 % |
| 250 ms | +17.23 % | -2.07 % | -4.04 % |
| 800 ms | +18.00 % | -3.16 % | -5.18 % |

The runner-up is the same shape at a smaller scale: +8.22 % at 0 ms, **-0.14 % at 25 ms**,
-2.98 % at 115 ms, on an entry price that rises 4.91 % in those first 25 ms.

Read the middle column: the "edge" **is** the price impact of the buy burst the gate fires
on. `buy(5) >= 2.94` is satisfied by a large buy landing, and that buy's own impact is the
whole +19.74 %. You are behind it by construction.

The previously recorded ladder ("flat: 115 ms +4.56 | 400 ms +3.88 | 800 ms +4.06 |
2000 ms +3.73") was measured on the truncated set and is wrong in both level and shape.

### No exit rescues it

The hold-horizon ladder at the 115 ms fill is negative gross at every horizon — best
-0.97 % at 30 s, and monotonically worse either side (-11.12 % at 5 s, -3.30 % at 60 s,
-8.09 % at 300 s). An exit redistributes; it cannot create. `simulate` at
`lag_115` + `pumpfun_impact` books **-12.81 %/trade** on week 1 (373 fires, PF 0.37,
win 22.3 %) and **-15.69 %** on 08-20..08-22 (294 fires, PF 0.36) — the extra ~12 pp over
the gross figure is the round-trip cost plus the `retrace 10` trail, which with the peak
seeded at the entry fill is a hard 10 % stop and closes at a median 11 s.

## What the gates DO earn

They are extremely strong **survival** filters, which is what the cohort thesis asked for.
First print past 60 s, one per token, 30 s hold, same fill and pricing throughout:

| gate | fires/day | mean @30 s | unsellable |
| --- | ---: | ---: | ---: |
| `age >= 60` alone | 2,145 | -65.33 % | 64.7 % |
| `+ gross_flow >= 43.6` | 841 | -32.03 % | 31.3 % |
| `+ trade_count <= 140` | 473 | -47.45 % | 48.6 % |
| `+ buy(5) >= 2.94` | 84 | **+19.72 %** | 2.0 % |
| `+ buy(3) <= 23.1` (full) | 84 | +19.72 % | 2.0 % |

Unsellable-bag rate falls **64.7 % -> 2.0 %**, and that alone moves the mean 85 pp. The
survival hypothesis is confirmed and is worth more than anything else measured on this
cohort. It is `buy(5) >= 2.94` that does nearly all of it — and the same term is what
puts the fill behind a burst. **The survival filter and the impact trap are the same
clause**, which is why the rule cannot be fixed by tuning it.

`buy(3) <= 23.1` moves 1 fire in 2,183: it is a no-op that survived a
drop-one-keep-the-rest ablation only because the ablation ran on the truncated set.

## What to carry forward

* **Count the fires before believing the money.** A fire count is cheap to check two ways
  and it is what makes every other number mean something. Both a standalone count and
  `simulate` on one short window would have caught this in minutes.
* **Price the entry at the fire print AND at the fill, and report the gap.** The gap
  column above is the diagnostic; a rule whose edge equals its entry-price rise is reading
  its own trigger's impact. This reproduces
  [`2026-08-18-copy-edge-is-own-price-impact.md`](2026-08-18-copy-edge-is-own-price-impact.md)
  and the FBvx intra-slot result from a third direction.
* **A latency ladder starting at 115 ms cannot see this.** The collapse is between 0 and
  25 ms. Ladders start at 0 and step in single-digit milliseconds through the first slot.
* The three metrics built for these rules (`m_flow_lifetime.trade_count`,
  `m_flow_window.trades_per_wallet`, `m_flow_burst.trade_share`,
  `m_snapshot.first_slot_buy`) are unaffected — they are correct measurements of what they
  name, verified against the lake in `track.rs`'s parity harness. A refuted rule does not
  refute its vocabulary.
