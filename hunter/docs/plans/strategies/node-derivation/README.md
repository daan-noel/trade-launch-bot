# node-derivation: from profitable wallets to a shippable rule

Everything about deriving a rule from the traders who run a decision node lives in this folder:
the method, the code that runs it, each node's case file and its scripts. Hot-tape is the first
node taken end to end; the next node reuses the method and the toolkit, and gets its own case
file and script folder beside hot-tape's.

| file | what it is | read it when |
| --- | --- | --- |
| [method.md](method.md) | the playbook: the basis, the standing corrections, the keep rule and ship bars, and every step (phases 0, A-G) with the toolkit call, the decision rule, the hot-tape result and the trap | deriving any node |
| [toolkit/README.md](toolkit/README.md) | the method as functions: tapes, facts, seats, trigger, contrast, candidates, book, walk-forward, exits, hazard, graduation, lake export | writing a step's script |
| [node-template.md](node-template.md) | the shape of a node's working file | starting a node |
| [hot-tape-rule-1.md](hot-tape-rule-1.md) | the hot-tape node: rule 1, its book, the chain of measurements that produced it, the member book | reading the worked example, or shipping rule 1 |
| [hot-tape/README.md](hot-tape/README.md) | every hot-tape script, by step, with its evidence section | re-running or auditing a step |
| `data/` | candidate tables, holdout tapes, outputs and logs; not tracked, rebuilt by the scripts | - |

The numbers live in [_!___evidence.md](../_!___evidence.md), the loop this plugs into is
[_!___workflow.md](../_!___workflow.md) section 8 G1, the ideas per slot are in
[_!___inventory.md](../_!___inventory.md), and the instruments are the roster's
[solo-traders.md](../solo-traders.md).

## Deriving the next node

1. Pick an open node in [_!___workflow.md](../_!___workflow.md) G1; its members are the roster
   rows with that node label (`toolkit.tapes.roster`).
2. Copy [node-template.md](node-template.md) to `<node>-rule-<n>.md`, make `<node>/` with a copy of
   `hot-tape/_paths.py`, and write one script per step of [method.md](method.md).
3. Record each step as the method's section 6 says: evidence section, workflow row, inventory
   status, the case file's chain row.
4. The holdout confirms and never chooses; the next unseen lake days are exported with
   `toolkit.lake_export` and registered in `toolkit.tapes.TAPES`.

## Running

Python 3.12 with numpy, pandas, pyarrow (and psycopg2 for the study tape's `wallet_dict` lookup,
which reads `DATABASE_URL` from `hunter/.env`). The shared study kernel stays in
`../study-kernel/` - `kernel.py` (the one pricing kernel), `tape.py`, `cvx.py` - with the study
tape `cvx_prints.parquet` and its sidecars, which the other studies use too. The `.py` files here
and those three kernel files are tracked; `data/` is not.
