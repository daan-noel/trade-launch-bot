# toolkit: the method as functions

Each module answers one phase of [../../_!___derive.md](../../_!___derive.md); the step column below
is that phase. They are generic: a node is named by its roster label, a member by its address
prefix, a trigger by a function over a coin's facts. Rule 1 runs on them
([../hot-tape/](../hot-tape/README.md)) and `hot-tape/toolkit_check.py` re-runs the trigger, seat,
contrast and hazard steps next to the recorded numbers.

```python
import sys; sys.path.insert(0, "<path to node-derivation>")
import toolkit                                   # puts study-kernel (kernel, tape, cvx) on sys.path
from toolkit import tapes, seat, trigger, contrast, candidates, book, walkforward, exits, hazard

S = tapes.load("study", tapes.roster("hot-tape re-entry"))   # the tape, the members marked
w = S.wallet("8fStGV")                                        # a member's wallet id
```

A script inside `node-derivation/<node>/` imports a local `_paths.py` that does the first two
lines (copy [../hot-tape/_paths.py](../hot-tape/_paths.py)).

## Conventions

- A **tape** is chain-ordered curve prints, one contiguous run per coin (`study-kernel/tape.py`):
  `T.t` s, `T.v` vsol after the print, `T.sol`, `T.side` (1 / -1), `T.wallet`, `T.build`
  (build_core code), `T.day` (day index from `cvx.DAY0`, 2026-08-30).
- A **run** `r` is one coin; `facts.Run(S, r)` holds its arrays and windowed facts. A print index
  `k` is local to its run.
- Every **fact at k** is built from prints before k; "public" excludes the members' prints.
- **Money** is net SOL at a 0.2 SOL clip through `kernel.net`, fills from `kernel.fill_idx`
  (115 ms). `%/trade` is SOL over capital.
- **Data** files resolve through `paths.data(name)`: `node-derivation/data/` first, the shared
  `study-kernel/` second; outputs go to `data/` (not tracked, regenerable).

## Modules

