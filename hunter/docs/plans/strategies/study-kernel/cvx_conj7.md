# C11 re-check: late-leg of a staged-leg machine

The scored 4-tuples and the numbers to match. Evidence: [_!___evidence.md](../_!___evidence.md)
6.16. Workflow campaign C11. Rank file: `cvx_conj7_rank.csv`.

C9 S1 fires on the first cluster. This walk is the unused moment: the first print of
cluster 2+. No cell ships. Re-run it; do not trim it. Do not AND onto this parent.

---

## Closest cell (exact spelling)

```
D  slow-wall 5 %: this token's creation build is in the trailing 5 % slow-wall
   launch door (previous UTC day, launch_build_day_stats)
E  late-leg: first non-racer buy >= 0.5 SOL of a staged-leg machine's
   second-or-later buy-cluster on this token. Cluster break = 10 empty slots of
   that machine. A staged-leg machine is a professional build (n>=200 prints,
   nw<=50) with 2+ buy-clusters on >= 25 % of the coins it buys (min 10) and
   median inter-cluster gap >= 10 slots. One fire per (token, machine).
   Not C9 S1 (first cluster)
P  creator has not sold, age 60-900 s, vsol 33-81
X  trail 40 % of price from the running peak, cap 600 s, plus close if that
   staged-leg machine sells
   controls beside it: clock 45; trail40 alone; shipped arm 21 / trail 36 /
   unarmed stop 43.75 / cap 1200
R  unlimited, one position per token
S  0.2 SOL
seat  last print landed by decision + 115 ms, both legs
cost  kernel.py: 125 bps + 0.000225 SOL a leg, impact on vsol
universe  last-leg week, cvx_prints.parquet, 6.76 days, age >= 60 s slice
```

Doors stay one at a time. Age >= 60 s is the ranking slice, not a written extra AND.

---

## Numbers to match

`python cvx_conj7.py`, from this directory.

Age>=60 row in `cvx_conj7_rank.csv`:
`age60 | slowwall | late | creator+av | derived`

| check | value |
| --- | --- |
| n | 241 |
| first-per-mint / day (mean) | 27.8 |
| SOL | **+6.93** |
| %/trade | 14.38 |
| days+ | **5/6**, worst **-0.25** |
| per-day first tickets | `2, 78, 92, 11, 3, 2` |
| d50 (days with first >= 50) | 2/6 |
| top 1 % of net | 23.1 % |
| without top 1 % | **+5.33** |
| fit / hold (day <= 3 vs later) | +6.59 / **+0.34** |
| gap < 50 ms | 27.8 % |
| gap p50 | 176 ms |
| age p50 | 219 s |
| clients (creation build) | 11, top **68.9 %**, LOO **+2.15**, boot 99.5 %, p5 +1.15 |
| trail fire rate | 75.9 % |
| silent fill | 0.0 % |

Trail40 on the same fires: n 240, +6.75, body +5.15, LOO +2.10, boot 99.2 %.
Shipped: n 240, +4.94, 6/6, LOO +2.44, boot 99.9 %.
Clock 45: **-0.31**.

Dropping the door (none × creator+av × derived): n 651, +4.72, top1 101 %, body
**-0.06**, hold -0.74, LOO -0.06, boot 81 %.

---

## Census

Staged-leg machines 44 (same C9 class). Late-leg fires 6,204; first-cluster buys
65,920; late-cluster starts 90,387. Age p50 195 s; 75 % age >= 60; 29 unique
machines.

---

## The walk (what was scored)

1 E × 7 D × 8 P × 4 X, full tape and age>=60. None of D, P, X frozen.
6,204 fires. Runner: `cvx_conj7.py`.

D: none, keep, slow-wall 5 %, documented, keep+ep, n_pro>=8, first-buy>=2
E: late-leg first buy of cluster 2+
P: none, creator, age/v, creator+age/v, cu_hi, creator+age/v+cu, crowd_gone,
   creator+age/v+crowd
X: clock 45, trail40 c600, shipped armed trail, derived tell (that machine sells)
