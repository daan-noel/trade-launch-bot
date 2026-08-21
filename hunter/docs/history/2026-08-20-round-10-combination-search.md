# Round 10 (2026-08-20): the exhaustive combination search, and what survives it

Round 9 crossed nineteen stored features with the deep cell one at a time and stacked the
three survivors. This round does the whole thing properly: **every 1-, 2- and 3-way
combination of 55 predicates, plus greedy OR-portfolios, ranked in money at the
money-optimal position size, with a max-statistic permutation null and a walk-forward.**

The search finds structure. **No procedure that selects on it survives the day boundary.**
What survives is the shape that was already there: the fresh-wallet screen plus entry depth,
sized conservatively.

## The setup

`iv.feat` is the full ten-day pool - 27,754 tokens, one row per token per day, carrying every
feature at its decision slot together with `arm`, `bsz` and realized `net` from the round-7
verified engine. 55 predicates are generated from it (natural thresholds plus within-pool
quantiles, both directions); every 1-, 2- and 3-way conjunction with at least 300 tokens is
scored - **17,744 cells**.

Two corrections to how a cell is read, both of which change the answer:

**Money is `e^2 * vsol / 8`, not `n * e`.** At the money-optimal size `B* = e*vsol/4` the SOL
per trade is `B*e - 2B^2/vsol - 2F = e^2*vsol/8 - 2F` - **quadratic in the edge**. Ranking
cells at a fixed size therefore understates every high-edge cell by its own edge ratio, which
is exactly the bias that made round 9 read the deep cut as "quality not throughput". Each
cell is instead scored at its own best pool fraction over a 0.25-3% grid.

**The cost model inverts exactly.** Recovering the pre-impact edge from the stored `net`
through the round-7 relation and re-applying it at the stored size reproduces `net` to
`max|diff| = 1.7e-13`. The stored sizing is a median 0.267% of pool.

The model is static - it does not re-fire triggers as size grows - so it reads about 20%
optimistic against round 9's SQL re-firing (2.93 against 2.44 SOL/day on the deep cell at 1%).
Rankings are unaffected; absolute numbers for a promoted cell need the SQL path.

## What the search finds

Ranked by money at optimal size, the top of the table is the **bare fresh-wallet rule** and
near-copies of it. Nothing beats it by more than noise: best cell
`rule & nb30<=8 & imp<=5.0` at 5.34 SOL/day against 5.09 for the rule alone. Every
high-edge cell loses on money at flat comparison because trade count falls faster than the
edge rises - even squared.

Greedy OR-portfolios do better still. Unioning five high-edge cells reaches **6.44 SOL/day**
at 118 trades/day and 8.89% per trade, against the rule's 5.09 at 688 trades/day and 3.21%.

Both numbers are mirages. The rest of this record is why.

## The max-statistic null

The entire search is re-run on 40 within-day permutations of the outcome column - features,
sizes and day structure held fixed, the feature-to-outcome link destroyed. Each permutation
reports the best cell its own search finds, which prices the whole selection procedure rather
than one cell.

| statistic | observed | null mean | null sd | null max | z |
| --- | --- | --- | --- | --- | --- |
| best SOL/day at optimal size | **5.34** | 1.34 | 0.39 | 2.29 | **10.22** |
| best per-trade edge (`wnet`) | 14.33 | 6.43 | 1.45 | **12.10** | 5.43 |
| greedy OR-portfolio | **6.44** | 0.91 | 0.99 | 3.12 | **5.61** |
| count of cells positive in IS *and* OOS *and* forward | 1,522 | 1,299 | 550 | 2,887 | **0.41** |

Three things follow, and two of them are gates:

- **Structure exists.** The best money cell is four null standard deviations past the best
  cell any shuffled search produces.
- **Per-trade edge is a weak statistic.** A search this size extracts **12.10%** per trade
  from pure noise. Round 9's headline stack at +12.91% sits essentially at that ceiling.
  Rank cells in money, never in percent - the money statistic separates at z=10.2 where
  percent separates at z=5.4.
- **"Positive in IS, OOS and forward" is worth nothing as a gate.** 8.6% of cells pass it;
  shuffled data passes 7.3% +/- 3.1%, z = **0.41**. Three-window agreement is what a
  three-way split of a fat-tailed venue produces by construction. It has been quoted as
  evidence in every round since 7 and it is not evidence.

## The blind test: select on eight days, read two

Every selection procedure is re-run using only 08-11..18, and its choice is then read on
08-19 and 08-20, which it never saw. The null re-runs the whole procedure per permutation.

| procedure | in-selection | **blind forward** | null mean | z |
| --- | --- | --- | --- | --- |
| max money | 7.18 | **-3.03** | -3.49 | 0.32 |
| max per-trade edge | 15.53 | **-3.17** | -1.60 | -1.15 |
| max edge, IS and OOS both positive | 15.53 | **-3.17** | -1.59 | -1.16 |
| max money, IS and OOS both positive | 7.18 | **-3.03** | -3.49 | 0.32 |
| greedy OR-portfolio | 9.96 | **-9.80** | -2.85 | **-3.19** |

