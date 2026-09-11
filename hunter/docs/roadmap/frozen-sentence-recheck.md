# Frozen sentence: the slow-wall burst-start cell, and how to recheck it on 30 days

One rule is frozen and waiting for data. This file is everything needed to re-run it and to
decide, without reading anything else. **Change nothing in it before the recheck** - trimming a
frozen sentence because a prefix holds out better is refused
([_!___derive.md](../plans/strategies/_!___derive.md) 14).

Recheck date: **2026-09-30**, or any day the print tape covers 30 days.

---

## 1. The sentence, exactly

```
DOOR        a coin whose creation ix_labels belongs to a launch build that, on the
            PREVIOUS UTC day, launched >= 20 coins of which >= 5 % became slow-wall
            runners (curve peak reserve >= 60 SOL, peak reached >= 60 s after birth),
            and whose creation cgroup is not a bundler group
            (3ix:Buy, 4ix:Buy, 3ix:BuyExactSolIn)

EVENT       a buy of >= 0.5 SOL by a router or terminal tool, which is the FIRST print
            of that tool's run after >= 2 slots in which that tool was silent on this
            coin; not a racer build; not one of our own instrument wallets

PERMISSION  coin age 60-900 s at the trigger print
            reserve after the trigger print in [33, 81] SOL
            the creator has not sold at any point before the trigger print

EXIT        thresholds are stated on the RESERVE and squared into price:
              arm      reserve +10 %   = price +21 %
              trail    reserve -20 %   = price -36 % off the running peak, live once armed
              stop     reserve -25 %   = price -43.75 % below the fill, live while UNARMED
              cap      1200 s
RE-ENTRY    one position per coin at a time; a new trigger is taken only after the
            previous exit
SIZE        0.2 SOL
SEAT        both legs fill at the last print landed by decision + 115 ms
```

**No wallet-identity term.** An earlier draft carried "at least 2 of the solo 26 already
bought this coin". It is removed and must not come back: the readers' mint list is never a
gate ([CLAUDE.md](../../CLAUDE.md), [_!___strategy.md](../plans/strategies/_!___strategy.md)
7.4 and 9). Removing it also made the cell better.

---

## 2. What it reads on the 6.76-day tape (2026-08-30 17:48 .. 09-06 12:00)

| | value |
| --- | ---: |
| trades | 986 |
| net SOL | **+13.75** |
| per trade, after all costs | **+6.97 %** |
| days positive | 5 of 6 |
| mean win / mean loss | +91.8 % / -33.1 % |
| trades at -50 % or worse | 7.2 % |
| distinct creation builds (clients) | 19 |
| top client's share of net | 64 % |
| net with the best client removed | **+4.93** |
| bootstrap over clients, share positive | **95.9 %** |
| top 1 % of trades, share of net | **91.5 %** |
| first-per-mint tickets, per day | **16, 177, 210, 24, 28, 9** |

Reproduce with `study-kernel/cvx_audit_refrozen.py` (door5 + E + P, SHIPPED exit) on
`cvx_prints.parquet` + `cvx_swdoor.parquet`.

---

## 3. What it passes and what it fails today

| gate | bar | today | verdict |
| --- | --- | --- | --- |
| money | net SOL > 0 on the whole sentence | +13.75 | **pass** |
| both exit families read | shipped and trail40 c600 side by side | +13.75 / +15.51 | **pass** |
| client gate | positive without its best client, and positive in >= 95 % of a bootstrap over clients | +4.93 and 95.9 % | **pass** |
| walk-forward | fit and hold both positive | not separable - see below | **not decidable** |
| tail | top 1 % of trades <= ~20 % of net (a real convex book runs 9-12 %) | 91.5 % | **FAIL** |
| ticket floor | >= 50 first-per-mint trades **on each day** | 2 of 6 days | **FAIL** |

**Why the two failures are the same failure.** 764 of the trades fall on two days, and those
are the two days one creation build - `fb455027566e5992b8a10792b1d8d7f6`, cgroup `5ix:ix#6f` -
was launching. A slow-wall build stays in the door a median of **2 days**, and a 6.76-day tape
holds only **34** of them. So this tape cannot distinguish "the rule works" from "one operator
was trading", and the day-based fit/hold split is measuring the wrong axis.

**It cannot be fixed by editing the sentence.** Measured:

| variant | tickets a day | floor | net SOL | best client removed |
| --- | --- | ---: | ---: | ---: |
| the frozen sentence | 16, 177, 210, 24, 28, 9 | 2/6 | **+13.75** | +4.93 |
| drop the creator permission | 141, 312, 270, 205, 347, 84 | **6/6** | **-17.26** | -26.29 |
| drop the age / reserve band | 218, 626, 587, 273, 483, 159, 99 | **7/7** | +10.50 | **-17.37** |
| door only, no permission | 526, 887, 644, 502, 1034, 228, 166 | 7/7 | -86.74 | -113.62 |

