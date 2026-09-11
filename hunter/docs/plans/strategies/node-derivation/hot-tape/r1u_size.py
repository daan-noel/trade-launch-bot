"""Rule 1 update, step G7: the clip, flat against a share of the reserve.

Each ticket of the updated rule re-priced from its entry and exit reserves at every clip
(toolkit/book.reprice semantics; an upper bound - a replay cannot price our buy moving the next
prints).

  python r1u_size.py [study] [holdout]
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path

import sys

import numpy as np
import pandas as pd

from kernel import net
from toolkit.book import occupy
from toolkit.exits import X
import cvx_r1u_exit as E

NEWX = X("tp15 sl40 t90", arm=0.15, sl=0.40, cap=90.0)

pd.set_option("display.width", 300)


def main() -> None:
    for name in sys.argv[1:] or ["study"]:
        O, days = E.outcomes(name, exits_=[NEWX])
        O = O.reset_index(drop=True)
        F = occupy(O, np.ones(len(O), dtype=bool))
        v0 = F.v0.to_numpy(); v1 = F.v1.to_numpy()
        print(name, "fires", len(F), "v0 p10/50/90", np.percentile(v0, [10, 50, 90]).round(1))
        rows = []
        clips = [("flat %.2f" % c, np.full(len(F), c)) for c in (0.2, 0.35, 0.5, 0.75, 1.0)] + \
                [("%.2f %% of reserve" % (100 * f), f * v0) for f in (0.004, 0.005, 0.0075, 0.01)]
        for lab, b in clips:
            y = np.array([net(a, c, bb) for a, c, bb in zip(v0, v1, b)])
            ag = pd.Series(y).groupby(F.day.to_numpy()).sum()
            top = np.sort(y)[::-1][:max(1, round(.01 * len(y)))].sum()
            rows.append(dict(size=lab, clip_p50=round(float(np.median(b)), 2),
                             pct=round(100 * y.sum() / b.sum(), 2), solday=round(y.sum() / days, 3),
                             pos="%d/%d" % ((ag > 0).sum(), len(ag)), worst=round(ag.min(), 2),
                             top1=round(100 * top / y.sum(), 1)))
        print(pd.DataFrame(rows).to_string(index=False))


if __name__ == "__main__":
    main()
