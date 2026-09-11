"""Rule 1 update, step G5: the exit re-read on the updated sentence's own pool.

The updated sentence (G2-G4) selects a different pool than the one the +15 % / -25 % / 90 s
bracket was chosen on. Every bracket of the grid is booked on it (toolkit/exits.outcomes), each
with its own occupancy (the exit index sets when the coin frees), on the study tape. The
one-axis walk-forward is r1u_exit_axes.py; the holdout books only the bracket kept.

  python cvx_r1u_exit.py [study|holdout]
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

import itertools
import sys

import numpy as np
import pandas as pd

from toolkit import exits, tapes
from toolkit.book import ledger, mask, occupy
from toolkit.exits import X
from toolkit.walkforward import folds, solsum
from cvx_r1u import RULE1_UPDATED as NEW, cand

TPS = [0.10, 0.12, 0.15, 0.18, 0.20, 0.25, 0.30]
SLS = [0.15, 0.20, 0.25, 0.30, 0.35]
CAPS = [45.0, 60.0, 90.0, 120.0, 180.0]
GRID = [X("tp%d sl%d t%d" % (100 * a, 100 * s, c), arm=a, sl=s, cap=c)
        for a, s, c in itertools.product(TPS, SLS, CAPS)]

pd.set_option("display.width", 300)
pd.set_option("display.max_rows", 400)


def outcomes(name, spec=NEW, exits_=GRID):
    """Every candidate the mask passes, booked under each exit: before occupancy."""
    C, days = cand(name)
    return exits.outcomes(tapes.load(name), C[mask(C, spec)], exits_), days


def book_exits(name, spec=NEW, exits_=GRID):
    E, days = outcomes(name, spec, exits_)
    return {xi: occupy(g.reset_index(drop=True), np.ones(len(g), dtype=bool))
            for xi, g in E.groupby("exit")}, days


def main() -> None:
    name = sys.argv[1] if len(sys.argv) > 1 else "study"
    Fx, days = book_exits(name)
    rows = []
    for xi, F in Fx.items():
        L = ledger(F, days)
        rows.append(dict(exit=GRID[xi]["name"], **{k: L[k] for k in (
            "nday", "pct", "sol", "solday", "pos", "worst", "body", "top1", "maxcoin", "h1", "h2",
            "sl", "win", "cap_sol")}))
    R = pd.DataFrame(rows)
    R.to_csv(data_file("r1u_exit_%s.csv" % name), index=False)
    cur = "tp15 sl25 t90"
    print(R.sort_values("sol", ascending=False).head(40).to_string(index=False))
    print("\ncurrent:", R[R.exit == cur].to_string(index=False, header=False))
    if name != "study":
        return
    halves = folds(Fx[0])[0]
    sums = pd.DataFrame({GRID[xi]["name"]: [solsum(F, hh) for hh in halves]
                         for xi, F in Fx.items()}).T
    sums.columns = ["h1", "h2"]
    for fit, test in ((0, 1), (1, 0)):
        pick = sums.iloc[:, fit].idxmax()
        print("fold fit h%d: pick %-16s test h%d %+.2f vs current %+.2f"
              % (fit + 1, pick, test + 1, sums.loc[pick].iloc[test], sums.loc[cur].iloc[test]))


if __name__ == "__main__":
    main()
