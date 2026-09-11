# C8 re-check: four-slot inventory walk

The scored 4-tuple and the numbers to match. Evidence: [_!___evidence.md](../_!___evidence.md)
6.13. Workflow campaign C8. Rank file: `cvx_conj4_rank.csv`.

This sentence does **not** ship. The +47.54 is a kernel-correct book on an illegal door
and a vacuous event (strategy 7.4 laws 30 and 31). Re-run it; do not trim it; do not
implement it.

---

## The leader (exact spelling)

```
D  documented-project: metadata JSON has a non-empty website AND
   (a non-empty telegram OR description length > 80)
E  token-silence: a non-racer buy >= 0.5 SOL whose slot gap from the previous
   print on this token is >= 10 empty slots
P  creator has not sold before this print
X  trail 40 % of price from the running peak, cap 600 s
   controls beside it: clock 45; shipped arm 21 / trail 36 / unarmed stop 43.75 / cap 1200
R  unlimited, one position per token
S  0.2 SOL
seat  last print landed by decision + 115 ms, both legs
cost  kernel.py: 125 bps + 0.000225 SOL a leg, impact on vsol
universe  last-leg week, cvx_prints.parquet, 6.76 days
```

What the runner actually scores for D: mint in `cvx_meta.parquet`, which is the 18,583
keep+ep50 mints, then the website cut. Missing meta = not documented.

What the runner actually scores for E: `gap_tok = 10**6 if i == 0`, so the first print
of the token always passes.

---

## Numbers to match

`python cvx_conj4.py` then `python cvx_conj4_diag.py`, from this directory.

Leader row in `cvx_conj4_rank.csv`: `documented | silence | creator | trail40`

| check | value |
| --- | --- |
| n | 1,622 |
| first-per-mint / day (mean) | 141.6 |
| SOL | **+47.54** |
| %/trade | 14.65 |
| days+ | **7/7**, worst **+0.86** |
| per-day first tickets | `47, 170, 220, 192, 140, 125, 63` |
| d50 (days with first >= 50) | 6/7 (day 0 = 47, floor fail) |
| top 1 % of net | 40.2 % |
| without top 1 % | **+28.43** |
| fit / hold (day <= 3 vs later) | +34.21 / **+13.33** |
| lag_0 trail40 | +144.85 |
| gap < 50 ms | 40.7 % of entries |
| gap p50 | 84 ms |
| age p50 (trail occupancy) | 4.7 s |
| clients (creation build) | 36, top 70.8 %, LOO **+13.90**, boot 99.7 % |
| top token share | 5.1 % |
| trail fire rate | 50.2 % |
| silent fill (xi == ei) | 0.5 % |

Causal and vacuous-event controls (same kernel, not in the rank file):

| check | n | SOL |
| --- | ---: | ---: |
| keep x silence x creator (no ep50 file) | 48,710 | **-1,034.87** |
| keep, age < 60 s only | 44,548 | **-1,007.34** |
| leader trades at local index 0 | 818 | **+46.08** |
| leader trades at k > 0 | 804 | +1.46 |
| leader, age < 1 s | 801 | +45.33 |
| age >= 60 s occupancy | 1,006 | **-0.82** |

Age split of the same fires, trail40 occupancy:

| slice | n | SOL | days+ | LOO | boot |
| --- | ---: | ---: | ---: | ---: | ---: |
| age < 60 s | 901 | **+48.23** | 7/7 | +11.73 | 99.7 % |
| age >= 60 s | 1,006 | **-0.82** | 3/7 | -3.40 | 43.4 % |
| age >= 300 s | 750 | -1.85 | 3/7 | -3.82 | 42.5 % |
| clock 45, age >= 60 s | 2,560 | **-10.88** | 0/7 | -11.08 | 0.4 % |

Neighbourhood (silence × creator × trail40, vary D): documented +47.54; n_pro>=8 +0.49;
keep+ep -7.93; slow-wall **-91.95**; open2 -94.75; keep -1034.87; none -1570.20.

Vary P on the leader: creator +47.54; none +42.28; cu_hi +11.10; creator+av **-4.13**.

Vary X on the leader: trail40 +47.54; shipped +42.75; clock45 +21.67; dies_g5+trail40 **-2.63**.

---

## What the sentence is

A keep+ep50 launch book with a website cut inside that file. Token-silence here is mostly
the first print of those tokens, not a mid-tape restart. The plus lives in age < 1 s.
Mid-tape age is red under both exit families.

Does not ship: the spelled D and E are not what was scored. Fetch metadata for the tape
before documented-project is a door again.

---

## The walk (what was scored)

7 D × 7 E × 8 P × 4 X = 1,528 occupancy cells. None of D, E, P, X frozen.
2,212,402 fires. 256 cells print plus. Runner: `cvx_conj4.py`.

D: none, keep, slow-wall 5 %, documented, keep+ep, n_pro>=8, first-buy>=2
E: burst START, token-silence >=10, returning-pro restart, dip print, size>=1,
   2-build slot, MONEY (2nd non-creator buyer, age 5-300 s)
P: none, creator, age/v, creator+age/v, cu_hi, creator+age/v+cu, crowd_gone,
   creator+age/v+crowd
X: clock 45, trail40 c600, shipped armed trail, burst-dies g5 + trail40

The short slot for a mid-tape harvester is **E**. Do not AND onto this parent.
