# node-derivation: from profitable wallets to a shippable rule

Everything about deriving a rule from the traders who run a decision node lives in this folder:
the method, the code that runs it, each node's case file and its scripts. Hot-tape is the first
node taken end to end; the next node reuses the method and the toolkit, and gets its own case
file and script folder beside hot-tape's.

| file | what it is | read it when |
| --- | --- | --- |
| [../_!___derive.md](../_!___derive.md) | the playbook: pick a member, DELAY, event, then D/X/P; inventory on that parent | deriving any node or wallet |
| [method.md](method.md) | toolkit mapping of each derive phase, with the hot-tape worked example | writing a step's script on the hot-tape pattern |
| [toolkit/README.md](toolkit/README.md) | the method as functions: tapes, facts, seats, trigger, contrast, candidates, book, walk-forward, exits, hazard, graduation, lake export | writing a step's script |
| [node-template.md](node-template.md) | the shape of a node's working file | starting a node |
| [hot-tape-rule-1.md](hot-tape-rule-1.md) | the hot-tape node: rule 1, its book, the chain of measurements that produced it, the member book | reading the worked example, or shipping rule 1 |
| [hot-tape/README.md](hot-tape/README.md) | every hot-tape script, by step, with its evidence section | re-running or auditing a step |
| [mid-tape-rule-1.md](mid-tape-rule-1.md) | the mid-tape node, instrument 9Uq8GV | deriving mid-tape |
| [mid-tape/README.md](mid-tape/README.md) | every mid-tape script, by step | re-running a mid-tape step |
| `data/` | candidate tables, holdout tapes, outputs and logs; not tracked, rebuilt by the scripts | - |

The numbers live in [_!___evidence.md](../_!___evidence.md). The playbook is
[_!___derive.md](../_!___derive.md). The gates and queue are
[_!___workflow.md](../_!___workflow.md). Ideas per slot:
[_!___inventory.md](../_!___inventory.md). Instruments:
[solo-traders.md](../solo-traders.md).

## Deriving the next node

1. Pick an open node in [_!___workflow.md](../_!___workflow.md) G1; its members are the roster
   rows with that node label (`toolkit.tapes.roster`). Split members; pick one that pays
   ([_!___derive.md](../_!___derive.md) phase 1).
2. Copy [node-template.md](node-template.md) to `<node>-rule-<n>.md`, make `<node>/` with a copy of
   `hot-tape/_paths.py`, and write one script per step of [_!___derive.md](../_!___derive.md)
   (toolkit calls as in [method.md](method.md)).
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
