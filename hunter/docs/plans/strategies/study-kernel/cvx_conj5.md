# C9 re-check: four new events, four-slot walk

The scored 4-tuples and the numbers to match. Evidence: [_!___evidence.md](../_!___evidence.md)
6.14. Workflow campaign C9. Rank file: `cvx_conj5_rank.csv`.

No cell ships. Re-run it; do not trim it. Do not AND onto the after-flush parent.

---

## Closest cell (exact spelling)

```
D  n_pro >= 8: at least eight professional builds (n>=200 prints, nw<=50) have
   already bought this token before the fire
E  after-flush: first non-racer buy >= 0.5 SOL after a flush episode has been
   still. Flush = price <= -20 % from the token's peak so far. Still = no new
   reserve low for >= 10 slots. Episode ends if price recovers above -10 %.
   One fire per episode
P  creator has not sold, age 60-900 s, vsol 33-81, cu_price >= tape p75
X  trail 40 % of price from the running peak, cap 600 s
   controls beside it: clock 45; shipped arm 21 / trail 36 / unarmed stop 43.75
   / cap 1200; derived = trail40 + close if reserve makes a new low below the
   flush low
R  unlimited, one position per token
S  0.2 SOL
seat  last print landed by decision + 115 ms, both legs
cost  kernel.py: 125 bps + 0.000225 SOL a leg, impact on vsol
universe  last-leg week, cvx_prints.parquet, 6.76 days, age >= 60 s slice
```

Doors stay one at a time. Age >= 60 s is the ranking slice (the C8 plus is age
< 60 s), not a written extra AND.

---

## Numbers to match

`python cvx_conj5.py` then `python cvx_conj5_diag.py`, from this directory.

Age>=60 row in `cvx_conj5_rank.csv`:
`age60 | n_pro>=8 | flush | creator+av+cu | trail40`

| check | value |
| --- | --- |
| n | 693 |
| first-per-mint / day (mean) | 79.6 |
| SOL | **+8.73** |
| %/trade | 6.30 |
| days+ | **5/7**, worst **-0.79** |
| per-day first tickets | `21, 72, 170, 143, 58, 35, 39` |
| d50 (days with first >= 50) | 4/7 |
| top 1 % of net | 83.9 % |
| without top 1 % | **+1.41** |
| fit / hold (day <= 3 vs later) | +6.70 / **+2.03** |
| gap < 50 ms | 41.3 % |
| gap p50 | 82 ms |
| age p50 | 253 s |
| clients (creation build) | 33, top **32.0 %**, LOO **+5.93**, boot 98.5 %, p5 +1.60 |
| trail fire rate | 71.9 % |
| silent fill | 0.0 % |

Shipped on the same fires: n 687, **+9.05**, 4/7, top1 92.6 %, wo +0.67, LOO +6.51,
boot 99.0 %, p5 +2.30.

SOL leader on the same slice (does not clear the client gate):
`age60 | slowwall | flush | creator | derived` — n 1,646, +10.22, 3/6, tickets
`15, 190, 257, 43, 24, 20`, top client **82.1 %**, LOO +1.83, boot 88.4 %, body
**-5.55**.

---

## The other three events

Machine census: 233 professional builds, 44 staged-leg, 46 campaign, **2 rebuy-bots**.

| E | fires | age p50 | age>=60 share | age>=60 best | note |
| --- | ---: | ---: | ---: | --- | --- |
| S12 after-flush | 53,225 | 159 s | 70 % | +10.22 slow-wall / +8.73 n_pro>=8 | closest, does not ship |
| S1 staged-leg | 7,882 | 60 s | 50 % | +5.42 slow-wall, first/day 24 | C2's door |
| S3 unfinished-budget | 338 | **0 s** | 8 % | 2 trades at age>=60 | launch |
| S4 rebuy-under-exit | 92 | 1,136 s | 97 % | +0.26, first/day 4.3 | starved under is_pro; C10 is 6.15 |

---

## The walk (what was scored)

4 E × 7 D × 8 P × 4 X, full tape and age>=60. None of D, E, P, X frozen.
61,537 fires. Runner: `cvx_conj5.py`. C8 events stay in 6.13.

D: none, keep, slow-wall 5 %, documented, keep+ep, n_pro>=8, first-buy>=2
E: staged-leg first buy, unfinished-budget, rebuy-under-exit, after-flush first buy
P: none, creator, age/v, creator+age/v, cu_hi, creator+age/v+cu, crowd_gone,
   creator+age/v+crowd
X: clock 45, trail40 c600, shipped armed trail, derived tell

n_pro>=8 as D on burst START is red (6.11). The same door on after-flush is the
closest cell. Dropping the door turns after-flush × creator red (**-7.91**).
