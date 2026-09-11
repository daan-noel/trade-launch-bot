"""Rule 1 update, step G5 check: the take profit re-read at the kept stop and clock (-40 %, 90 s).

The clock walked to 240 s on the study tape and failed the holdout, so the take profit is
re-checked at the clock kept. Study tape only.
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path

import pandas as pd

from toolkit.book import ledger
from toolkit.exits import X
from toolkit.walkforward import folds, solsum
import cvx_r1u_exit as E

TPS = [0.12, 0.15, 0.18, 0.20, 0.22, 0.25, 0.30]
G = [X("tp%d sl40 t90" % (100 * a), arm=a, sl=0.40, cap=90.0) for a in TPS]

pd.set_option("display.width", 300)


def main() -> None:
    Fx, days = E.book_exits("study", exits_=G)
    H = folds(Fx[0])[0]
    S = {xi: [solsum(F, hh) for hh in H] for xi, F in Fx.items()}
    cur = TPS.index(0.15)
    for fit, test in ((0, 1), (1, 0)):
        b = max(S, key=lambda xi: S[xi][fit])
        print("fit h%d pick %s -> test %+.2f vs tp15 %+.2f"
              % (fit + 1, G[b]["name"], S[b][test], S[cur][test]))
    print(pd.DataFrame([dict(exit=G[xi]["name"], **{k: ledger(F, days)[k] for k in (
        "nday", "pct", "sol", "pos", "top1", "sl", "cap_sol")}) for xi, F in Fx.items()])
        .to_string(index=False))


if __name__ == "__main__":
    main()