| module | step | call | returns |
| --- | --- | --- | --- |
| `paths` | - | `data(name)`; `ROOT`, `DATA`, `SHARED`, `LAKE`, `LOCAL`, `HUNTER` | where things live |
| `lake_export` | 2.1 | `export(prefix, days, all_legs=False)`; `python -m toolkit.lake_export PREFIX DAY ... [--all-legs]` | `data/PREFIX_prints/_wallets/_tok.parquet`; register it in `tapes.TAPES`. The default keeps the last leg of each transaction (the study tape's grain); `--all-legs` keeps every leg (the engine's grain, tapes `holdout_legs`, `study_exact`, `holdout_exact`); every export carries `t_us` (the engine's clock) and `vtok` (spot = vsol / vtok) |
| `tapes` | 2.1-2.2 | `load(name, addresses)`, `roster(node)`, `Session.wallet(prefix)` | a `Session`: `T`, `c_s` (creation s per run), `t_min` (first fire time), `days`, `ids`, `is_node` |
| pick / 4.0 | 1, 4.0 | tape share on coins he prints; lake `ix_labels` for `InitUserVolumeAccumulator`, bundled `TransferChecked`, `CreateCoinAndBuy` | drop volume manufacture before FIND E (evidence 5.1). [rb-actor-tape-share.py](../../rb-actor-tape-share.py) is the share script for a machine |
| `facts` | all | `Run(S, r)`: `.j(w)`, `.recipes(k, w)`, `.wallets(k, w)`, `.bought(k, w)`, `.sold(k, w)`, `.move(k, w)`, `.holders()`, `.holders_and_seller(flag)`, `.pub_bought_incl(w)` | the public tape state at every print. `.holders()` counts reserve-sized bags above zero, and a full exit leaves float residue, so it reads about distinct buyers, not holders (evidence 1.22) |
| `seat` | 3-4, 5.2 | `episodes(S, w)`; `seat_book(S, E, caps)`; `reaction(S, E, trigger_fn, max_trig)`; `leftover(S, w, E, trigger_fn, max_trig=0.3)`, `leftover_summary(L)`, `veto(L, n)` | positions (k, ks, pnl, peak, held); RACE / FOLLOW clock books; `reaction`: lag, ahead and a clock (5.3 columns); `leftover`: per acted and ignored ticket the reaction cost, peak leftover at its hold p10 / p50 / p90, missed, break-even first; the summary's **behind** row is the derive 5.2 veto; `veto`: its one reader - `PASS` / `kill` (why: peak<=0, missed>=50, race) / `corner`, a `thin` flag under `THIN_PEAK`, cost reported and never a line |
| `trigger` | 5.1, 8.1 | `excess_intensity(S, {group: [ids]}, cases="buy"/"close", controls="coin"/"hold", near="node"/"group")`; `peak(lift, cls)` | lift and excess tables, class x lag bin (14 lag bins to 5 s). Classes: WHO (`tool` `nonce` `direct` `pro` `seed_racer`), this-print history, then priced screens. Identity beats a priced class when both spike. [derive section 5](../../_!___derive.md#print-classes-51-scan) |
| `contrast` | 6.1 | `label_acted(C, S, w, window)`; `strat_rank(acted, ignored, cols)` | the within-coin rank of each fact (0.50 = nothing) |
| `candidates` | 6.1-6.2, 12.3 | `build(S, trigger, floor, exit, actor, extra, runs)` | one row per trigger print: 26 standard facts, the exit outcome (y, x, why, hold, v0, v1), actor diagnostics (act, act_lag, act_pre, act_in). `runs` limits the coins |
| `book` | every book | `mask`, `occupy(C, m, cool_sl, max_per_coin)`, `fires(C, spec)`, `Occupier(run, k, xs).keep(m, e)` (occupy for the ladder's many masks and exits), `ledger(F, days)`, `capped(F)`, `reprice(F, b)`, `save` / `load` | the ledger of derive section 11 |
| `walkforward` | 6.1, 10, 12.4-12.7 | `folds`, `thresholds`, `converge(..., veto)`, `new_terms`, `axes`, `cut_noise`; `ladder`, `walk_ladder`, `coin_shuffle` | the keep rule's verdicts and logs; the ladder's path per fold against chance |
| `exits` | 8.2, 12.7 | `X(name, kind, arm, sl, cap, ...)`, `run_exit(sp, t, v, side, sol, mv, cb3, ei)`, `outcomes(S, C, exits)` | net SOL, exit index, reason, hold; every candidate under each exit |
| `hazard` | 8.1 | `closing_hazard(S, w, pool)` | its closes by pool, and the hazard table (profit band x time held) |
| `graduation` | 12.5 | `grad_flag(F, T)`, `amm_net(v0, d)` | which tickets exit on the completing print; their price in the migrated pool at a drop d |

## A new node in ten calls

```python
S = tapes.load("study", tapes.roster(NODE))
w = {p: S.wallet(p) for p in MEMBERS}
E = {p: seat.seat_book(S, seat.episodes(S, w[p]), caps=(15.0,)) for p in MEMBERS}       # 4
lift = trigger.excess_intensity(S, {p: [w[p]] for p in PAYERS})                          # 5.1
L = seat.leftover(S, w[p], E[p], lambda run: <the trigger class as a mask>)              # 5.2
C = contrast.label_acted(candidates.build(S, <trigger on its coins>), S, w[p], window)   # 6.1
contrast.strat_rank(C[C.act == 1], C[(C.act == 0) & C.run.isin(C[C.act == 1].run)], facts)
C = candidates.build(S, trigger, floor, exits.X(...), actor=w[p]); book.save(C, S.days, PFX, "study")
Ecl, hz, cnt = hazard.closing_hazard(S, w[p], pool)                                       # 8.1
base, log = walkforward.converge(C, S.days, spec, grid, veto=bars)                        # 12.4
```

Then the holdout: `tapes.load("holdout", ...)`, the same `candidates.build`, and `book.fires` with
the spec chosen - nothing re-fitted.

## Limits

- `exits.outcomes` passes the tape-reactive inputs as zeros: exact for the bracket and scale
  families; book `sellbuy`, `ride`, `fade` and `dump` through `run_exit` with a `facts.Run`.
- `candidates.build` computes every fact before the floor; a full tape takes about a minute.
- `trigger.excess_intensity` draws random controls: lifts agree between runs to sampling noise,
  not to the digit.
