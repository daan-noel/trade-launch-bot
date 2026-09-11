# C16 re-check: first run of an operator structure

The scored 4-tuples and the numbers to match. Evidence: [_!___evidence.md](../_!___evidence.md)
6.21. Workflow campaign C16. Rank file: `cvx_conj12_rank.csv`.

No cell ships. Re-run it; do not trim it. Do not AND onto the slow-wall TYPE
reading or the n_pro SOL leader. The reading is the TYPE-pass, not that leader.

---

## TYPE reading (exact spelling)

```
D  slow-wall 5 %: previous UTC day, >= 20 coins, >= 5 % made a slow wall,
   not a bundle launch group
E  first run of an operator structure: first >= 0.5 buy of a non-creator,
   non-seed operator structure (n>=200, nw<=50) that has never printed
   here, after at least one prior print that is neither the creator nor a
   seed racer. One fire per (coin, structure).
   Not C15 (prior is only creator and seed). Not C9 S1 (staged-leg class).
   Not C12 (second burst). Not C8 restart.
P  cu_hi: this print's CU price >= tape p75 of buy CU
X  derived = trail40 + close if that structure sells
   controls beside it: clock 45; trail40 c600; shipped arm 21 / trail 36 /
   unarmed stop 43.75 / cap 1200; follow = trail40 + close if no new
   operator for >= 10 slots, below fill
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

`python cvx_conj12.py`, from this directory.

Age>=60 TYPE-pass row in `cvx_conj12_rank.csv`:
`age60 | slowwall | firstrun | cu_hi | derived`

| check | value |
| --- | --- |
| n | 491 |
| SOL | **+2.27** |
| %/trade | 2.32 |
| days+ | **3/6** |
| per-day first tickets | `40, 101, 120, 75, 92, 17` |
| d50 | **4/6** |
| peak/trough (full days) | **7.06x** against door 4.7x |
| top2 share | **50 %** against door 49 % |
| top 1 % of net | 219.2 % |
| without top 1 % | **-2.71** |
| fit / hold (day <= 3 vs later) | +5.20 / **-2.93** |
| k==0 | **0** |
| gap < 50 ms | 37.7 % |
| gap p50 | 158 ms |
| age p50 | 216.3 s |
| clients (creation build) | 26, top **158 %**, LOO **-1.32**, boot 65.4 %, p5 -5.45 |
| trail fire rate | 57.0 % |
| silent fill | 0.0 % |
| tell | 0.8 % |

SOL leader on the same slice (TYPE fails, not the reading):
`age60 | n_pro>=8 | firstrun | creator+av+cu | derived` — n 331, **+9.14**,
4/7, d50 2/7, top1 39.2 %, body **+5.56**. Tickets `7, 33, 86, 104, 49, 15, 20`,
peak/trough 6.9x against door 3.3x, top2 62 % against 45 %.

`follow` on the TYPE-pass D×P is **-3.65**.

---

## Census

New operator-here 192,964. Of which k==0 582. No-other-yet 7,984.
First buy < 0.5 SOL 141,505. Fires **42,893**. Fire k==0 **0**.
Age p50 **6.4 s**; 22.0 % age >= 60. n_other p50 36. n_op_before p50 5.
Prior-gap p50 **0 slots** (68.7 % gap 0). 118 structures.
Next-print gap p50 **56 ms**; gap<50 **47.2 %**.
Same-structure next buy 9,666/42,893 (22.5 %), p50 **8,758 ms**.

Funnel: `cvx_meta.parquet` 18,583 / 127,833 tape tokens, not scored.
`cvx_swdoor.parquet` / c_build overlap 119,234 / 127,833.
group-live-now 78,452 tokens; births peak/trough 2.5x, top2 41 %
against tape 2.1x / 40 %.

---

## The walk (what was scored)

1 E × 7 D × 8 P × 5 X, full tape and age>=60. documented dropped. None of D, P, X frozen.
42,893 fires. Runner: `cvx_conj12.py`.

D: none, keep, slow-wall 5 %, keep+ep, n_pro>=8, first-buy>=2, group-live-now
E: first run of an operator structure (coin already has others)
P: none, creator, age/v, creator+age/v, cu_hi, creator+age/v+cu, crowd_gone,
   creator+age/v+crowd
X: clock 45, trail40 c600, shipped armed trail, derived tell (that structure sells),
   follow (no new operator for >= 10 slots, below fill)
