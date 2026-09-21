# node-derivation: from profitable wallets to a shippable rule

The code that runs the method, and each wallet's case file. The method is nine steps - pick,
portrait, his exit, test each clause, our version, fit D / P / X, coverage, prove, record - on the
first page of [../_!___derive.md](../_!___derive.md). Hot-tape is the first node taken end to end; the next node
reuses the method and the toolkit, and gets its own case file and script folder beside hot-tape's.

| file | what it is | read it when |
| --- | --- | --- |
| [../_!___derive.md](../_!___derive.md) | the method: the nine steps on its first page, then the detail of each; the gates; how a result is recorded | deriving any wallet; at the start of every session |
| [toolkit/README.md](toolkit/README.md) | the method as functions, by derive phase | writing a step's script |
| [node-template.md](node-template.md) | the shape of a wallet's case file: the step checklist, his logic in clauses, his exit, the rule, the chain, the coverage table | starting a wallet |
| [hot-tape-rule-1.md](hot-tape-rule-1.md) | the worked example: rule 1, its book, the chain of steps that produced it, the member book | reading the worked example, or shipping rule 1 |
| [hot-tape/README.md](hot-tape/README.md) | the scripts rule 1 rests on, and where the step scripts are kept | re-running rule 1, or restoring a step |
| **one wallet, one case file** | [8dtx2t](mid-tape-8dtx2t.md) . [3Xk2Eu](mid-tape-3Xk2Eu.md) . [8aaRWu](mid-tape-8aaRWu.md) . [88887Q](mid-tape-88887Q.md) . [9999hu](mid-tape-9999hu.md) . [9Uq8GV](mid-tape-9Uq8GV.md) | each holds that wallet's checklist, its logic, its own exit, its sentence and its **coverage table** - which inventory family is tried, partly, not tried, no data or not his. Read one before touching its wallet |
| [mid-tape-rule-3.md](mid-tape-rule-3.md) | the mid-tape node's shared chain and frame; the inventory reaches its rows as `mt3 <row>` | open: E not spelled (0.25 % acted; named `build` list is the market at 0.29 % or a 5.3 % corner; silence-then-K is a 1.8 % corner; run_k>=2 is 0.63 % of that slice; Two lists agree K>=2 leftover cover<10 at 1.77 % slice; First run here leftover PASSES at 0.50 % slice / rank 0.501; Size buy leftover PASSES at 0.56 % slice; `alone_in_slot` rank 0.354 low, leftover PASSES at 0.08 % slice); next 6.1 remaining this-print unread (this-wallet / neighborhood / lake fee), not 7.1 / phase 8. 8dtx2t / 3Xk2Eu burst and 88887Q sell/down leftover exists under the peak-leftover veto; 3Xk2Eu stays open with its door the empty slot: E spelled and reachable (1 in 7, peak leftover +8.57 %), occupancy -4.54 %/trade 0/6 on every coin, D none on every door read, and the only green universe is its coins before it arrives (+13.12 % 6/6); ApfmkS is volume manufacture |
| [mid-tape-8dtx2t-logic.md](mid-tape-8dtx2t-logic.md) | 8dtx2t's entry and exit as plain reasoning, each number named by its chain row | explaining why 8dtx2t buys and sells |
| [mid-tape-rule-1.md](mid-tape-rule-1.md) | fill-copy chain on 9Uq8GV / 8dtx2t / ApfmkS | prior spelling; not the live derivation |
| [mid-tape-rule-2.md](mid-tape-rule-2.md) | fill-copy chain on 9999hu | prior spelling; not the live derivation |
| [mid-tape/README.md](mid-tape/README.md) | every mid-tape script, by step | re-running a mid-tape step |
| `data/` | candidate tables, holdout tapes, outputs and logs; not tracked, rebuilt by the scripts | - |

## Deriving the next node

1. Take a wallet from [_!___workflow.md](../_!___workflow.md); a node's members are the roster rows
   with that node label (`toolkit.tapes.roster`). One wallet, one case file; no wallet is closed
   ([_!___derive.md](../_!___derive.md) step 1).
2. Copy [node-template.md](node-template.md) to `<node>-rule-<n>.md`, make `<node>/` with a copy of
   `hot-tape/_paths.py` (set its `NODE_NAME`), and write one script per step of
   [_!___derive.md](../_!___derive.md), with the toolkit calls of [toolkit/README.md](toolkit/README.md).
   Tick the checklist in order: the portrait and his exit come before any scan.
3. Record each step once, as [_!___derive.md](../_!___derive.md) section 13 says: a row of the
   case file's chain; an evidence section only for a number a rule, a law or an open line stands on.
4. The holdout confirms and never chooses; the next unseen lake days are exported with
   `toolkit.lake_export` and registered in `toolkit.tapes.TAPES`.

## Running

Python 3.12 with numpy, pandas, pyarrow (and psycopg2 for the study tape's `wallet_dict` lookup,
which reads `DATABASE_URL` from `hunter/.env`). The shared study kernel stays in
`../study-kernel/` - `kernel.py` (the one pricing kernel), `tape.py`, `cvx.py` - with the study
tape `cvx_prints.parquet` and its sidecars, which the other studies use too.

What is tracked: the toolkit, each case folder's `_paths.py`, the three kernel files, and the
scripts a rule or a gate needs re-run, named in the root `.gitignore`. A step script is local
scratch: its step and its numbers are a chain row. `data/` is never tracked.
