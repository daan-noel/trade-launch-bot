# 2026-09-09: the out-of-sample unit behind a launch door is the client, not the day

Three campaigns ran on the last-leg week tape: C0b (door-v3 MONEY through the gates), the
L-axis on its fires, and C2 re-run behind the slow-wall launch door that C0a had just
labelled. Two of them produced the strongest numbers the program has ever printed, and the
third measurement explained why neither can be believed yet.

## What was run

| run | script | question |
| --- | --- | --- |
| C0b | `cvx_c0b.py` | does door-v3 MONEY clear the floor, tail and walk-forward gates at `lag_115`? |
| L-axis | `cvx_c0b_ldoor.py` | is the -50 % trade predictable, on a book that has one? |
| control | `cvx_c0b_control.py` | is that prediction, or is it headroom to fall? |
| C2 | `cvx_c2_slowwall.py` | does the door with 4-6x forward survival recover the burst-start event? |
| diagnosis | `cvx_c2_diag.py`, `cvx_creator_diag.py`, `cvx_client_diag.py`, `cvx_build_holdout.py` | why does the hold half hold 11 % of the trades? |

The kernel gained one key to do it: `spec['stop']`, the unarmed leg of the shipped
door-v3 exit (armed trail 10/20 with a -25 % stop while unarmed, all stated on the reserve).
Its thresholds square into the price basis the kernel books in - 21 / 36 / -43.75 - which is
the only correct conversion and the one the engine gate had already caught once.
`kernel_test3.py` asserts the key is inert when absent, exact when present, and that the
conversion is the square.

## The three results

**C0b does not ship, and it reproduced the other session's book to one trade.** 1,666 fires,
+5.39 SOL, 246.5 first-per-mint a day. Floor clears. Tail fails at 357 % of net in the top
1 %, and without those seventeen trades the book is -13.84. Walk-forward fails at -1.93. The
door and the permission are each load-bearing and monotone: the event alone is -10.34 %/trade,
plus the permission -8.00, plus the door -2.55, plus both +1.62.

**The L axis is answered, after a control killed the obvious answer.** On the raw panel the
entry reserve looked like the strongest -50 % predictor ever measured: 0.01 % in the lowest
quintile against 34.10 % in the highest. It predicts nothing. `price = vsol^2 / k` with the
curve floored at 30, so a -50 % is arithmetically impossible below entry reserve 42.43, and
80.4 % of the fires sit below it. Reserve separates trades that *can* collapse from trades
that cannot - C1's scope error, mirrored.

Read inside a fixed reserve band, one term survives: **bundle share below 0.20**. In band
40-50 it cuts the L-rate 25.55 % -> 10.97 % while raising the `>= +200 %` rate; among the 601
door-v3 trades that can reach -50 % at all it reads 9.5 % against 48.3 %. On the money it
takes the book from +5.39 to +10.43 SOL and the loser count from 235 to 17, and it is the
only cell that stays positive with its top 1 % removed. It books **-151 SOL alone on the full
tape**. Snipers, fresh share and buyer count go flat inside a band; they were reserve in
disguise. The creator permission *inverts* on this axis - it buys survival and it buys the
coins whose dev still holds the supply.

**C2 behind the slow-wall door is the strongest cell the program has produced.** 1,006 trades,
**+15.51 SOL, +7.71 % a trade**, 68.7 first-per-mint a day, 4 of 6 days, -50 % rate 2.5 %.
The same event books -5.34 %/trade behind the door with no permission and -3.33 behind the
best public door. Clock 45 on the same cell is +0.81, which is the story's own prediction: the
harvester is the derived exit and the scalper is the control.

## Why none of it ships

The C2 cell's hold half had 111 trades against the fit half's 895, an 8x drop where the
undoored event drops 1.9x. Chasing that down:

* it is not the tape - prints per day fall 2x, not 8x;
* it is not the age or reserve band - each holds 48-56 % and 77-87 % of door fires every
  single day;
* it is not the creator field - identified on 95-97 % of tokens every day, and creator-in is
  a flat 16-21 % tape-wide once the creator trades;
* it is **the creator permission inside the door**, which reads 11.7 % of door fires on day 1,
  81.5 % on day 3 and 12.4 % on day 6.

That is the door's client changing. One creation build,
`fb455027566e5992b8a10792b1d8d7f6` (`5ix:ix#6f`), in the door for two days, carries **708 of
the 1,006 C2 trades and 82.1 % of its net**, and **710 of the 1,666 C0b trades and 220.7 % of
its net**. A one-week print tape holds 34 slow-wall builds with a median life of two days.

So a 1,006-trade cell is about 19 independent draws, and a day split puts the one that matters
entirely inside the fit half. Under leave-one-build-out and a bootstrap over builds:

| cell | builds | positive | drop the best | bootstrap positive |
| --- | ---: | ---: | ---: | ---: |
| slow-wall 5 % + E + P, trail40 | 19 | 9 | **+2.77** | 89.8 % |
| slow-wall 5 % + E + P, clock 45 | 19 | 5 | -2.34 | 55.2 % |
| door-v3 MONEY, shipped | 23 | 10 | **-6.50** | 61.7 % |
| door-v3 MONEY + bundle < 0.20, shipped | 11 | 6 | **+1.42** | 93.8 % |
| door-v3 MONEY + bundle < 0.20, trail40 | 11 | 5 | -0.10 | 79.8 % |

The slow-wall cell is the first in the program to survive its best client leaving. It still
lands at 89.8 % rather than 95 %, and its tail gate fails at 78.3 % - though the body pays:
the other 996 trades are +3.37 SOL, about +1.7 % a trade net.

## What changed in the standing method

The gate list gained a **client gate** ahead of walk-forward
([_!___workflow.md](../../hunter/docs/plans/strategies/_!___workflow.md) section 4): report the
client count, the top client's share, the book with each client removed in turn, and a
bootstrap over clients. Behind a launch door the client is the creation build; on a trader
node it is the machine; on an event-only sentence it is the token. A cell that dies when its
best client leaves is that client's book. A cell that survives and still sits under the bar
needs **more clients, never another term**.

Two laws joined [_!___strategy.md](../../hunter/docs/plans/strategies/_!___strategy.md) 7.4:
count clients rather than trades, and compare a rate only where the outcome is reachable.

The queue gained **C7, thirty days of prints**, and it is the binding constraint on every
door-behind sentence: it turns 19 draws into about 150. Two cells are frozen exactly as
written, waiting on it.
