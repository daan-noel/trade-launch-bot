# C15 re-check: first operator after only creator and seed racers

The scored 4-tuples and the numbers to match. Evidence: [_!___evidence.md](../_!___evidence.md)
6.20. Workflow campaign C15. Rank file: `cvx_conj11_rank.csv`.

No cell ships. Re-run it; do not trim it. Do not AND onto the SOL leader.
The reading is empty: no SOL>0 cell passes TYPE.

---

## Locked spelling

```
D  none . keep . slow-wall 5 % . keep+ep . n_pro>=8 . first-buy>=2 .
   group-live-now (sibling of the same creation fingerprint printed in
   the last 10 slots at this coin's first print)
   one at a time. documented is not a door (cvx_meta is keep+ep50)
E  first operator after only creator and seed racers: first >= 0.5 buy
   of a non-creator, non-seed operator structure (n>=200, nw<=50) on a
   coin whose prior prints are only the creator and seed racers
   (CreateAccountWithSeed). One fire per coin. i==0 cannot fire
P  none . creator . age/v . creator+age/v . cu_hi . creator+age/v+cu .
   crowd_gone . creator+age/v+crowd
X  clock 45 . trail40 c600 . shipped armed trail .
   derived = trail40 + close if that structure sells .
   follow = trail40 + close if no new operator for >= 10 slots, below fill
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

`python cvx_conj11.py`, from this directory.

READING: none. No SOL>0 age>=60 cell passes TYPE (8 fires on that slice).

Legal SOL leader (not the reading):
`age60 | none | firstop | creator | clock45`

| check | value |
| --- | --- |
| n | 7 |
| SOL | **+0.01** |
| %/trade | 0.75 |
| days+ | **2/3** |
| d50 | **0/3** |
| top 1 % of net | 105.7 % |
| without top 1 % | **0.00** |

Full-tape wide cell (not the reading):
`all | none | firstop | none | clock45` — n 1,360, **-50.75**, 0/8,
d50 7/8, body **-61.51**.

`follow` is red (best age>=60 **-0.01**). `n_pro>=8` is empty.

---

## Census

First operator-structure buy 51,170. Of which k==0 582. Killed by another
actor first 42,603. First buy < 0.5 SOL 6,586. Fires **1,399**. Fire k==0 **0**.
Age p50 **0.0 s**; 0.6 % age >= 60. Prior p50 1.0 (creator 1, seed 0).
Prior-gap p50 **0 slots** (89.1 % gap 0). 46 structures.
Next-print gap p50 **2 ms**; gap<50 **74.0 %**.
Same-structure next buy 481/1,399, p50 **1 ms**.

Funnel: `cvx_meta.parquet` 18,583 / 127,833 tape tokens, not scored.
`cvx_swdoor.parquet` / c_build overlap 119,234 / 127,833.
group-live-now 78,452 tokens; births peak/trough 2.5x, top2 41 %
against tape 2.1x / 40 %.

---

## The walk (what was scored)

1 E × 7 D × 8 P × 5 X, full tape and age>=60. documented dropped. None of D, P, X frozen.
1,399 fires. Runner: `cvx_conj11.py`.

D: none, keep, slow-wall 5 %, keep+ep, n_pro>=8, first-buy>=2, group-live-now
E: first operator after only creator and seed racers
P: none, creator, age/v, creator+age/v, cu_hi, creator+age/v+cu, crowd_gone,
   creator+age/v+crowd
X: clock 45, trail40 c600, shipped armed trail, derived tell (that structure sells),
   follow (no new operator for >= 10 slots, below fill)