All four single-cell procedures lose money forward and none beats its null. The portfolio is
worse: at z = -3.19 it is **significantly worse than chance**. Selecting hard on in-sample
money does not merely fail to generalise - it actively picks the cells that break.

The winner selected blind is `rule & nb30<=5`, and all fifteen of the top money cells lose
forward. Adding the IS/OOS-positivity gate changes nothing, as the null above predicts.

## The walk-forward: six held-out days

Two forward days is thin, so the same procedures are run as a rolling walk-forward - train on
every prior day, trade the next - giving six genuinely held-out days (08-15..08-20).

| procedure | held-out SOL/day | days positive | z vs null |
| --- | --- | --- | --- |
| re-search for max money each day | **-0.23** | 2/6 | 2.41 |
| re-search for max edge each day | +1.21 | 5/6 | 3.07 |
| **fixed `rule`, no search** | **+3.83** | 5/6 | **9.49** |
| **fixed `rule & vsol>=40`, no search** | **+2.49** | 5/6 | **8.25** |

**The two fixed rules beat every adaptive procedure.** Re-deriving the best combination each
day and trading it makes less money than trading the rule and never searching at all. This is
the round's central result, and it is the opposite of the premise the round started from.

## Which fixed rule, and at what size

The in-sample-optimal pool fraction does not transfer either. In selection the deep cells
peak at 2.5-3% of pool; forward they peak at **1-1.5%**, and the cells sized at their
in-sample optimum flip sign - `rule & vsol>=40 & nb30<=3` reads +5.62 SOL/day in selection at
3% and **-0.29 forward at the same 3%**, against +1.27 forward at 1%. Sizing on a fitted edge
squares the fitting error. **Size at 1-1.5% of pool, not at the measured optimum.**

At a fixed 1%, over the six held-out days:

| cell | trades/day | SOL/day | **drop the single best day** | days positive |
| --- | --- | --- | --- | --- |
| `rule` | 671 | 4.46 | **0.90** | 5/6 |
| `rule & vsol>=35` | 145 | 2.26 | 1.52 | **6/6** |
| **`rule & vsol>=40`** | 100 | **2.34** | **1.73** | **6/6** |
| `rule & vsol>=40 & nb30<=3` | 46 | 1.86 | 1.55 | 5/6 |
| `rule & vsol>=40 & r10<=0` | 74 | 1.62 | 1.20 | 5/6 |

**The bare rule's money is one day.** Across all ten days at 1% it books 5.09 SOL/day, of
which 08-17 alone is 22.27 of 50.93 - 44% of the book from 10% of the tape. Drop the best day
and it falls to 3.18; on held-out days alone, 4.46 falls to **0.90**. Its top 1% of trades is
**108% of the book** - the other 99% are net negative together - top 5% is 277%, and **67% of
its trades lose money.**

The depth cell is the first selection in this program that is not that. Top 1% is 43%, top 5%
is 122%, **33% of trades lose** - a majority of its trades win. Ten-day maximum drawdown is
0.89 SOL against the rule's 2.78, and it is positive on 9 of 10 days and 6 of 6 held-out days.

Adding round 9's `nb30<=3` costs money at fixed size (1.86 against 2.34 held-out) and buys
risk reduction instead: worst day -0.10 against -0.89, top 1% concentration 25% against 43%.
It is a risk knob, not an edge.

## The features that are not usable

- **`a5_uwshare` is 99.2% ties** on this pool and carries no information where it is applied.
  The distribution gate kills it outright; round 3 reached the same place by a longer route.
- `f_selldecel` and `f_buyaccel` are 92% and 88% NULL on every day, so their non-null cells
  are too small to rank.
- `alive` and `uwb30` stay disqualified from round 9 - look-ahead, and a duplicate of `nb30`.

## What is still untested

`a_deficit`, `b_wall` and `b_uwz` derive from `iv.wide` and exist only for 08-11..18, so
nothing above is a forward test of them. On the eight sealed days they are the only features
that add money to the rule rather than trimming it:

| cell | n | SOL/day (8d) | per trade | IS | OOS |
| --- | --- | --- | --- | --- | --- |
| `rule` (baseline) | 5,790 | 6.69 | 3.73 | 3.30 | 4.39 |
| `rule & nb30<=5 & adef>0.5` | 5,416 | **7.46** | 4.09 | 3.66 | 4.79 |
| `rule & vsol>=40 & nb30<=3` | 408 | 5.62 | 13.10 | 14.45 | 9.57 |
| ` ... & wall=0` | 326 | 5.73 | **15.31** | 16.68 | 11.21 |

`wall=0` lifts the deep cell's per-trade edge by 2.2pp while keeping 80% of its trades, and
`adef>0.5` is the only predicate found that raises the rule's money without cutting its trade
count. Both are subject to everything above - they are in-sample numbers from a search, and
this round's own result is that such numbers do not transfer. Rebuilding them on 08-19 onward
is what would settle it.

## Scratch

Session scratchpad: `feat.csv` (the exported pool), `lib1.py` (loader, predicate generator,
cost model), `s2`-`s17.py`. Nothing new in Postgres; `iv.feat` is unchanged.
