# C12 re-check: this-coin second-attempt burst

The scored 4-tuples and the numbers to match. Evidence: [_!___evidence.md](../_!___evidence.md)
6.17. Workflow campaign C12. Rank file: `cvx_conj8_rank.csv`.

No cell ships. Re-run it; do not trim it. Do not AND onto the slow-wall SOL leader.
The reading is the TYPE-pass, not that leader.

---

## TYPE reading (exact spelling)

```
D  n_pro >= 8: at least eight operator structures (n>=200 prints, nw<=50) have
   already bought this coin before the fire
E  second-attempt burst: first non-racer buy >= 0.5 SOL of an ix structure's
   second-or-later burst on this coin. Burst break = that structure silent
   >= 10 slots. One fire per (coin, structure).
   Not C8 restart (operator structures, every return).
   Not C11 (staged-leg class)
P  creator has not sold
X  trail 40 % of price from the running peak, cap 600 s
   controls beside it: clock 45; shipped arm 21 / trail 36 / unarmed stop 43.75
   / cap 1200; derived = trail40 + close if that structure sells
R  unlimited, one position per token
S  0.2 SOL
seat  last print landed by decision + 115 ms, both legs
cost  kernel.py: 125 bps + 0.000225 SOL a leg, impact on vsol
universe  last-leg week, cvx_prints.parquet, 6.76 days, age >= 60 s slice
```

documented is not a door on this walk (`cvx_meta` is keep+ep50, law 30).
Age >= 60 s is the ranking slice. Rank TYPE first, then SOL.

---

## Numbers to match

`python cvx_conj8.py`, from this directory.

Age>=60 TYPE-pass row in `cvx_conj8_rank.csv`:
`age60 | n_pro>=8 | second | creator | trail40`

| check | value |
| --- | --- |
| n | 2,936 |
| SOL | **+3.12** |
| %/trade | 0.53 |
| days+ | **4/8** |
| per-day first tickets | `53, 171, 344, 377, 203, 142, 102, 2` |
| d50 | 7/8 (full days all >= 50; day 0 and day 7 are stubs) |
| peak/trough (full days) | **3.7x** against door 3.3x |
| top2 share | **54 %** against door 45 % |
| top 1 % of net | 935 % |
| without top 1 % | **-26.04** |
| fit / hold (day <= 3 vs later) | +10.73 / **-7.61** |
| k==0 | **0** |
| gap < 50 ms | 28.7 % |
| gap p50 | 178 ms |
| age p50 | 372 s |
| clients (creation build) | 48, top **330 %**, LOO **-7.18**, boot 57.4 %, p5 -16.58 |
| trail fire rate | 56.0 % |
| silent fill | 0.1 % |

SOL leader on the same slice (TYPE fails, not the reading):
`age60 | slowwall | second | creator+av | trail40` — n 817, **+17.78**, 4/6,
d50 2/6, top1 54.6 %, body **+8.07**. C2's door.

---

## Census

First-burst buys 1,499,804. Second-burst starts 2,068,498. Fires 105,497.
k==0 **0**. Age p50 83.8 s; 57.4 % age >= 60. 699 distinct structures.

Funnel: `cvx_meta.parquet` 18,583 / 127,833 tape tokens, not scored.
`cvx_swdoor.parquet` overlap 119,234 / 127,833.

---

## The walk (what was scored)

1 E × 6 D × 8 P × 4 X, full tape and age>=60. documented dropped. None of D, P, X frozen.
105,497 fires. Runner: `cvx_conj8.py`.

D: none, keep, slow-wall 5 %, keep+ep, n_pro>=8, first-buy>=2
E: this-coin second-attempt burst
P: none, creator, age/v, creator+age/v, cu_hi, creator+age/v+cu, crowd_gone,
   creator+age/v+crowd
X: clock 45, trail40 c600, shipped armed trail, derived tell (that structure sells)
