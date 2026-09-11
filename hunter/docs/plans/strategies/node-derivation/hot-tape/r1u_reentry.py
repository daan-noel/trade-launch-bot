"""Rule 1 update, step G6: re-entry (R) on the updated pool.

A cool-down after a stop-out and a cap on entries per coin, each against the standing R (one
position per coin, re-entry after the exit); then the book of the n-th entry on a coin and of
entries that follow a stop-out. Study tape only.
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path

import sys

import numpy as np
import pandas as pd

from toolkit.book import ledger, occupy
from toolkit.exits import X
from toolkit.walkforward import folds
import cvx_r1u_exit as E

pd.set_option("display.width", 300)


def main() -> None:
    cap = float(sys.argv[1]) if len(sys.argv) > 1 else 240.0
    NEWX = X("tp15 sl40 t%d" % cap, arm=0.15, sl=0.40, cap=cap)
    O, days = E.outcomes("study", exits_=[NEWX])
    O = O.reset_index(drop=True)
    H = folds(occupy(O, np.ones(len(O), dtype=bool)))[0]
    rows = []
    for cool in (0, 30, 60, 120, 300, 600, 1e9):
        for mx in (None, 1, 2, 3, 5):
            F = occupy(O, np.ones(len(O), dtype=bool), cool_sl=cool, max_per_coin=mx)
            L = ledger(F, days)
            rows.append(dict(cool_after_stop=cool if cool < 1e9 else "never",
                             max_per_coin=mx or "any",
                             h1sol=round(F[F.day.isin(H[0])].y.sum(), 2),
                             h2sol=round(F[F.day.isin(H[1])].y.sum(), 2),
                             **{k: L[k] for k in ("nday", "pct", "sol", "pos", "worst", "top1",
                                                  "maxcoin", "sl", "cap_sol")}))
    print(pd.DataFrame(rows).to_string(index=False))
    F = occupy(O, np.ones(len(O), dtype=bool))
    F = F.assign(nth=F.groupby("run").cumcount() + 1)
    F["prev_sl"] = F.groupby("run").why.shift(1).eq("sl")
    agg = dict(n=("y", "size"), pct=("y", lambda s: round(100 * s.mean() / 0.2, 2)), sol=("y", "sum"))
    print(F.groupby(np.minimum(F.nth, 6)).agg(**agg).round(2).to_string())
    print(F.groupby("prev_sl").agg(**agg).round(2).to_string())


if __name__ == "__main__":
    main()
