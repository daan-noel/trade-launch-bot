# C14 re-check: first buy after a run of sells

The scored 4-tuples and the numbers to match. Evidence: [_!___evidence.md](../_!___evidence.md)
6.19. Workflow campaign C14. Rank file: `cvx_conj10_rank.csv`.

No cell ships. Re-run it; do not trim it. Do not AND onto the slow-wall SOL leader.
The reading is empty: no SOL>0 cell passes TYPE.

---

## Locked spelling

```
D  none . keep . slow-wall 5 % . keep+ep . n_pro>=8 . first-buy>=2
   one at a time. documented is not a door (cvx_meta is keep+ep50)
E  first buy after a run of sells: a non-racer buy >= 0.5 SOL that breaks
   >= 3 consecutive sell prints on this coin. Any buy breaks the run; only
   a qualifying buy fires. One fire per sell-run. Not after-flush, not
   "last print was a sell" (K=1), not C13
P  none . creator . age/v . creator+age/v . cu_hi . creator+age/v+cu .
   crowd_gone . creator+age/v+crowd
X  clock 45 . trail40 c600 . shipped armed trail .
   derived = trail40 + close if that structure sells .
   low = trail40 + close if vsol revisits the sell-run low
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

`python cvx_conj10.py`, from this directory.

READING: none. No SOL>0 age>=60 cell passes TYPE.

Legal SOL leader (not the reading):
`age60 | slowwall | sellrun | creator+av | shipped`

| check | value |
| --- | --- |
| n | 850 |
| SOL | **+10.40** |
| %/trade | 6.12 |
| days+ | **5/6** |
| d50 | **2/6** |
| top 1 % of net | 90.2 % |
| without top 1 % | **+1.02** |
| fire tickets age>=60 (D×P) | `0, 23, 159, 189, 35, 25, 5, 0` |
| peak/trough (full days) | **37.8x** against door 4.7x |
| top2 share | **80 %** against door 49 % |

Closest TYPE-shaped D×P (SOL is red, not a reading):
`age60 | slowwall | sellrun | cu_hi | derived` — n 2,116, **-5.24**,
tickets `110, 222, 199, 153, 216, 70`, peak/trough 3.17x against door 4.7x,
top2 45 % against 49 %, floor on every full day, body **-21.63**, hold **-8.47**.
`n_pro>=8 × creator × trail40` is **-5.40**, floor on full days, body **-34.47**.
Wide TYPE cells (none / keep / n_pro>=8 × none × trail40) are **-194 to -289 SOL**.

`low` is red as a TYPE-pass. Its SOL-positive cells fail TYPE (crowd_gone / slow-wall).

---

## Census

Sell-runs >=2/3/4/5: 1,004,972 / 598,709 / 386,838 / 263,653.
Fires 122,486. k==0 **0**. Sell-gap p50 **1.0 slot** (37.1 % gap 0).
n_sells p50 4.0. sell_sol p50 2.18. Age p50 92.6 s; 57.6 % age >= 60.
1,415 distinct structures. Next-print gap p50 **81 ms**; gap<50 **41.9 %**.

Funnel: `cvx_meta.parquet` 18,583 / 127,833 tape tokens, not scored.
`cvx_swdoor.parquet` overlap 119,234 / 127,833.

---

## The walk (what was scored)

1 E × 6 D × 8 P × 5 X, full tape and age>=60. documented dropped. None of D, P, X frozen.
122,486 fires. Runner: `cvx_conj10.py`.

D: none, keep, slow-wall 5 %, keep+ep, n_pro>=8, first-buy>=2
E: first buy after a run of sells (K>=3 consecutive sells, then >=0.5 non-racer buy)
P: none, creator, age/v, creator+age/v, cu_hi, creator+age/v+cu, crowd_gone,
   creator+age/v+crowd
X: clock 45, trail40 c600, shipped armed trail, derived tell (that structure sells),
   low (vsol revisits the sell-run low)
