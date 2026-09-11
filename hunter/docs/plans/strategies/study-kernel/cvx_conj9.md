# C13 re-check: arrives from another coin

The scored 4-tuples and the numbers to match. Evidence: [_!___evidence.md](../_!___evidence.md)
6.18. Workflow campaign C13. Rank file: `cvx_conj9_rank.csv`.

No cell ships. Re-run it; do not trim it. Do not AND onto the slow-wall SOL leader.
The reading is empty: no SOL>0 cell passes TYPE.

---

## Locked spelling

```
D  none . keep . slow-wall 5 % . keep+ep . n_pro>=8 . first-buy>=2
   one at a time. documented is not a door (cvx_meta is keep+ep50)
E  arrives from another coin: first non-racer buy >= 0.5 SOL on this coin
   by an ix structure whose latest print is a buy on a different coin,
   still inside that other-coin burst (slot gap < 10). One fire per
   arrival by construction. Not C12, not C11, not the S9 door
P  none . creator . age/v . creator+age/v . cu_hi . creator+age/v+cu .
   crowd_gone . creator+age/v+crowd
X  clock 45 . trail40 c600 . shipped armed trail .
   derived = trail40 + close if that structure sells .
   leave = derived, or close when that structure prints on another coin
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

`python cvx_conj9.py`, from this directory.

READING: none. No SOL>0 age>=60 cell passes TYPE.

Legal SOL leader (not the reading):
`age60 | slowwall | arrive | creator+av | trail40`

| check | value |
| --- | --- |
| n | 1,029 |
| SOL | **+16.46** |
| %/trade | 8.00 |
| days+ | **4/6** |
| d50 | **2/6** |
| top 1 % of net | 73.6 % |
| without top 1 % | **+4.35** |
| fire tickets age>=60 (D×P) | `0, 18, 180, 211, 36, 30, 8, 0` |
| peak/trough (full days) | **26.4x** against door 4.7x |
| top2 share | **81 %** against door 49 % |

Closest TYPE-shaped D×P (SOL is red, not a reading):
`age60 | n_pro>=8 | arrive | creator | trail40` — n 3,851, **-4.64**, 2/8,
d50 7/8, body **-42.07**. Wide TYPE-pass cells (none / keep / n_pro>=8 × none)
are **-112 to -437 SOL**.

`leave` is red on every cell in the rank head (best **-3.04**).

---

## Census

Switches 7,585,976. From-buy 4,195,500. Live (gap < 10) 2,397,238.
Live-gap p50 **1.0 slot**. Marked fires 281,560. Emitted 280,791.
k==0 **5,443 (1.9 %)**. Age p50 138.5 s; 63.0 % age >= 60. 300 distinct
structures. Leave-time finite 100 %.

Funnel: `cvx_meta.parquet` 18,583 / 127,833 tape tokens, not scored.
`cvx_swdoor.parquet` overlap 119,234 / 127,833.

---

## The walk (what was scored)

1 E × 6 D × 8 P × 5 X, full tape and age>=60. documented dropped. None of D, P, X frozen.
280,791 fires. Runner: `cvx_conj9.py`.

D: none, keep, slow-wall 5 %, keep+ep, n_pro>=8, first-buy>=2
E: arrives from another coin (live buy elsewhere, gap < 10 slots)
P: none, creator, age/v, creator+age/v, cu_hi, creator+age/v+cu, crowd_gone,
   creator+age/v+crowd
X: clock 45, trail40 c600, shipped armed trail, derived tell (that structure sells),
   leave (sells or prints on another coin)