The term that earns the money is the term that removes the tickets.

---

## 4. What is already ruled out as the cause

The sentence is audited against every recorded way a study positive dies in the engine
([_!___evidence.md](../plans/strategies/_!___evidence.md) 4.8). These are settled and do not
need re-checking:

- **the clock** - `t_ms` carries real milliseconds (8 distinct stamps per 9-print slot, 205 ms
  span), so `lag_115` is a 115 ms model and not a slot model;
- **the door label** - `runners` is the slow wall exactly, from the same
  `launch_build_day_stats` the live engine stamps at `TokenCreated`;
- **the universe** - `aa.pxf` is the full curve tape (98,338 mints, matching `trades` on the
  shared window, rows within 0.04 %), so no filter is hiding in the query that builds it;
- **the seat** - four times worse on both legs still books +8.71; zero lag is 1.98x, so the
  fill model costs half the book and charges it honestly;
- **capacity** - never more than 10 positions open, 2.0 SOL of capital;
- **pricing** - impact on the virtual reserve, exits filled 115 ms after their trigger and
  never at it.

---

## 5. The recheck, step by step

1. **Confirm the tape.** `hunter/lake-data/trades/dt=*` must hold 30 consecutive sealed days.
   *If days are missing they are gone for good* - Postgres drops at 30 days and the lake
   export is hand-run. Nothing below is worth doing on a gapped tape.
2. **Rebuild the study tape** over the full window, in the schema `study-kernel/cvx_export.py`
   produces (`mint, slot, tx_index, t_ms, reserve_lamports, amount_lamports, side, wallet_id,
   payer_id, proxied, build, creator_id`), last leg per `(mint, slot, tx_index)`.
3. **Rebuild the door labels** with `study-kernel/cvx_door_export.py` over the same window.
4. **Re-run the sentence unchanged**: the event with `cvx_burst.py`, the book and the gates
   with `cvx_audit_refrozen.py`, the client gate with `cvx_build_holdout.py`.
5. **Read the gates in this order**, and stop at the first failure:

| | pass | fail |
| --- | --- | --- |
| clients | >= 100 distinct creation builds carry trades | the tape is still too short; wait |
| ticket floor | >= 50 first-per-mint on >= 80 % of days | **dead as written** - the ticket supply is a rotating client, not a market |
| money | net SOL > 0 | dead |
| client gate | positive with the best client removed, and >= 95 % of a bootstrap | dead - it was one operator |
| tail | top 1 % <= 20 % of net | not shippable as a standalone book; it is a lottery, and the L-terms below are the only route |

6. **If every gate passes**, freeze again and go to a disjoint week, then engine
   reconciliation per trade, then paper, then small real - never straight to real.

---

## 6. Two terms held back deliberately

Both cut the loser cost and both are **kept out of the frozen sentence on purpose**, so that
the recheck tests one thing. Add them only after the sentence passes on its own.

- **bundle share < 0.20** - the share of live supply held by wallets that bought in the
  creation slot, computed strictly before the trigger. On the door-v3 book it takes the -50 %
  rate from 14.1 % to 2.3 % and the book from +5.39 to +10.43 SOL. It is red on its own on the
  full tape (-151 SOL), which is what a conjunction term should look like.
- **agreement among the solo 26** - the strongest loss-side term measured (a 0.37 lift on the
  -50 % rate against a 0.82 activity-matched null, holding outside the window the roster was
  fitted on). **It is a thermometer, never a term**: naming their coins is wallet identity. It
  says the loss axis is real; a durable version has to be written in ix structure or tape
  state.

Detail: [_!___evidence.md](../plans/strategies/_!___evidence.md) 3.7 and 6.8.

---

## 7. The exit is settled and is not a free parameter

The exit above wins an exit lab run with the entry held fixed
([_!___evidence.md](../plans/strategies/_!___evidence.md) 4.7). Two results from it carry
forward to any future sentence:

- **a state-conditional cut does hold `W` while `L` falls** - the arrivals-stall cut takes the
  mean loss from 31.5 % to 10.1 % while the mean win holds at 101.9 %, break-even 23.4 % ->
  9.0 %. The static abort grid could never do this (it cuts `L` 60 % and `W` 75 %);
- **it must only be able to fire while the position is under water, keyed to the fill price.**
  The same cuts allowed to fire above the fill book -14 to -20 SOL, with the mean win
  collapsing from ~100 % to 8-14 %. A cut that can fire while the position is ahead is a clock,
  not a cut;
- **a better distribution is not more money.** The state cut triples the ticket count, and the
  toll is charged per ticket: 1,073 extra round trips cost about 7.7 SOL against 4.3 earned
  back.
