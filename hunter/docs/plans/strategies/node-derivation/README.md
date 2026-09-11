# node-derivation: from profitable wallets to a shippable rule

The code that runs the method, and each node's case file. The method itself is
[../_!___derive.md](../_!___derive.md). Hot-tape is the first node taken end to end; the next node
reuses the method and the toolkit, and gets its own case file and script folder beside hot-tape's.

| file | what it is | read it when |
| --- | --- | --- |
| [../_!___derive.md](../_!___derive.md) | the method: pick a member, DELAY, event, then D/X/P; the gates; how a result is recorded | deriving any node or wallet |
| [toolkit/README.md](toolkit/README.md) | the method as functions, by derive phase | writing a step's script |
| [node-template.md](node-template.md) | the shape of a node's case file | starting a node |
| [hot-tape-rule-1.md](hot-tape-rule-1.md) | the worked example: rule 1, its book, the chain of steps that produced it, the member book | reading the worked example, or shipping rule 1 |
| [hot-tape/README.md](hot-tape/README.md) | the scripts rule 1 rests on, and where the step scripts are kept | re-running rule 1, or restoring a step |
| [mid-tape-rule-1.md](mid-tape-rule-1.md) | the mid-tape node, 9Uq8GV book | 9Uq8GV leftover unread on burst start / nb2 |
| [mid-tape-rule-2.md](mid-tape-rule-2.md) | the mid-tape node, instrument 9999hu | sell >= 1 is a 5.2 kill behind its buy (evidence 1.27); another class or a state next |
| [mid-tape/README.md](mid-tape/README.md) | every mid-tape script, by step | re-running a mid-tape step |
| `data/` | candidate tables, holdout tapes, outputs and logs; not tracked, rebuilt by the scripts | - |

## Deriving the next node

1. Pick an open node in [_!___workflow.md](../_!___workflow.md) section 0; its members are the
   roster rows with that node label (`toolkit.tapes.roster`). Split members; pick one that pays
   ([_!___derive.md](../_!___derive.md) phase 1).
2. Copy [node-template.md](node-template.md) to `<node>-rule-<n>.md`, make `<node>/` with a copy of
   `hot-tape/_paths.py` (set its `NODE_NAME`), and write one script per step of
   [_!___derive.md](../_!___derive.md), with the toolkit calls of [toolkit/README.md](toolkit/README.md).
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
scratch: its step is a chain row and its number an evidence section. `data/` is never tracked.
