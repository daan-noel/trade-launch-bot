"""Rule 1 update, step G5: the exit, one axis at a time by walk-forward (toolkit/walkforward.axes).

Take profit, stop and clock on the updated pool, from the bracket before the update (+15 % /
-25 % / 90 s). The result is booked on the holdout by r1u_holdout.py.
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path

import itertools

import pandas as pd

from toolkit.book import ledger
from toolkit.exits import X
from toolkit.walkforward import axes
import cvx_r1u_exit as E

TPS = [0.12, 0.15, 0.18, 0.20, 0.22, 0.25, 0.30, 0.35]
SLS = [0.20, 0.25, 0.30, 0.35, 0.40, 0.50]
CAPS = [60.0, 90.0, 120.0, 180.0, 240.0, 300.0]
KEYS = list(itertools.product(TPS, SLS, CAPS))
G = [X("tp%d sl%d t%d" % (100 * a, 100 * s, c), arm=a, sl=s, cap=c) for a, s, c in KEYS]
COLS = ("nday", "pct", "sol", "pos", "worst", "body", "top1", "maxcoin", "h1", "h2", "sl", "win",
        "cap_sol")

pd.set_option("display.width", 300)


def main() -> None:
    Fx, days = E.book_exits("study", exits_=G)
    books = {KEYS[xi]: F for xi, F in Fx.items()}
    kept, log = axes(books, [TPS, SLS, CAPS], (0.15, 0.25, 90.0), ["tp", "sl", "clock"])
    print(log.to_string(index=False))
    print("kept exit", kept)
    rows = []
    for key in [(a, kept[1], kept[2]) for a in TPS] + [(kept[0], s, kept[2]) for s in SLS] + \
               [(kept[0], kept[1], c) for c in CAPS]:
        L = ledger(books[key], days)
        rows.append(dict(exit="tp%d sl%d t%d" % (100 * key[0], 100 * key[1], key[2]),
                         **{k: L[k] for k in COLS}))
    print(pd.DataFrame(rows).to_string(index=False))


if __name__ == "__main__":
    main()
