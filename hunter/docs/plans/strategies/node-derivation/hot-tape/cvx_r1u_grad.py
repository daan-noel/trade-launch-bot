"""Rule 1 update, step G3: the exits that land on the graduation print (toolkit/graduation.py).

Counts, per sentence and tape, the trades whose exit fill is the coin's completing buy at vsol
115, and re-prices them as sells into the migrated pool at a post-migration drop d. The standing
convention books them at the migration point (d = 0); the reserve cap keeps the sentence off them.
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path

import numpy as np
import pandas as pd

from toolkit import tapes
from toolkit.book import fires, ledger
from toolkit.graduation import amm_net, grad_flag
from cvx_r1u import RULE1, cand

DROPS = [0.0, 0.05, 0.10, 0.20, 0.30, 0.50]

pd.set_option("display.width", 300)


def main() -> None:
    specs = [("rule 1 as booked", RULE1), ("step 1 (buys2 dropped)", dict(RULE1, buys2=(">=", 0.0))),
             ("step 1 + reserve <= 107.2", dict(RULE1, buys2=(">=", 0.0), vres=("<=", 107.2)))]
    out = []
    for nm in ("study", "holdout"):
        C, days = cand(nm)
        T = tapes.load(nm).T
        for lab, spec in specs:
            F = fires(C, spec)
            g = grad_flag(F, T)
            for d in DROPS:
                y = F.y.to_numpy().copy()
                y[g] = [amm_net(v0, d) for v0 in F.v0.to_numpy()[g]]
                L = ledger(F.assign(y2=y), days, yc="y2")
                out.append(dict(tape=nm, sentence=lab, drop=d, grad=int(g.sum()),
                                **{k: L[k] for k in ("nday", "pct", "sol", "solday", "pos",
                                                     "worst", "body", "top1", "maxcoin")}))
            y = F.y.to_numpy()
            print("%-8s %-26s graduation exits %3d: curve-booked %+.2f SOL, AMM at d=0 %+.2f, "
                  "median v0 %.1f" % (nm, lab, g.sum(), y[g].sum(),
                                      sum(amm_net(v0, 0.0) for v0 in F.v0.to_numpy()[g]),
                                      np.median(F.v0.to_numpy()[g])))
    print(pd.DataFrame(out).to_string(index=False))


if __name__ == "__main__":
    main()
