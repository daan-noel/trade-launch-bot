"""Rule 1 update, step G8: the holdout, one change at a time.

Each change chosen on the study tape is booked once on the holdout, in the order it was taken, so
each shows whether it holds out of sample on its own. The 240 s clock is booked to record that it
fails; the updated rule is the "+ stop -40" row. Writes data/cvx_r1u_final_{study,holdout}.parquet
(the updated rule's tickets).
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

import numpy as np
import pandas as pd

from toolkit import tapes
from toolkit.book import ledger, occupy
from toolkit.exits import X
from toolkit.graduation import grad_flag
import cvx_r1u_exit as E
from cvx_r1u import RULE1, RULE1_UPDATED

EXS = [X("tp15 sl25 t90", arm=0.15, sl=0.25, cap=90.0), X("tp15 sl40 t90", arm=0.15, sl=0.40, cap=90.0),
       X("tp15 sl25 t240", arm=0.15, sl=0.25, cap=240.0),
       X("tp15 sl40 t240", arm=0.15, sl=0.40, cap=240.0)]
S1 = dict(RULE1, buys2=(">=", 0.0))
STEPS = [("rule 1 as booked", RULE1, 0), ("- bought 2 SOL in 2 s", S1, 0),
         ("+ reserve <= 100", RULE1_UPDATED, 0), ("+ stop -40 = updated rule 1", RULE1_UPDATED, 1),
         ("+ clock 240 (stop -25)", RULE1_UPDATED, 2), ("+ stop -40 + clock 240", RULE1_UPDATED, 3)]
COLS = ("nday", "pct", "sol", "solday", "pos", "worst", "body", "top1", "maxcoin", "h1", "h2", "sl",
        "win", "cap_sol")

pd.set_option("display.width", 320)


def main() -> None:
    rows = []
    for name in ("study", "holdout"):
        T = tapes.load(name).T
        cache = {}
        for lab, spec, xi in STEPS:
            if id(spec) not in cache:
                cache[id(spec)] = E.outcomes(name, spec=spec, exits_=EXS)
            O, days = cache[id(spec)]
            O = O[O.exit == xi].reset_index(drop=True)
            F = occupy(O, np.ones(len(O), dtype=bool))
            L = ledger(F, days)
            rows.append(dict(tape=name, sentence=lab, grad=int(grad_flag(F, T).sum()),
                             **{k: L[k] for k in COLS}))
            if lab.endswith("updated rule 1"):
                F.to_parquet(data_file("cvx_r1u_final_%s.parquet" % name), index=False)
    print(pd.DataFrame(rows).to_string(index=False))


if __name__ == "__main__":
    main()
