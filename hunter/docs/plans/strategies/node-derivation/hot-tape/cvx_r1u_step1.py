"""Rule 1 update, step G2: every threshold re-derived by walk-forward on money.

G0 shows 8fStGV's selection adds nothing inside rule 1 (the sells it buys book no better than
those it skips), so a term's cut is read off money, not off its picks. For each term, the others
held at `base`, every cut on GRID is booked on the study tape with occupancy, and the keep rule of
toolkit/walkforward.py decides. The holdout is never read here.

  python cvx_r1u_step1.py [round]   round 1 starts from rule 1, later rounds from the last kept set
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

import json
import sys

import pandas as pd

from toolkit.book import fires, ledger
from toolkit.walkforward import allpos, solsum, thresholds  # noqa: F401  - re-exported
from cvx_r1u import RULE1, cand

GRID = {
    "ssize": [0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 2.5, 3.0, 4.0],
    "nb5": [8, 10, 12, 13, 14, 15, 16, 17, 18, 20, 22, 25, 30],
    "buys2": [0.0, 0.25, 0.5, 0.75, 1.0, 1.5, 2.0, 2.5, 3.0, 4.0, 5.0, 6.0, 8.0],
    "stall": [2.0, 5.0, 10.0, 15.0, 20.0, 30.0, 45.0, 60.0, 90.0],
    "shold": [5.0, 10.0, 15.0, 20.0, 30.0, 45.0, 60.0, 90.0, 120.0, 180.0, 300.0],
    "age": [60.0, 90.0, 120.0, 158.0, 200.0, 250.0, 300.0, 400.0, 600.0],
    "hold_n": [0, 50, 100, 150, 200, 250, 300, 368, 450, 550, 700, 900],
    "vres": [85.0, 90.0, 95.0, 100.0, 104.0, 107.2, 110.0, 116.0],
}

pd.set_option("display.width", 300)
pd.set_option("display.max_rows", 300)


def main() -> None:
    rnd = int(sys.argv[1]) if len(sys.argv) > 1 else 1
    base = dict(RULE1)
    kept = data_file("cvx_r1u_kept.json")
    if rnd > 1 and kept.exists():
        base.update({k: tuple(v) for k, v in json.loads(kept.read_text()).items()})
    C, days = cand("study")
    print("round %d base %s" % (rnd, {k: v[1] for k, v in base.items()}))
    print("base study", ledger(fires(C, base), days))
    W, curves = thresholds(C, days, base, GRID)
    print("\n=== study curves, one term moved, the rest at base")
    print(curves.to_string(index=False))
    print("\n=== walk-forward")
    print(W.to_string(index=False))
    new = dict(base)
    for _, r in W[W["take"]].iterrows():
        new[r.term] = (base[r.term][0], r.value)
    print("\njoint, study", {k: v[1] for k, v in new.items()})
    print(ledger(fires(C, new), days))
    kept.write_text(json.dumps({k: list(v) for k, v in new.items()}))


if __name__ == "__main__":
    main()
