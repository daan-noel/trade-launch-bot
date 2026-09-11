# C10 re-check: S4 as sell-then-buy, not is_pro

The scored 4-tuples and the numbers to match. Evidence: [_!___evidence.md](../_!___evidence.md)
6.15. Workflow campaign C10. Rank file: `cvx_conj6_rank.csv`.

The C9 class is empty (2 machines). This walk drops `n>=200, nw<=50` and keeps
sell-then-buy. No cell ships. Re-run it; do not trim it. Do not AND onto this
parent, and do not put `nw<=50` back.

---

## Closest cell (exact spelling)

```
D  slow-wall 5 %: this token's creation build is in the trailing 5 % slow-wall
   launch door (previous UTC day, launch_build_day_stats)
E  price under own exit: first print after >= 10 empty slots on the token whose
   reserve is below a rebuy-bot's last sell on this token, that build having not
   bought back since that sell. A rebuy-bot is a build that sells and later buys
   on >= 25 % of the coins it sells (min 10 coins sold). No professional-print cut
P  creator has not sold, age 60-900 s, vsol 33-81
X  trail 40 % of price from the running peak, cap 600 s
   controls beside it: clock 45; shipped arm 21 / trail 36 / unarmed stop 43.75
   / cap 1200; derived = trail40 + close if that rebuy-bot sells
R  unlimited, one position per token
S  0.2 SOL
seat  last print landed by decision + 115 ms, both legs
cost  kernel.py: 125 bps + 0.000225 SOL a leg, impact on vsol
universe  last-leg week, cvx_prints.parquet, 6.76 days, age >= 60 s slice
```

Doors stay one at a time. Age >= 60 s is the ranking slice, not a written extra AND.

---

## Numbers to match

`python cvx_conj6.py`, from this directory.

Age>=60 row in `cvx_conj6_rank.csv`:
`age60 | slowwall | rebuy | creator+av | trail40`

| check | value |
| --- | --- |
| n | 831 |
| first-per-mint / day (mean) | 70.3 |
| SOL | **+4.47** |
| %/trade | 2.69 |
| days+ | **3/6**, worst **-0.66** |
| per-day first tickets | `16, 179, 208, 33, 32, 7` |
| d50 (days with first >= 50) | 2/6 |
| top 1 % of net | 181.3 % |
| without top 1 % | **-3.63** |
| fit / hold (day <= 3 vs later) | +5.60 / **-1.13** |
| gap < 50 ms | 19.1 % |
| gap p50 | 615 ms |
| age p50 | 204 s |
| clients (creation build) | 21, top **87.2 %**, LOO **+0.57**, boot 81.8 %, p5 -2.86 |
| trail fire rate | 61.1 % |
| silent fill | 0.0 % |

Shipped on the same fires: n 804, +4.14, hold +0.48, top 96.9 %, LOO +0.13, boot 81.5 %.

Derived (firing machine sells) on the same fires: n 1,366, **-7.17**.

Dropping the door (none × creator+av × trail40): n 4,594, **-82.70**.

---

## Census (which cut emptied C9)

| cut | machines |
| --- | ---: |
| any sell-then-buy (n_rebuy >= 1) | 170 |
| n_sell >= 10 & frac >= 0.25 (**C10**) | **30** |
| nw <= 50 & n_sell >= 10 & frac >= 0.25 | 11 |
| n >= 200 & n_sell >= 10 & frac >= 0.25 | 21 |
| is_pro (n >= 200, nw <= 50) | 233 |
| is_pro & n_sell >= 10 | 26 |
| is_pro & n_sell >= 10 & frac >= 0.25 (**C9**) | **2** |

The emptying cut is `nw<=50` on a sell-then-buy machine: tight professional builds
almost never rebuy at 25 %. The C10 class of 30 has median 5,009 buy-prints and
median 958 wallets - wide spray, not a tight bot. Fires 272,191; age p50 390 s;
86 % age >= 60; 30 unique machines.

---

## The walk (what was scored)

1 E × 7 D × 8 P × 4 X, full tape and age>=60. None of D, P, X frozen.
272,191 fires. Runner: `cvx_conj6.py`.

D: none, keep, slow-wall 5 %, documented, keep+ep, n_pro>=8, first-buy>=2
E: price under own exit (sell-then-buy class, no is_pro)
P: none, creator, age/v, creator+age/v, cu_hi, creator+age/v+cu, crowd_gone,
   creator+age/v+crowd
X: clock 45, trail40 c600, shipped armed trail, derived tell (that bot sells)
